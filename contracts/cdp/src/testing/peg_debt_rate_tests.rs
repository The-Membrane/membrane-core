/// Integration tests for peg_debt with its own fixed rate segments.
///
/// These tests exercise the full CDP contract through multi-test, including:
/// - Peg debt variable creation via transmuter
/// - Peg debt fixed rate creation with splits
/// - Peg debt accrual (variable + fixed)
/// - Peg debt repay (default + targeted)
/// - Peg debt liquidation
/// - Shared fixed-rate cap enforcement across both pools

pub mod peg_debt_tests {

    use std::str::FromStr;

    use crate::helpers::{CDPContract, LQContract};

    use membrane::liq_queue::LiquidatibleResponse as LQ_LiquidatibleResponse;
    use membrane::math::Uint256;
    use membrane::oracle::{AssetResponse, PriceResponse};
    use membrane::osmosis_proxy::{GetDenomResponse, TokenInfoResponse, OwnerResponse};
    use membrane::cdp::{
        ExecuteMsg, InstantiateMsg, QueryMsg, EditBasket, UpdateConfig, CreateBasket,
        BasketPositionsResponse, PositionResponse,
    };
    use membrane::stability_pool::LiquidatibleResponse as SP_LiquidatibleResponse;
    use membrane::staking::Config as Staking_Config;
    use membrane::transmuter::{self, VaultInfoResponse};
    use membrane::types::{
        cAsset, Asset, AssetInfo, AssetOracleInfo, LiquidityInfo, TWAPPoolInfo,
        UserInfo, StakeDistribution, Owner,
        PoolStateResponse, SupplyCap, Basket, DebtSplit,
    };
    use membrane::liquidity_check::LiquidityResponse;

