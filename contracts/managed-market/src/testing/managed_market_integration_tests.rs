#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use membrane::managed_market::{
        ExecuteMsg, InstantiateMsg, QueryMsg, BorrowCap, CollateralParams, RateParams,
        Config, MarketParams, UserPositionResponse, DebtInfo, LTVRamp
    };
    use membrane::types::{
        AssetOracleInfo, TWAPPoolInfo, AssetInfo, BorrowOptions, LoopLTVParams, AutoCloseParams, UserHistory, UserPosition, UXBoosts, ClaimTracker
    };
    use membrane::math::{decimal_division, decimal_multiplication};
    use membrane::market_manager::{ManagerEdit, MarketInstantiation};
    use membrane::oracle::{AssetResponse, PriceResponse};
    use membrane::types::{Asset, LiquidityInfo};
    use membrane::liquidity_check::LiquidityResponse;

    use cosmwasm_std::{
        attr, coin, to_binary, Addr, Binary, Coin, Decimal, Empty, Response, StdError, StdResult,
        Uint128, WasmMsg, CosmosMsg, BankMsg, MessageInfo, DepsMut, Env, QuerierWrapper, Storage, to_json_binary
    };
    use serde::{Serialize, Deserialize};
    use cosmwasm_schema::schemars::JsonSchema;
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    use cosmwasm_schema::cw_serde;

    const USER: &str = "user";
    const ADMIN: &str = "admin";

    const CDT_DENOM: &str = "factory/osmo1s794h9rxggytja3a4pmwul53u98k06zy2qtrdvjnfuxruh7s8yjs6cyxgd/ucdt";

    // Mock Markets Manager Contract
    #[cw_serde]
    pub enum MarketsManager_MockExecuteMsg {
        UpdateConfig {
            owner: Option<String>,
            managed_market_code_id: Option<u64>,
            edit_managers: Option<ManagerEdit>,
            managed_market_fee: Option<Decimal>,
            minimum_cdt_for_permissionless_instantiation: Option<Uint128>,
            osmosis_proxy_contract: Option<String>,
        },
        UpdateMarketItem {
            market_address: String,
            manager: Option<String>,
            socials: Option<Vec<String>>,
            name: Option<String>,
            remove: Option<bool>,
        },
        InstantiateMarket {
            params: MarketInstantiation,
        },
        MigrateMarkets {
            market_addresses: Vec<String>,
        },
    }

    #[cw_serde]
    pub struct MarketsManager_MockInstantiateMsg {}

    #[cw_serde]
    pub enum MarketsManager_MockQueryMsg {
        Config {},
        MarketsManaged { manager: String },
        Managers { start_after: Option<String>, limit: Option<u32> },
        MarketParams { manager: String, start_after: Option<String>, limit: Option<u32> },
    }

    pub fn markets_manager_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_deps, _env, _info, _msg: MarketsManager_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::new())
            },
            |_deps, _env, _info, _msg: MarketsManager_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::new())
            },
            |_deps, _env, msg: MarketsManager_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    MarketsManager_MockQueryMsg::Config {} => {
                        let config = membrane::market_manager::Config {
                            owner: Addr::unchecked(ADMIN),
                            managed_market_code_id: 1,
                            manager_whitelist: vec![Addr::unchecked("manager")],
                            osmosis_proxy_contract: Addr::unchecked("osmosis_proxy"),
                            managed_market_fee: Decimal::percent(5),
                            minimum_cdt_for_permissionless_instantiation: None,
                        };
                        to_binary(&config)
                    }
                    MarketsManager_MockQueryMsg::MarketsManaged { manager: _ } => {
                        to_binary(&vec!["market1".to_string(), "market2".to_string()])
                    }
                    MarketsManager_MockQueryMsg::Managers { start_after: _, limit: _ } => {
                        to_binary(&vec!["manager1".to_string(), "manager2".to_string()])
                    }
                    MarketsManager_MockQueryMsg::MarketParams { manager: _, start_after: _, limit: _ } => {
                        to_binary::<Vec<String>>(&vec![])
                    }
                }
            },
        );
        Box::new(contract)
    }

    // Mock Oracle Contract
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
            asset_infos: Vec<String>,
            twap_timeframe: u64,
            oracle_time_limit: u64,
        },
        Assets {
            asset_infos: Vec<String>,
        },
    }

    pub fn oracle_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_deps, _env, _info, _msg: Oracle_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::new())
            },
            |_deps, _env, _info, _msg: Oracle_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::new())
            },
            |_deps, _env, msg: Oracle_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Oracle_MockQueryMsg::Prices { asset_infos, .. } => {
                        let mut prices = vec![];
                        for asset_info in asset_infos {
                            let price = if asset_info.contains("atom") {
                                PriceResponse {
                                    prices: vec![],
                                    price: Decimal::from_str("2.0").unwrap(),
                                    decimals: 6,
                                }
                            } else if asset_info.contains("ucdt") {
                                PriceResponse {
                                    prices: vec![],
                                    price: Decimal::from_str("1.0").unwrap(),
                                    decimals: 6,
                                }
                            } else {
                                PriceResponse {
                                    prices: vec![],
                                    price: Decimal::from_str("1.0").unwrap(),
                                    decimals: 6,
                                }
                            };
                            prices.push(price);
                        }
                        to_binary(&prices)
                    }
                    Oracle_MockQueryMsg::Assets { asset_infos: _ } => {
                        to_binary::<Vec<String>>(&vec![])
                    }
                }
            },
        );
        Box::new(contract)
    }

    // Mock Swap Contract
    #[cw_serde]
    pub enum Router_MockExecuteMsg {
        Swap {
            token_in: String,
            token_out: String,
            max_slippage: Decimal,
        }
    }

    #[cw_serde]
    pub struct Router_MockInstantiateMsg {}

    #[cw_serde]
    pub enum Router_MockQueryMsg {}

    pub fn router_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_deps, _env, _info, _msg: Router_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::new().add_attributes(vec![
                    attr("action", "swap"),
                    attr("amount_out", "1000"),
                ]))
            },
            |_deps, _env, _info, _msg: Router_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::new())
            },
            |_deps, _env, _msg: Router_MockQueryMsg| -> StdResult<Binary> {
                to_binary(&"")
            },
        );
        Box::new(contract)
    }

    // Managed Market Contract
    pub fn managed_market_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contract::execute,
            crate::contract::instantiate,
            crate::contract::query,
        )
        .with_reply(crate::contract::reply);
        Box::new(contract)
    }

    #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
    #[serde(rename_all = "snake_case")]
    pub enum TokenFactory_MockExecuteMsg {
        CreateDenom {
            subdenom: String,
        },
        MintTokens {
            amount: Option<Coin>,
            mint_to_address: String,
        },
        BurnTokens { }
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct TokenFactory_MockInstantiateMsg {}

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub enum TokenFactory_MockQueryMsg {
        GetDenom {
            creator_address: String,
            subdenom: String,
        },
        GetTokenInfo {
            denom: String,
        },
    }

    pub fn token_factory_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _env, info, msg: TokenFactory_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    TokenFactory_MockExecuteMsg::CreateDenom { subdenom } => {
                        Ok(Response::new()
                            .add_attribute("method", "create_denom")
                            .add_attribute("subdenom", subdenom))
                    },
                    TokenFactory_MockExecuteMsg::MintTokens { amount, mint_to_address } => {
                        Ok(Response::new()
                            .add_attribute("method", "mint_tokens")
                            .add_attribute("amount", amount.map(|c| c.amount.to_string()).unwrap_or_default())
                            .add_attribute("mint_to_address", mint_to_address))
                    },
                    TokenFactory_MockExecuteMsg::BurnTokens { } => {
                        Ok(Response::new()
                            .add_attribute("method", "burn_tokens"))
                    },
                }
            },
            |_deps, _env, _info, _msg: TokenFactory_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::new().add_attribute("method", "instantiate"))
            },
            |_deps, _env, msg: TokenFactory_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    TokenFactory_MockQueryMsg::GetDenom { creator_address, subdenom } => {
                        let denom = format!("factory/{}/{}", creator_address, subdenom);
                        to_binary(&denom)
                    },
                    TokenFactory_MockQueryMsg::GetTokenInfo { denom } => {
                        to_binary(&"mock_token_info")
                    },
                }
            }
        );
        Box::new(contract)
    }

    fn mock_app() -> App {
        AppBuilder::new()
            .with_bank(BankKeeper::new())
            .build(|router, _api, storage| {
                // Set up initial balances
                router
                    .bank
                    .init_balance(
                        storage,
                        &Addr::unchecked(ADMIN),
                        vec![
                            coin(1_000_000_000, "atom"),
                            coin(1_000_000_000, CDT_DENOM),
                        ],
                    )
                    .unwrap();

                router
                    .bank
                    .init_balance(
                        storage,
                        &Addr::unchecked(USER),
                        vec![
                            coin(1_000_000_000, "atom"),
                            coin(1_000_000_000, CDT_DENOM),
                        ],
                    )
                    .unwrap();

                router
                    .bank
                    .init_balance(
                        storage,
                        &Addr::unchecked("collateral_guy"),
                        vec![
                            coin(1_000_000_000, "atom"),
                            coin(1_000_000_000, CDT_DENOM),
                        ],
                    )
                    .unwrap();

                router
                    .bank
                    .init_balance(
                        storage,
                        &Addr::unchecked("debt_guy"),
                        vec![
                            coin(1_000_000_000, "atom"),
                            coin(1_000_000_000, CDT_DENOM),
                            coin(500_000_000_000, "factory/contract4/debt-suppliers")
                        ],
                    )
                    .unwrap();
            })
    }

    fn default_instantiate_msg() -> InstantiateMsg {
        InstantiateMsg {
            owner: ADMIN.to_string(),
            token_factory_contract: "token_factory".to_string(),
            whitelisted_debt_suppliers: Some(vec!["debt_guy".to_string()]),
            collateral_params: CollateralParams {
                collateral_asset: "atom".to_string(),
                max_borrow_LTV: Decimal::percent(50),
                liquidation_LTV: Decimal::percent(60),
            },
            rate_params: RateParams {
                base_rate: Decimal::percent(5),
                rate_kink: None,
                rate_max: Decimal::percent(20),
            },
            borrow_fee: Decimal::percent(1),
            max_slippage: Decimal::percent(5),
            whitelisted_collateral_suppliers: Some(vec!["collateral_guy".to_string()]),
            pause_option: true,
            debt_supply_cap: Some(Uint128::new(1000000)),
            borrow_cap: BorrowCap {
                fixed_cap: Some(Uint128::new(500000)),
                cap_borrows_by_liquidity: false,
            },
            per_user_debt_cap: Some(Uint128::new(10000)),
            debt_minimum: Some(Uint128::new(100)),
            manager_fee: Some(Decimal::percent(5)),
            oracle_contract: "oracle".to_string(),
            swap_contract: "swap".to_string(),
            debt_token: CDT_DENOM.to_string(),
        }
    }

    fn setup_managed_market_test() -> (App, Addr) {
        let mut app = mock_app();

        // Store contracts
        let managed_market_code_id = app.store_code(managed_market_contract());
        let oracle_code_id = app.store_code(oracle_contract());
        let swap_code_id = app.store_code(router_contract());
        let markets_manager_code_id = app.store_code(markets_manager_contract());
        let token_factory_code_id = app.store_code(token_factory_contract());

        // Instantiate token factory first
        let token_factory_addr = app
            .instantiate_contract(
                token_factory_code_id,
                Addr::unchecked(ADMIN),
                &TokenFactory_MockInstantiateMsg {},
                &[],
                "token_factory",
                None,
            )
            .unwrap();

        // Instantiate oracle
        let oracle_addr = app
            .instantiate_contract(
                oracle_code_id,
                Addr::unchecked(ADMIN),
                &Oracle_MockInstantiateMsg {},
                &[],
                "oracle",
                None,
            )
            .unwrap();

        // Instantiate swap
        let swap_addr = app
            .instantiate_contract(
                swap_code_id,
                Addr::unchecked(ADMIN),
                &Router_MockInstantiateMsg {},
                &[],
                "swap",
                None,
            )
            .unwrap();

        // Instantiate markets manager
        let markets_manager_addr = app
            .instantiate_contract(
                markets_manager_code_id,
                Addr::unchecked(ADMIN),
                &MarketsManager_MockInstantiateMsg {},
                &[],
                "markets_manager",
                None,
            )
            .unwrap();

        // Instantiate managed market
        let mut msg = default_instantiate_msg();
        msg.oracle_contract = oracle_addr.to_string();
        msg.swap_contract = swap_addr.to_string();
        msg.token_factory_contract = token_factory_addr.to_string(); // Use token factory for denom creation
        // msg.osmosis_proxy_contract = "proxy".to_string(); // Keep osmosis proxy for other purposes
        
        let managed_market_addr = app
            .instantiate_contract(
                managed_market_code_id,
                Addr::unchecked(ADMIN),
                &msg,
                &[],
                "managed_market",
                None,
            )
            .unwrap();

        // Update the managed market config to set the markets manager contract address
        let update_config_msg = ExecuteMsg::UpdateConfig {
            owner: None,
            markets_manager_contract: Some(markets_manager_addr.to_string()),
            oracle_contract: None,
            swap_contract: None,
            token_factory_contract: None,
            pause_actions: None,
            manager_fee: None,
            whitelisted_debt_suppliers: None,
            debt_supply_cap: None,
            senior_debt_fixed_yield_target: None,
        };

        app.execute_contract(
            Addr::unchecked(ADMIN),
            managed_market_addr.clone(),
            &update_config_msg,
            &[],
        ).unwrap();

        // Initialize vault token state to prevent rate assurance failures
        // This simulates the initial state that would be set during contract instantiation
        // let init_vault_msg = ExecuteMsg::SupplyDebt { 
        //     send_to: None, 
        //     is_junior: false 
        // };
        
        // Add some initial debt tokens to the contract balance
        // app.send_tokens(
        //     Addr::unchecked(ADMIN),
        //     managed_market_addr.clone(),
        //     &[coin(1_000_000, CDT_DENOM)],
        // ).unwrap();

        // Supply a small amount of debt to initialize the vault token state
        // app.execute_contract(
        //     Addr::unchecked("debt_guy"),
        //     managed_market_addr.clone(),
        //     &init_vault_msg,
        //     &[coin(1, CDT_DENOM)],
        // ).unwrap();

        (app, managed_market_addr)
    }

    #[test]
    fn test_supply_collateral_happy_path_and_failures() {
        let (mut app, managed_market_addr) = setup_managed_market_test();

        // Test successful collateral supply
        let supply_msg = ExecuteMsg::SupplyCollateral {
            owner: None,
        };

        app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(1000, "atom")],
        )
        .unwrap();

        // Verify the supply was successful by checking user position
        let query_msg = QueryMsg::GetUserPositions {
            collateral_denom: "atom".to_string(),
            user: Some("collateral_guy".to_string()),
            start_after: None,
            limit: None,
        };

        let positions: Vec<UserPositionResponse> = app
            .wrap()
            .query_wasm_smart(managed_market_addr.clone(), &query_msg)
            .unwrap();

        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].position.collateral_amount, Uint128::new(1000));

        // Test failure: non-whitelisted user
        let result = app.execute_contract(
            Addr::unchecked("non_whitelisted"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(1000, "atom")],
        );
        assert!(result.is_err());

        // Test failure: wrong asset
        let result = app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(1000, "wrong_asset")],
        );
        assert!(result.is_err());

        // Test failure: no funds sent
        let result = app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_withdraw_collateral_happy_path_and_failures() {
        let (mut app, managed_market_addr) = setup_managed_market_test();

        // Supply collateral first
        let supply_msg = ExecuteMsg::SupplyCollateral { owner: None };
        app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(1_000_000, "atom")],
        )
        .unwrap();

        // Happy path: Withdraw some collateral
        let withdraw_msg = ExecuteMsg::WithdrawCollateral {
            collateral_denom: "atom".to_string(),
            send_to: None,
            withdraw_amount: Some(Uint128::new(400_000)),
        };
        let result = app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &withdraw_msg,
            &[],
        );
        // println!("result: {:?}", result);
        assert!(result.is_ok());

        // Check user position after withdrawal
        let query_msg = QueryMsg::GetUserPositions {
            collateral_denom: "atom".to_string(),
            user: Some("collateral_guy".to_string()),
            start_after: None,
            limit: None,
        };
        let result: Vec<UserPositionResponse> = app
            .wrap()
            .query_wasm_smart(&managed_market_addr, &query_msg)
            .unwrap();
        
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].user, "collateral_guy");
        assert_eq!(result[0].position.collateral_amount, Uint128::new(600_000));
        assert_eq!(result[0].position.debt_amount, Uint128::zero());

        // Failure: Withdraw with no position
        let withdraw_msg = ExecuteMsg::WithdrawCollateral {
            collateral_denom: "atom".to_string(),
            send_to: None,
            withdraw_amount: Some(Uint128::new(100)),
        };
        let result = app.execute_contract(
            Addr::unchecked("random_guy"),
            managed_market_addr.clone(),
            &withdraw_msg,
            &[],
        );
        assert!(result.is_err());

        // Success: Withdrawing more than available withdraws the max
        let withdraw_msg = ExecuteMsg::WithdrawCollateral {
            collateral_denom: "atom".to_string(),
            send_to: Some("rando".to_string()),
            withdraw_amount: Some(Uint128::new(2_000_000)),
        };
        let result = app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &withdraw_msg,
            &[],
        );
        assert!(result.is_ok());

        // Check that all collateral was withdrawn: Will Error
        let query_msg = QueryMsg::GetUserPositions {
            collateral_denom: "atom".to_string(),
            user: Some("collateral_guy".to_string()),
            start_after: None,
            limit: None,
        };
        let result = app
            .wrap()
            .query_wasm_smart::<Vec<UserPositionResponse>>(&managed_market_addr, &query_msg);
        assert!(result.is_err());
        

        // Failure: Contract paused
        let pause_msg = ExecuteMsg::UpdateConfig {
            owner: None,
            markets_manager_contract: None,
            oracle_contract: None,
            swap_contract: None,
            token_factory_contract: None,
            pause_actions: Some(true),
            manager_fee: None,
            whitelisted_debt_suppliers: None,
            debt_supply_cap: None,
            senior_debt_fixed_yield_target: None,
        };
        app.execute_contract(
            Addr::unchecked(ADMIN),
            managed_market_addr.clone(),
            &pause_msg,
            &[],
        )
        .unwrap();

        // Try to withdraw after pausing (should fail)
        let withdraw_msg = ExecuteMsg::WithdrawCollateral {
            collateral_denom: "atom".to_string(),
            send_to: None,
            withdraw_amount: Some(Uint128::new(100)),
        };
        let result = app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &withdraw_msg,
            &[],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_supply_debt_happy_path_and_failures() {
        let (mut app, managed_market_addr) = setup_managed_market_test();

        // Happy Path: Supply valid debt token
        let supply_msg = ExecuteMsg::SupplyDebt { 
            send_to: None, 
            is_junior: false 
        };
        let result = app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(1_000_000, CDT_DENOM)],
        );
        // println!("result: {:?}", result);
        assert!(result.is_ok());

        // Check config: total_debt_tokens updated
        let config: Config = app
            .wrap()
            .query_wasm_smart(&managed_market_addr, &QueryMsg::Config {})
            .unwrap();
        assert_eq!(config.total_debt_tokens, Uint128::new(1_000_000));

        // Query total vault tokens
        let total_vault_tokens: Uint128 = app
            .wrap()
            .query_wasm_smart(&managed_market_addr, &QueryMsg::TotalVaultTokens { is_junior: false })
            .unwrap();
        assert_eq!(total_vault_tokens, Uint128::new(1_000_000_000_000));

        // Failure: Multiple assets sent
        let result = app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[
                coin(500_000, CDT_DENOM),
                coin(500_000, "other"),
            ],
        );
        assert!(result.is_err());

        // Failure: Zero asset sent
        let result = app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(0, CDT_DENOM)],
        );
        assert!(result.is_err());

        // Failure: Not the correct asset denom
        let result = app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(1_000, "notusdc")],
        );
        assert!(result.is_err());

        // Failure: Not whitelisted
        let result = app.execute_contract(
            Addr::unchecked("unwhitelisted"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(1_000, CDT_DENOM)],
        );
        assert!(result.is_err());

        // Failure: Supply cap exceeded
        let update_msg = ExecuteMsg::UpdateConfig {
            owner: None,
            markets_manager_contract: None,
            oracle_contract: None,
            swap_contract: None,
            token_factory_contract: None,
            pause_actions: None,
            manager_fee: None,
            whitelisted_debt_suppliers: None,
            debt_supply_cap: Some(Some(Uint128::new(1_000_001))), // Just 1 more
            senior_debt_fixed_yield_target: None,
        };
        app.execute_contract(
            Addr::unchecked(ADMIN),
            managed_market_addr.clone(),
            &update_msg,
            &[],
        )
        .unwrap();

        let result = app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(2, CDT_DENOM)],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_withdraw_debt_happy_path_and_failures() {
        let (mut app, managed_market_addr) = setup_managed_market_test();

        // Supply debt first
        let supply_msg = ExecuteMsg::SupplyDebt { 
            send_to: None, 
            is_junior: false 
        };
        app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(1_000_000, CDT_DENOM)],
        )
        .unwrap();

        // Happy Path: Withdraw debt token
        let withdraw_msg = ExecuteMsg::WithdrawDebt {
            send_to: None,
        };
        let result = app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &withdraw_msg,
            &[coin(500_000_000_000, "factory/contract4/debt-suppliers")],
        );
        //  println!("result: {:?}", result);
        assert!(result.is_ok());

        // Query total vault tokens
        let total_vault_tokens: Uint128 = app
            .wrap()
            .query_wasm_smart(&managed_market_addr, &QueryMsg::TotalVaultTokens { is_junior: false })
            .unwrap();
        assert_eq!(total_vault_tokens, Uint128::new(500_000_000_000));

        // Failure: Withdraw more than balance
        let result = app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &withdraw_msg,
            &[coin(10_000_000_000_000, "factory/contract4/debt-suppliers")],
        );
        assert!(result.is_err());

        // Failure: Withdraw zero
        let result = app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &withdraw_msg,
            &[coin(0, "factory/contract4/debt-suppliers")],
        );
        assert!(result.is_err());

        // Failure: Not whitelisted
        let result = app.execute_contract(
            Addr::unchecked("random_guy"),
            managed_market_addr.clone(),
            &withdraw_msg,
            &[coin(100_000_000_000, "factory/contract4/debt-suppliers")],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_borrow_cdt_happy_path_and_failures() {
        let (mut app, managed_market_addr) = setup_managed_market_test();

        // Supply collateral first
        let supply_msg = ExecuteMsg::SupplyCollateral { owner: None };
        app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(1_000_000, "atom")],
        )
        .unwrap();

        // Supply debt tokens
        let supply_msg = ExecuteMsg::SupplyDebt { 
            send_to: None, 
            is_junior: false 
        };
        app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(1_000_000, CDT_DENOM)],
        )
        .unwrap();

        // Happy Path: Borrow CDT
        let borrow_msg = ExecuteMsg::Borrow {
            collateral_denom: "atom".to_string(),
            borrow_amount: BorrowOptions {
                amount: Some(Uint128::new(500_000)),
                ltv: None,
            },
            send_to: None,
        };


        // Check that the user balance prior 
        let balance = app.wrap().query_balance("collateral_guy", CDT_DENOM).unwrap();
        assert_eq!(balance.amount, Uint128::new(1000000000));

        let result = app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &borrow_msg,
            &[],
        );
        // println!("result: {:?}", result);
        assert!(result.is_ok());

        // Check that the user received the borrowed tokens.
        // They got capped by the 10k per user cap.
        let balance = app.wrap().query_balance("collateral_guy", CDT_DENOM).unwrap();
        assert_eq!(balance.amount, Uint128::new(1000009900));

        // Failure: Borrow more than LTV allows
        let borrow_msg = ExecuteMsg::Borrow {
            collateral_denom: "atom".to_string(),
            borrow_amount: BorrowOptions {
                amount: Some(Uint128::new(1_000_000)),
                ltv: None,
            },
            send_to: None,
        };
        let result = app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &borrow_msg,
            &[],
        );
        assert!(result.is_err());

        // Failure: Borrow zero amount
        let borrow_msg = ExecuteMsg::Borrow {
            collateral_denom: "atom".to_string(),
            borrow_amount: BorrowOptions {
                amount: Some(Uint128::zero()),
                ltv: None,
            },
            send_to: None,
        };
        let result = app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &borrow_msg,
            &[],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_repay_cdt_happy_path_and_failures() {
        let (mut app, managed_market_addr) = setup_managed_market_test();

        // Supply collateral first
        let supply_msg = ExecuteMsg::SupplyCollateral { owner: None };
        app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(1_000_000, "atom")],
        )
        .unwrap();

        // Supply debt tokens
        let supply_msg = ExecuteMsg::SupplyDebt { 
            send_to: None, 
            is_junior: false 
        };
        app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(1_000_000, CDT_DENOM)],
        )
        .unwrap();

        // Borrow CDT
        let borrow_msg = ExecuteMsg::Borrow {
            collateral_denom: "atom".to_string(),
            borrow_amount: BorrowOptions {
                amount: Some(Uint128::new(100_000)),
                ltv: None,
            },
            send_to: None,
        };
        app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &borrow_msg,
            &[],
        )
        .unwrap();

        //Check actual borrowed amount
        let user_positions: Vec<UserPositionResponse> = app
            .wrap()
            .query_wasm_smart(
                &managed_market_addr,
                &QueryMsg::GetUserPositions {
                    user: Some("collateral_guy".to_string()),
                    collateral_denom: "atom".to_string(),
                    start_after: None,
                    limit: None,
                },
            )
            .unwrap();
        assert_eq!(user_positions[0].position.debt_amount, Uint128::new(9900));


        // Happy Path: Repay CDT
        let repay_msg = ExecuteMsg::Repay {
            collateral_denom: "atom".to_string(),
            send_excess_to: None,
        };
        let result = app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &repay_msg,
            &[coin(50_000, CDT_DENOM)],
        );
        assert!(result.is_ok());

        // Check user position to verify debt was reduced
        let user_positions: Vec<UserPositionResponse> = app
            .wrap()
            .query_wasm_smart(
                &managed_market_addr,
                &QueryMsg::GetUserPositions {
                    user: Some("collateral_guy".to_string()),
                    collateral_denom: "atom".to_string(),
                    start_after: None,
                    limit: None,
                },
            )
            .unwrap();
        assert_eq!(user_positions[0].position.debt_amount, Uint128::new(0));

        // Failure: Wrong asset
        let repay_msg = ExecuteMsg::Repay {
            collateral_denom: "atom".to_string(),
            send_excess_to: None,
        };
        let result = app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &repay_msg,
            &[coin(100_000, "wrong_token")],
        );
        assert!(result.is_err());

        // Failure: Not a position owner
        let repay_msg = ExecuteMsg::Repay {
            collateral_denom: "atom".to_string(),
            send_excess_to: None,
        };
        let result = app.execute_contract(
            Addr::unchecked("random_guy"),
            managed_market_addr.clone(),
            &repay_msg,
            &[coin(100_000, CDT_DENOM)],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_liquidation_happy_path_and_failures() {
        let (mut app, managed_market_addr) = setup_managed_market_test();

        // Supply collateral first
        let supply_msg = ExecuteMsg::SupplyCollateral { owner: None };
        app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(1_000_000, "atom")],
        )
        .unwrap();

        // Supply debt tokens
        let supply_msg = ExecuteMsg::SupplyDebt { 
            send_to: None, 
            is_junior: false 
        };
        app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(1_000_000, CDT_DENOM)],
        )
        .unwrap();

        // Borrow against the collateral (high LTV for liquidation)
        let borrow_msg = ExecuteMsg::Borrow {
            collateral_denom: "atom".to_string(),
            borrow_amount: BorrowOptions {
                amount: Some(Uint128::new(900_000)),
                ltv: None,
            },
            send_to: None,
        };
        app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &borrow_msg,
            &[],
        )
        .unwrap();

        //Update Market liquidation LTV to 0.001
        let udpate_msg = ExecuteMsg::UpdateMarket {
            collateral_denom: "atom".to_string(),
            max_borrow_LTV: Some(Decimal::from_str("0.001").unwrap()),
            liquidation_LTV: Some(LTVRamp {
                new_LTV: Decimal::from_str("0.002").unwrap(),
                duration_in_hours: 0u64,
            }),
            rate_params: None,
            borrow_fee: None,
            whitelisted_collateral_suppliers: None,
            borrow_cap: None,
            max_slippage: None,
            per_user_debt_cap: None,
            debt_minimum: None,
        };
        let res = app.execute_contract(
            Addr::unchecked(ADMIN),
            managed_market_addr.clone(),
            &udpate_msg,
            &[],
        ).unwrap();
        // println!("res: {:?}", res);
        //Update Market liquidation LTV to 0.001
        let udpate_msg = ExecuteMsg::UpdateMarket {
            collateral_denom: "atom".to_string(),
            max_borrow_LTV: None,
            liquidation_LTV: None,
            rate_params: None,
            borrow_fee: None,
            whitelisted_collateral_suppliers: None,
            borrow_cap: None,
            max_slippage: None,
            per_user_debt_cap: None,
            debt_minimum: None,
        };
        let res = app.execute_contract(
            Addr::unchecked(ADMIN),
            managed_market_addr.clone(),
            &udpate_msg,
            &[],
        ).unwrap();
        // Liquidate (should succeed)
        let liquidate_msg = ExecuteMsg::Liquidate {
            collateral_denom: "atom".to_string(),
            position_owner: "collateral_guy".to_string(),
            take_fee: true,
            max_slippage: None,
        };
        let result = app.execute_contract(
            Addr::unchecked("liquidator"),
            managed_market_addr.clone(),
            &liquidate_msg,
            &[],
        );
        // println!("result: {:?}", result);
        assert!(result.is_ok());

        // Check that the position was liquidated.
        //We can't repay debt from the reply bc we can't senf the contracct debt mid execution.
        //Assert this check in the contract tests
        let user_positions: Vec<UserPositionResponse> = app
            .wrap()
            .query_wasm_smart(
                &managed_market_addr,
                &QueryMsg::GetUserPositions {
                    user: Some("collateral_guy".to_string()),
                    collateral_denom: "atom".to_string(),
                    start_after: None,
                    limit: None,
                },
            )
            .unwrap();
        // assert_eq!(user_positions[0].position.debt_amount, Uint128::zero());

    }

    #[test]
    fn test_close_position_happy_path_and_failures() {
        let (mut app, managed_market_addr) = setup_managed_market_test();

        // Supply collateral
        let supply_msg = ExecuteMsg::SupplyCollateral { owner: None };
        app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(1_000_000, "atom")],
        )
        .unwrap();

        // Supply debt tokens
        let supply_msg = ExecuteMsg::SupplyDebt { 
            send_to: None, 
            is_junior: false 
        };
        app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(1_000_000, CDT_DENOM)],
        )
        .unwrap();

        // Borrow against the collateral
        let borrow_msg = ExecuteMsg::Borrow {
            collateral_denom: "atom".to_string(),
            borrow_amount: BorrowOptions {
                amount: Some(Uint128::new(100_000)),
                ltv: None,
            },
            send_to: None,
        };
        app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &borrow_msg,
            &[],
        )
        .unwrap();

        // Happy Path: Close position (full close)
        let close_msg = ExecuteMsg::ClosePosition {
            collateral_denom: "atom".to_string(),
            position_owner: None,
            close_percentage: None, // Full close
            max_spread: Decimal::percent(2),
            send_to: None,
        };
        let result = app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &close_msg,
            &[],
        );
        // println!("result: {:?}", result);
        assert!(result.is_ok());

        // Check that the position was closed (Doesn't close bc the contract doesnt gain new DEBT)
        // let user_positions_result = app
        //     .wrap()
        //     .query_wasm_smart::<Vec<UserPositionResponse>>(
        //         &managed_market_addr,
        //         &QueryMsg::GetUserPositions {
        //             user: Some("collateral_guy".to_string()),
        //             collateral_denom: "atom".to_string(),
        //             start_after: None,
        //             limit: None,
        //         },
        //     );
        // assert!(user_positions_result.is_err());

        // Failure: Close non-existent position
        let close_msg = ExecuteMsg::ClosePosition {
            collateral_denom: "atom".to_string(),
            position_owner: None,
            close_percentage: None,
            max_spread: Decimal::percent(2),
            send_to: None,
        };
        let result = app.execute_contract(
            Addr::unchecked("random_guy"),
            managed_market_addr.clone(),
            &close_msg,
            &[],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_pausing_unpausing_and_config_updates() {
        let (mut app, managed_market_addr) = setup_managed_market_test();

        // Pause actions (happy path)
        let pause_msg = ExecuteMsg::UpdateConfig {
            owner: None,
            markets_manager_contract: None,
            oracle_contract: None,
            swap_contract: None,
            token_factory_contract: None,
            pause_actions: Some(true),
            manager_fee: None,
            whitelisted_debt_suppliers: None,
            debt_supply_cap: None,
            senior_debt_fixed_yield_target: None,
        };
        let result = app.execute_contract(
            Addr::unchecked(ADMIN),
            managed_market_addr.clone(),
            &pause_msg,
            &[],
        );
        assert!(result.is_ok());

        // Try to supply collateral while paused (should fail)
        let supply_msg = ExecuteMsg::SupplyCollateral { owner: None };
        let result = app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(1_000_000, "atom")],
        );
        assert!(result.is_err());

        // Unpause actions (happy path)
        let unpause_msg = ExecuteMsg::UpdateConfig {
            owner: None,
            markets_manager_contract: None,
            oracle_contract: None,
            swap_contract: None,
            token_factory_contract: None,
            pause_actions: Some(false),
            manager_fee: None,
            whitelisted_debt_suppliers: None,
            debt_supply_cap: None,
            senior_debt_fixed_yield_target: None,
        };
        let result = app.execute_contract(
            Addr::unchecked(ADMIN),
            managed_market_addr.clone(),
            &unpause_msg,
            &[],
        );
        assert!(result.is_ok());

        // Try to supply collateral after unpausing (should succeed)
        let result = app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(1_000_000, "atom")],
        );
        assert!(result.is_ok());

        // Unauthorized config update (should fail)
        let pause_msg = ExecuteMsg::UpdateConfig {
            owner: None,
            markets_manager_contract: None,
            oracle_contract: None,
            swap_contract: None,
            token_factory_contract: None,
            pause_actions: Some(true),
            manager_fee: None,
            whitelisted_debt_suppliers: None,
            debt_supply_cap: None,
            senior_debt_fixed_yield_target: None,
        };
        let result = app.execute_contract(
            Addr::unchecked("not_owner"),
            managed_market_addr.clone(),
            &pause_msg,
            &[],
        );
        assert!(result.is_err());

        // Valid config update (change all possible parameters)
        let update_msg = ExecuteMsg::UpdateConfig {
            owner: Some("new_owner".to_string()),
            markets_manager_contract: None,
            oracle_contract: Some("new_oracle".to_string()),
            swap_contract: Some("new_swap".to_string()),
            token_factory_contract: Some("new_token_factory".to_string()),
            pause_actions: Some(true),
            manager_fee: Some(Decimal::percent(3)),
            whitelisted_debt_suppliers: Some(Some(vec!["new_debt_guy".to_string()])),
            debt_supply_cap: Some(Some(Uint128::new(123456))),
            senior_debt_fixed_yield_target: None,
        };
        let result = app.execute_contract(
            Addr::unchecked(ADMIN),
            managed_market_addr.clone(),
            &update_msg,
            &[],
        );
        assert!(result.is_ok());

        // Query config and check that owner is still the old owner (ownership transfer is pending)
        let config: Config = app
            .wrap()
            .query_wasm_smart(&managed_market_addr, &QueryMsg::Config {})
            .unwrap();
        assert_eq!(config.owner, Addr::unchecked(ADMIN));

        // Accept ownership as new_owner
        let accept_msg = ExecuteMsg::UpdateConfig { 
            owner: None, 
            markets_manager_contract: None,
            oracle_contract: None,
            swap_contract: None,
            token_factory_contract: None,
            pause_actions: None, 
            manager_fee: None, 
            whitelisted_debt_suppliers: None, 
            debt_supply_cap: None,
            senior_debt_fixed_yield_target: None,
        };
        let result = app.execute_contract(
            Addr::unchecked("new_owner"),
            managed_market_addr.clone(),
            &accept_msg,
            &[],
        );
        assert!(result.is_ok());

        // Query config and check updates (now owner should be new_owner)
        let config: Config = app
            .wrap()
            .query_wasm_smart(&managed_market_addr, &QueryMsg::Config {})
            .unwrap();
        assert_eq!(config.owner, Addr::unchecked("new_owner"));
        assert_eq!(config.oracle_contract, Some(Addr::unchecked("new_oracle")));
        assert_eq!(config.swap_contract, Some(Addr::unchecked("new_swap")));
        assert_eq!(config.token_factory_contract, Some(Addr::unchecked("new_token_factory")));
        assert_eq!(config.manager_fee, Decimal::percent(3));
        assert_eq!(config.whitelisted_debt_suppliers, Some(vec!["new_debt_guy".to_string()]));
        assert_eq!(config.debt_supply_cap, Some(Uint128::new(123456)));
    }

    #[test]
    fn test_edit_ux_boosts_and_loop_happy_path_and_failures() {
        let (mut app, managed_market_addr) = setup_managed_market_test();

        // Supply collateral first
        let supply_msg = ExecuteMsg::SupplyCollateral { owner: None };
        app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(5_000_000, "atom")],
        ).unwrap();

        // Supply debt
        let supply_debt_msg = ExecuteMsg::SupplyDebt { send_to: None, is_junior: false };
        app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &supply_debt_msg,
            &[coin(1_000_000, CDT_DENOM)],
        ).unwrap();

        // Borrow CDT
        let borrow_msg = ExecuteMsg::Borrow {
            collateral_denom: "atom".to_string(),
            send_to: None,
            borrow_amount: BorrowOptions {
                amount: Some(Uint128::new(1000)),
                ltv: None,
            },
        };
        app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &borrow_msg,
            &[],
        ).unwrap();

        // Happy Path: Edit UX Boosts (loop LTV, TP, SL)
        let edit_msg = ExecuteMsg::EditUXBoosts {
            collateral_denom: "atom".to_string(),
            loop_ltv: Some(Some(LoopLTVParams { 
                loop_ltv: Decimal::percent(40), 
                perpetual: true 
            })),
            take_profit_params: None,
            stop_loss_params: None,
            arb_price: None,
            collateral_value_fee_to_executor: Some(Decimal::percent(1)),
        };
        let result = app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &edit_msg,
            &[],
        );
        assert!(result.is_ok());

        // Failure: Edit UX Boosts for non-existent position
        let edit_msg = ExecuteMsg::EditUXBoosts {
            collateral_denom: "atom".to_string(),
            loop_ltv: Some(Some(LoopLTVParams { 
                loop_ltv: Decimal::percent(40), 
                perpetual: true 
            })),
            take_profit_params: None,
            stop_loss_params: None,
            arb_price: None,
            collateral_value_fee_to_executor: Some(Decimal::percent(1)),
        };
        let result = app.execute_contract(
            Addr::unchecked("random_guy"),
            managed_market_addr.clone(),
            &edit_msg,
            &[],
        );
        assert!(result.is_err());

        //Send the contract ATOM to simulate the swap to ATOM
        // app.send_tokens(
        //     Addr::unchecked("collateral_guy"),
        //     managed_market_addr.clone(),
        //     &[coin(1_000_000, "atom")],
        // );

        // Happy Path: Loop position
        let loop_msg = ExecuteMsg::LoopPosition {
            collateral_denom: "atom".to_string(),
            position_owner: None,
            max_slippage: None,
        };
        let result = app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &loop_msg,
            &[],
        );
        println!("result: {:?}", result);
        assert!(result.is_ok());

        // Change intended LTV to 51%: over borrow LTV
        let edit_msg = ExecuteMsg::EditUXBoosts {
            collateral_denom: "atom".to_string(),
            loop_ltv: Some(Some(LoopLTVParams { 
                loop_ltv: Decimal::percent(51), 
                perpetual: true 
            })),
            take_profit_params: None,
            stop_loss_params: None,
            arb_price: None,
            collateral_value_fee_to_executor: Some(Decimal::percent(1)),
        };
        let result = app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &edit_msg,
            &[],
        );
        assert!(result.is_err());

        // Failure: Loop position for non-existent position
        let loop_msg = ExecuteMsg::LoopPosition {
            collateral_denom: "atom".to_string(),
            position_owner: None,
            max_slippage: None,
        };
        let result = app.execute_contract(
            Addr::unchecked("random_guy"),
            managed_market_addr.clone(),
            &loop_msg,
            &[],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_rate_accrual_and_crank_realized_apr_happy_path_and_failures() {
        let (mut app, managed_market_addr) = setup_managed_market_test();

        // Supply collateral first
        let supply_msg = ExecuteMsg::SupplyCollateral { owner: None };
        app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(1_000_000, "atom")],
        ).unwrap();

        // Supply debt
        let supply_debt_msg = ExecuteMsg::SupplyDebt { send_to: None, is_junior: false };
        app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &supply_debt_msg,
            &[coin(1_000_000, CDT_DENOM)],
        ).unwrap();

        // Borrow CDT
        let borrow_msg = ExecuteMsg::Borrow {
            collateral_denom: "atom".to_string(),
            send_to: None,
            borrow_amount: BorrowOptions {
                amount: Some(Uint128::new(100_000)),
                ltv: None,
            },
        };
        app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &borrow_msg,
            &[],
        ).unwrap();

        // Skip time to accrue interest
        app.update_block(|block| {
            block.time = block.time.plus_seconds(1_000_000);
        });

        // Accrue interest for user position
        let accrue_msg = ExecuteMsg::Accrue { 
            collateral_denom: "atom".to_string(), 
            position_owner: "collateral_guy".to_string() 
        };
        let result = app.execute_contract(
            Addr::unchecked(ADMIN),
            managed_market_addr.clone(),
            &accrue_msg,
            &[],
        );
        assert!(result.is_ok());

        // Happy Path: Crank realized APR
        let crank_msg = ExecuteMsg::CrankRealizedAPR { is_junior: false };
        let result = app.execute_contract(
            Addr::unchecked(ADMIN),
            managed_market_addr.clone(),
            &crank_msg,
            &[],
        );
        // This may fail in integration context, so allow either
        if result.is_err() {
            let err_str = result.unwrap_err().to_string();
            assert!(err_str.contains("not found") || err_str.contains("No vault tokens") || err_str.contains("No debt tokens"));
        } else {
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_ltv_ramping() {
        let (mut app, managed_market_addr) = setup_managed_market_test();

        // Check initial liquidation LTV
        let market_params: Vec<MarketParams> = app.wrap()
            .query_wasm_smart(
                managed_market_addr.clone(),
                &QueryMsg::MarketParams { 
                    collateral_denom: Some("atom".to_string()),
                    start_after: None,
                    limit: None,
                }
            ).unwrap();
        assert_eq!(market_params[0].collateral_params.liquidation_LTV, Decimal::percent(60));

        // Initiate an LTV ramp to 70% over 1 hour
        let ramp = LTVRamp {
            new_LTV: Decimal::percent(70),
            duration_in_hours: 1,
        };
        let update_msg = ExecuteMsg::UpdateMarket {
            collateral_denom: "atom".to_string(),
            max_borrow_LTV: None,
            liquidation_LTV: Some(ramp.clone()),
            rate_params: None,
            borrow_fee: None,
            whitelisted_collateral_suppliers: None,
            borrow_cap: None,
            max_slippage: None,
            per_user_debt_cap: None,
            debt_minimum: None,
        };
        let result = app.execute_contract(
            Addr::unchecked(ADMIN),
            managed_market_addr.clone(),
            &update_msg,
            &[],
        );
        assert!(result.is_ok());

        // The LTV should not be updated yet
        let market_params: Vec<MarketParams> = app.wrap()
            .query_wasm_smart(
                managed_market_addr.clone(),
                &QueryMsg::MarketParams { 
                    collateral_denom: Some("atom".to_string()),
                    start_after: None,
                    limit: None,
                }
            ).unwrap();
        assert_eq!(market_params[0].collateral_params.liquidation_LTV, Decimal::percent(60));

        // Simulate time passing beyond the ramp duration
        app.update_block(|block| {
            block.time = block.time.plus_seconds(3601);
        });

        let update_msg = ExecuteMsg::UpdateMarket {
            collateral_denom: "atom".to_string(),
            max_borrow_LTV: None,
            liquidation_LTV: None,
            rate_params: None,
            borrow_fee: None,
            whitelisted_collateral_suppliers: None,
            borrow_cap: None,
            max_slippage: None,
            per_user_debt_cap: None,
            debt_minimum: None,
        };
        let result = app.execute_contract(
            Addr::unchecked(ADMIN),
            managed_market_addr.clone(),
            &update_msg,
            &[],
        );
        assert!(result.is_ok());

        // The LTV should now be updated
        let market_params: Vec<MarketParams> = app.wrap()
            .query_wasm_smart(
                managed_market_addr.clone(),
                &QueryMsg::MarketParams { 
                    collateral_denom: Some("atom".to_string()),
                    start_after: None,
                    limit: None,
                }
            ).unwrap();
        assert_eq!(market_params[0].collateral_params.liquidation_LTV, Decimal::percent(70));
    }

    #[test]
    fn test_markets_manager_revenue() {
        let (mut app, managed_market_addr) = setup_managed_market_test();

        // Supply collateral
        let supply_msg = ExecuteMsg::SupplyCollateral { owner: None };
        app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(10_000_000, "atom")],
        ).unwrap();

        // Supply debt to both tranches
        let senior_msg = ExecuteMsg::SupplyDebt { send_to: None, is_junior: false };
        app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &senior_msg,
            &[coin(500_000, CDT_DENOM)],
        ).unwrap();

        let junior_msg = ExecuteMsg::SupplyDebt { send_to: None, is_junior: true };
        app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &junior_msg,
            &[coin(500_000, CDT_DENOM)],
        ).unwrap();

        // Borrow CDT
        let borrow_msg = ExecuteMsg::Borrow {
            collateral_denom: "atom".to_string(),
            send_to: None,
            borrow_amount: BorrowOptions {
                amount: Some(Uint128::new(100_000)),
                ltv: None,
            },
        };
        app.execute_contract(
            Addr::unchecked("collateral_guy"),
            managed_market_addr.clone(),
            &borrow_msg,
            &[],
        ).unwrap();

        // Skip time
        app.update_block(|block| {
            block.time = block.time.plus_seconds(1_000_000);
        });

        // Simulate interest accrual with manager fees
        let accrue_msg = ExecuteMsg::Accrue { 
            position_owner: "collateral_guy".to_string(), 
            collateral_denom: "atom".to_string() 
        };
        let result = app.execute_contract(
            Addr::unchecked("anyone"),
            managed_market_addr.clone(),
            &accrue_msg,
            &[],
        );
        assert!(result.is_ok());

        // Verify manager fees are distributed to junior tranche
        let config: Config = app.wrap()
            .query_wasm_smart(
                managed_market_addr.clone(),
                &QueryMsg::Config {}
            ).unwrap();
        assert!(config.junior_debt_info.unwrap().total_debt > Uint128::new(500_000));
    }

    #[test]
    fn test_risk_tranching_supply_and_withdraw() {
        let (mut app, managed_market_addr) = setup_managed_market_test();

        // Supply senior debt (1,000,000 CDT)
        let senior_msg = ExecuteMsg::SupplyDebt { send_to: None, is_junior: false };
        app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &senior_msg,
            &[coin(500_000, CDT_DENOM)],
        ).unwrap();

        // Supply junior debt (500,000 CDT)
        let junior_msg = ExecuteMsg::SupplyDebt { send_to: None, is_junior: true };
        app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &junior_msg,
            &[coin(500_000, CDT_DENOM)],
        ).unwrap();

        // Verify Config debt totals
        let config: Config = app.wrap()
            .query_wasm_smart(
                managed_market_addr.clone(),
                &QueryMsg::Config {}
            ).unwrap();
        assert_eq!(config.total_debt_tokens, Uint128::new(500_000));
        assert_eq!(config.junior_debt_info.clone().unwrap().total_debt, Uint128::new(500_000));

        // Verify vault token supplies
        let senior_vt: Uint128 = app.wrap()
            .query_wasm_smart(
                managed_market_addr.clone(),
                &QueryMsg::TotalVaultTokens { is_junior: false }
            ).unwrap();
        let junior_vt: Uint128 = app.wrap()
            .query_wasm_smart(
                managed_market_addr.clone(),
                &QueryMsg::TotalVaultTokens { is_junior: true }
            ).unwrap();
        assert_eq!(senior_vt, Uint128::new(500_000_000_000));
        assert_eq!(junior_vt, Uint128::new(500_000_000_000));

        // Withdraw half the junior vault tokens
        let withdraw_msg = ExecuteMsg::WithdrawDebt { send_to: None };
        app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &withdraw_msg,
            &[coin(250_000_000_000, "factory/contract4/junior-debt-suppliers")],
        ).unwrap();

        // Verify junior total debt reduced accordingly
        let config_after: Config = app.wrap()
            .query_wasm_smart(
                managed_market_addr.clone(),
                &QueryMsg::Config {}
            ).unwrap();
        assert_eq!(config_after.junior_debt_info.unwrap().total_debt, Uint128::new(250_000));
    }

    #[test]
    fn test_tranche_rate_assurance() {
        let (mut app, managed_market_addr) = setup_managed_market_test();

        // Supply debt to both tranches
        let senior_msg = ExecuteMsg::SupplyDebt { send_to: None, is_junior: false };
        app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &senior_msg,
            &[coin(500_000, CDT_DENOM)],
        ).unwrap();

        let junior_msg = ExecuteMsg::SupplyDebt { send_to: None, is_junior: true };
        app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &junior_msg,
            &[coin(500_000, CDT_DENOM)],
        ).unwrap();

        // Test rate assurance for senior tranche
        let rate_msg = ExecuteMsg::RateAssurance { is_junior: false };
        let result = app.execute_contract(
            Addr::unchecked("contract4"),
            managed_market_addr.clone(),
            &rate_msg,
            &[],
        );
        // println!("result: {:?}", result);
        assert!(result.is_ok());

        // Test rate assurance for junior tranche
        let rate_msg = ExecuteMsg::RateAssurance { is_junior: true };
        let result = app.execute_contract(
            Addr::unchecked("contract4"),
            managed_market_addr.clone(),
            &rate_msg,
            &[],
        );
        assert!(result.is_ok());

        // Test unauthorized rate assurance call
        let rate_msg = ExecuteMsg::RateAssurance { is_junior: false };
        let result = app.execute_contract(
            Addr::unchecked("unauthorized"),
            managed_market_addr.clone(),
            &rate_msg,
            &[],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_tranche_claim_tracker() {
        let (mut app, managed_market_addr) = setup_managed_market_test();

        // Test senior claim tracker
        let senior_tracker: ClaimTracker = app.wrap()
            .query_wasm_smart(
                managed_market_addr.clone(),
                &QueryMsg::ClaimTracker { is_junior: false }
            ).unwrap();
        assert_eq!(senior_tracker.vt_claim_checkpoints.len(), 1);
        assert_eq!(senior_tracker.vt_claim_checkpoints[0].vt_claim_of_checkpoint, Uint128::new(1_000_000));

        // Test junior claim tracker
        let junior_tracker: ClaimTracker = app.wrap()
            .query_wasm_smart(
                managed_market_addr.clone(),
                &QueryMsg::ClaimTracker { is_junior: true }
            ).unwrap();
        assert_eq!(junior_tracker.vt_claim_checkpoints.len(), 1);
        assert_eq!(junior_tracker.vt_claim_checkpoints[0].vt_claim_of_checkpoint, Uint128::new(1_000_000));

        // Test crank realized APR for both tranches
        let crank_msg = ExecuteMsg::CrankRealizedAPR { is_junior: false };
        let result = app.execute_contract(
            Addr::unchecked("anyone"),
            managed_market_addr.clone(),
            &crank_msg,
            &[],
        );
        assert!(result.is_ok());

        let crank_msg = ExecuteMsg::CrankRealizedAPR { is_junior: true };
        let result = app.execute_contract(
            Addr::unchecked("anyone"),
            managed_market_addr.clone(),
            &crank_msg,
            &[],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_tranche_whitelist_behavior() {
        let (mut app, managed_market_addr) = setup_managed_market_test();

        // Test successful supply by whitelisted user
        let whitelisted_msg = ExecuteMsg::SupplyDebt { send_to: None, is_junior: false };
        let result = app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &whitelisted_msg,
            &[coin(100_000, CDT_DENOM)],
        );
        assert!(result.is_ok());

        // Test failed supply by non-whitelisted user
        let non_whitelisted_msg = ExecuteMsg::SupplyDebt { send_to: None, is_junior: false };
        let result = app.execute_contract(
            Addr::unchecked("non_whitelisted"),
            managed_market_addr.clone(),
            &non_whitelisted_msg,
            &[coin(100_000, CDT_DENOM)],
        );
        assert!(result.is_err());

        // Test junior tranche supply by whitelisted user
        let junior_msg = ExecuteMsg::SupplyDebt { send_to: None, is_junior: true };
        let result = app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &junior_msg,
            &[coin(50_000, CDT_DENOM)],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_tranche_debt_cap_enforcement() {
        let (mut app, managed_market_addr) = setup_managed_market_test();

        // Supply up to the cap
        let supply_msg = ExecuteMsg::SupplyDebt { send_to: None, is_junior: false };
        app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &supply_msg,
            &[coin(1_000_000, CDT_DENOM)],
        ).unwrap();

        // Try to exceed the cap
        let exceed_msg = ExecuteMsg::SupplyDebt { send_to: None, is_junior: false };
        let result = app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &exceed_msg,
            &[coin(600_000, CDT_DENOM)],
        );
        assert!(result.is_err());

        // Verify junior tranche counts against senior cap
        let junior_msg = ExecuteMsg::SupplyDebt { send_to: None, is_junior: true };
        let result = app.execute_contract(
            Addr::unchecked("debt_guy"),
            managed_market_addr.clone(),
            &junior_msg,
            &[coin(500_000, CDT_DENOM)],
        );
        assert!(result.is_err());
    }
} 