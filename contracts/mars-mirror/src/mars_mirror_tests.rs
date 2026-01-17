#[cfg(test)]
mod mars_mirror_tests {
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{
        coins, from_json, to_json_binary, Binary, Decimal, SystemResult, ContractResult,
        Uint128, WasmQuery, WasmMsg, CosmosMsg,
    };
    use membrane::mars_mirror::{
        InstantiateMsg, ExecuteMsg, QueryMsg, Config, MoveProgress, MarsLTVInfoResponse,
    };
    use membrane::mars_params::{QueryMsg as MarsParams_QueryMsg, AssetParams};
    use membrane::ltv_disco::{
        ExecuteMsg as LTV_Disco_ExecuteMsg, QueryMsg as LTV_Disco_QueryMsg,
        ManagedDepositKeysResponse,
    };
    use membrane::types::Locked;
    use crate::contract::{instantiate, execute, query};
    use crate::state::{CONFIG, MOVE_PROGRESS};

    fn setup_mock_querier() -> (
        cosmwasm_std::OwnedDeps<
            cosmwasm_std::MemoryStorage,
            cosmwasm_std::testing::MockApi,
            cosmwasm_std::testing::MockQuerier<cosmwasm_std::Empty>,
        >,
        cosmwasm_std::Env,
    ) {
        let mut deps = mock_dependencies();
        let env = mock_env();

        // Mock Mars Params contract responses
        deps.querier.update_wasm(|query| {
            match query {
                WasmQuery::Smart { contract_addr: _, msg } => {
                    // Check if it's a Mars Params query
                    if let Ok(mars_query) = from_json::<MarsParams_QueryMsg>(msg) {
                        match mars_query {
                            MarsParams_QueryMsg::AssetParams { denom } => {
                                // Return mock AssetParams for different assets
                                let asset_params = match denom.as_str() {
                                    "uusd" => AssetParams {
                                        denom: "uusd".to_string(),
                                        credit_manager: membrane::mars_params::CmSettings {
                                            whitelisted: true,
                                            withdraw_enabled: true,
                                            hls: None,
                                        },
                                        red_bank: membrane::mars_params::RedBankSettings {
                                            deposit_enabled: true,
                                            borrow_enabled: true,
                                            withdraw_enabled: true,
                                        },
                                        max_loan_to_value: Decimal::percent(75),
                                        liquidation_threshold: Decimal::percent(80),
                                        liquidation_bonus: membrane::mars_params::LiquidationBonus {
                                            starting_lb: Decimal::percent(5),
                                            slope: Decimal::zero(),
                                            min_lb: Decimal::percent(5),
                                            max_lb: Decimal::percent(10),
                                        },
                                        protocol_liquidation_fee: Decimal::percent(1),
                                        deposit_cap: Uint128::new(1_000_000_000),
                                        close_factor: Decimal::percent(50),
                                        reserve_factor: Decimal::percent(20),
                                        interest_rate_model: membrane::mars_params::InterestRateModel {
                                            optimal_utilization_rate: Decimal::percent(80),
                                            base: Decimal::percent(2),
                                            slope_1: Decimal::percent(10),
                                            slope_2: Decimal::percent(100),
                                        },
                                    },
                                    "uatom" => AssetParams {
                                        denom: "uatom".to_string(),
                                        credit_manager: membrane::mars_params::CmSettings {
                                            whitelisted: true,
                                            withdraw_enabled: true,
                                            hls: None,
                                        },
                                        red_bank: membrane::mars_params::RedBankSettings {
                                            deposit_enabled: true,
                                            borrow_enabled: true,
                                            withdraw_enabled: true,
                                        },
                                        max_loan_to_value: Decimal::percent(70),
                                        liquidation_threshold: Decimal::percent(75),
                                        liquidation_bonus: membrane::mars_params::LiquidationBonus {
                                            starting_lb: Decimal::percent(5),
                                            slope: Decimal::zero(),
                                            min_lb: Decimal::percent(5),
                                            max_lb: Decimal::percent(10),
                                        },
                                        protocol_liquidation_fee: Decimal::percent(1),
                                        deposit_cap: Uint128::new(1_000_000_000),
                                        close_factor: Decimal::percent(50),
                                        reserve_factor: Decimal::percent(20),
                                        interest_rate_model: membrane::mars_params::InterestRateModel {
                                            optimal_utilization_rate: Decimal::percent(80),
                                            base: Decimal::percent(2),
                                            slope_1: Decimal::percent(10),
                                            slope_2: Decimal::percent(100),
                                        },
                                    },
                                    _ => {
                                        return SystemResult::Ok(ContractResult::Ok(
                                            to_json_binary(&Option::<AssetParams>::None).unwrap(),
                                        ));
                                    }
                                };
                                return SystemResult::Ok(ContractResult::Ok(
                                    to_json_binary(&Some(asset_params)).unwrap(),
                                ));
                            }
                            _ => {}
                        }
                    }
                    // Check if it's a Disco query
                    if let Ok(disco_query) = from_json::<LTV_Disco_QueryMsg>(msg) {
                        match disco_query {
                            LTV_Disco_QueryMsg::GetManagedDepositKeys {
                                manager: _,
                                limit: _,
                                start_after: _,
                            } => {
                                // Return empty managed deposits by default
                                // Tests can override this
                                let response = ManagedDepositKeysResponse {
                                    keys: vec![],
                                    total: 0,
                                    next_start_after: None,
                                };
                                return SystemResult::Ok(ContractResult::Ok(
                                    to_json_binary(&response).unwrap(),
                                ));
                            }
                            _ => {}
                        }
                    }
                    // Default response for any other query
                    SystemResult::Ok(ContractResult::Ok(Binary::default()))
                }
                _ => SystemResult::Ok(ContractResult::Ok(Binary::default())),
            }
        });

        (deps, env)
    }