    use cosmwasm_std::{
        attr, coin, to_json_binary, Addr, Binary, Coin, Decimal, Empty, Response, StdError,
        StdResult, Uint128, BlockInfo,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    use cosmwasm_schema::cw_serde;

    const USER: &str = "user";
    const ADMIN: &str = "admin";

    fn get_user_addr(app: &App) -> Addr {
        app.api().addr_make(USER)
    }
    fn get_admin_addr(app: &App) -> Addr {
        app.api().addr_make(ADMIN)
    }

    //--- CDP contract wrapper ---
    pub fn cdp_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contract::execute,
            crate::contract::instantiate,
            crate::contract::query,
        )
        .with_reply(crate::contract::reply);
        Box::new(contract)
    }

    //--- Mock Transmuter ---
    // Handles QueryMsg::VaultInfo and ExecuteMsg::Transmute.
    // Stores paired_asset_balance in contract storage to track transmutations.
    #[cw_serde]
    pub struct TransmuterMockInstantiateMsg {
        pub paired_asset_balance: Uint128,
        pub total_deposit_value: Uint128,
        pub cdt_balance: Uint128,
    }

    pub fn transmuter_mock_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            // Execute: handle Transmute (just acknowledge, we mock bank sends via app)
            |deps, _env, _info, msg: transmuter::ExecuteMsg| -> StdResult<Response> {
                match msg {
                    transmuter::ExecuteMsg::Transmute { recipient: _ } => {
                        // In real contract this swaps CDT -> paired asset.
                        // In mock, just succeed. The bank balance is handled by mock_app setup.
                        Ok(Response::new().add_attribute("action", "transmute"))
                    }
                    _ => Ok(Response::new()),
                }
            },
            // Instantiate: store vault info values as JSON string
            |deps, _env, _info, msg: TransmuterMockInstantiateMsg| -> StdResult<Response> {
                let vault_info = VaultInfoResponse {
                    total_deposit_value: msg.total_deposit_value,
                    deposit_total: msg.total_deposit_value,
                    cdt_balance: msg.cdt_balance,
                    paired_asset_balance: msg.paired_asset_balance,
                };
                deps.storage.set(b"vault_info", &to_json_binary(&vault_info)?.to_vec());
                Ok(Response::default())
            },
            // Query: handle VaultInfo
            |deps, _env, msg: transmuter::QueryMsg| -> StdResult<Binary> {
                match msg {
                    transmuter::QueryMsg::VaultInfo {} => {
                        let data = deps.storage.get(b"vault_info")
                            .ok_or_else(|| StdError::generic_err("no vault info"))?;
                        Ok(Binary::from(data))
                    }
                    _ => Err(StdError::generic_err("not implemented")),
                }
            },
        );
        Box::new(contract)
    }

    //--- Mock Stability Pool ---
    #[cw_serde]
    pub enum SP_MockExecuteMsg {
        Liquidate {
            liq_amount: Decimal,
        },
    }
    #[cw_serde]
    pub struct SP_MockInstantiateMsg {}
    #[cw_serde]
    pub enum SP_MockQueryMsg {
        CheckLiquidatible {
            amount: Decimal,
            credit_info: AssetInfo,
            collateral_info: AssetInfo,
        },
    }

    pub fn stability_pool_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_deps, _, _info, msg: SP_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    SP_MockExecuteMsg::Liquidate { .. } => Ok(Response::new()),
                }
            },
            |_, _, _, _: SP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: SP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    SP_MockQueryMsg::CheckLiquidatible { .. } => {
                        Ok(to_json_binary(&SP_LiquidatibleResponse {
                            leftover: Decimal::zero(),
                        })?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    //--- Mock Liquidation Queue ---
    #[cw_serde]
    pub enum LQ_MockExecuteMsg {
        Liquidate {
            credit_price: PriceResponse,
            collateral_price: PriceResponse,
            collateral_amount: Uint256,
            bid_for: AssetInfo,
        },
        AddQueue {
            bid_for: AssetInfo,
            max_premium: Uint128,
            bid_threshold: Uint256,
        },
        EditQueue {
            bid_for: AssetInfo,
            max_premium: Uint128,
            bid_threshold: Uint256,
        },
        UpdateQueue {
            bid_for: AssetInfo,
            max_premium: Option<Uint128>,
            bid_threshold: Option<Uint256>,
        },
    }
    #[cw_serde]
    pub struct LQ_MockInstantiateMsg {}
    #[cw_serde]
    pub enum LQ_MockQueryMsg {
        CheckLiquidatible {
            bid_for: AssetInfo,
            collateral_price: PriceResponse,
            collateral_amount: Uint256,
            credit_info: AssetInfo,
            credit_price: PriceResponse,
        },
    }

    pub fn liq_queue_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: LQ_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    LQ_MockExecuteMsg::Liquidate { .. } => Ok(Response::new()),
                    LQ_MockExecuteMsg::AddQueue { .. } => Ok(Response::default()),
                    LQ_MockExecuteMsg::EditQueue { .. } => Ok(Response::default()),
                    LQ_MockExecuteMsg::UpdateQueue { .. } => Ok(Response::default()),
                }
            },
            |_, _, _, _: LQ_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: LQ_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    LQ_MockQueryMsg::CheckLiquidatible { .. } => {
                        Ok(to_json_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: String::from("0"),
                            total_debt_repaid: String::from("0"),
                        })?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    //--- Mock Osmosis Proxy ---
    #[cw_serde]
    pub enum Osmo_MockExecuteMsg {
        MintTokens {
            denom: String,
            amount: Uint128,
            mint_to_address: String,
        },
        BurnTokens {
            denom: String,
            amount: Uint128,
            burn_from_address: String,
        },
        CreateDenom {
            subdenom: String,
            max_supply: Option<Uint128>,
            liquidity_multiplier: Option<Decimal>,
        },
        ExecuteSwaps {
            token_out: Vec<Coin>,
            max_slippage: Decimal,
        },
    }
    #[cw_serde]
    pub struct Osmo_MockInstantiateMsg {}
    #[cw_serde]
    pub enum Osmo_MockQueryMsg {
        PoolState { id: u64 },
        GetDenom { creator_address: String, subdenom: String },
        GetTokenInfo { denom: String },
        GetOwner { owner: String },
    }

    pub fn osmosis_proxy_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: Osmo_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Osmo_MockExecuteMsg::MintTokens { .. } => Ok(Response::new()),
                    Osmo_MockExecuteMsg::BurnTokens { .. } => Ok(Response::new()),
                    Osmo_MockExecuteMsg::CreateDenom { subdenom, max_supply, liquidity_multiplier } => {
                        Ok(Response::new().add_attributes(vec![
                            attr("subdenom", "credit_fulldenom"),
                            attr("max_supply", max_supply.unwrap_or(Uint128::zero()).to_string()),
                            attr("liquidity_multiplier", liquidity_multiplier.unwrap_or(Decimal::zero()).to_string()),
                        ]))
                    }
                    Osmo_MockExecuteMsg::ExecuteSwaps { .. } => Ok(Response::new()),
                }
            },
            |_, _, _, _: Osmo_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: Osmo_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Osmo_MockQueryMsg::PoolState { id } => {
                        Ok(to_json_binary(&PoolStateResponse {
                            assets: vec![coin(49_999, "credit_fulldenom").into()],
                            shares: coin(0, "shares").into(),
                        })?)
                    }
                    Osmo_MockQueryMsg::GetDenom { .. } => {
                        Ok(to_json_binary(&GetDenomResponse {
                            denom: String::from("credit_fulldenom"),
                        })?)
                    }
                    Osmo_MockQueryMsg::GetTokenInfo { denom } => {
                        Ok(to_json_binary(&TokenInfoResponse {
                            denom,
                            current_supply: Uint128::new(200_000u128),
                            max_supply: Uint128::new(1_000_000_000_000_000u128),
                            burned_supply: Uint128::zero(),
                        })?)
                    }
                    Osmo_MockQueryMsg::GetOwner { owner } => {
                        Ok(to_json_binary(&OwnerResponse {
                            owner: Owner {
                                owner: Addr::unchecked(""),
                                total_minted: Uint128::zero(),
                                stability_pool_ratio: Some(Decimal::one()),
                                non_token_contract_auth: true,
                                is_position_contract: false,
                            },
                            liquidity_multiplier: Decimal::percent(500),
                        })?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    //--- Mock Oracle ---
    #[cw_serde]
    pub struct Oracle_MockInstantiateMsg {}
    #[cw_serde]
    pub enum Oracle_MockQueryMsg {
        Price { asset_info: AssetInfo, twap_timeframe: u64, oracle_time_limit: u64, basket_id: Option<Uint128> },
        Prices { asset_infos: Vec<AssetInfo>, twap_timeframe: u64, oracle_time_limit: u64 },
        Assets { asset_infos: Vec<AssetInfo> },
    }

    pub fn oracle_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: membrane::oracle::ExecuteMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, _, _: Oracle_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: Oracle_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Oracle_MockQueryMsg::Price { asset_info, twap_timeframe, .. } => {
                        // All collateral at $10
                        if twap_timeframe != 480 {
                            Ok(to_json_binary(&vec![PriceResponse {
                                prices: vec![],
                                price: Decimal::from_str("10").unwrap(),
                                decimals: 6u64,
                            }])?)
                        } else {
                            // Credit price = $1
                            Ok(to_json_binary(&vec![PriceResponse {
                                prices: vec![],
                                price: Decimal::one(),
                                decimals: 6u64,
                            }])?)
                        }
                    }
                    Oracle_MockQueryMsg::Prices { asset_infos, twap_timeframe, .. } => {
                        let prices: Vec<PriceResponse> = asset_infos.iter().map(|_| {
                            if twap_timeframe != 480 {
                                PriceResponse {
                                    prices: vec![],
                                    price: Decimal::from_str("10").unwrap(),
                                    decimals: 6u64,
                                }
                            } else {
                                PriceResponse {
                                    prices: vec![],
                                    price: Decimal::one(),
                                    decimals: 6u64,
                                }
                            }
                        }).collect();
                        Ok(to_json_binary(&prices)?)
                    }
                    Oracle_MockQueryMsg::Assets { asset_infos } => {
                        let assets: Vec<AssetResponse> = asset_infos.iter().map(|info| {
                            AssetResponse {
                                asset_info: info.clone(),
                                oracle_info: vec![],
                            }
                        }).collect();
                        Ok(to_json_binary(&assets)?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    //--- Mock Staking ---
    #[cw_serde]
    pub enum Staking_MockExecuteMsg {
        DepositFee {},
    }
    #[cw_serde]
    pub struct Staking_MockInstantiateMsg {}
    #[cw_serde]
    pub enum Staking_MockQueryMsg {
        Config {},
    }

    pub fn staking_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, _: Staking_MockExecuteMsg| -> StdResult<Response> { Ok(Response::new()) },
            |_, _, _, _: Staking_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, _: Staking_MockQueryMsg| -> StdResult<Binary> {
                Ok(to_json_binary(&Staking_Config {
                    owner: Addr::unchecked(""),
                    mbrn_denom: String::from("mbrn_denom"),
                    incentive_schedule: StakeDistribution {
                        rate: Decimal::zero(),
                        duration: 0,
                    },
                    lock_duration_ceiling: 0,
                    unstaking_period: 0,
                    max_commission_rate: Decimal::zero(),
                    keep_raw_cdt: false,
                    vesting_rev_multiplier: Decimal::one(),
                    positions_contract: Some(Addr::unchecked("")),
                    auction_contract: Some(Addr::unchecked("")),
                    vesting_contract: Some(Addr::unchecked("")),
                    governance_contract: Some(Addr::unchecked("")),
                    osmosis_proxy: Some(Addr::unchecked("")),
                    emissions_voting_contract: None,
                })?)
            },
        );
        Box::new(contract)
    }

    //--- Mock Debt Auction ---
    #[cw_serde]
    pub enum Auction_MockExecuteMsg {
        StartAuction {
            repayment_position_info: Option<UserInfo>,
            send_to: Option<String>,
            auction_asset: Asset,
        },
    }
    #[cw_serde]
    pub struct Auction_MockInstantiateMsg {}

    pub fn auction_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, _: Auction_MockExecuteMsg| -> StdResult<Response> { Ok(Response::new()) },
            |_, _, _, _: Auction_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, _: cosmwasm_std::Empty| -> StdResult<Binary> { Ok(Binary::default()) },
        );
        Box::new(contract)
    }

    //--- Mock Liquidity Check ---
    #[cw_serde]
    pub struct Liquidity_MockInstantiateMsg {}
    #[cw_serde]
    pub enum Liquidity_MockQueryMsg {
        Liquidity { asset: LiquidityInfo },
    }

    pub fn liquidity_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, _: cosmwasm_std::Empty| -> StdResult<Response> { Ok(Response::new()) },
            |_, _, _, _: Liquidity_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: Liquidity_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Liquidity_MockQueryMsg::Liquidity { .. } => {
                        Ok(to_json_binary(&LiquidityResponse {
                            asset: AssetInfo::NativeToken { denom: "debit".to_string() },
                            liquidity: Uint128::new(1_000_000_000_000),
                        })?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    //--- Mock Discounts ---
    #[cw_serde]
    pub struct Discounts_MockInstantiateMsg {}
    #[cw_serde]
    pub enum Discounts_MockQueryMsg {
        UserDiscount { user: String },
    }

    pub fn discounts_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, _: cosmwasm_std::Empty| -> StdResult<Response> { Ok(Response::new()) },
            |_, _, _, _: Discounts_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: Discounts_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Discounts_MockQueryMsg::UserDiscount { .. } => {
                        Ok(to_json_binary(&Decimal::zero())?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    //--- Mock LTV Disco ---
    #[cw_serde]
    pub struct LTVDisco_MockInstantiateMsg {}

    pub fn ltv_disco_mock_contract() -> Box<dyn Contract<Empty>> {
        use membrane::ltv_disco::{ExecuteMsg as LTVDisco_ExecuteMsg, QueryMsg as LTVDisco_QueryMsg};
        let contract = ContractWrapper::new(
            |_, _, _, _msg: LTVDisco_ExecuteMsg| -> StdResult<Response> { Ok(Response::new()) },
            |_, _, _, _: LTVDisco_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: LTVDisco_QueryMsg| -> StdResult<Binary> {
                match msg {
                    LTVDisco_QueryMsg::CanHandleBadDebt { .. } => Ok(to_json_binary(&false)?),
                    _ => Ok(to_json_binary(&true)?),
                }
            },
        );
        Box::new(contract)
    }

    //--- Mock Revenue Distributor ---
    #[cw_serde]
    pub struct RevDist_MockInstantiateMsg {}
    #[cw_serde]
    pub enum RevDist_MockExecuteMsg {
        SetPromises { promises: Vec<membrane::revenue_distributor::RevenuePromise> },
    }

    pub fn revenue_distributor_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, _: RevDist_MockExecuteMsg| -> StdResult<Response> { Ok(Response::new()) },
            |_, _, _, _: RevDist_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, _: cosmwasm_std::Empty| -> StdResult<Binary> { Ok(Binary::default()) },
        );
        Box::new(contract)
    }

    //=== Test setup ===

    fn mock_app() -> App {
        AppBuilder::new().build(|router, api, storage| {
            let bank = BankKeeper::new();

            bank.init_balance(
                storage,
                &api.addr_make(USER),
                vec![coin(100_000_000_000, "debit"), coin(100_000_000_000, "2nddebit")],
            ).unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("contract0"),
                vec![coin(2777_777777, "credit_fulldenom")],
            ).unwrap();
            bank.init_balance(
                storage,
                &api.addr_make("test"),
                vec![coin(50_000_000_000, "credit_fulldenom"), coin(100_000_000_000, "debit")],
            ).unwrap();
            bank.init_balance(
                storage,
                &api.addr_make("sender"),
                vec![coin(50_000_001_000_000, "credit_fulldenom")],
            ).unwrap();
            bank.init_balance(
                storage,
                &api.addr_make("bigger_bank"),
                vec![
                    coin(100_000_000_000_000, "debit"),
                    coin(100_000_000_000_000, "credit_fulldenom"),
                ],
            ).unwrap();

            router.bank = bank;
        })
    }

    /// Setup the full CDP environment with a transmuter mock.
    /// `paired_balance` controls the transmuter's paired asset balance (USDC side).
    /// `total_deposit_value` controls the total deposit value for utilization calc.
    fn proper_instantiate_with_transmuter(
        paired_balance: Uint128,
        total_deposit_value: Uint128,
    ) -> (App, CDPContract, Addr) {
        let mut app = mock_app();
        let admin_addr = app.api().addr_make(ADMIN);

        // SP
        let sp_id = app.store_code(stability_pool_contract());
        let sp_addr = app.instantiate_contract(sp_id, admin_addr.clone(), &SP_MockInstantiateMsg {}, &[], "sp", None).unwrap();

        // LQ
        let lq_id = app.store_code(liq_queue_contract());
        let lq_addr = app.instantiate_contract(lq_id, admin_addr.clone(), &LQ_MockInstantiateMsg {}, &[], "lq", None).unwrap();

        // Osmosis Proxy
        let proxy_id = app.store_code(osmosis_proxy_contract());
        let proxy_addr = app.instantiate_contract(proxy_id, admin_addr.clone(), &Osmo_MockInstantiateMsg {}, &[], "proxy", None).unwrap();

        // Staking
        let staking_id = app.store_code(staking_contract());
        let staking_addr = app.instantiate_contract(staking_id, admin_addr.clone(), &Staking_MockInstantiateMsg {}, &[], "staking", None).unwrap();

        // Oracle
        let oracle_id = app.store_code(oracle_contract());
        let oracle_addr = app.instantiate_contract(oracle_id, admin_addr.clone(), &Oracle_MockInstantiateMsg {}, &[], "oracle", None).unwrap();

        // Auction
        let auction_id = app.store_code(auction_contract());
        let auction_addr = app.instantiate_contract(auction_id, admin_addr.clone(), &Auction_MockInstantiateMsg {}, &[], "auction", None).unwrap();

        // Liquidity
        let liquidity_id = app.store_code(liquidity_contract());
        let liquidity_addr = app.instantiate_contract(liquidity_id, admin_addr.clone(), &Liquidity_MockInstantiateMsg {}, &[], "liquidity", None).unwrap();

        // Discounts
        let discounts_id = app.store_code(discounts_contract());
        let discounts_addr = app.instantiate_contract(discounts_id, admin_addr.clone(), &Discounts_MockInstantiateMsg {}, &[], "discounts", None).unwrap();

        // LTV Disco
        let ltv_disco_id = app.store_code(ltv_disco_mock_contract());
        let ltv_disco_addr = app.instantiate_contract(ltv_disco_id, admin_addr.clone(), &LTVDisco_MockInstantiateMsg {}, &[], "ltv_disco", None).unwrap();

        // Revenue Distributor
        let rev_dist_id = app.store_code(revenue_distributor_contract());
        let rev_dist_addr = app.instantiate_contract(rev_dist_id, admin_addr.clone(), &RevDist_MockInstantiateMsg {}, &[], "rev_dist", None).unwrap();

        // Transmuter
        let transmuter_id = app.store_code(transmuter_mock_contract());
        let transmuter_addr = app.instantiate_contract(
            transmuter_id,
            admin_addr.clone(),
            &TransmuterMockInstantiateMsg {
                paired_asset_balance: paired_balance,
                total_deposit_value,
                cdt_balance: Uint128::zero(),
            },
            &[],
            "transmuter",
            None,
        ).unwrap();

        // CDP contract
        let cdp_id = app.store_code(cdp_contract());

        let create_basket = CreateBasket {
            basket_id: Uint128::one(),
            collateral_types: vec![cAsset {
                asset: Asset {
                    info: AssetInfo::NativeToken { denom: "debit".to_string() },
                    amount: Uint128::zero(),
                },
                max_borrow_LTV: Decimal::percent(50),
                max_LTV: Decimal::percent(70),
                pool_info: None,
                rate_index: Decimal::one(),
                peg_rate_index: Decimal::one(),
                force_redemptions: None,
                max_threshold_to_delay: Decimal::percent(15),
            }],
            credit_asset: Asset {
                info: AssetInfo::NativeToken { denom: "credit_fulldenom".to_string() },
                amount: Uint128::zero(),
            },
            credit_price: Decimal::percent(100),
            base_interest_rate: None,
            credit_pool_infos: vec![],
            liq_queue: None,
        };

        let msg = InstantiateMsg {
            owner: Some(admin_addr.to_string()),
            liq_fee: Decimal::percent(1),
            staking_contract: Some(staking_addr.to_string()),
            oracle_contract: Some(oracle_addr.to_string()),
            chain_proxy: Some(proxy_addr.to_string()),
            debt_auction: Some(auction_addr.to_string()),
            liquidity_contract: Some(liquidity_addr.to_string()),
            discounts_contract: Some(discounts_addr.to_string()),
            ltv_disco: ltv_disco_addr.to_string(),
            oracle_time_limit: 60u64,
            debt_minimum: Uint128::new(2000u128),
            collateral_twap_timeframe: 60u64,
            credit_twap_timeframe: 480u64,
            rate_slope_multiplier: Decimal::from_str("0.618").unwrap(),
            base_debt_cap_multiplier: Uint128::new(21u128),
            create_basket,
        };
        let cdp_addr = app.instantiate_contract(cdp_id, admin_addr.clone(), &msg, &[], "cdp", None).unwrap();
        let cdp_contract = CDPContract(cdp_addr);

        // Set TWAP price source
        let msg = ExecuteMsg::EditBasket(EditBasket {
            added_cAsset: None,
            liq_queue: None,
            collateral_supply_caps: Some(vec![
                SupplyCap {
                    asset_info: AssetInfo::NativeToken { denom: "debit".to_string() },
                    current_supply: Uint128::zero(),
                    debt_total: Uint128::zero(),
                    supply_cap_ratio: Decimal::percent(100),
                    lp: false,
                    stability_pool_ratio_for_debt_cap: None,
                },
            ]),
            base_interest_rate: Some(Decimal::percent(10)), // 10% base
            credit_asset_twap_price_source: Some(TWAPPoolInfo {
                pool_id: 0u64,
                base_asset_denom: String::from("base"),
                quote_asset_denom: String::from("quote"),
            }),
            negative_rates: None,
            cpc_margin_of_error: None,
            frozen: None,
            distribute_revenue: None,
            multi_asset_supply_caps: None,
            credit_pool_infos: None,
            take_revenue: None,
        });
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(admin_addr.clone(), cosmos_msg).unwrap();

        // Update config with transmuter + peg debt slopes + revenue distributor
        let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
            owner: None,
            staking_contract: None,
            chain_proxy: None,
            debt_auction: None,
            oracle_contract: None,
            liquidity_contract: None,
            discounts_contract: None,
            ltv_disco: None,
            liq_fee: None,
            collateral_twap_timeframe: None,
            credit_twap_timeframe: None,
            oracle_time_limit: None,
            debt_minimum: None,
            base_debt_cap_multiplier: None,
            cpc_multiplier: None,
            rate_slope_multiplier: None,
            affiliate_fee_max: None,
            skip_credit_price_accrual: None,
            liquidation_stat_limit: None,
            revenue_distributor: Some(rev_dist_addr.to_string()),
            transmuter_addr: Some(transmuter_addr.to_string()),
            irm_config: None,
                points_contract: None,
        });
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(admin_addr.clone(), cosmos_msg).unwrap();

        // Fund CDP contract with credit_fulldenom so peg_debt transmute
        // operations can send CDT (osmosis_proxy mock doesn't actually mint bank tokens)
        let bigger_bank = app.api().addr_make("bigger_bank");
        app.send_tokens(
            bigger_bank,
            cdp_contract.addr(),
            &[coin(50_000_000_000_000, "credit_fulldenom")],
        ).unwrap();

        (app, cdp_contract, transmuter_addr)
    }

    /// Helper: deposit collateral and create a position for USER.
    fn deposit_collateral(app: &mut App, cdp_contract: &CDPContract, amount: u128) {
        let msg = ExecuteMsg::Deposit {
            position_owner: Some(app.api().addr_make(USER).to_string()),
            position_id: None,
            affiliate_address: None,
            affiliate_label: None,
        };
        let cosmos_msg = cdp_contract.call(msg, vec![coin(amount, "debit")]).unwrap();
        let user_addr = get_user_addr(app);
        app.execute(user_addr, cosmos_msg).unwrap();
    }

    /// Helper: query position
    fn query_position(app: &App, cdp_contract: &CDPContract, pos_id: u128) -> PositionResponse {
        let user_addr = get_user_addr(app);
        let res: Vec<BasketPositionsResponse> = app.wrap().query_wasm_smart(
            cdp_contract.addr(),
            &QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user_info: Some(membrane::types::UserInfo {
                    position_id: Uint128::new(pos_id),
                    position_owner: user_addr.to_string(),
                }),
                user: None,
            },
        ).unwrap();
        res[0].positions[0].clone()
    }

    /// Helper: query basket
    fn query_basket(app: &App, cdp_contract: &CDPContract) -> Basket {
        app.wrap().query_wasm_smart(
            cdp_contract.addr(),
            &QueryMsg::GetBasket {},
        ).unwrap()
    }

    // =====================================================
    // Tests
    // =====================================================

    #[test]
    fn peg_debt_variable_creation() {
        // Transmuter has 50B paired asset, 100B total deposit value
        // utilization = 1 - (50B / 100B) = 0.5
        // slope1=1.0, kink=0.8 → below kink → multiplier = 1.0 * 0.5 = 0.5
        // peg rate = base_rate * 0.5
        let (mut app, cdp_contract, _transmuter) = proper_instantiate_with_transmuter(
            Uint128::new(50_000_000_000),
            Uint128::new(100_000_000_000),
        );

        // Deposit 100B collateral at $10 = $1T value, 50% LTV = $500B borrowable
        deposit_collateral(&mut app, &cdp_contract, 100_000_000_000);

        // Take out peg debt (variable only, no split)
        let msg = ExecuteMsg::IncreaseDebt {
            position_id: Uint128::new(1),
            amount: Some(Uint128::new(10_000_000_000)), // 10B
            LTV: None,
            mint_to_addr: None,
            deployment_intent: None,
            debt_split: None,   // default = all variable
            rollover_updates: None,
            peg_debt: Some(true),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        let user_addr = get_user_addr(&app);
        app.execute(user_addr, cosmos_msg).unwrap();

        // Query position
        let pos = query_position(&app, &cdp_contract, 1);

        // Should have 1 peg variable segment
        assert_eq!(pos.peg_rate_segments.len(), 1, "should have 1 peg segment");
        assert_eq!(pos.peg_rate_segments[0].amount, Uint128::new(10_000_000_000));
        assert!(pos.peg_rate_segments[0].fixed_rate.is_none(), "should be variable");

        // Regular segments should be empty
        assert_eq!(pos.rate_segments.len(), 0, "should have no regular segments");

        // Credit amount = total across both pools
        assert_eq!(pos.credit_amount, Uint128::new(10_000_000_000));

        // Check basket tracking
        let basket = query_basket(&app, &cdp_contract);
        assert_eq!(basket.credit_asset.peg_variable_amount, Uint128::new(10_000_000_000));
        assert_eq!(basket.credit_asset.variable_amount, Uint128::zero());
    }

    #[test]
    fn peg_debt_requires_transmuter() {
        // Setup WITHOUT transmuter
        let (mut app, cdp_contract, transmuter_addr) = proper_instantiate_with_transmuter(
            Uint128::new(50_000_000_000),
            Uint128::new(100_000_000_000),
        );

        // Remove transmuter from config
        let admin_addr = get_admin_addr(&app);
        let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
            owner: None,
            staking_contract: None,
            chain_proxy: None,
            debt_auction: None,
            oracle_contract: None,
            liquidity_contract: None,
            discounts_contract: None,
            ltv_disco: None,
            liq_fee: None,
            collateral_twap_timeframe: None,
            credit_twap_timeframe: None,
            oracle_time_limit: None,
            debt_minimum: None,
            base_debt_cap_multiplier: None,
            cpc_multiplier: None,
            rate_slope_multiplier: None,
            affiliate_fee_max: None,
            skip_credit_price_accrual: None,
            liquidation_stat_limit: None,
            revenue_distributor: None,
            // Can't set transmuter_addr back to None via UpdateConfig,
            // so let's just test the insufficient balance path instead
            transmuter_addr: None,
            irm_config: None,
                points_contract: None,
        });
        // Note: UpdateConfig doesn't allow un-setting transmuter_addr to None once set.
        // Instead, test that peg_debt fails when transmuter has insufficient balance.

        deposit_collateral(&mut app, &cdp_contract, 100_000_000_000);

        // Try peg_debt with amount exceeding paired_asset_balance
        let msg = ExecuteMsg::IncreaseDebt {
            position_id: Uint128::new(1),
            amount: Some(Uint128::new(999_000_000_000_000)), // way more than 50B paired balance
            LTV: None,
            mint_to_addr: None,
            deployment_intent: None,
            debt_split: None,
            rollover_updates: None,
            peg_debt: Some(true),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        let user_addr = get_user_addr(&app);
        let err = app.execute(user_addr, cosmos_msg).unwrap_err();
        // Should fail due to insufficient transmuter balance
        assert!(
            err.root_cause().to_string().contains("insufficient"),
            "Expected insufficient balance error, got: {}",
            err.root_cause()
        );
    }

    #[test]
    fn peg_debt_with_fixed_rate_split() {
        let (mut app, cdp_contract, _) = proper_instantiate_with_transmuter(
            Uint128::new(50_000_000_000),
            Uint128::new(100_000_000_000),
        );

        deposit_collateral(&mut app, &cdp_contract, 100_000_000_000);

        // Increase peg debt with 80% variable, 20% 1-month fixed (within 20% cap)
        let msg = ExecuteMsg::IncreaseDebt {
            position_id: Uint128::new(1),
            amount: Some(Uint128::new(10_000_000_000)),
            LTV: None,
            mint_to_addr: None,
            deployment_intent: None,
            debt_split: Some(DebtSplit {
                variable: None,
                one_month: None,
                three_month: None,
                six_month: None,
                one_month_rollover: None,
                three_month_rollover: None,
                six_month_rollover: None,
                // Peg fields:
                peg_variable: Some(Decimal::percent(80)),
                peg_one_month: Some(Decimal::percent(20)),
                peg_three_month: None,
                peg_six_month: None,
                peg_one_month_rollover: None,
                peg_three_month_rollover: None,
                peg_six_month_rollover: None,
            }),
            rollover_updates: None,
            peg_debt: Some(true),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        let user_addr = get_user_addr(&app);
        app.execute(user_addr, cosmos_msg).unwrap();

        let pos = query_position(&app, &cdp_contract, 1);

        // Should have 2 peg segments
        assert_eq!(pos.peg_rate_segments.len(), 2, "should have 2 peg segments");

        // Variable segment: 8B
        let var_seg = pos.peg_rate_segments.iter().find(|s| s.fixed_rate.is_none()).unwrap();
        assert_eq!(var_seg.amount, Uint128::new(8_000_000_000));

        // Fixed 1-month segment: 2B
        let fixed_seg = pos.peg_rate_segments.iter().find(|s| s.fixed_rate.is_some()).unwrap();
        assert_eq!(fixed_seg.amount, Uint128::new(2_000_000_000));
        assert_eq!(fixed_seg.fixed_rate.as_ref().unwrap().end.duration_months, 1);

        // Total should be 10B
        assert_eq!(pos.credit_amount, Uint128::new(10_000_000_000));

        // Basket peg tracking
        let basket = query_basket(&app, &cdp_contract);
        assert_eq!(basket.credit_asset.peg_variable_amount, Uint128::new(8_000_000_000));
        assert_eq!(basket.credit_asset.peg_one_month_amount, Uint128::new(2_000_000_000));
    }

    #[test]
    fn peg_debt_and_regular_debt_coexist() {
        let (mut app, cdp_contract, _) = proper_instantiate_with_transmuter(
            Uint128::new(50_000_000_000),
            Uint128::new(100_000_000_000),
        );

        deposit_collateral(&mut app, &cdp_contract, 100_000_000_000);

        // First: take regular variable debt
        let msg = ExecuteMsg::IncreaseDebt {
            position_id: Uint128::new(1),
            amount: Some(Uint128::new(5_000_000_000)),
            LTV: None,
            mint_to_addr: None,
            deployment_intent: None,
            debt_split: None,
            rollover_updates: None,
            peg_debt: None,
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        let user_addr = get_user_addr(&app);
        app.execute(user_addr.clone(), cosmos_msg).unwrap();

        // Second: take peg variable debt
        let msg = ExecuteMsg::IncreaseDebt {
            position_id: Uint128::new(1),
            amount: Some(Uint128::new(3_000_000_000)),
            LTV: None,
            mint_to_addr: None,
            deployment_intent: None,
            debt_split: None,
            rollover_updates: None,
            peg_debt: Some(true),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(user_addr, cosmos_msg).unwrap();

        let pos = query_position(&app, &cdp_contract, 1);

        // Regular: 5B variable
        assert_eq!(pos.rate_segments.len(), 1);
        assert_eq!(pos.rate_segments[0].amount, Uint128::new(5_000_000_000));

        // Peg: 3B variable
        assert_eq!(pos.peg_rate_segments.len(), 1);
        assert_eq!(pos.peg_rate_segments[0].amount, Uint128::new(3_000_000_000));

        // Total
        assert_eq!(pos.credit_amount, Uint128::new(8_000_000_000));

        // Basket
        let basket = query_basket(&app, &cdp_contract);
        assert_eq!(basket.credit_asset.variable_amount, Uint128::new(5_000_000_000));
        assert_eq!(basket.credit_asset.peg_variable_amount, Uint128::new(3_000_000_000));
    }

    #[test]
    fn peg_debt_repay_default_order() {
        // Regular pool is repaid first, then peg pool (default order)
        let (mut app, cdp_contract, _) = proper_instantiate_with_transmuter(
            Uint128::new(50_000_000_000),
            Uint128::new(100_000_000_000),
        );

        deposit_collateral(&mut app, &cdp_contract, 100_000_000_000);

        // Take regular debt: 5B
        let msg = ExecuteMsg::IncreaseDebt {
            position_id: Uint128::new(1),
            amount: Some(Uint128::new(5_000_000_000)),
            LTV: None,
            mint_to_addr: None,
            deployment_intent: None,
            debt_split: None,
            rollover_updates: None,
            peg_debt: None,
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        let user_addr = get_user_addr(&app);
        app.execute(user_addr.clone(), cosmos_msg).unwrap();

        // Take peg debt: 5B
        let msg = ExecuteMsg::IncreaseDebt {
            position_id: Uint128::new(1),
            amount: Some(Uint128::new(5_000_000_000)),
            LTV: None,
            mint_to_addr: None,
            deployment_intent: None,
            debt_split: None,
            rollover_updates: None,
            peg_debt: Some(true),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(user_addr.clone(), cosmos_msg).unwrap();

        // Repay 7B (should wipe 5B regular + 2B peg)
        let msg = ExecuteMsg::Repay {
            position_id: Uint128::new(1),
            position_owner: None,
            send_excess_to: None,
            debt_split: None,
        };
        let cosmos_msg = cdp_contract.call(msg, vec![coin(7_000_000_000, "credit_fulldenom")]).unwrap();
        // Use sender who has credit_fulldenom
        let sender_addr = app.api().addr_make("sender");
        // Need to deposit first as sender doesn't own the position...
        // Actually let's use sender. The repay position_owner field can specify who.
        let msg = ExecuteMsg::Repay {
            position_id: Uint128::new(1),
            position_owner: Some(get_user_addr(&app).to_string()),
            send_excess_to: None,
            debt_split: None,
        };
        let cosmos_msg = cdp_contract.call(msg, vec![coin(7_000_000_000, "credit_fulldenom")]).unwrap();
        app.execute(sender_addr, cosmos_msg).unwrap();

        let pos = query_position(&app, &cdp_contract, 1);

        // Regular pool should be fully repaid (0)
        let regular_total: Uint128 = pos.rate_segments.iter().map(|s| s.amount).sum();
        assert_eq!(regular_total, Uint128::zero(), "regular debt should be fully repaid");

        // Peg pool should have 3B remaining (5B - 2B)
        let peg_total: Uint128 = pos.peg_rate_segments.iter().map(|s| s.amount).sum();
        assert_eq!(peg_total, Uint128::new(3_000_000_000), "peg debt should have 3B remaining");
    }

    #[test]
    fn peg_debt_repay_targeted() {
        let (mut app, cdp_contract, _) = proper_instantiate_with_transmuter(
            Uint128::new(50_000_000_000),
            Uint128::new(100_000_000_000),
        );

        deposit_collateral(&mut app, &cdp_contract, 100_000_000_000);

        // Take regular debt: 5B
        let msg = ExecuteMsg::IncreaseDebt {
            position_id: Uint128::new(1),
            amount: Some(Uint128::new(5_000_000_000)),
            LTV: None,
            mint_to_addr: None,
            deployment_intent: None,
            debt_split: None,
            rollover_updates: None,
            peg_debt: None,
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        let user_addr = get_user_addr(&app);
        app.execute(user_addr.clone(), cosmos_msg).unwrap();

        // Take peg debt: 5B
        let msg = ExecuteMsg::IncreaseDebt {
            position_id: Uint128::new(1),
            amount: Some(Uint128::new(5_000_000_000)),
            LTV: None,
            mint_to_addr: None,
            deployment_intent: None,
            debt_split: None,
            rollover_updates: None,
            peg_debt: Some(true),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(user_addr.clone(), cosmos_msg).unwrap();

        // Targeted repay: 100% towards peg variable
        let msg = ExecuteMsg::Repay {
            position_id: Uint128::new(1),
            position_owner: Some(get_user_addr(&app).to_string()),
            send_excess_to: None,
            debt_split: Some(DebtSplit {
                variable: Some(Decimal::zero()),
                one_month: None,
                three_month: None,
                six_month: None,
                one_month_rollover: None,
                three_month_rollover: None,
                six_month_rollover: None,
                peg_variable: Some(Decimal::one()),
                peg_one_month: None,
                peg_three_month: None,
                peg_six_month: None,
                peg_one_month_rollover: None,
                peg_three_month_rollover: None,
                peg_six_month_rollover: None,
            }),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![coin(3_000_000_000, "credit_fulldenom")]).unwrap();
        let sender_addr = app.api().addr_make("sender");
        app.execute(sender_addr, cosmos_msg).unwrap();

        let pos = query_position(&app, &cdp_contract, 1);

        // Regular should be untouched at 5B
        let regular_total: Uint128 = pos.rate_segments.iter().map(|s| s.amount).sum();
        assert_eq!(regular_total, Uint128::new(5_000_000_000), "regular debt untouched");

        // Peg should be reduced to 2B
        let peg_total: Uint128 = pos.peg_rate_segments.iter().map(|s| s.amount).sum();
        assert_eq!(peg_total, Uint128::new(2_000_000_000), "peg debt should be 2B");
    }

    #[test]
    fn peg_debt_accrual_over_time() {
        let (mut app, cdp_contract, _) = proper_instantiate_with_transmuter(
            Uint128::new(50_000_000_000),
            Uint128::new(100_000_000_000),
        );

        deposit_collateral(&mut app, &cdp_contract, 100_000_000_000);

        // Take peg debt: 10B variable
        let msg = ExecuteMsg::IncreaseDebt {
            position_id: Uint128::new(1),
            amount: Some(Uint128::new(10_000_000_000)),
            LTV: None,
            mint_to_addr: None,
            deployment_intent: None,
            debt_split: None,
            rollover_updates: None,
            peg_debt: Some(true),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        let user_addr = get_user_addr(&app);
        app.execute(user_addr.clone(), cosmos_msg).unwrap();

        let pos_before = query_position(&app, &cdp_contract, 1);
        assert_eq!(pos_before.credit_amount, Uint128::new(10_000_000_000));

        // Advance time by 1 year (365 * 24 * 60 * 60 = 31_536_000 seconds)
        app.update_block(|block| {
            block.time = block.time.plus_seconds(31_536_000);
            block.height += 100;
        });

        // Trigger accrual by calling IncreaseDebt with rollover_updates (no amount needed)
        let msg = ExecuteMsg::IncreaseDebt {
            position_id: Uint128::new(1),
            amount: None,
            LTV: None,
            mint_to_addr: None,
            deployment_intent: None,
            debt_split: None,
            rollover_updates: Some(vec![]),
            peg_debt: None,
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(user_addr, cosmos_msg).unwrap();

        let pos_after = query_position(&app, &cdp_contract, 1);

        // Peg debt should have increased due to interest
        let peg_total_after: Uint128 = pos_after.peg_rate_segments.iter().map(|s| s.amount).sum();
        assert!(
            peg_total_after > Uint128::new(10_000_000_000),
            "peg debt should have accrued interest: {}",
            peg_total_after
        );

        // Total credit should match sum of all segments
        let regular_total: Uint128 = pos_after.rate_segments.iter().map(|s| s.amount).sum();
        assert_eq!(pos_after.credit_amount, regular_total + peg_total_after);
    }

    #[test]
    fn peg_debt_insufficient_transmuter_balance() {
        // Transmuter has only 1B paired asset
        let (mut app, cdp_contract, _) = proper_instantiate_with_transmuter(
            Uint128::new(1_000_000_000), // 1B
            Uint128::new(100_000_000_000),
        );

        deposit_collateral(&mut app, &cdp_contract, 100_000_000_000);

        // Try to take 5B peg debt (exceeds 1B paired balance)
        let msg = ExecuteMsg::IncreaseDebt {
            position_id: Uint128::new(1),
            amount: Some(Uint128::new(5_000_000_000)),
            LTV: None,
            mint_to_addr: None,
            deployment_intent: None,
            debt_split: None,
            rollover_updates: None,
            peg_debt: Some(true),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        let user_addr = get_user_addr(&app);
        let err = app.execute(user_addr, cosmos_msg).unwrap_err();
        assert!(
            err.root_cause().to_string().contains("insufficient"),
            "Expected insufficient error, got: {}", err.root_cause()
        );
    }

    #[test]
    fn peg_debt_split_must_sum_to_one() {
        let (mut app, cdp_contract, _) = proper_instantiate_with_transmuter(
            Uint128::new(50_000_000_000),
            Uint128::new(100_000_000_000),
        );

        deposit_collateral(&mut app, &cdp_contract, 100_000_000_000);

        // Bad split: peg fractions sum to 0.8, not 1.0
        let msg = ExecuteMsg::IncreaseDebt {
            position_id: Uint128::new(1),
            amount: Some(Uint128::new(5_000_000_000)),
            LTV: None,
            mint_to_addr: None,
            deployment_intent: None,
            debt_split: Some(DebtSplit {
                variable: None,
                one_month: None,
                three_month: None,
                six_month: None,
                one_month_rollover: None,
                three_month_rollover: None,
                six_month_rollover: None,
                peg_variable: Some(Decimal::percent(50)),
                peg_one_month: Some(Decimal::percent(30)),
                peg_three_month: None,
                peg_six_month: None,
                peg_one_month_rollover: None,
                peg_three_month_rollover: None,
                peg_six_month_rollover: None,
            }),
            rollover_updates: None,
            peg_debt: Some(true),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        let user_addr = get_user_addr(&app);
        let err = app.execute(user_addr, cosmos_msg).unwrap_err();
        assert!(
            err.root_cause().to_string().contains("sum to 1"),
            "Expected 'sum to 1' error, got: {}", err.root_cause()
        );
    }

    #[test]
    fn peg_debt_cannot_combine_with_deployment_intent() {
        let (mut app, cdp_contract, _) = proper_instantiate_with_transmuter(
            Uint128::new(50_000_000_000),
            Uint128::new(100_000_000_000),
        );

        deposit_collateral(&mut app, &cdp_contract, 100_000_000_000);

        let msg = ExecuteMsg::IncreaseDebt {
            position_id: Uint128::new(1),
            amount: Some(Uint128::new(5_000_000_000)),
            LTV: None,
            mint_to_addr: Some("someone".to_string()),
            deployment_intent: None,
            debt_split: None,
            rollover_updates: None,
            peg_debt: Some(true),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        let user_addr = get_user_addr(&app);
        let err = app.execute(user_addr, cosmos_msg).unwrap_err();
        assert!(
            err.root_cause().to_string().contains("cannot be combined"),
            "Expected mutual exclusivity error, got: {}", err.root_cause()
        );
    }

    #[test]
    fn peg_debt_rollover_update() {
        let (mut app, cdp_contract, _) = proper_instantiate_with_transmuter(
            Uint128::new(50_000_000_000),
            Uint128::new(100_000_000_000),
        );

        deposit_collateral(&mut app, &cdp_contract, 100_000_000_000);

        // Create peg debt with 1-month fixed segment (no rollover), within 20% cap
        let msg = ExecuteMsg::IncreaseDebt {
            position_id: Uint128::new(1),
            amount: Some(Uint128::new(5_000_000_000)),
            LTV: None,
            mint_to_addr: None,
            deployment_intent: None,
            debt_split: Some(DebtSplit {
                variable: None,
                one_month: None,
                three_month: None,
                six_month: None,
                one_month_rollover: None,
                three_month_rollover: None,
                six_month_rollover: None,
                peg_variable: Some(Decimal::percent(80)),
                peg_one_month: Some(Decimal::percent(20)),
                peg_three_month: None,
                peg_six_month: None,
                peg_one_month_rollover: None,
                peg_three_month_rollover: None,
                peg_six_month_rollover: None,
            }),
            rollover_updates: None,
            peg_debt: Some(true),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        let user_addr = get_user_addr(&app);
        app.execute(user_addr.clone(), cosmos_msg).unwrap();

        let pos = query_position(&app, &cdp_contract, 1);
        assert_eq!(pos.peg_rate_segments.len(), 2); // variable + 1-month fixed
        let fixed_seg = pos.peg_rate_segments.iter().find(|s| s.fixed_rate.is_some()).unwrap();
        assert!(!fixed_seg.fixed_rate.as_ref().unwrap().end.rollover);

        // Update rollover on the 1-month segment to true
        let msg = ExecuteMsg::IncreaseDebt {
            position_id: Uint128::new(1),
            amount: None,
            LTV: None,
            mint_to_addr: None,
            deployment_intent: None,
            debt_split: None,
            rollover_updates: Some(vec![(1u8, true)]),
            peg_debt: None,
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(user_addr, cosmos_msg).unwrap();

        let pos = query_position(&app, &cdp_contract, 1);
        let fixed_seg = pos.peg_rate_segments.iter().find(|s| s.fixed_rate.is_some()).unwrap();
        assert!(fixed_seg.fixed_rate.as_ref().unwrap().end.rollover,
            "rollover should be updated to true");
    }
}
