use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info, MockApi, MockQuerier};
use cosmwasm_std::{from_json, Decimal, Uint128, OwnedDeps, MemoryStorage, to_json_binary, SystemResult, ContractResult};
use membrane::ltv_disco::{Config, InstantiateMsg, LTVQueue, QueryMsg, TotalInsuranceResponse};
use membrane::types::{DepositDenom, AssetInfo};
use membrane::oracle::{QueryMsg as Oracle_QueryMsg, PriceResponse};

use ltv_disco::contract::{instantiate, query};
use ltv_disco::state::{CONFIG, DISPERSAL, LTV_QUEUES};
use membrane::ltv_disco::{Dispersal, ActiveDispersal, MaxLTVSlot, MaxBorrowLTVGroup};

/// Helper to create a standard config for testing
fn create_test_config(deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>) -> Config {
    let msg = InstantiateMsg {
        owner: Some("owner".to_string()),
        cdp_contract: "cdp_contract".to_string(),
        deposit_denom: DepositDenom {
            denom: "uusd".to_string(),
            vault_info: None,
        },
        cdt_denom: "cdt".to_string(),
        minimum_deposit: Uint128::new(1000),
        max_ltv: Decimal::percent(95),
        percent_to_disperse: Decimal::percent(10),
        dispersal_window: 24,
        activation_window: 48,
        oracle_contract: "oracle".to_string(),
        chain_proxy_contract: "chain_proxy".to_string(),
        lock_duration_ceiling: Some(365),
        affiliate_fee: Some(Decimal::percent(1)),
        max_management_fee: None,
        ltv_delta_minimum: Some(Decimal::percent(1)),
        emissions_voting_contract: None,
        points_system_contract: None,
        revenue_distributor: None,
        auction_contract: None,
        mbrn_denom: None,
    };
    
    let info = mock_info("creator", &[]);
    let env = mock_env();
    instantiate(deps.as_mut(), env, info, msg).unwrap();
    
    CONFIG.load(&deps.storage).unwrap()
}

/// Helper to setup mock oracle responses
fn setup_mock_oracle(
    querier: &mut MockQuerier,
    asset_denom: &str,
    asset_price: Decimal,
    cdt_denom: &str,
    cdt_price: Decimal,
) {
    let asset_info = AssetInfo::NativeToken {
        denom: asset_denom.to_string(),
    };
    let cdt_info = AssetInfo::NativeToken {
        denom: cdt_denom.to_string(),
    };
    
    let prices = vec![
        PriceResponse {
            prices: vec![],
            price: asset_price,
            decimals: 6,
        },
        PriceResponse {
            prices: vec![],
            price: cdt_price,
            decimals: 6,
        },
    ];
    
    querier.update_wasm(move |query| {
        match query {
            cosmwasm_std::WasmQuery::Smart { contract_addr, msg } => {
                if contract_addr == "oracle" {
                    match from_json::<Oracle_QueryMsg>(msg) {
                        Ok(Oracle_QueryMsg::Prices { asset_infos, .. }) => {
                            if asset_infos.len() == 2 
                                && asset_infos[0].equal(&asset_info)
                                && asset_infos[1].equal(&cdt_info) {
                                SystemResult::Ok(ContractResult::Ok(to_json_binary(&prices).unwrap()))
                            } else {
                                SystemResult::Err(cosmwasm_std::SystemError::InvalidRequest {
                                    error: "Unexpected asset infos".to_string(),
                                    request: msg.clone(),
                                })
                            }
                        }
                        _ => SystemResult::Err(cosmwasm_std::SystemError::InvalidRequest {
                            error: "Unexpected query".to_string(),
                            request: msg.clone(),
                        }),
                    }
                } else {
                    SystemResult::Err(cosmwasm_std::SystemError::InvalidRequest {
                        error: "Unknown contract".to_string(),
                        request: msg.clone(),
                    })
                }
            }
            _ => SystemResult::Err(cosmwasm_std::SystemError::InvalidRequest {
                error: "Unexpected query type".to_string(),
                request: Default::default(),
            }),
        }
    });
}