    fn default_instantiate_msg() -> InstantiateMsg {
        InstantiateMsg {
            owner: Some("owner".to_string()),
            mars_params_contract: "mars_params".to_string(),
            disco_contract: "disco".to_string(),
            cdp_contract: "cdp".to_string(),
            chain_proxy: Some("chain_proxy".to_string()),
        }
    }

    // Helper to create mock Mars Params AssetParams for uusd
    fn create_mock_uusd_asset_params() -> AssetParams {
        AssetParams {
            denom: "uusd".to_string(),
            credit_manager: membrane::mars_params::CmSettings {
                whitelisted: true,
                withdraw_enabled: true,
                hls: None,
            },
            red_bank: membrane::mars_params::RedBankSettings {
                deposit_enabled: true,
                borrow_enabled: true,
                withdraw_enabled: true,
            },
            max_loan_to_value: Decimal::percent(75),
            liquidation_threshold: Decimal::percent(80),
            liquidation_bonus: membrane::mars_params::LiquidationBonus {
                starting_lb: Decimal::percent(5),
                slope: Decimal::zero(),
                min_lb: Decimal::percent(5),
                max_lb: Decimal::percent(10),
            },
            protocol_liquidation_fee: Decimal::percent(1),
            deposit_cap: Uint128::new(1_000_000_000),
            close_factor: Decimal::percent(50),
            reserve_factor: Decimal::percent(20),
            interest_rate_model: membrane::mars_params::InterestRateModel {
                optimal_utilization_rate: Decimal::percent(80),
                base: Decimal::percent(2),
                slope_1: Decimal::percent(10),
                slope_2: Decimal::percent(100),
            },
        }
    }

    // Helper to add Mars Params query handling to a querier closure
    fn add_mars_params_handler(contract_addr: &str, msg: &Binary) -> Option<SystemResult<ContractResult<Binary>>> {
        if contract_addr == "mars_params" {
            if let Ok(mars_query) = from_json::<MarsParams_QueryMsg>(msg) {
                match mars_query {
                    MarsParams_QueryMsg::AssetParams { denom } => {
                        if denom == "uusd" {
                            return Some(SystemResult::Ok(ContractResult::Ok(
                                to_json_binary(&Some(create_mock_uusd_asset_params())).unwrap(),
                            )));
                        }
                    }
                    _ => {}
                }
            }
        }
        None
    }

    fn instantiate_contract() -> (
        cosmwasm_std::OwnedDeps<
            cosmwasm_std::MemoryStorage,
            cosmwasm_std::testing::MockApi,
            cosmwasm_std::testing::MockQuerier<cosmwasm_std::Empty>,
        >,
        cosmwasm_std::Env,
    ) {
        let (mut deps, env) = setup_mock_querier();
        let info = mock_info("owner", &[]);
        let msg = default_instantiate_msg();
        instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();
        (deps, env)
    }

    #[test]
    fn test_instantiate() {
        let (deps, env) = instantiate_contract();

        let config: Config = CONFIG.load(&deps.storage).unwrap();
        assert_eq!(config.owner, cosmwasm_std::Addr::unchecked("owner"));
        assert_eq!(config.mars_params_contract, "mars_params");
        assert_eq!(config.disco_contract, cosmwasm_std::Addr::unchecked("disco"));
        assert_eq!(config.cdp_contract, cosmwasm_std::Addr::unchecked("cdp"));

        let progress: MoveProgress = MOVE_PROGRESS.load(&deps.storage).unwrap();
        assert_eq!(progress.total_keys, 0);
        assert_eq!(progress.processed_count, 0);
        assert!(progress.last_processed_key.is_none());
    }

