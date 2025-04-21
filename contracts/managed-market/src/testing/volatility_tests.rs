
mod tests {

    use std::str::FromStr;

    use crate::helpers::{CDPContract, LQContract, OracleContract};


    use membrane::liq_queue::LiquidatibleResponse as LQ_LiquidatibleResponse;
    use membrane::math::Uint256;
    use membrane::oracle::{AssetResponse, PriceResponse};
    use membrane::osmosis_proxy::{GetDenomResponse, TokenInfoResponse, OwnerResponse};
    use membrane::cdp::{ExecuteMsg, InstantiateMsg, QueryMsg, EditBasket, UpdateConfig, CreateBasket};
    use membrane::stability_pool::LiquidatibleResponse as SP_LiquidatibleResponse;
    use membrane::staking::Config as Staking_Config;
    use membrane::types::{
        cAsset, Asset, AssetInfo, AssetOracleInfo, AssetPool, DebtCap, Deposit, LiquidityInfo, MultiAssetSupplyCap, Owner, PoolStateResponse, PoolType, StakeDistribution, SupplyCap, TWAPPoolInfo, UserInfo
    };
    use membrane::liquidity_check::LiquidityResponse;

    use cosmwasm_std::{
        attr, coin, to_binary, Addr, Binary, Coin, Decimal, Empty, Response, StdError, StdResult,
        Uint128,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    use cosmwasm_schema::cw_serde;

    const USER: &str = "user";
    const ADMIN: &str = "admin";

    //CDP Contract
    pub fn cdp_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contract::execute,
            crate::contract::instantiate,
            crate::contract::query,
        )
        .with_reply(crate::contract::reply);
        Box::new(contract)
    }

    //Mock LQ Contract
    #[cw_serde]
    pub enum LQ_MockExecuteMsg {
        Liquidate {
            credit_price: PriceResponse,     //Sent from Position's contract
            collateral_price: PriceResponse, //Sent from Position's contract
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
                    LQ_MockExecuteMsg::Liquidate {
                        credit_price: _,
                        collateral_price: _,
                        collateral_amount,
                        bid_for,
                    } => if collateral_amount.to_string() == String::from("1165777777777778") {
                        Ok(Response::new().add_attributes(vec![
                            attr("action", "execute_bid"),
                            attr("repay_amount", "1158".to_string()),
                            attr("collateral_token", bid_for.to_string()),
                            attr("collateral_info", "native_token"),
                            attr("collateral_amount", collateral_amount),
                        ]))
                    } else if collateral_amount.to_string() == String::from("1166666666666666") {
                        Ok(Response::new().add_attributes(vec![
                            attr("action", "execute_bid"),
                            attr("repay_amount", 1166.to_string()),
                            attr("collateral_token", bid_for.to_string()),
                            attr("collateral_info", "native_token"),
                            attr("collateral_amount", collateral_amount),
                        ]))
                    } else if collateral_amount.to_string() == String::from("1054777777777778") {
                        Ok(Response::new().add_attributes(vec![
                            attr("action", "execute_bid"),
                            attr("repay_amount", 1054.to_string()),
                            attr("collateral_token", bid_for.to_string()),
                            attr("collateral_info", "native_token"),
                            attr("collateral_amount", collateral_amount),
                        ]))
                    //Liquidate_LPs()
                    } else if collateral_amount.to_string() == String::from("1277666666388888888889") {
                        Ok(Response::new().add_attributes(vec![
                            attr("action", "execute_bid"),
                            attr("repay_amount", 2555555555u128.to_string()),
                            attr("collateral_token", bid_for.to_string()),
                            attr("collateral_info", "native_token"),
                            attr("collateral_amount", collateral_amount),
                        ]))
                    } else if collateral_amount.to_string() == String::from("1277777777388888888889") {
                        Ok(Response::new().add_attributes(vec![
                            attr("action", "execute_bid"),
                            attr("repay_amount", 2555555555u128.to_string()),
                            attr("collateral_token", bid_for.to_string()),
                            attr("collateral_info", "native_token"),
                            attr("collateral_amount", collateral_amount),
                        ]))
                    } else if collateral_amount.to_string() == String::from("1166666666388888889") {
                        Ok(Response::new().add_attributes(vec![
                            attr("action", "execute_bid"),
                            attr("repay_amount", 2555555555u128.to_string()),
                            attr("collateral_token", bid_for.to_string()),
                            attr("collateral_info", "native_token"),
                            attr("collateral_amount", collateral_amount),
                        ]))
                    } else {
                        // panic!("{}", collateral_amount);
                       Ok(Response::new().add_attributes(vec![
                            attr("action", "execute_bid"),
                            attr("repay_amount", collateral_amount.to_string()),
                            attr("collateral_token", bid_for.to_string()),
                            attr("collateral_info", "native_token"),
                            attr("collateral_amount", collateral_amount),
                        ]))
                    },
                    LQ_MockExecuteMsg::AddQueue {
                        bid_for: _,
                        max_premium: _,
                        bid_threshold: _,
                    } => Ok(Response::new()),
                    LQ_MockExecuteMsg::EditQueue {
                        bid_for,
                        max_premium,
                        bid_threshold,
                    } => Ok(Response::new()),
                    LQ_MockExecuteMsg::UpdateQueue {
                        bid_for,
                        max_premium,
                        bid_threshold,
                    } => {
                        let premium = max_premium.unwrap_or_default();
                        if premium != Uint128::new(10) && premium != Uint128::new(12) {
                            panic!("{}", premium);
                        }
                        Ok(Response::new())
                    },
                }
            },
            |_, _, _, _: LQ_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: LQ_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    LQ_MockQueryMsg::CheckLiquidatible {
                        bid_for: _,
                        collateral_price: _,
                        collateral_amount,
                        credit_info: _,
                        credit_price: _,
                    } => 
                    if collateral_amount.to_string() == String::from("1387999999999778") {
                        Ok(to_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "222222222222222".to_string(),
                            total_debt_repaid: (Uint256::from(1387u128) - Uint256::from(222u128))
                                .to_string(),
                        })?)
                    } else if collateral_amount.to_string() == String::from("1388000000000000"){
                        Ok(to_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "222222222222222".to_string(),
                            total_debt_repaid: (Uint256::from(1380u128) - Uint256::from(222u128))
                                .to_string(),
                        })?)
                    } else if collateral_amount.to_string() == String::from("1277000000000000"){
                        Ok(to_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "222222222222222".to_string(),
                            total_debt_repaid: (Uint256::from(1277u128) - Uint256::from(222u128))
                                .to_string(),
                        })?)
                    } else if collateral_amount.to_string() == String::from("1062000000000000"){
                        Ok(to_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "222222222222222".to_string(),
                            total_debt_repaid: (Uint256::from(1062u128) - Uint256::from(222u128))
                                .to_string(),
                        })?)
                    } else if collateral_amount.to_string() == String::from("1388888888888888"){
                        Ok(to_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "222222222222222".to_string(),
                            total_debt_repaid: (Uint256::from(1388u128) - Uint256::from(222u128))
                                .to_string(),
                        })?)
                    //liquidite()
                    } else if collateral_amount.to_string() == String::from("2222222222"){
                        Ok(to_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "222222222".to_string(),
                            total_debt_repaid: (Uint256::from(2222222222u128) - Uint256::from(222222222u128))
                                .to_string(),
                        })?)
                    } else if collateral_amount.to_string() == String::from("2000000000"){
                        Ok(to_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "0".to_string(),
                            total_debt_repaid: (Uint256::from(2222222222u128) - Uint256::from(222222222u128))
                                .to_string(),
                        })?)
                    } else if collateral_amount.to_string() == String::from("1250000000"){
                        Ok(to_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "0".to_string(),
                            total_debt_repaid: (Uint256::from(1250000000u128))
                                .to_string(),
                        })?)
                    //liquidate_LPs()
                    } else if collateral_amount.to_string() == String::from("1388888888500000000"){
                        Ok(to_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "111111111111111111".to_string(),
                            total_debt_repaid: (Uint256::from(2777_777777u128) - Uint256::from(222_222222u128))
                            .to_string(),
                        })?)
                    } else if collateral_amount.to_string() == String::from("1388888888500000000000"){
                        Ok(to_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "111111111111111111111".to_string(),
                            total_debt_repaid: (Uint256::from(2777_777777u128) - Uint256::from(222_222222u128))
                            .to_string(),
                        })?)
                    } else if collateral_amount.to_string() == String::from("1277777777500000000000"){
                        Ok(to_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "111111111111111111".to_string(),
                            total_debt_repaid: (Uint256::from(2777_777777u128) - Uint256::from(222_222222u128))
                            .to_string(),
                        })?)
                    } else {                        
                        // panic!("{}", collateral_amount.to_string());
                        Ok(to_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "222".to_string(),
                            total_debt_repaid: (collateral_amount - Uint256::from(222u128))
                                .to_string(),
                        })?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    //Mock SP Contract    
    #[cw_serde]
    pub enum SP_MockExecuteMsg {
        Liquidate { liq_amount: Decimal },
        Distribute {
            distribution_assets: Vec<Asset>,
            distribution_asset_ratios: Vec<Decimal>,
            distribute_for: Uint128,
        },
        Repay {
            user_info: UserInfo,
            repayment: Asset,
        },
    }

    
    #[cw_serde]
    pub struct SP_MockInstantiateMsg {}

    
    #[cw_serde]
    pub enum SP_MockQueryMsg {
        CheckLiquidatible { amount: Decimal },
        AssetPool { 
            /// User address
            user: Option<String>,
            /// Deposit limit
            deposit_limit: Option<u32>,
            /// Deposit to start after
            start_after: Option<u32>,    
        },
    }

    pub fn stability_pool_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: SP_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    SP_MockExecuteMsg::Liquidate { liq_amount } => {
                        if liq_amount.to_string() != "222.222225".to_string()
                            && liq_amount.to_string() != "222.22225".to_string()
                            && liq_amount.to_string() != "222.88888".to_string()
                            && liq_amount.to_string() != "2000".to_string()
                            && liq_amount.to_string() != "222.2222".to_string()
                            && liq_amount.to_string() != "222.22222".to_string()
                            && liq_amount.to_string()
                                != "22222.22225".to_string()
                            && liq_amount.to_string()
                                != "20222.22225".to_string()
                            && liq_amount.to_string() != "22000".to_string()
                            && liq_amount.to_string() != "222.777774844".to_string()
                        {
                            // panic!("{}", liq_amount.to_string());
                        }

                        Ok(Response::new()
                            .add_attribute("method", "liquidate")
                            .add_attribute("leftover_repayment", "0"))
                    }
                    SP_MockExecuteMsg::Distribute {
                        distribution_assets,
                        distribution_asset_ratios: _,
                        distribute_for: _,
                    } => {
                        if distribution_assets
                            != vec![Asset {
                                info: AssetInfo::NativeToken {
                                    denom: "debit".to_string(),
                                },
                                amount: Uint128::new(244),
                            }]
                            && distribution_assets
                                != vec![Asset {
                                    info: AssetInfo::NativeToken {
                                        denom: "debit".to_string(),
                                    },
                                    amount: Uint128::new(2447),
                                }]
                            && distribution_assets
                                != vec![Asset {
                                    info: AssetInfo::NativeToken {
                                        denom: "debit".to_string(),
                                    },
                                    amount: Uint128::new(55000),
                                }]
                            && distribution_assets
                            != vec![Asset {
                                info: AssetInfo::NativeToken {
                                    denom: "lp_denom".to_string(),
                                },
                                amount: Uint128::new(244),
                            }]
                            && distribution_assets
                                != vec![Asset {
                                    info: AssetInfo::NativeToken {
                                        denom: "lp_denom".to_string(),
                                    },
                                    amount: Uint128::new(2447),
                                }]
                            && distribution_assets
                                != vec![Asset {
                                    info: AssetInfo::NativeToken {
                                        denom: "lp_denom".to_string(),
                                    },
                                    amount: Uint128::new(55000),
                                }]
                        {
                            //assert_ne!(distribution_assets, distribution_assets);
                        }

                        Ok(Response::new()
                            .add_attribute("method", "distribute")
                            .add_attribute("credit_asset", "cdt"))
                    }
                    SP_MockExecuteMsg::Repay {
                        user_info: _,
                        repayment: _,
                    } => Ok(Response::new()),
                }
            },
            |_, _, _, _: SP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: SP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    SP_MockQueryMsg::CheckLiquidatible {amount: _ } => {
                        Ok(to_binary(&SP_LiquidatibleResponse {
                            leftover: Decimal::zero(),
                        })?)
                    }
                    SP_MockQueryMsg::AssetPool { user: _, deposit_limit: _, start_after: _ } => Ok(to_binary(&AssetPool {
                        credit_asset: Asset {
                            info: AssetInfo::NativeToken {
                                denom: "cdt".to_string(),
                            },
                            amount: Uint128::new(50_000),
                        },
                        liq_premium: Decimal::percent(10),
                        deposits: vec![],
                    })?),
                }
            },
        );
        Box::new(contract)
    }

    //Mock Osmo Proxy Contract    
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
    }

    
    #[cw_serde]
    pub struct Osmo_MockInstantiateMsg {}

    
    #[cw_serde]
    pub enum Osmo_MockQueryMsg {
        PoolState {
            id: u64,
        },
        GetDenom {
            creator_address: String,
            subdenom: String,
        },
        GetTokenInfo {
            denom: String,
        },
        GetOwner { owner: String },
    }

    pub fn osmosis_proxy_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: Osmo_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Osmo_MockExecuteMsg::MintTokens {
                        denom,
                        amount,
                        mint_to_address,
                    } => {
                        if amount == Uint128::new(1428u128) {
                            assert_eq!(
                                String::from("credit_fulldenom 1428 revenue_collector"),
                                format!("{} {} {}", denom, amount.to_string(), mint_to_address)
                            );
                        }

                        Ok(Response::new())
                    }
                    Osmo_MockExecuteMsg::BurnTokens {
                        denom: _,
                        amount,
                        burn_from_address: _,
                    } => {
                        // if amount == Uint128::new(50000_000_000u128) {
                        //     panic!();
                        // }
                        Ok(Response::new())                    
                    },
                    Osmo_MockExecuteMsg::CreateDenom {
                        subdenom: _,
                        max_supply,
                        liquidity_multiplier,
                    } => Ok(Response::new().add_attributes(vec![
                        attr("subdenom", "credit_fulldenom"),
                        attr(
                            "max_supply",
                            max_supply.unwrap_or_else(|| Uint128::zero()).to_string(),
                        ),
                        attr(
                            "liquidity_multiplier",
                            liquidity_multiplier
                                .unwrap_or_else(|| Decimal::zero())
                                .to_string(),
                        ),
                    ])),
                }
            },
            |_, _, _, _: Osmo_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Osmo_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Osmo_MockQueryMsg::PoolState { id } => {
                        if id == 99u64 {
                            Ok(to_binary(&PoolStateResponse {
                                assets: vec![coin(112_914_609, "base").into(), coin(112_914_609, "quote").into()],
                                shares: coin(100_000_000_000_000_000_000, "lp_denom").into(),
                            })?)
                        } else {
                            Ok(to_binary(&PoolStateResponse {
                                assets: vec![coin(49_999, "credit_fulldenom").into()],
                                shares: coin(0, "shares").into(),
                            })?)
                        }
                    }
                    Osmo_MockQueryMsg::GetDenom {
                        creator_address: _,
                        subdenom: _,
                    } => Ok(to_binary(&GetDenomResponse {
                        denom: String::from("credit_fulldenom"),
                    })?),
                    Osmo_MockQueryMsg::GetTokenInfo { denom } => {
                        Ok(to_binary(&TokenInfoResponse {
                            denom,
                            current_supply: Uint128::new(200_000u128),
                            max_supply: Uint128::new(1_000_000_000_000_000u128),
                            burned_supply: Uint128::zero(),
                        })?)
                    },
                    Osmo_MockQueryMsg::GetOwner { owner } => {
                        Ok(to_binary(&OwnerResponse {
                            owner: Owner {
                                owner: Addr::unchecked(""),
                                total_minted: Uint128::zero(),
                                stability_pool_ratio: Some(Decimal::one()),
                                non_token_contract_auth: true,
                                is_position_contract: false
                            },
                            liquidity_multiplier: Decimal::percent(500),
                        })?)
                    },
                }
            },
        );
        Box::new(contract)
    }

    //Mock Router Contract
     #[cw_serde]    
    pub enum Router_MockExecuteMsg {
        BasketLiquidate {
            offer_assets: apollo_cw_asset::AssetListUnchecked,
            receive_asset: apollo_cw_asset::AssetInfoUnchecked,
            minimum_receive: Option<Uint128>,
            to: Option<String>,
        }
    }

     #[cw_serde]    
    pub struct Router_MockInstantiateMsg {}

     #[cw_serde]    
    pub enum Router_MockQueryMsg {}

     #[cw_serde]    
    pub struct MockResponse {}

    pub fn router_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Router_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Router_MockExecuteMsg::
                    BasketLiquidate {
                        offer_assets,
                        receive_asset,
                        minimum_receive,
                        to,      
                    } => {
                        Ok(Response::default())
                    }
                }
            },
            |_, _, _, _: Router_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Router_MockQueryMsg| -> StdResult<Binary> {
                Ok(to_binary(&MockResponse {})?)
            },
        );
        Box::new(contract)
    }

    //Mock Auction Contract
     #[cw_serde]    
    pub enum Auction_MockExecuteMsg {
        StartAuction {
            position_id: Uint128,
            position_owner: String,
            debt_amount: Uint128,
        },
    }

     #[cw_serde]    
    pub struct Auction_MockInstantiateMsg {}

     #[cw_serde]    
    pub enum Auction_MockQueryMsg {}

    pub fn auction_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Auction_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Auction_MockExecuteMsg::StartAuction {
                        position_id,
                        position_owner,
                        debt_amount,
                    } => Ok(Response::default()),
                }
            },
            |_, _, _, _: Auction_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Auction_MockQueryMsg| -> StdResult<Binary> {
                Ok(to_binary(&MockResponse {})?)
            },
        );
        Box::new(contract)
    }

    //Mock Staking Contract
     #[cw_serde]    
    pub enum Staking_MockExecuteMsg {
        DepositFee {},
    }

     #[cw_serde]    
    pub struct Staking_MockInstantiateMsg {}

     #[cw_serde]    
    pub enum Staking_MockQueryMsg {
        Config { }
    }

    pub fn staking_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Staking_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Staking_MockExecuteMsg::DepositFee {} => Ok(Response::default()),
                }
            },
            |_, _, _, _: Staking_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Staking_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Staking_MockQueryMsg::Config {  } => {
                        Ok(to_binary(&Staking_Config {
                            owner: Addr::unchecked(""),
                            mbrn_denom: String::from("mbrn_denom"),
                            incentive_schedule: StakeDistribution { rate: Decimal::zero(), duration: 0 },
                            keep_raw_cdt: false,
                            vesting_rev_multiplier: Decimal::zero(),
                            max_commission_rate: Decimal::zero(),
                            unstaking_period: 0,
                            positions_contract: None,
                            auction_contract: None,
                            vesting_contract: None,
                            governance_contract: None,
                            osmosis_proxy: None,
                        })?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    //Mock Discounts Contract
     #[cw_serde]    
    pub enum Discounts_MockExecuteMsg {}

     #[cw_serde]    
    pub struct Discounts_MockInstantiateMsg {}

     #[cw_serde]    
    pub enum Discounts_MockQueryMsg {
        UserDiscount { user: String }
    }

    pub fn discounts_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Discounts_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _, _: Discounts_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Discounts_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Discounts_MockQueryMsg::UserDiscount { user } => {

                        if user == String::from("discounty"){
                            Ok(to_binary(&Decimal::percent(90))?)
                        } else {
                            Ok(to_binary(&Decimal::zero())?)
                        }
                        
                    }
                }
            },
        );
        Box::new(contract)
    }

    //Mock Oracle Contract
     #[cw_serde]    
    pub enum Oracle_MockExecuteMsg {
        AddAsset {
            asset_info: AssetInfo,
            oracle_info: AssetOracleInfo,
        },
        EditAsset {
            asset_info: AssetInfo,
            oracle_info: Option<AssetOracleInfo>,
            remove: bool,
        },
    }

     #[cw_serde]    
    pub struct Oracle_MockInstantiateMsg {}

     #[cw_serde]    
    pub enum Oracle_MockQueryMsg {
        Prices {
            asset_infos: Vec<AssetInfo>,
            twap_timeframe: u64,
            oracle_time_limit: u64,
        },
        Assets {
            asset_infos: Vec<AssetInfo>,
        },
    }

    pub fn oracle_contract_one() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Oracle_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Oracle_MockExecuteMsg::AddAsset {
                        asset_info,
                        oracle_info,
                    } => Ok(Response::default()),
                    Oracle_MockExecuteMsg::EditAsset {
                        asset_info,
                        oracle_info,
                        remove,
                    } => Ok(Response::default()),
                }
            },
            |_, _, _, _: Oracle_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Oracle_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Oracle_MockQueryMsg::Prices {
                        asset_infos,
                        twap_timeframe,
                        oracle_time_limit,
                    } => {
                        let mut prices = vec![];
                        for _ in 0..asset_infos.len() {
                                prices.push(PriceResponse {
                                    prices: vec![],
                                    price: Decimal::one(),
                                    decimals: 6,
                                });
                            }
                                                
                        Ok(to_binary(&prices)?)                        
                    }
                    Oracle_MockQueryMsg::Assets { asset_infos } => Ok(to_binary(&vec![
                        AssetResponse {
                            asset_info: AssetInfo::NativeToken {
                                denom: String::from("denom"),
                            },
                            oracle_info: vec![AssetOracleInfo {
                                basket_id: Uint128::new(1u128),
                                pools_for_osmo_twap: vec![TWAPPoolInfo {
                                    pool_id: 0u64,
                                    base_asset_denom: String::from("denom"),
                                    quote_asset_denom: String::from("denom"),
                                }],
                                is_usd_par: false,
                                lp_pool_info: None,
                                decimals: 6,
                                pyth_price_feed_id: None,
                            }],
                        },
                        AssetResponse {
                            asset_info: AssetInfo::NativeToken {
                                denom: String::from("denom"),
                            },
                            oracle_info: vec![AssetOracleInfo {
                                basket_id: Uint128::new(1u128),
                                pools_for_osmo_twap: vec![TWAPPoolInfo {
                                    pool_id: 0u64,
                                    base_asset_denom: String::from("denom"),
                                    quote_asset_denom: String::from("denom"),
                                }],
                                is_usd_par: false,
                                lp_pool_info: None,
                                decimals: 6,
                                pyth_price_feed_id: None,
                            }],
                        }
                    ])?),
                }
            },
        );
        Box::new(contract)
    }
    pub fn oracle_contract_two() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Oracle_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Oracle_MockExecuteMsg::AddAsset {
                        asset_info,
                        oracle_info,
                    } => Ok(Response::default()),
                    Oracle_MockExecuteMsg::EditAsset {
                        asset_info,
                        oracle_info,
                        remove,
                    } => Ok(Response::default()),
                }
            },
            |_, _, _, _: Oracle_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Oracle_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Oracle_MockQueryMsg::Prices {
                        asset_infos,
                        twap_timeframe,
                        oracle_time_limit,
                    } => {
                        let mut prices = vec![];
                        for _ in 0..asset_infos.len() {
                                prices.push(PriceResponse {
                                    prices: vec![],
                                    price: Decimal::percent(200),
                                    decimals: 6,
                                });
                            }
                                                
                        Ok(to_binary(&prices)?)     
                    }
                    Oracle_MockQueryMsg::Assets { asset_infos } => Ok(to_binary(&vec![
                        AssetResponse {
                            asset_info: AssetInfo::NativeToken {
                                denom: String::from("denom"),
                            },
                            oracle_info: vec![AssetOracleInfo {
                                basket_id: Uint128::new(1u128),
                                pools_for_osmo_twap: vec![TWAPPoolInfo {
                                    pool_id: 0u64,
                                    base_asset_denom: String::from("denom"),
                                    quote_asset_denom: String::from("denom"),
                                }],
                                is_usd_par: false,
                                lp_pool_info: None,
                                decimals: 6,
                                pyth_price_feed_id: None,
                            }],
                        },
                        AssetResponse {
                            asset_info: AssetInfo::NativeToken {
                                denom: String::from("denom"),
                            },
                            oracle_info: vec![AssetOracleInfo {
                                basket_id: Uint128::new(1u128),
                                pools_for_osmo_twap: vec![TWAPPoolInfo {
                                    pool_id: 0u64,
                                    base_asset_denom: String::from("denom"),
                                    quote_asset_denom: String::from("denom"),
                                }],
                                is_usd_par: false,
                                lp_pool_info: None,
                                decimals: 6,
                                pyth_price_feed_id: None,
                            }],
                        }
                    ])?),
                }
            },
        );
        Box::new(contract)
    }
    pub fn oracle_contract_onefive() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Oracle_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Oracle_MockExecuteMsg::AddAsset {
                        asset_info,
                        oracle_info,
                    } => Ok(Response::default()),
                    Oracle_MockExecuteMsg::EditAsset {
                        asset_info,
                        oracle_info,
                        remove,
                    } => Ok(Response::default()),
                }
            },
            |_, _, _, _: Oracle_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Oracle_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Oracle_MockQueryMsg::Prices {
                        asset_infos,
                        twap_timeframe,
                        oracle_time_limit,
                    } => {
                        let mut prices = vec![];
                        for _ in 0..asset_infos.len() {
                                prices.push(PriceResponse {
                                    prices: vec![],
                                    price: Decimal::percent(150),
                                    decimals: 6,
                                });
                            }
                           
                                                
                        Ok(to_binary(&prices)?)     
                    }
                    Oracle_MockQueryMsg::Assets { asset_infos } => Ok(to_binary(&vec![
                        AssetResponse {
                            asset_info: AssetInfo::NativeToken {
                                denom: String::from("denom"),
                            },
                            oracle_info: vec![AssetOracleInfo {
                                basket_id: Uint128::new(1u128),
                                pools_for_osmo_twap: vec![TWAPPoolInfo {
                                    pool_id: 0u64,
                                    base_asset_denom: String::from("denom"),
                                    quote_asset_denom: String::from("denom"),
                                }],
                                is_usd_par: false,
                                lp_pool_info: None,
                                decimals: 6,
                                pyth_price_feed_id: None,
                            }],
                        },
                        AssetResponse {
                            asset_info: AssetInfo::NativeToken {
                                denom: String::from("denom"),
                            },
                            oracle_info: vec![AssetOracleInfo {
                                basket_id: Uint128::new(1u128),
                                pools_for_osmo_twap: vec![TWAPPoolInfo {
                                    pool_id: 0u64,
                                    base_asset_denom: String::from("denom"),
                                    quote_asset_denom: String::from("denom"),
                                }],
                                is_usd_par: false,
                                lp_pool_info: None,
                                decimals: 6,
                                pyth_price_feed_id: None,
                            }],
                        }
                    ])?),
                }
            },
        );
        Box::new(contract)
    }
    pub fn oracle_contract_three() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Oracle_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Oracle_MockExecuteMsg::AddAsset {
                        asset_info,
                        oracle_info,
                    } => Ok(Response::default()),
                    Oracle_MockExecuteMsg::EditAsset {
                        asset_info,
                        oracle_info,
                        remove,
                    } => Ok(Response::default()),
                }
            },
            |_, _, _, _: Oracle_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Oracle_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Oracle_MockQueryMsg::Prices {
                        asset_infos,
                        twap_timeframe,
                        oracle_time_limit,
                    } => {
                        let mut prices = vec![];
                        for _ in 0..asset_infos.len() {
                                prices.push(PriceResponse {
                                    prices: vec![],
                                    price: Decimal::percent(300),
                                    decimals: 6,
                                });
                            }
                           
                                                
                        Ok(to_binary(&prices)?)     
                    }
                    Oracle_MockQueryMsg::Assets { asset_infos } => Ok(to_binary(&vec![
                        AssetResponse {
                            asset_info: AssetInfo::NativeToken {
                                denom: String::from("denom"),
                            },
                            oracle_info: vec![AssetOracleInfo {
                                basket_id: Uint128::new(1u128),
                                pools_for_osmo_twap: vec![TWAPPoolInfo {
                                    pool_id: 0u64,
                                    base_asset_denom: String::from("denom"),
                                    quote_asset_denom: String::from("denom"),
                                }],
                                is_usd_par: false,
                                lp_pool_info: None,
                                decimals: 6,
                                pyth_price_feed_id: None,
                            }],
                        },
                        AssetResponse {
                            asset_info: AssetInfo::NativeToken {
                                denom: String::from("denom"),
                            },
                            oracle_info: vec![AssetOracleInfo {
                                basket_id: Uint128::new(1u128),
                                pools_for_osmo_twap: vec![TWAPPoolInfo {
                                    pool_id: 0u64,
                                    base_asset_denom: String::from("denom"),
                                    quote_asset_denom: String::from("denom"),
                                }],
                                is_usd_par: false,
                                lp_pool_info: None,
                                decimals: 6,
                                pyth_price_feed_id: None,
                            }],
                        }
                    ])?),
                }
            },
        );
        Box::new(contract)
    }
    pub fn oracle_contract_five() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Oracle_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Oracle_MockExecuteMsg::AddAsset {
                        asset_info,
                        oracle_info,
                    } => Ok(Response::default()),
                    Oracle_MockExecuteMsg::EditAsset {
                        asset_info,
                        oracle_info,
                        remove,
                    } => Ok(Response::default()),
                }
            },
            |_, _, _, _: Oracle_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Oracle_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Oracle_MockQueryMsg::Prices {
                        asset_infos,
                        twap_timeframe,
                        oracle_time_limit,
                    } => {
                        let mut prices = vec![];
                        for _ in 0..asset_infos.len() {
                                prices.push(PriceResponse {
                                    prices: vec![],
                                    price: Decimal::percent(500),
                                    decimals: 6,
                                });
                            }
                                                
                        Ok(to_binary(&prices)?)     
                    }
                    Oracle_MockQueryMsg::Assets { asset_infos } => Ok(to_binary(&vec![
                        AssetResponse {
                            asset_info: AssetInfo::NativeToken {
                                denom: String::from("denom"),
                            },
                            oracle_info: vec![AssetOracleInfo {
                                basket_id: Uint128::new(1u128),
                                pools_for_osmo_twap: vec![TWAPPoolInfo {
                                    pool_id: 0u64,
                                    base_asset_denom: String::from("denom"),
                                    quote_asset_denom: String::from("denom"),
                                }],
                                is_usd_par: false,
                                lp_pool_info: None,
                                decimals: 6,
                                pyth_price_feed_id: None,
                            }],
                        },
                        AssetResponse {
                            asset_info: AssetInfo::NativeToken {
                                denom: String::from("denom"),
                            },
                            oracle_info: vec![AssetOracleInfo {
                                basket_id: Uint128::new(1u128),
                                pools_for_osmo_twap: vec![TWAPPoolInfo {
                                    pool_id: 0u64,
                                    base_asset_denom: String::from("denom"),
                                    quote_asset_denom: String::from("denom"),
                                }],
                                is_usd_par: false,
                                lp_pool_info: None,
                                decimals: 6,
                                pyth_price_feed_id: None,
                            }],
                        }
                    ])?),
                }
            },
        );
        Box::new(contract)
    }
    pub fn oracle_contract_fivetwo() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Oracle_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Oracle_MockExecuteMsg::AddAsset {
                        asset_info,
                        oracle_info,
                    } => Ok(Response::default()),
                    Oracle_MockExecuteMsg::EditAsset {
                        asset_info,
                        oracle_info,
                        remove,
                    } => Ok(Response::default()),
                }
            },
            |_, _, _, _: Oracle_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Oracle_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Oracle_MockQueryMsg::Prices {
                        asset_infos,
                        twap_timeframe,
                        oracle_time_limit,
                    } => {
                        let mut prices = vec![];
                        for _ in 0..asset_infos.len() {
                            prices.push(PriceResponse {
                                prices: vec![],
                                price: Decimal::percent(520),
                                decimals: 6,
                            });
                        }
                        
                                                
                        Ok(to_binary(&prices)?)     
                    }
                    Oracle_MockQueryMsg::Assets { asset_infos } => Ok(to_binary(&vec![
                        AssetResponse {
                            asset_info: AssetInfo::NativeToken {
                                denom: String::from("denom"),
                            },
                            oracle_info: vec![AssetOracleInfo {
                                basket_id: Uint128::new(1u128),
                                pools_for_osmo_twap: vec![TWAPPoolInfo {
                                    pool_id: 0u64,
                                    base_asset_denom: String::from("denom"),
                                    quote_asset_denom: String::from("denom"),
                                }],
                                is_usd_par: false,
                                lp_pool_info: None,
                                decimals: 6,
                                pyth_price_feed_id: None,
                            }],
                        },
                        AssetResponse {
                            asset_info: AssetInfo::NativeToken {
                                denom: String::from("denom"),
                            },
                            oracle_info: vec![AssetOracleInfo {
                                basket_id: Uint128::new(1u128),
                                pools_for_osmo_twap: vec![TWAPPoolInfo {
                                    pool_id: 0u64,
                                    base_asset_denom: String::from("denom"),
                                    quote_asset_denom: String::from("denom"),
                                }],
                                is_usd_par: false,
                                lp_pool_info: None,
                                decimals: 6,
                                pyth_price_feed_id: None,
                            }],
                        }
                    ])?),
                }
            },
        );
        Box::new(contract)
    }


    //Mock Liquidity Contract
     #[cw_serde]    
    pub enum Liquidity_MockExecuteMsg {
        AddAsset { asset: LiquidityInfo },
        EditAsset { asset: LiquidityInfo },
    }

     #[cw_serde]    
    pub struct Liquidity_MockInstantiateMsg {}

     #[cw_serde]    
    pub enum Liquidity_MockQueryMsg {
        Liquidity { asset: AssetInfo },
    }

    pub fn liquidity_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Liquidity_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Liquidity_MockExecuteMsg::AddAsset { asset } => Ok(Response::default()),
                    Liquidity_MockExecuteMsg::EditAsset { asset } => Ok(Response::default()),
                }
            },
            |_, _, _, _: Liquidity_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Liquidity_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Liquidity_MockQueryMsg::Liquidity { asset } => {
                        Ok(to_binary(&LiquidityResponse { 
                            asset: AssetInfo::NativeToken {
                                denom: String::from("credit_fulldenom"),
                            },
                            liquidity: Uint128::new(49999_000_000u128)
                        })?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    fn mock_app() -> App {
        AppBuilder::new().build(|router, _, storage| {
            let bank = BankKeeper::new();

            bank.init_balance(
                storage,
                &Addr::unchecked(USER),
                vec![coin(100_000_000_000, "debit"), coin(100_000_000_000, "2nddebit")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("contract0"),
                vec![coin(2777_777777, "credit_fulldenom")],
            )
            .unwrap(); //contract1 = Stability Pool contract
            bank.init_balance(
                storage,
                &Addr::unchecked("test"),
                vec![coin(50_000_000_000, "credit_fulldenom"), coin(100_000_000_000, "debit")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("sender"),
                vec![coin(50_000_001_000_000, "credit_fulldenom")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("big_bank"),
                vec![coin(10_000_000, "debit"), coin(10_000_000, "double_debit")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("bigger_bank"),
                vec![
                    coin(100_000_000_000_000, "debit"),
                    coin(100_000_000_000_000, "quote"),
                    coin(100_000_000_000_000, "base"),
                    coin(100_000_000_000_000, "double_debit"),
                    coin(200_000_000_000_000_000_000_000_000_000_000, "lp_denom"),
                    coin(100_000_000_000_000, "credit_fulldenom"),
                ],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("little_bank"),
                vec![coin(1_000, "debit")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("redeemer"),
                vec![coin(100_000_000000, "credit_fulldenom"), coin(1, "not_redeemable")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("LP_assets"),
                vec![ coin(2_328, "base"), coin(2_328, "quote")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("coin_God"),
                vec![coin(2_250_000_000_000, "credit_fulldenom")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("lp_tester"),
                vec![coin(100_000_000_000_000_000_000_000, "lp_denom")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("faker"),
                vec![coin(666, "fake_debit")],
            )
            .unwrap();

            router.bank = bank;
        })
    }

    pub fn proper_instantiate( ) -> (App, CDPContract, LQContract, OracleContract, OracleContract, OracleContract, OracleContract, OracleContract, OracleContract) {
        let mut app = mock_app();

        //Instanitate SP
        let sp_id: u64 = app.store_code(stability_pool_contract());        

        let sp_contract_addr = app
            .instantiate_contract(
                sp_id,
                Addr::unchecked(ADMIN),
                &SP_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instanitate Router
        let router_id = app.store_code(router_contract());

        let router_contract_addr = app
            .instantiate_contract(
                router_id,
                Addr::unchecked(ADMIN),
                &Router_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate LQ
        let lq_id: u64 = app.store_code(liq_queue_contract());

        let lq_contract_addr = app
            .instantiate_contract(
                lq_id,
                Addr::unchecked(ADMIN),
                &LQ_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        let lq_contract = LQContract(lq_contract_addr.clone());

        //Instaniate Osmosis Proxy
        let proxy_id: u64 = app.store_code(osmosis_proxy_contract());

        let osmosis_proxy_contract_addr = app
            .instantiate_contract(
                proxy_id,
                Addr::unchecked(ADMIN),
                &Osmo_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate Auction Contract
        let auction_id = app.store_code(auction_contract());

        let auction_contract_addr = app
            .instantiate_contract(
                auction_id,
                Addr::unchecked(ADMIN),
                &Auction_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate Staking Contract
        let staking_id = app.store_code(staking_contract());

        let staking_contract_addr = app
            .instantiate_contract(
                staking_id,
                Addr::unchecked(ADMIN),
                &Staking_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate Oracle Contracts
        let oracle_id: u64 = app.store_code(oracle_contract_one());

        let oracle_contract_addr = app
            .instantiate_contract(
                oracle_id,
                Addr::unchecked(ADMIN),
                &Oracle_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();
        let oracle_contract_one = OracleContract(oracle_contract_addr);
        //2
        let oracle_id: u64 = app.store_code(oracle_contract_two());

        let oracle_contract_addr = app
            .instantiate_contract(
                oracle_id,
                Addr::unchecked(ADMIN),
                &Oracle_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();
        let oracle_contract_two = OracleContract(oracle_contract_addr);
        //1.5
        let oracle_id: u64 = app.store_code(oracle_contract_onefive());

        let oracle_contract_addr = app
            .instantiate_contract(
                oracle_id,
                Addr::unchecked(ADMIN),
                &Oracle_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();
        let oracle_contract_onefive = OracleContract(oracle_contract_addr);
        //3
        let oracle_id: u64 = app.store_code(oracle_contract_three());

        let oracle_contract_addr = app
            .instantiate_contract(
                oracle_id,
                Addr::unchecked(ADMIN),
                &Oracle_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();
        let oracle_contract_three = OracleContract(oracle_contract_addr);
        //5
        let oracle_id: u64 = app.store_code(oracle_contract_five());

        let oracle_contract_addr = app
            .instantiate_contract(
                oracle_id,
                Addr::unchecked(ADMIN),
                &Oracle_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();
        let oracle_contract_five = OracleContract(oracle_contract_addr);        
        //5.2
        let oracle_id: u64 = app.store_code(oracle_contract_fivetwo());

        let oracle_contract_addr = app
            .instantiate_contract(
                oracle_id,
                Addr::unchecked(ADMIN),
                &Oracle_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();
        let oracle_contract_fivetwo = OracleContract(oracle_contract_addr);

        //Instaniate Liquidity Contract
        let liq_id: u64 = app.store_code(liquidity_contract());

        let liquidity_contract_addr = app
            .instantiate_contract(
                liq_id,
                Addr::unchecked(ADMIN),
                &Liquidity_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate Discounts Contract
        let dc_id: u64 = app.store_code(discounts_contract());        

        let discounts_contract_addr = app
            .instantiate_contract(
                dc_id,
                Addr::unchecked(ADMIN),
                &Discounts_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instantiate CDP contract
        let cdp_id = app.store_code(cdp_contract());
        
        let create_basket = CreateBasket {
            basket_id: Uint128::one(),
            collateral_types: vec![cAsset {
                asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: "debit".to_string(),
                    },
                    amount: Uint128::from(0u128),
                },
                max_borrow_LTV: Decimal::percent(50),
                max_LTV: Decimal::percent(70),
                pool_info: None,
                rate_index: Decimal::one(),
            }],
            credit_asset: Asset {
                info: AssetInfo::NativeToken {
                    denom: "credit_fulldenom".to_string(),
                },
                amount: Uint128::from(0u128),
            },
            credit_price: Decimal::percent(100),
            base_interest_rate: Some(Decimal::percent(100)),
            credit_pool_infos: vec![],
            liq_queue: Some(lq_contract_addr.to_string()),
        };

        let msg = InstantiateMsg {
            owner: Some(ADMIN.to_string()),
            liq_fee: Decimal::percent(1),
            stability_pool: Some(sp_contract_addr.to_string()),
            dex_router: Some(router_contract_addr.to_string()),
            staking_contract: Some(staking_contract_addr.to_string()),
            oracle_contract: Some(oracle_contract_fivetwo.0.to_string()),
            osmosis_proxy: Some(osmosis_proxy_contract_addr.to_string()),
            debt_auction: Some(auction_contract_addr.to_string()),
            liquidity_contract: Some(liquidity_contract_addr.to_string()),
            discounts_contract: Some(discounts_contract_addr.to_string()),
            oracle_time_limit: 60u64,
            debt_minimum: Uint128::new(2000u128),
            collateral_twap_timeframe: 60u64,
            credit_twap_timeframe: 480u64,
            rate_slope_multiplier: Decimal::from_str("0.618").unwrap(),
            base_debt_cap_multiplier: Uint128::new(21u128),
            create_basket,
        };
        let cdp_contract_addr = app
            .instantiate_contract(cdp_id, Addr::unchecked(ADMIN), &msg, &[], "test", None)
            .unwrap();

        let cdp_contract = CDPContract(cdp_contract_addr);
        //Add Base
        let msg = ExecuteMsg::EditBasket(EditBasket {
            take_revenue: None,
            added_cAsset: Some(cAsset {
                asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: "base".to_string(),
                    },
                    amount: Uint128::zero(),
                },
                max_borrow_LTV: Decimal::percent(40),
                max_LTV: Decimal::percent(60),
                pool_info: None,
                rate_index: Decimal::one(),
            }),
            liq_queue: None,
            credit_pool_infos: None,
            collateral_supply_caps: None,
            base_interest_rate: None,
            credit_asset_twap_price_source: None,
            negative_rates: None,
            cpc_margin_of_error: None,
            frozen: None,
            rev_to_stakers: None,
            multi_asset_supply_caps: None,
        });
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

        let msg = ExecuteMsg::EditBasket(EditBasket {
            added_cAsset: None,
            liq_queue: None,
            collateral_supply_caps: Some(vec![
                SupplyCap {
                    asset_info: AssetInfo::NativeToken {
                        denom: "debit".to_string(),
                    },
                    supply_cap_ratio: Decimal::percent(100),
                    stability_pool_ratio_for_debt_cap: None,
                    current_supply: Uint128::zero(),
                    lp: false,
                    debt_total: Uint128::zero(),
                },
                SupplyCap {
                    asset_info: AssetInfo::NativeToken {
                        denom: "base".to_string(),
                    },
                    supply_cap_ratio: Decimal::percent(100),
                    stability_pool_ratio_for_debt_cap: None,
                    current_supply: Uint128::zero(),
                    lp: false,
                    debt_total: Uint128::zero(),
                }
            ]),
            base_interest_rate: None,
            credit_asset_twap_price_source: Some(TWAPPoolInfo {
                pool_id: 0u64,
                base_asset_denom: String::from("base"),
                quote_asset_denom: String::from("quote"),
            }),
            negative_rates: None,
            cpc_margin_of_error: None,
            frozen: None,
            rev_to_stakers: None,
            multi_asset_supply_caps: None,
            credit_pool_infos: None,
            take_revenue: None,
        });
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

        (app, cdp_contract, lq_contract, oracle_contract_one, oracle_contract_two, oracle_contract_onefive, oracle_contract_three, oracle_contract_five, oracle_contract_fivetwo)
    }

    mod cdp {

        use crate::contract::query;

        use super::*;
        use cosmwasm_std::{testing::mock_env, BlockInfo};
        use membrane::{cdp::ExecuteMsg, types::Basket};

        #[test]
        fn volatility_tests() {
            let (mut app, cdp_contract, _, one, two, onefive, three, five, fivetwo) = proper_instantiate();

            //Initial Deposit
            //Current Position: 100 _000_000_000_000_000_000 lp_denom
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("bigger_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(100_000_000_000u128),
                    }, Coin {
                        denom: "base".to_string(),
                        amount: Uint128::from(100_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(2_000_000_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(two.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };       
            //Swap Oracle price to $1
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(one.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            //Swap Oracle price to $2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(two.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            ////Jiggle btwn 5 & 5.2 to lower vol and increase index
            /// 
            //Swap Oracle price to $5.2
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(fivetwo.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Query basket to check interest rates
            let msg = QueryMsg::GetBasket {  };
            let resp: Basket = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &msg.clone())
                .unwrap();
            panic!("{:?}", resp.lastest_collateral_rates);

            //Edit Basket Supply Caps
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: Some(vec![SupplyCap {
                    asset_info: AssetInfo::NativeToken {
                        denom: "debit".to_string(),
                    },
                    current_supply: Uint128::zero(),
                    debt_total: Uint128::zero(),
                    supply_cap_ratio: Decimal::percent(100),
                    lp: false,
                    stability_pool_ratio_for_debt_cap: None,
                },SupplyCap {
                    asset_info: AssetInfo::NativeToken {
                        denom: "base".to_string(),
                    },
                    current_supply: Uint128::zero(),
                    debt_total: Uint128::zero(),
                    supply_cap_ratio: Decimal::percent(100),
                    lp: false,
                    stability_pool_ratio_for_debt_cap: None,
                }]),
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            //Edit Basket Multi asset Supply Caps
            //make sure this resets the index
            // let msg = ExecuteMsg::EditBasket(EditBasket {
            //     take_revenue: None,
            //     added_cAsset: None,
            //     liq_queue: None,
            //     credit_pool_infos: None,
            //     collateral_supply_caps: None,
            //     base_interest_rate: None,
            //     credit_asset_twap_price_source: None,
            //     negative_rates: None,
            //     cpc_margin_of_error: None,
            //     frozen: None,
            //     rev_to_stakers: None,
            //     multi_asset_supply_caps: Some(vec![MultiAssetSupplyCap {
            //         assets: vec![AssetInfo::NativeToken {
            //             denom: "debit".to_string(),
            //         },
            //         AssetInfo::NativeToken {
            //             denom: "base".to_string(),
            //         }],
            //         supply_cap_ratio: Decimal::percent(100),
            //     }]),
            // });
            // let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            // app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            //Swap Oracle price to $5
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: Some(five.0.to_string()),
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 11 mins to store new price and new vol
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(60*11u64), //11 mins
                chain_id: app.block_info().chain_id,
            });

            //Accrue position to set Volatility
            let msg = ExecuteMsg::Accrue { 
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("bigger_bank".to_string()),
            };      
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Check that the index is reset to 1 on debt cap updates
            // This panic is to show any printlns: panic!();
        }
    }
}