#[test]
fn test_query_total_insurance_with_oracle() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let _config = create_test_config(&mut deps);
    
    // Setup mock oracle: 1 uusd = $1, 1 cdt = $1
    setup_mock_oracle(&mut deps.querier, "uusd", Decimal::one(), "cdt", Decimal::one());
    
    // Create a queue with deposits
    let queue = LTVQueue {
        slots: vec![MaxLTVSlot {
            ltv: Decimal::percent(75),
            deposit_groups: vec![MaxBorrowLTVGroup {
                max_borrow_ltv: Decimal::percent(50),
                total_deposit_tokens: Uint128::new(1_000_000), // 1M uusd
                total_vault_tokens: Uint128::new(1_000_000),
                total_locked_vault_tokens: Uint128::zero(),
                total_unused_locked_vault_tokens: Uint128::zero(),
                effective_epoch_start: None,
                lvt_tracking: membrane::ltv_disco::GroupLVTTracking {
                    base_total: Uint128::zero(),
                    reference_time: 0,
                    base_daily_delta: cosmwasm_std::Int128::zero(),
                    time_cliffs: vec![],
                },
            }],
            total_deposit_tokens: Uint128::new(1_000_000),
            bad_debt: Uint128::zero(),
        }],
        borrow_ltv: membrane::ltv_disco::DecimalMinMax {
            min: Decimal::zero(),
            max: Decimal::percent(50),
        },
        liquidation_ltv: membrane::ltv_disco::DecimalMinMax {
            min: Decimal::percent(50),
            max: Decimal::percent(75),
        },
        current_deposit_id: Uint128::one(),
        percent_to_disperse: None,
    };
    
    LTV_QUEUES.save(&mut deps.storage, "uusd".to_string(), &queue).unwrap();
    
    // Add pending dispersal
    let dispersal = Dispersal {
        total_to_disperse: Uint128::new(500_000), // 500k CDT
        dispersal_window: 24,
        active_dispersal: ActiveDispersal {
            dispersal_start: env.block.time.seconds(),
            amount_dispersed: Uint128::new(200_000), // 200k already dispersed
        },
        pending_dispersal: Uint128::new(100_000), // 100k pending
    };
    DISPERSAL.save(&mut deps.storage, "uusd".to_string(), &dispersal).unwrap();
    
    // Query total insurance
    let response: TotalInsuranceResponse = from_json(
        query(deps.as_ref(), env.clone(), QueryMsg::GetTotalInsurance {}).unwrap()
    ).unwrap();
    
    // Expected: 1M uusd deposits = 1M CDT (via oracle) + 400k CDT (300k available from active + 100k pending) = 1.4M CDT
    match response {
        TotalInsuranceResponse::WithOracle { total_insurance } => {
            assert_eq!(total_insurance, Uint128::new(1_400_000));
        }
        _ => panic!("Expected WithOracle response"),
    }
}

#[test]
fn test_query_total_insurance_without_oracle() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let _config = create_test_config(&mut deps);
    
    // Don't setup oracle - will fail
    
    // Create a queue with deposits
    let queue = LTVQueue {
        slots: vec![MaxLTVSlot {
            ltv: Decimal::percent(75),
            deposit_groups: vec![MaxBorrowLTVGroup {
                max_borrow_ltv: Decimal::percent(50),
                total_deposit_tokens: Uint128::new(1_000_000), // 1M uusd
                total_vault_tokens: Uint128::new(1_000_000),
                total_locked_vault_tokens: Uint128::zero(),
                total_unused_locked_vault_tokens: Uint128::zero(),
                effective_epoch_start: None,
                lvt_tracking: membrane::ltv_disco::GroupLVTTracking {
                    base_total: Uint128::zero(),
                    reference_time: 0,
                    base_daily_delta: cosmwasm_std::Int128::zero(),
                    time_cliffs: vec![],
                },
            }],
            total_deposit_tokens: Uint128::new(1_000_000),
            bad_debt: Uint128::zero(),
        }],
        borrow_ltv: membrane::ltv_disco::DecimalMinMax {
            min: Decimal::zero(),
            max: Decimal::percent(50),
        },
        liquidation_ltv: membrane::ltv_disco::DecimalMinMax {
            min: Decimal::percent(50),
            max: Decimal::percent(75),
        },
        current_deposit_id: Uint128::one(),
        percent_to_disperse: None,
    };
    
    LTV_QUEUES.save(&mut deps.storage, "uusd".to_string(), &queue).unwrap();
    
    // Add pending dispersal
    let dispersal = Dispersal {
        total_to_disperse: Uint128::new(500_000),
        dispersal_window: 24,
        active_dispersal: ActiveDispersal {
            dispersal_start: env.block.time.seconds(),
            amount_dispersed: Uint128::new(200_000),
        },
        pending_dispersal: Uint128::new(100_000),
    };
    DISPERSAL.save(&mut deps.storage, "uusd".to_string(), &dispersal).unwrap();
    
    // Query total insurance
    let response: TotalInsuranceResponse = from_json(
        query(deps.as_ref(), env.clone(), QueryMsg::GetTotalInsurance {}).unwrap()
    ).unwrap();
    
    // Should return WithoutOracle with breakdown
    match response {
        TotalInsuranceResponse::WithoutOracle { pending_cdt, mbrn_deposit_totals } => {
            assert_eq!(pending_cdt, Uint128::new(400_000)); // 300k available + 100k pending
            assert_eq!(mbrn_deposit_totals.len(), 1);
            assert_eq!(mbrn_deposit_totals[0].0, "uusd");
            assert_eq!(mbrn_deposit_totals[0].1, Uint128::new(1_000_000));
        }
        _ => panic!("Expected WithoutOracle response"),
    }
}