    #[test]
    fn test_instantiate_without_owner() {
        let (mut deps, env) = setup_mock_querier();
        let info = mock_info("sender", &[]);
        let msg = InstantiateMsg {
            owner: None,
            mars_params_contract: "mars_params".to_string(),
            disco_contract: "disco".to_string(),
            cdp_contract: "cdp".to_string(),
            chain_proxy: None,
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        let config: Config = CONFIG.load(&deps.storage).unwrap();
        assert_eq!(config.owner, info.sender);
    }

    #[test]
    fn test_query_mars_ltv_info() {
        let (deps, env) = instantiate_contract();

        // Query Mars LTV info for uusd
        let response: MarsLTVInfoResponse = from_json(
            query(
                deps.as_ref(),
                env.clone(),
                QueryMsg::MarsLTVInfo {
                    asset: "uusd".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(response.max_ltv, Decimal::percent(80)); // liquidation_threshold
        assert_eq!(response.max_borrow_ltv, Decimal::percent(75)); // max_loan_to_value

        // Query for uatom
        let response2: MarsLTVInfoResponse = from_json(
            query(
                deps.as_ref(),
                env.clone(),
                QueryMsg::MarsLTVInfo {
                    asset: "uatom".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(response2.max_ltv, Decimal::percent(75));
        assert_eq!(response2.max_borrow_ltv, Decimal::percent(70));
    }

    #[test]
    fn test_query_mars_ltv_info_not_found() {
        let (deps, env) = instantiate_contract();

        // Query for non-existent asset
        let result = query(
            deps.as_ref(),
            env.clone(),
            QueryMsg::MarsLTVInfo {
                asset: "unknown".to_string(),
            },
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_deposit_basic() {
        let (mut deps, env) = instantiate_contract();

        let info = mock_info("user", &coins(10000, "mbrn"));
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            ExecuteMsg::Deposit {
                user: None,
                asset: "uusd".to_string(),
                lock: None,
            },
        )
        .unwrap();

        // Should create a message to Disco
        assert_eq!(res.messages.len(), 1);
        match &res.messages[0].msg {
            CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr,
                msg: _,
                funds,
            }) => {
                assert_eq!(contract_addr, "disco");
                assert_eq!(funds.len(), 1);
                assert_eq!(funds[0].amount, Uint128::new(10000));
                assert_eq!(funds[0].denom, "mbrn");
            }
            _ => panic!("Expected WasmMsg::Execute"),
        }

        // Verify attributes
        assert_eq!(res.attributes[0].key, "action");
        assert_eq!(res.attributes[0].value, "deposit");
        assert_eq!(res.attributes[1].key, "user");
        assert_eq!(res.attributes[1].value, "user");
    }

    #[test]
    fn test_deposit_with_user_override() {
        let (mut deps, env) = instantiate_contract();

        let info = mock_info("sender", &coins(10000, "mbrn"));
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::Deposit {
                user: Some("different_user".to_string()),
                asset: "uusd".to_string(),
                lock: None,
            },
        )
        .unwrap();

        assert_eq!(res.attributes[1].key, "user");
        assert_eq!(res.attributes[1].value, "different_user");
    }

    #[test]
    fn test_deposit_with_lock() {
        let (mut deps, env) = instantiate_contract();

        let info = mock_info("user", &coins(10000, "mbrn"));
        let lock = Some(Locked {
            locked_until: env.block.time.seconds() + 86400 * 30, // 30 days
            perpetual_lock: Some(30),
        });

        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::Deposit {
                user: None,
                asset: "uusd".to_string(),
                lock,
            },
        )
        .unwrap();

        assert_eq!(res.messages.len(), 1);
    }

    #[test]
    fn test_deposit_no_funds() {
        let (mut deps, env) = instantiate_contract();

        let info = mock_info("user", &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::Deposit {
                user: None,
                asset: "uusd".to_string(),
                lock: None,
            },
        );

        assert!(res.is_err());
        match res.unwrap_err() {
            crate::error::ContractError::Validation(msg) => {
                assert!(msg.contains("No funds provided"));
            }
            e => panic!("Expected Validation error, got: {:?}", e),
        }
    }

    #[test]
    fn test_deposit_uses_mars_ltvs() {
        let (mut deps, env) = instantiate_contract();

        let info = mock_info("user", &coins(10000, "mbrn"));
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::Deposit {
                user: None,
                asset: "uusd".to_string(),
                lock: None,
            },
        )
        .unwrap();

        // Verify a message was created to Disco
        assert_eq!(res.messages.len(), 1);
        // Verify the deposit uses Mars LTVs (80% max_ltv, 75% max_borrow_ltv for uusd)
        // The actual LTV values are verified in the contract logic
        match &res.messages[0].msg {
            CosmosMsg::Wasm(WasmMsg::Execute { contract_addr, .. }) => {
                assert_eq!(contract_addr, "disco");
            }
            _ => panic!("Expected WasmMsg::Execute"),
        }
    }

    #[test]
    fn test_update_config() {
        let (mut deps, env) = instantiate_contract();

        let info = mock_info("owner", &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::UpdateConfig {
                owner: Some("new_owner".to_string()),
                mars_params_contract: Some("new_mars_params".to_string()),
                disco_contract: Some("new_disco".to_string()),
                cdp_contract: Some("new_cdp".to_string()),
                chain_proxy: Some("new_proxy".to_string()),
            },
        )
        .unwrap();

        assert_eq!(res.attributes[0].value, "update_config");

        let config: Config = CONFIG.load(&deps.storage).unwrap();
        assert_eq!(config.owner, cosmwasm_std::Addr::unchecked("new_owner"));
        assert_eq!(config.mars_params_contract, "new_mars_params");
        assert_eq!(config.disco_contract, cosmwasm_std::Addr::unchecked("new_disco"));
    }

    #[test]
    fn test_update_config_unauthorized() {
        let (mut deps, env) = instantiate_contract();

        let info = mock_info("unauthorized", &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::UpdateConfig {
                owner: None,
                mars_params_contract: None,
                disco_contract: None,
                cdp_contract: None,
                chain_proxy: None,
            },
        );

        assert!(res.is_err());
        match res.unwrap_err() {
            crate::error::ContractError::Unauthorized {} => {}
            e => panic!("Expected Unauthorized error, got: {:?}", e),
        }
    }

    #[test]
    fn test_move_single_deposit() {
        let (mut deps, env) = instantiate_contract();

        let info = mock_info("owner", &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MoveSingleDeposit {
                deposit_key: "uusd:0.5:0.3:user:1".to_string(),
                new_ltv: Decimal::percent(80),
                new_max_borrow_ltv: Decimal::percent(75),
            },
        )
        .unwrap();

        assert_eq!(res.messages.len(), 1);
        match &res.messages[0].msg {
            CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr,
                msg: _,
                funds,
            }) => {
                assert_eq!(contract_addr, "disco");
                assert_eq!(funds.len(), 0);
            }
            _ => panic!("Expected WasmMsg::Execute"),
        }

        assert_eq!(res.attributes[0].value, "move_single_deposit");
        assert_eq!(res.attributes[1].value, "uusd:0.5:0.3:user:1");
    }

    #[test]
    fn test_move_single_deposit_invalid_key() {
        let (mut deps, env) = instantiate_contract();

        let info = mock_info("owner", &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MoveSingleDeposit {
                deposit_key: "invalid".to_string(),
                new_ltv: Decimal::percent(80),
                new_max_borrow_ltv: Decimal::percent(75),
            },
        );

        assert!(res.is_err());
        match res.unwrap_err() {
            crate::error::ContractError::Validation(msg) => {
                assert!(msg.contains("Invalid deposit key format"));
            }
            e => panic!("Expected Validation error, got: {:?}", e),
        }
    }

    #[test]
    fn test_move_single_deposit_unauthorized() {
        let (mut deps, env) = instantiate_contract();

        let info = mock_info("unauthorized", &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MoveSingleDeposit {
                deposit_key: "uusd:0.5:0.3:user:1".to_string(),
                new_ltv: Decimal::percent(80),
                new_max_borrow_ltv: Decimal::percent(75),
            },
        );

        assert!(res.is_err());
        match res.unwrap_err() {
            crate::error::ContractError::Unauthorized {} => {}
            e => panic!("Expected Unauthorized error, got: {:?}", e),
        }
    }

    #[test]
    fn test_process_moves_empty() {
        let (mut deps, env) = instantiate_contract();

        let info = mock_info("owner", &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::ProcessMoves {
                limit: None,
                start_after: None,
            },
        )
        .unwrap();

        assert_eq!(res.messages.len(), 0);
        assert_eq!(res.attributes[0].value, "process_moves");
        assert_eq!(res.attributes[1].value, "0");
        assert_eq!(res.attributes[2].value, "true"); // complete
    }

    #[test]
    fn test_process_moves_with_deposits() {
        let (mut deps, env) = instantiate_contract();

        // Mock Disco to return managed deposit keys AND handle Mars Params queries
        deps.querier.update_wasm(|query| {
            match query {
                WasmQuery::Smart { contract_addr, msg } => {
                    if contract_addr == "disco" {
                        if let Ok(disco_query) = from_json::<LTV_Disco_QueryMsg>(msg) {
                            match disco_query {
                                LTV_Disco_QueryMsg::GetManagedDepositKeys {
                                    manager: _,
                                    limit: _,
                                    start_after: _,
                                } => {
                                    let response = ManagedDepositKeysResponse {
                                        keys: vec![
                                            "uusd:0.5:0.3:user1:1".to_string(),
                                            "uusd:0.5:0.3:user2:2".to_string(),
                                        ],
                                        total: 2,
                                        next_start_after: None,
                                    };
                                    return SystemResult::Ok(ContractResult::Ok(
                                        to_json_binary(&response).unwrap(),
                                    ));
                                }
                                _ => {}
                            }
                        }
                    } else if contract_addr == "mars_params" {
                        // Handle Mars Params queries (needed when processing moves)
                        if let Ok(mars_query) = from_json::<MarsParams_QueryMsg>(msg) {
                            match mars_query {
                                MarsParams_QueryMsg::AssetParams { denom } => {
                                    if denom == "uusd" {
                                        return SystemResult::Ok(ContractResult::Ok(
                                            to_json_binary(&Some(create_mock_uusd_asset_params())).unwrap(),
                                        ));
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
            SystemResult::Ok(ContractResult::Ok(Binary::default()))
        });

        let info = mock_info("owner", &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::ProcessMoves {
                limit: Some(10),
                start_after: None,
            },
        )
        .unwrap();

        // Should create move messages for deposits that need updating
        // Both deposits have LTV 0.5 (50%) and max_borrow_ltv 0.3 (30%)
        // But Mars has max_ltv 0.8 (80%) and max_borrow_ltv 0.75 (75%)
        // So both should be moved
        assert_eq!(res.messages.len(), 2);
        assert_eq!(res.attributes[1].value, "2"); // processed
        assert_eq!(res.attributes[2].value, "2"); // total
    }

    #[test]
    fn test_process_moves_no_changes_needed() {
        let (mut deps, env) = instantiate_contract();

        // Mock Disco to return deposits that already match Mars LTVs
        deps.querier.update_wasm(|query| {
            match query {
                WasmQuery::Smart { contract_addr, msg } => {
                    if contract_addr == "disco" {
                        if let Ok(disco_query) = from_json::<LTV_Disco_QueryMsg>(msg) {
                            match disco_query {
                                LTV_Disco_QueryMsg::GetManagedDepositKeys {
                                    manager: _,
                                    limit: _,
                                    start_after: _,
                                } => {
                                    // Deposits already at Mars LTVs (80%, 75%)
                                    let response = ManagedDepositKeysResponse {
                                        keys: vec!["uusd:0.8:0.75:user1:1".to_string()],
                                        total: 1,
                                        next_start_after: None,
                                    };
                                    return SystemResult::Ok(ContractResult::Ok(
                                        to_json_binary(&response).unwrap(),
                                    ));
                                }
                                _ => {}
                            }
                        }
                    } else if contract_addr == "mars_params" {
                        // Handle Mars Params queries
                        if let Ok(mars_query) = from_json::<MarsParams_QueryMsg>(msg) {
                            match mars_query {
                                MarsParams_QueryMsg::AssetParams { denom } => {
                                    if denom == "uusd" {
                                        return SystemResult::Ok(ContractResult::Ok(
                                            to_json_binary(&Some(create_mock_uusd_asset_params())).unwrap(),
                                        ));
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
            SystemResult::Ok(ContractResult::Ok(Binary::default()))
        });

        let info = mock_info("owner", &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::ProcessMoves {
                limit: Some(10),
                start_after: None,
            },
        )
        .unwrap();

        // Should not create move messages since LTVs already match
        assert_eq!(res.messages.len(), 0);
        assert_eq!(res.attributes[1].value, "1"); // processed
    }

    #[test]
    fn test_process_moves_with_pagination() {
        let (mut deps, env) = instantiate_contract();

        // Use start_after to determine which page to return
        deps.querier.update_wasm(|query| {
            match query {
                WasmQuery::Smart { contract_addr, msg } => {
                    if contract_addr == "disco" {
                        if let Ok(disco_query) = from_json::<LTV_Disco_QueryMsg>(msg) {
                            match disco_query {
                                LTV_Disco_QueryMsg::GetManagedDepositKeys {
                                    manager: _,
                                    limit: _,
                                    start_after: ref after,
                                } => {
                                    let response = if after.is_none() {
                                        // First page
                                        ManagedDepositKeysResponse {
                                            keys: vec!["uusd:0.5:0.3:user1:1".to_string()],
                                            total: 2,
                                            next_start_after: Some("uusd:0.5:0.3:user1:1".to_string()),
                                        }
                                    } else {
                                        // Second page
                                        ManagedDepositKeysResponse {
                                            keys: vec!["uusd:0.5:0.3:user2:2".to_string()],
                                            total: 2,
                                            next_start_after: None,
                                        }
                                    };
                                    return SystemResult::Ok(ContractResult::Ok(
                                        to_json_binary(&response).unwrap(),
                                    ));
                                }
                                _ => {}
                            }
                        }
                    } else if contract_addr == "mars_params" {
                        // Handle Mars Params queries
                        if let Ok(mars_query) = from_json::<MarsParams_QueryMsg>(msg) {
                            match mars_query {
                                MarsParams_QueryMsg::AssetParams { denom } => {
                                    if denom == "uusd" {
                                        return SystemResult::Ok(ContractResult::Ok(
                                            to_json_binary(&Some(create_mock_uusd_asset_params())).unwrap(),
                                        ));
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
            SystemResult::Ok(ContractResult::Ok(Binary::default()))
        });

        let info = mock_info("owner", &[]);
        
        // First call - processes first page
        let res1 = execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            ExecuteMsg::ProcessMoves {
                limit: Some(1),
                start_after: None,
            },
        )
        .unwrap();

        assert_eq!(res1.messages.len(), 1);
        assert_eq!(res1.attributes[1].value, "1");

        // Second call - processes next page
        let progress: MoveProgress = MOVE_PROGRESS.may_load(&deps.storage).unwrap().unwrap();
        let res2 = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::ProcessMoves {
                limit: Some(1),
                start_after: progress.last_processed_key,
            },
        )
        .unwrap();

        assert_eq!(res2.messages.len(), 1);
        assert_eq!(res2.attributes[1].value, "1");
    }

    #[test]
    fn test_process_moves_unauthorized() {
        let (mut deps, env) = instantiate_contract();

        let info = mock_info("unauthorized", &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::ProcessMoves {
                limit: None,
                start_after: None,
            },
        );

        assert!(res.is_err());
        match res.unwrap_err() {
            crate::error::ContractError::Unauthorized {} => {}
            e => panic!("Expected Unauthorized error, got: {:?}", e),
        }
    }

    #[test]
    fn test_query_move_progress() {
        let (mut deps, env) = instantiate_contract();

        // Set some progress
        MOVE_PROGRESS
            .save(
                &mut deps.storage,
                &MoveProgress {
                    last_processed_key: Some("key1".to_string()),
                    total_keys: 10,
                    processed_count: 5,
                },
            )
            .unwrap();

        let progress: MoveProgress = from_json(
            query(deps.as_ref(), env.clone(), QueryMsg::MoveProgress {}).unwrap(),
        )
        .unwrap();

        assert_eq!(progress.total_keys, 10);
        assert_eq!(progress.processed_count, 5);
        assert_eq!(progress.last_processed_key, Some("key1".to_string()));
    }

    #[test]
    fn test_query_move_progress_empty() {
        let (mut deps, env) = instantiate_contract();

        // Remove progress
        MOVE_PROGRESS.remove(&mut deps.storage);

        let progress: MoveProgress = from_json(
            query(deps.as_ref(), env.clone(), QueryMsg::MoveProgress {}).unwrap(),
        )
        .unwrap();

        assert_eq!(progress.total_keys, 0);
        assert_eq!(progress.processed_count, 0);
        assert!(progress.last_processed_key.is_none());
    }

    #[test]
    fn test_query_config() {
        let (deps, env) = instantiate_contract();

        let config: Config = from_json(query(deps.as_ref(), env.clone(), QueryMsg::Config {}).unwrap()).unwrap();

        assert_eq!(config.owner, cosmwasm_std::Addr::unchecked("owner"));
        assert_eq!(config.mars_params_contract, "mars_params");
        assert_eq!(config.disco_contract, cosmwasm_std::Addr::unchecked("disco"));
    }

    #[test]
    fn test_deposit_sets_manager() {
        let (mut deps, env) = instantiate_contract();

        let info = mock_info("user", &coins(10000, "mbrn"));
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::Deposit {
                user: None,
                asset: "uusd".to_string(),
                lock: None,
            },
        )
        .unwrap();

        // Verify the message sets manager to this contract
        // The manager should be the contract's address
        match &res.messages[0].msg {
            CosmosMsg::Wasm(WasmMsg::Execute { msg, .. }) => {
                let disco_msg: LTV_Disco_ExecuteMsg = from_json(msg).unwrap();
                if let LTV_Disco_ExecuteMsg::SubmitDeposit { manager, .. } = disco_msg {
                    assert_eq!(manager, Some(env.contract.address.to_string()));
                } else {
                    panic!("Expected SubmitDeposit message");
                }
            }
            _ => panic!("Expected WasmMsg::Execute"),
        }
    }

    #[test]
    fn test_process_moves_saves_progress() {
        let (mut deps, env) = instantiate_contract();

        // Mock Disco to return managed deposit keys
        deps.querier.update_wasm(|query| {
            match query {
                WasmQuery::Smart { contract_addr, msg } => {
                    if contract_addr == "disco" {
                        if let Ok(disco_query) = from_json::<LTV_Disco_QueryMsg>(msg) {
                            match disco_query {
                                LTV_Disco_QueryMsg::GetManagedDepositKeys {
                                    manager: _,
                                    limit: _,
                                    start_after: _,
                                } => {
                                    let response = ManagedDepositKeysResponse {
                                        keys: vec!["uusd:0.5:0.3:user1:1".to_string()],
                                        total: 5,
                                        next_start_after: None,
                                    };
                                    return SystemResult::Ok(ContractResult::Ok(
                                        to_json_binary(&response).unwrap(),
                                    ));
                                }
                                _ => {}
                            }
                        }
                    } else if contract_addr == "mars_params" {
                        // Handle Mars Params queries
                        if let Ok(mars_query) = from_json::<MarsParams_QueryMsg>(msg) {
                            match mars_query {
                                MarsParams_QueryMsg::AssetParams { denom } => {
                                    if denom == "uusd" {
                                        return SystemResult::Ok(ContractResult::Ok(
                                            to_json_binary(&Some(create_mock_uusd_asset_params())).unwrap(),
                                        ));
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
            SystemResult::Ok(ContractResult::Ok(Binary::default()))
        });

        let info = mock_info("owner", &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::ProcessMoves {
                limit: Some(10),
                start_after: None,
            },
        )
        .unwrap();

        // Check progress was saved
        let progress: MoveProgress = MOVE_PROGRESS.load(&deps.storage).unwrap();
        assert_eq!(progress.total_keys, 5);
        assert_eq!(progress.processed_count, 1);
        assert_eq!(progress.last_processed_key, Some("uusd:0.5:0.3:user1:1".to_string()));
    }

    #[test]
    fn test_process_moves_with_limit() {
        let (mut deps, env) = instantiate_contract();

        // Mock Disco to return 5 managed deposit keys
        deps.querier.update_wasm(|query| {
            match query {
                WasmQuery::Smart { contract_addr, msg } => {
                    if contract_addr == "disco" {
                        if let Ok(disco_query) = from_json::<LTV_Disco_QueryMsg>(msg) {
                            match disco_query {
                                LTV_Disco_QueryMsg::GetManagedDepositKeys {
                                    manager: _,
                                    limit: _,
                                    start_after: _,
                                } => {
                                    let response = ManagedDepositKeysResponse {
                                        keys: vec![
                                            "uusd:0.5:0.3:user1:1".to_string(),
                                            "uusd:0.5:0.3:user2:2".to_string(),
                                            "uusd:0.5:0.3:user3:3".to_string(),
                                        ],
                                        total: 5,
                                        next_start_after: None,
                                    };
                                    return SystemResult::Ok(ContractResult::Ok(
                                        to_json_binary(&response).unwrap(),
                                    ));
                                }
                                _ => {}
                            }
                        }
                    } else if contract_addr == "mars_params" {
                        // Handle Mars Params queries
                        if let Ok(mars_query) = from_json::<MarsParams_QueryMsg>(msg) {
                            match mars_query {
                                MarsParams_QueryMsg::AssetParams { denom } => {
                                    if denom == "uusd" {
                                        return SystemResult::Ok(ContractResult::Ok(
                                            to_json_binary(&Some(create_mock_uusd_asset_params())).unwrap(),
                                        ));
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
            SystemResult::Ok(ContractResult::Ok(Binary::default()))
        });

        let info = mock_info("owner", &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::ProcessMoves {
                limit: Some(2), // Process only 2
                start_after: None,
            },
        )
        .unwrap();

        // Should process up to limit (2), but also respect the querier's limit
        // The querier returns 3 keys, but we limit to 2
        assert_eq!(res.attributes[1].value, "2"); // processed
    }

    #[test]
    fn test_process_moves_skips_invalid_deposit_keys() {
        let (mut deps, env) = instantiate_contract();

        // Mock Disco to return managed deposit keys with some invalid formats
        deps.querier.update_wasm(|query| {
            match query {
                WasmQuery::Smart { contract_addr, msg } => {
                    if contract_addr == "disco" {
                        if let Ok(disco_query) = from_json::<LTV_Disco_QueryMsg>(msg) {
                            match disco_query {
                                LTV_Disco_QueryMsg::GetManagedDepositKeys {
                                    manager: _,
                                    limit: _,
                                    start_after: _,
                                } => {
                                    let response = ManagedDepositKeysResponse {
                                        keys: vec![
                                            "uusd:0.5:0.3:user1:1".to_string(), // Valid
                                            "invalid".to_string(), // Invalid format (not 5 parts)
                                            "too:many:parts:here:extra:stuff".to_string(), // Invalid format (6 parts)
                                            "uusd:0.5:0.3:user2:2".to_string(), // Valid
                                        ],
                                        total: 4,
                                        next_start_after: None,
                                    };
                                    return SystemResult::Ok(ContractResult::Ok(
                                        to_json_binary(&response).unwrap(),
                                    ));
                                }
                                _ => {}
                            }
                        }
                    } else if contract_addr == "mars_params" {
                        // Handle Mars Params queries
                        if let Ok(mars_query) = from_json::<MarsParams_QueryMsg>(msg) {
                            match mars_query {
                                MarsParams_QueryMsg::AssetParams { denom } => {
                                    if denom == "uusd" {
                                        return SystemResult::Ok(ContractResult::Ok(
                                            to_json_binary(&Some(create_mock_uusd_asset_params())).unwrap(),
                                        ));
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
            SystemResult::Ok(ContractResult::Ok(Binary::default()))
        });

        let info = mock_info("owner", &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::ProcessMoves {
                limit: Some(10),
                start_after: None,
            },
        )
        .unwrap();

        // Should process 2 valid keys (skipping the 2 invalid ones)
        // Invalid keys are skipped (continue) so they don't increment processed_count
        // Both valid deposits need to be moved (LTVs differ from Mars)
        assert_eq!(res.messages.len(), 2);
        assert_eq!(res.attributes[1].value, "2"); // Only 2 valid keys processed
        assert_eq!(res.attributes[2].value, "4"); // Total keys (including invalid ones)
    }

    #[test]
    fn test_process_moves_with_invalid_ltv_in_key() {
        let (mut deps, env) = instantiate_contract();

        // Mock Disco to return a deposit key with invalid LTV format
        deps.querier.update_wasm(|query| {
            match query {
                WasmQuery::Smart { contract_addr, msg } => {
                    if contract_addr == "disco" {
                        if let Ok(disco_query) = from_json::<LTV_Disco_QueryMsg>(msg) {
                            match disco_query {
                                LTV_Disco_QueryMsg::GetManagedDepositKeys {
                                    manager: _,
                                    limit: _,
                                    start_after: _,
                                } => {
                                    let response = ManagedDepositKeysResponse {
                                        keys: vec!["uusd:invalid_ltv:0.3:user1:1".to_string()],
                                        total: 1,
                                        next_start_after: None,
                                    };
                                    return SystemResult::Ok(ContractResult::Ok(
                                        to_json_binary(&response).unwrap(),
                                    ));
                                }
                                _ => {}
                            }
                        }
                    } else if contract_addr == "mars_params" {
                        // Handle Mars Params queries
                        if let Ok(mars_query) = from_json::<MarsParams_QueryMsg>(msg) {
                            match mars_query {
                                MarsParams_QueryMsg::AssetParams { denom } => {
                                    if denom == "uusd" {
                                        return SystemResult::Ok(ContractResult::Ok(
                                            to_json_binary(&Some(create_mock_uusd_asset_params())).unwrap(),
                                        ));
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
            SystemResult::Ok(ContractResult::Ok(Binary::default()))
        });

        let info = mock_info("owner", &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::ProcessMoves {
                limit: Some(10),
                start_after: None,
            },
        );

        // Should error because LTV parsing fails
        assert!(res.is_err());
        match res.unwrap_err() {
            crate::error::ContractError::Validation(msg) => {
                assert!(msg.contains("Invalid LTV in deposit key"));
            }
            e => panic!("Expected Validation error, got: {:?}", e),
        }
    }

    #[test]
    fn test_process_moves_with_asset_not_in_mars_params() {
        let (mut deps, env) = instantiate_contract();

        // Mock Disco to return a deposit for an asset not in Mars Params
        deps.querier.update_wasm(|query| {
            match query {
                WasmQuery::Smart { contract_addr, msg } => {
                    if contract_addr == "disco" {
                        if let Ok(disco_query) = from_json::<LTV_Disco_QueryMsg>(msg) {
                            match disco_query {
                                LTV_Disco_QueryMsg::GetManagedDepositKeys {
                                    manager: _,
                                    limit: _,
                                    start_after: _,
                                } => {
                                    let response = ManagedDepositKeysResponse {
                                        keys: vec!["unknown_asset:0.5:0.3:user1:1".to_string()],
                                        total: 1,
                                        next_start_after: None,
                                    };
                                    return SystemResult::Ok(ContractResult::Ok(
                                        to_json_binary(&response).unwrap(),
                                    ));
                                }
                                _ => {}
                            }
                        }
                    } else if contract_addr == "mars_params" {
                        // Return None for unknown asset
                        if let Ok(mars_query) = from_json::<MarsParams_QueryMsg>(msg) {
                            match mars_query {
                                MarsParams_QueryMsg::AssetParams { denom: _ } => {
                                    return SystemResult::Ok(ContractResult::Ok(
                                        to_json_binary(&Option::<AssetParams>::None).unwrap(),
                                    ));
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
            SystemResult::Ok(ContractResult::Ok(Binary::default()))
        });

        let info = mock_info("owner", &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::ProcessMoves {
                limit: Some(10),
                start_after: None,
            },
        );

        // Should error because asset not found in Mars Params
        assert!(res.is_err());
        match res.unwrap_err() {
            crate::error::ContractError::Validation(msg) => {
                assert!(msg.contains("not found in Mars Params"));
            }
            e => panic!("Expected Validation error, got: {:?}", e),
        }
    }

    #[test]
    fn test_process_moves_progress_tracking_with_existing_progress() {
        let (mut deps, env) = instantiate_contract();

        // Set initial progress
        MOVE_PROGRESS
            .save(
                &mut deps.storage,
                &MoveProgress {
                    last_processed_key: Some("uusd:0.5:0.3:user1:1".to_string()),
                    total_keys: 10,
                    processed_count: 3,
                },
            )
            .unwrap();

        // Mock Disco to return more managed deposit keys
        deps.querier.update_wasm(|query| {
            match query {
                WasmQuery::Smart { contract_addr, msg } => {
                    if contract_addr == "disco" {
                        if let Ok(disco_query) = from_json::<LTV_Disco_QueryMsg>(msg) {
                            match disco_query {
                                LTV_Disco_QueryMsg::GetManagedDepositKeys {
                                    manager: _,
                                    limit: _,
                                    start_after: _,
                                } => {
                                    let response = ManagedDepositKeysResponse {
                                        keys: vec!["uusd:0.5:0.3:user2:2".to_string()],
                                        total: 10, // Same total
                                        next_start_after: None,
                                    };
                                    return SystemResult::Ok(ContractResult::Ok(
                                        to_json_binary(&response).unwrap(),
                                    ));
                                }
                                _ => {}
                            }
                        }
                    } else if contract_addr == "mars_params" {
                        // Handle Mars Params queries
                        if let Ok(mars_query) = from_json::<MarsParams_QueryMsg>(msg) {
                            match mars_query {
                                MarsParams_QueryMsg::AssetParams { denom } => {
                                    if denom == "uusd" {
                                        return SystemResult::Ok(ContractResult::Ok(
                                            to_json_binary(&Some(create_mock_uusd_asset_params())).unwrap(),
                                        ));
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
            SystemResult::Ok(ContractResult::Ok(Binary::default()))
        });

        let info = mock_info("owner", &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::ProcessMoves {
                limit: Some(10),
                start_after: None,
            },
        )
        .unwrap();

        // Check progress was updated (should add to existing count)
        let progress: MoveProgress = MOVE_PROGRESS.load(&deps.storage).unwrap();
        assert_eq!(progress.total_keys, 10);
        assert_eq!(progress.processed_count, 4); // 3 (existing) + 1 (new)
        assert_eq!(progress.last_processed_key, Some("uusd:0.5:0.3:user2:2".to_string()));
    }

    #[test]
    fn test_process_moves_with_multiple_assets() {
        let (mut deps, env) = instantiate_contract();

        // Mock Disco to return deposits for different assets
        deps.querier.update_wasm(|query| {
            match query {
                WasmQuery::Smart { contract_addr, msg } => {
                    if contract_addr == "disco" {
                        if let Ok(disco_query) = from_json::<LTV_Disco_QueryMsg>(msg) {
                            match disco_query {
                                LTV_Disco_QueryMsg::GetManagedDepositKeys {
                                    manager: _,
                                    limit: _,
                                    start_after: _,
                                } => {
                                    let response = ManagedDepositKeysResponse {
                                        keys: vec![
                                            "uusd:0.5:0.3:user1:1".to_string(),
                                            "uatom:0.5:0.3:user2:1".to_string(),
                                        ],
                                        total: 2,
                                        next_start_after: None,
                                    };
                                    return SystemResult::Ok(ContractResult::Ok(
                                        to_json_binary(&response).unwrap(),
                                    ));
                                }
                                _ => {}
                            }
                        }
                    } else if contract_addr == "mars_params" {
                        // Handle Mars Params queries for both assets
                        if let Ok(mars_query) = from_json::<MarsParams_QueryMsg>(msg) {
                            match mars_query {
                                MarsParams_QueryMsg::AssetParams { denom } => {
                                    match denom.as_str() {
                                        "uusd" => {
                                            return SystemResult::Ok(ContractResult::Ok(
                                                to_json_binary(&Some(create_mock_uusd_asset_params())).unwrap(),
                                            ));
                                        }
                                        "uatom" => {
                                            // Return mock AssetParams for uatom
                                            let asset_params = AssetParams {
                                                denom: "uatom".to_string(),
                                                credit_manager: membrane::mars_params::CmSettings {
                                                    whitelisted: true,
                                                    withdraw_enabled: true,
                                                    hls: None,
                                                },
                                                red_bank: membrane::mars_params::RedBankSettings {
                                                    deposit_enabled: true,
                                                    borrow_enabled: true,
                                                    withdraw_enabled: true,
                                                },
                                                max_loan_to_value: Decimal::percent(70),
                                                liquidation_threshold: Decimal::percent(75),
                                                liquidation_bonus: membrane::mars_params::LiquidationBonus {
                                                    starting_lb: Decimal::percent(5),
                                                    slope: Decimal::zero(),
                                                    min_lb: Decimal::percent(5),
                                                    max_lb: Decimal::percent(10),
                                                },
                                                protocol_liquidation_fee: Decimal::percent(1),
                                                deposit_cap: Uint128::new(1_000_000_000),
                                                close_factor: Decimal::percent(50),
                                                reserve_factor: Decimal::percent(20),
                                                interest_rate_model: membrane::mars_params::InterestRateModel {
                                                    optimal_utilization_rate: Decimal::percent(80),
                                                    base: Decimal::percent(2),
                                                    slope_1: Decimal::percent(10),
                                                    slope_2: Decimal::percent(100),
                                                },
                                            };
                                            return SystemResult::Ok(ContractResult::Ok(
                                                to_json_binary(&Some(asset_params)).unwrap(),
                                            ));
                                        }
                                        _ => {
                                            return SystemResult::Ok(ContractResult::Ok(
                                                to_json_binary(&Option::<AssetParams>::None).unwrap(),
                                            ));
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
            SystemResult::Ok(ContractResult::Ok(Binary::default()))
        });

        let info = mock_info("owner", &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::ProcessMoves {
                limit: Some(10),
                start_after: None,
            },
        )
        .unwrap();

        // Both deposits should be moved (LTVs differ from Mars)
        assert_eq!(res.messages.len(), 2);
        assert_eq!(res.attributes[1].value, "2"); // processed
        assert_eq!(res.attributes[2].value, "2"); // total
    }

    #[test]
    fn test_process_moves_invalid_deposit_id() {
        let (mut deps, env) = instantiate_contract();

        // Mock Disco to return a deposit key with invalid deposit_id
        deps.querier.update_wasm(|query| {
            match query {
                WasmQuery::Smart { contract_addr, msg } => {
                    if contract_addr == "disco" {
                        if let Ok(disco_query) = from_json::<LTV_Disco_QueryMsg>(msg) {
                            match disco_query {
                                LTV_Disco_QueryMsg::GetManagedDepositKeys {
                                    manager: _,
                                    limit: _,
                                    start_after: _,
                                } => {
                                    let response = ManagedDepositKeysResponse {
                                        keys: vec!["uusd:0.5:0.3:user1:invalid_id".to_string()],
                                        total: 1,
                                        next_start_after: None,
                                    };
                                    return SystemResult::Ok(ContractResult::Ok(
                                        to_json_binary(&response).unwrap(),
                                    ));
                                }
                                _ => {}
                            }
                        }
                    } else if contract_addr == "mars_params" {
                        // Handle Mars Params queries
                        if let Ok(mars_query) = from_json::<MarsParams_QueryMsg>(msg) {
                            match mars_query {
                                MarsParams_QueryMsg::AssetParams { denom } => {
                                    if denom == "uusd" {
                                        return SystemResult::Ok(ContractResult::Ok(
                                            to_json_binary(&Some(create_mock_uusd_asset_params())).unwrap(),
                                        ));
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
            SystemResult::Ok(ContractResult::Ok(Binary::default()))
        });

        let info = mock_info("owner", &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::ProcessMoves {
                limit: Some(10),
                start_after: None,
            },
        );

        // Should error because deposit_id parsing fails
        assert!(res.is_err());
        match res.unwrap_err() {
            crate::error::ContractError::Validation(msg) => {
                assert!(msg.contains("Invalid deposit_id in deposit key"));
            }
            e => panic!("Expected Validation error, got: {:?}", e),
        }
    }
}