#[test]
fn test_query_total_insurance_multiple_assets() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let _config = create_test_config(&mut deps);
    
    // Setup mock oracle for both assets
    // 1 uusd = $1, 1 untrn = $10, 1 cdt = $1
    // So 1 untrn = 10 CDT
    
    // For uusd
    let uusd_info = AssetInfo::NativeToken { denom: "uusd".to_string() };
    let cdt_info = AssetInfo::NativeToken { denom: "cdt".to_string() };
    
    // For untrn
    let untrn_info = AssetInfo::NativeToken { denom: "untrn".to_string() };
    
    deps.querier.update_wasm(move |query| {
        match query {
            cosmwasm_std::WasmQuery::Smart { contract_addr, msg } => {
                if contract_addr == "oracle" {
                    match from_json::<Oracle_QueryMsg>(msg) {
                        Ok(Oracle_QueryMsg::Prices { asset_infos, .. }) => {
                            if asset_infos.len() == 2 {
                                if asset_infos[0].equal(&uusd_info) && asset_infos[1].equal(&cdt_info) {
                                    // uusd -> cdt: 1:1
                                    SystemResult::Ok(ContractResult::Ok(to_json_binary(&vec![
                                        PriceResponse { prices: vec![], price: Decimal::one(), decimals: 6 },
                                        PriceResponse { prices: vec![], price: Decimal::one(), decimals: 6 },
                                    ]).unwrap()))
                                } else if asset_infos[0].equal(&untrn_info) && asset_infos[1].equal(&cdt_info) {
                                    // untrn -> cdt: 10:1
                                    SystemResult::Ok(ContractResult::Ok(to_json_binary(&vec![
                                        PriceResponse { prices: vec![], price: Decimal::from_ratio(10u128, 1u128), decimals: 6 },
                                        PriceResponse { prices: vec![], price: Decimal::one(), decimals: 6 },
                                    ]).unwrap()))
                                } else {
                                    SystemResult::Err(cosmwasm_std::SystemError::InvalidRequest {
                                        error: "Unexpected asset pair".to_string(),
                                        request: msg.clone(),
                                    })
                                }
                            } else {
                                SystemResult::Err(cosmwasm_std::SystemError::InvalidRequest {
                                    error: "Expected 2 assets".to_string(),
                                    request: msg.clone(),
                                })
                            }
                        }
                        _ => SystemResult::Err(cosmwasm_std::SystemError::InvalidRequest {
                            error: "Unexpected query".to_string(),
                            request: msg.clone(),
                        }),
                    }
                } else {
                    SystemResult::Err(cosmwasm_std::SystemError::InvalidRequest {
                        error: "Unknown contract".to_string(),
                        request: msg.clone(),
                    })
                }
            }
            _ => SystemResult::Err(cosmwasm_std::SystemError::InvalidRequest {
                error: "Unexpected query type".to_string(),
                request: Default::default(),
            }),
        }
    });
    
    // Create queue for uusd with 1M deposits
    let queue_uusd = LTVQueue {
        slots: vec![MaxLTVSlot {
            ltv: Decimal::percent(75),
            deposit_groups: vec![MaxBorrowLTVGroup {
                max_borrow_ltv: Decimal::percent(50),
                total_deposit_tokens: Uint128::new(1_000_000),
                total_vault_tokens: Uint128::new(1_000_000),
                total_locked_vault_tokens: Uint128::zero(),
                total_unused_locked_vault_tokens: Uint128::zero(),
                effective_epoch_start: None,
                lvt_tracking: membrane::ltv_disco::GroupLVTTracking {
                    base_total: Uint128::zero(),
                    reference_time: 0,
                    base_daily_delta: cosmwasm_std::Int128::zero(),
                    time_cliffs: vec![],
                },
            }],
            total_deposit_tokens: Uint128::new(1_000_000),
            bad_debt: Uint128::zero(),
        }],
        borrow_ltv: membrane::ltv_disco::DecimalMinMax {
            min: Decimal::zero(),
            max: Decimal::percent(50),
        },
        liquidation_ltv: membrane::ltv_disco::DecimalMinMax {
            min: Decimal::percent(50),
            max: Decimal::percent(75),
        },
        current_deposit_id: Uint128::one(),
        percent_to_disperse: None,
    };
    LTV_QUEUES.save(&mut deps.storage, "uusd".to_string(), &queue_uusd).unwrap();
    
    // Create queue for untrn with 100k deposits (worth 1M CDT)
    let queue_untrn = LTVQueue {
        slots: vec![MaxLTVSlot {
            ltv: Decimal::percent(75),
            deposit_groups: vec![MaxBorrowLTVGroup {
                max_borrow_ltv: Decimal::percent(50),
                total_deposit_tokens: Uint128::new(100_000),
                total_vault_tokens: Uint128::new(100_000),
                total_locked_vault_tokens: Uint128::zero(),
                total_unused_locked_vault_tokens: Uint128::zero(),
                effective_epoch_start: None,
                lvt_tracking: membrane::ltv_disco::GroupLVTTracking {
                    base_total: Uint128::zero(),
                    reference_time: 0,
                    base_daily_delta: cosmwasm_std::Int128::zero(),
                    time_cliffs: vec![],
                },
            }],
            total_deposit_tokens: Uint128::new(100_000),
            bad_debt: Uint128::zero(),
        }],
        borrow_ltv: membrane::ltv_disco::DecimalMinMax {
            min: Decimal::zero(),
            max: Decimal::percent(50),
        },
        liquidation_ltv: membrane::ltv_disco::DecimalMinMax {
            min: Decimal::percent(50),
            max: Decimal::percent(75),
        },
        current_deposit_id: Uint128::one(),
        percent_to_disperse: None,
    };
    LTV_QUEUES.save(&mut deps.storage, "untrn".to_string(), &queue_untrn).unwrap();
    
    // Add dispersals for both assets
    let dispersal_uusd = Dispersal {
        total_to_disperse: Uint128::new(500_000),
        dispersal_window: 24,
        active_dispersal: ActiveDispersal {
            dispersal_start: env.block.time.seconds(),
            amount_dispersed: Uint128::new(200_000),
        },
        pending_dispersal: Uint128::new(100_000),
    };
    DISPERSAL.save(&mut deps.storage, "uusd".to_string(), &dispersal_uusd).unwrap();
    
    let dispersal_untrn = Dispersal {
        total_to_disperse: Uint128::new(200_000),
        dispersal_window: 24,
        active_dispersal: ActiveDispersal {
            dispersal_start: 0, // No active dispersal
            amount_dispersed: Uint128::zero(),
        },
        pending_dispersal: Uint128::new(200_000),
    };
    DISPERSAL.save(&mut deps.storage, "untrn".to_string(), &dispersal_untrn).unwrap();
    
    // Query total insurance
    let response: TotalInsuranceResponse = from_json(
        query(deps.as_ref(), env.clone(), QueryMsg::GetTotalInsurance {}).unwrap()
    ).unwrap();
    
    // Expected: 
    // - 1M uusd = 1M CDT
    // - 100k untrn = 1M CDT (100k * 10)
    // - Pending CDT: 400k (uusd: 300k available + 100k pending) + 200k (untrn) = 600k
    // Total: 1M + 1M + 600k = 2.6M CDT
    match response {
        TotalInsuranceResponse::WithOracle { total_insurance } => {
            assert_eq!(total_insurance, Uint128::new(2_600_000));
        }
        _ => panic!("Expected WithOracle response"),
    }
}

#[test]
fn test_query_total_insurance_no_deposits() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let _config = create_test_config(&mut deps);
    
    // No queues, no deposits
    
    // Query total insurance
    let response: TotalInsuranceResponse = from_json(
        query(deps.as_ref(), env.clone(), QueryMsg::GetTotalInsurance {}).unwrap()
    ).unwrap();
    
    // Should return WithOracle with zero (or WithoutOracle with empty)
    match response {
        TotalInsuranceResponse::WithOracle { total_insurance } => {
            assert_eq!(total_insurance, Uint128::zero());
        }
        TotalInsuranceResponse::WithoutOracle { pending_cdt, mbrn_deposit_totals } => {
            assert_eq!(pending_cdt, Uint128::zero());
            assert_eq!(mbrn_deposit_totals.len(), 0);
        }
    }
}
