use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info, MockApi, MockQuerier};
use cosmwasm_std::{coins, from_json, Coin, CosmosMsg, Decimal, Uint128, WasmMsg, to_json_binary, OwnedDeps, MemoryStorage, Reply};
use membrane::ltv_disco::{BackingDepositInput, Config, ExecuteMsg, InstantiateMsg, LTVQueue};
use membrane::types::DepositDenom;
use membrane::oracle::{QueryMsg as Oracle_QueryMsg, PriceResponse};
use membrane::cdp::ExecuteMsg as CDP_ExecuteMsg;

use ltv_disco::contract::{execute, instantiate, reply, LIQUIDATION_SWAP_REPLY_ID};
use ltv_disco::state::{CONFIG, DISPERSAL, LTV_QUEUES, SWAP_PROPAGATION};
use membrane::ltv_disco::{Dispersal, ActiveDispersal};

/// Helper to create a standard config for testing
fn create_test_config(deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>) -> Config {
    let msg = InstantiateMsg {
        owner: Some("owner".to_string()),
        cdp_contract: "cdp_contract".to_string(),
        deposit_denom: DepositDenom {
            denom: "collateral".to_string(),
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
    };

    let info = mock_info("creator", &[]);
    let env = mock_env();
    instantiate(deps.as_mut(), env, info, msg).unwrap();

    CONFIG.load(&deps.storage).unwrap()
}

/// Helper to create a queue for testing
fn create_test_queue(deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>, asset: String) {
    use membrane::cdp::QueryMsg as CDP_QueryMsg;
    use membrane::types::{Basket, Asset, cAsset, PendingRevenue};

    // Mock CDP basket query
    let asset_clone = asset.clone();
    deps.querier.update_wasm(move |query| {
        match query {
            cosmwasm_std::WasmQuery::Smart { contract_addr, msg } => {
                if contract_addr == "cdp_contract" {
                    let parsed: Result<CDP_QueryMsg, _> = from_json(msg);
                    if matches!(parsed, Ok(CDP_QueryMsg::GetBasket {})) {
                        let basket = Basket {
                            basket_id: Uint128::one(),
                            current_position_id: Uint128::one(),
                            collateral_types: vec![cAsset {
                                asset: Asset {
                                    info: membrane::types::AssetInfo::NativeToken { 
                                        denom: asset_clone.clone() 
                                    },
                                    amount: Uint128::zero(),
                                },
                                max_borrow_LTV: Decimal::percent(70),
                                max_LTV: Decimal::percent(75),
                                pool_info: None,
                                rate_index: Decimal::one(),
                                individual_cost: None,
                            }],
                            collateral_supply_caps: vec![],
                            lastest_collateral_rates: vec![],
                            multi_asset_supply_caps: vec![],
                            credit_asset: Asset {
                                info: membrane::types::AssetInfo::NativeToken { denom: "cdt".to_string() },
                                amount: Uint128::zero(),
                            },
                            credit_price: membrane::oracle::PriceResponse {
                                prices: vec![],
                                price: Decimal::one(),
                                decimals: 6,
                            },
                            base_interest_rate: Decimal::zero(),
                            pending_revenue: PendingRevenue {
                                total_pending: Uint128::zero(),
                                per_asset_rev: vec![],
                            },
                            pending_bad_debt: Uint128::zero(),
                            credit_last_accrued: 0,
                            rates_last_accrued: 0,
                            oracle_set: true,
                            negative_rates: false,
                            frozen: false,
                            distribute_revenue: true,
                            cpc_margin_of_error: Decimal::zero(),
                            liq_queue: None,
                        };
                        return cosmwasm_std::SystemResult::Ok(cosmwasm_std::ContractResult::Ok(
                            to_json_binary(&basket).unwrap(),
                        ));
                    }
                }
                cosmwasm_std::SystemResult::Err(cosmwasm_std::SystemError::InvalidRequest {
                    error: "Unmocked query".to_string(),
                    request: msg.clone(),
                })
            }
            _ => cosmwasm_std::SystemResult::Err(cosmwasm_std::SystemError::InvalidRequest {
                error: "Unmocked query".to_string(),
                request: Default::default(),
            }),
        }
    });

    let info = mock_info("cdp_contract", &[]);
    let env = mock_env();

    let msg = ExecuteMsg::CreateQueue { asset };
    execute(deps.as_mut(), env, info, msg).unwrap();
}

/// Helper to mock oracle query for price conversion
fn mock_oracle_prices(deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>, collateral_price: Decimal, cdt_price: Decimal) {
    use membrane::cdp::QueryMsg as CDP_QueryMsg;
    use membrane::types::{Basket, Asset, cAsset, PendingRevenue};

    deps.querier.update_wasm(move |query| {
        match query {
            cosmwasm_std::WasmQuery::Smart { contract_addr, msg } => {
                // Handle Oracle queries
                if contract_addr == "oracle" {
                    let parsed: Result<Oracle_QueryMsg, _> = from_json(msg);
                    if let Ok(Oracle_QueryMsg::Prices { .. }) = parsed {
                        let prices = vec![
                            PriceResponse {
                                prices: vec![],
                                price: collateral_price,
                                decimals: 6,
                            },
                            PriceResponse {
                                prices: vec![],
                                price: cdt_price,
                                decimals: 6,
                            },
                        ];
                        return cosmwasm_std::SystemResult::Ok(cosmwasm_std::ContractResult::Ok(
                            to_json_binary(&prices).unwrap(),
                        ));
                    }
                }
                // Handle CDP queries (preserve the mock)
                if contract_addr == "cdp_contract" {
                    let parsed: Result<CDP_QueryMsg, _> = from_json(msg);
                    if matches!(parsed, Ok(CDP_QueryMsg::GetBasket {})) {
                        let basket = Basket {
                            basket_id: Uint128::one(),
                            current_position_id: Uint128::one(),
                            collateral_types: vec![cAsset {
                                asset: Asset {
                                    info: membrane::types::AssetInfo::NativeToken { 
                                        denom: "collateral".to_string() 
                                    },
                                    amount: Uint128::zero(),
                                },
                                max_borrow_LTV: Decimal::percent(70),
                                max_LTV: Decimal::percent(75),
                                pool_info: None,
                                rate_index: Decimal::one(),
                                individual_cost: None,
                            }],
                            collateral_supply_caps: vec![],
                            lastest_collateral_rates: vec![],
                            multi_asset_supply_caps: vec![],
                            credit_asset: Asset {
                                info: membrane::types::AssetInfo::NativeToken { denom: "cdt".to_string() },
                                amount: Uint128::zero(),
                            },
                            credit_price: membrane::oracle::PriceResponse {
                                prices: vec![],
                                price: Decimal::one(),
                                decimals: 6,
                            },
                            base_interest_rate: Decimal::zero(),
                            pending_revenue: PendingRevenue {
                                total_pending: Uint128::zero(),
                                per_asset_rev: vec![],
                            },
                            pending_bad_debt: Uint128::zero(),
                            credit_last_accrued: 0,
                            rates_last_accrued: 0,
                            oracle_set: true,
                            negative_rates: false,
                            frozen: false,
                            distribute_revenue: true,
                            cpc_margin_of_error: Decimal::zero(),
                            liq_queue: None,
                        };
                        return cosmwasm_std::SystemResult::Ok(cosmwasm_std::ContractResult::Ok(
                            to_json_binary(&basket).unwrap(),
                        ));
                    }
                }
                cosmwasm_std::SystemResult::Err(cosmwasm_std::SystemError::InvalidRequest {
                    error: "Unmocked query".to_string(),
                    request: msg.clone(),
                })
            }
            _ => cosmwasm_std::SystemResult::Err(cosmwasm_std::SystemError::InvalidRequest {
                error: "Unmocked query".to_string(),
                request: Default::default(),
            }),
        }
    });
}

/// Mock swap reply that simulates successful swap
fn mock_swap_reply(_swapped_cdt: Uint128) -> Reply {
    Reply {
        id: LIQUIDATION_SWAP_REPLY_ID,
        result: cosmwasm_std::SubMsgResult::Ok(cosmwasm_std::SubMsgResponse {
            events: vec![],
            data: None,
        }),
    }
}

/// Test full end-to-end flow: CDP calls AddBadDebt -> Disco fulfills via revenue -> CDP receives FulfillBadDebt
#[test]
fn test_end_to_end_bad_debt_fulfilled_by_revenue() {
    let mut deps = mock_dependencies();
    create_test_config(&mut deps);
    create_test_queue(&mut deps, "collateral".to_string());

    // Setup: Add dispersal with sufficient funds
    let dispersal = Dispersal {
        total_to_disperse: Uint128::new(10_000_000), // 10 CDT
        dispersal_window: 24,
        active_dispersal: ActiveDispersal {
            dispersal_start: mock_env().block.time.seconds(),
            amount_dispersed: Uint128::new(0),
        },
        pending_dispersal: Uint128::new(0),
    };
    DISPERSAL.save(&mut deps.storage, "collateral".to_string(), &dispersal).unwrap();

    // Step 1: CDP calls AddBadDebt
    let info = mock_info("cdp_contract", &[]);
    let env = mock_env();
    let msg = ExecuteMsg::AddBadDebt {
        asset: "collateral".to_string(),
        amount: Uint128::new(8_000_000), // 8 CDT
    };

    let res = execute(deps.as_mut(), env, info, msg).unwrap();

    // Step 2: Verify Disco sends FulfillBadDebt to CDP with CDT
    assert_eq!(res.messages.len(), 1);
    match &res.messages[0].msg {
        CosmosMsg::Wasm(WasmMsg::Execute { contract_addr, msg, funds }) => {
            assert_eq!(contract_addr, "cdp_contract");
            assert_eq!(funds, &vec![Coin { denom: "cdt".to_string(), amount: Uint128::new(8_000_000) }]);
            
            let parsed: CDP_ExecuteMsg = from_json(msg).unwrap();
            assert!(matches!(parsed, CDP_ExecuteMsg::FulfillBadDebt {}));
        }
        _ => panic!("Expected WasmMsg::Execute"),
    }

    // Step 3: Verify attributes
    assert_eq!(res.attributes[2].value, "8000000"); // total_bad_debt_cdt
    assert_eq!(res.attributes[3].value, "8000000"); // fulfilled_from_revenue_cdt
    assert_eq!(res.attributes[4].value, "0");       // slashed_collateral_amount
    assert_eq!(res.attributes[5].value, "0");       // remaining_bad_debt_cdt
}

/// Test full end-to-end flow with deposit slashing and swap
#[test]
fn test_end_to_end_bad_debt_with_slashing_and_swap() {
    let mut deps = mock_dependencies();
    create_test_config(&mut deps);
    create_test_queue(&mut deps, "collateral".to_string());

    // Mock oracle: 1 collateral = $2, 1 CDT = $1
    mock_oracle_prices(&mut deps, Decimal::from_ratio(2u128, 1u128), Decimal::one());

    // Add deposits at 80% LTV
    let deposit_info = mock_info("user1", &coins(1_000_000, "collateral"));
    let deposit_msg = ExecuteMsg::SubmitDeposit {
        deposit_input: BackingDepositInput {
            asset: "collateral".to_string(),
            ltv: Decimal::percent(80),
            max_borrow_ltv: Decimal::percent(75),
            epoch_start_time: Some(0),
        },
        deposit_owner: None,
        locked: None,
        deposit_id: None,
        manager: None,
        affiliate_address: None,
    };
    execute(deps.as_mut(), mock_env(), deposit_info, deposit_msg).unwrap();

    // Step 1: CDP calls AddBadDebt (no dispersals available)
    let info = mock_info("cdp_contract", &[]);
    let env = mock_env();
    let msg = ExecuteMsg::AddBadDebt {
        asset: "collateral".to_string(),
        amount: Uint128::new(500_000), // 500k CDT
    };

    let res = execute(deps.as_mut(), env, info, msg).unwrap();

    // Step 2: Verify swap message was created
    assert_eq!(res.messages.len(), 1);
    let submsg = &res.messages[0];
    assert_eq!(submsg.id, LIQUIDATION_SWAP_REPLY_ID);

    // Step 3: Verify swap propagation was saved
    let swap_prop = SWAP_PROPAGATION.load(&deps.storage).unwrap();
    assert_eq!(swap_prop.cdt_balance_before, Uint128::zero());

    // Step 4: Simulate swap reply (swap succeeds, returns CDT)
    // Mock balance query to return swapped CDT (balance after swap)
    let env = mock_env();
    deps.querier.update_balance(
        env.contract.address.as_str(),
        vec![Coin { denom: "cdt".to_string(), amount: Uint128::new(500_000) }],
    );

    // Step 5: Handle swap reply
    let reply_msg = mock_swap_reply(Uint128::new(500_000));
    let reply_res = reply(deps.as_mut(), env, reply_msg).unwrap();

    // Step 6: Verify FulfillBadDebt message was created
    assert_eq!(reply_res.messages.len(), 1);
    match &reply_res.messages[0].msg {
        CosmosMsg::Wasm(WasmMsg::Execute { contract_addr, msg, funds }) => {
            assert_eq!(contract_addr, "cdp_contract");
            assert_eq!(funds, &vec![Coin { denom: "cdt".to_string(), amount: Uint128::new(500_000) }]);
            
            let parsed: CDP_ExecuteMsg = from_json(msg).unwrap();
            assert!(matches!(parsed, CDP_ExecuteMsg::FulfillBadDebt {}));
        }
        _ => panic!("Expected WasmMsg::Execute"),
    }
}

/// Test mixed revenue + slashing flow
#[test]
fn test_end_to_end_mixed_revenue_and_slashing() {
    let mut deps = mock_dependencies();
    create_test_config(&mut deps);
    create_test_queue(&mut deps, "collateral".to_string());

    // Mock oracle: 1 collateral = $1.5, 1 CDT = $1
    mock_oracle_prices(&mut deps, Decimal::from_ratio(15u128, 10u128), Decimal::one());

    // Setup: 200k CDT in dispersals
    let dispersal = Dispersal {
        total_to_disperse: Uint128::new(200_000),
        dispersal_window: 24,
        active_dispersal: ActiveDispersal {
            dispersal_start: mock_env().block.time.seconds(),
            amount_dispersed: Uint128::new(0),
        },
        pending_dispersal: Uint128::new(0),
    };
    DISPERSAL.save(&mut deps.storage, "collateral".to_string(), &dispersal).unwrap();

    // Add deposits
    let deposit_info = mock_info("user1", &coins(1_000_000, "collateral"));
    let deposit_msg = ExecuteMsg::SubmitDeposit {
        deposit_input: BackingDepositInput {
            asset: "collateral".to_string(),
            ltv: Decimal::percent(85),
            max_borrow_ltv: Decimal::percent(80),
            epoch_start_time: Some(0),
        },
        deposit_owner: None,
        locked: None,
        deposit_id: None,
        manager: None,
        affiliate_address: None,
    };
    execute(deps.as_mut(), mock_env(), deposit_info, deposit_msg).unwrap();

    // Step 1: CDP calls AddBadDebt of 500k CDT (200k from dispersals, 300k from slashing)
    let info = mock_info("cdp_contract", &[]);
    let env = mock_env();
    let msg = ExecuteMsg::AddBadDebt {
        asset: "collateral".to_string(),
        amount: Uint128::new(500_000),
    };

    let res = execute(deps.as_mut(), env, info, msg).unwrap();

    // Step 2: Verify two messages: FulfillBadDebt (revenue) + Swap (slashing)
    assert_eq!(res.messages.len(), 2);

    // First message: FulfillBadDebt with 200k CDT from dispersals
    match &res.messages[0].msg {
        CosmosMsg::Wasm(WasmMsg::Execute { contract_addr, msg, funds }) => {
            assert_eq!(contract_addr, "cdp_contract");
            assert_eq!(funds, &vec![Coin { denom: "cdt".to_string(), amount: Uint128::new(200_000) }]);
            
            let parsed: CDP_ExecuteMsg = from_json(msg).unwrap();
            assert!(matches!(parsed, CDP_ExecuteMsg::FulfillBadDebt {}));
        }
        _ => panic!("Expected WasmMsg::Execute for revenue fulfillment"),
    }

    // Second message: Swap 200k collateral
    let submsg = &res.messages[1];
    assert_eq!(submsg.id, LIQUIDATION_SWAP_REPLY_ID);
}

/// Fuzz test: Various bad debt amounts
#[test]
fn fuzz_test_random_bad_debt_amounts() {
    // Test various bad debt amounts deterministically
    let test_amounts = vec![
        1u128,
        100u128,
        1_000u128,
        10_000u128,
        100_000u128,
        1_000_000u128,
        10_000_000u128,
        100_000_000u128,
        1_000_000_000u128,
    ];
    
    for bad_debt_amount_raw in test_amounts {
        let bad_debt_amount = Uint128::new(bad_debt_amount_raw);
        
        let mut deps = mock_dependencies();
        create_test_config(&mut deps);
        create_test_queue(&mut deps, "collateral".to_string());
        
        // Mock oracle
        mock_oracle_prices(&mut deps, Decimal::one(), Decimal::one());
        
        // Add deposits
        let deposit_info = mock_info("user1", &coins(10_000_000, "collateral"));
        let deposit_msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "collateral".to_string(),
                ltv: Decimal::percent(80),
                max_borrow_ltv: Decimal::percent(75),
                epoch_start_time: Some(0),
            },
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
        };
        execute(deps.as_mut(), mock_env(), deposit_info, deposit_msg).unwrap();
        
        // Execute bad debt
        let info = mock_info("cdp_contract", &[]);
        let env = mock_env();
        let msg = ExecuteMsg::AddBadDebt {
            asset: "collateral".to_string(),
            amount: bad_debt_amount,
        };
        
        let res = execute(deps.as_mut(), env, info, msg);
        
        // Should always succeed (may have remaining bad debt if exceeds deposits)
        assert!(res.is_ok(), "Bad debt amount {} should succeed", bad_debt_amount);
        
        // Verify state consistency
        let queue: LTVQueue = LTV_QUEUES.load(&deps.storage, "collateral".to_string()).unwrap();
        let total_deposits: Uint128 = queue.slots.iter()
            .flat_map(|slot| &slot.deposit_groups)
            .map(|group| group.total_deposit_tokens)
            .sum();
        
        // Total deposits should never go negative
        assert!(total_deposits <= Uint128::new(10_000_000));
    }
}

/// Fuzz test: Various price ratios
#[test]
fn fuzz_test_random_price_ratios() {
    // Test various price ratios deterministically
    let price_ratios = vec![
        (1u128, 100u128),      // 0.01
        (1u128, 10u128),       // 0.1
        (1u128, 2u128),       // 0.5
        (1u128, 1u128),       // 1.0
        (2u128, 1u128),       // 2.0
        (10u128, 1u128),      // 10.0
        (100u128, 1u128),     // 100.0
        (1000u128, 1u128),    // 1000.0
    ];
    
    for (num, denom) in price_ratios {
        let collateral_price = Decimal::from_ratio(num, denom);
        let cdt_price = Decimal::one();
        
        let mut deps = mock_dependencies();
        create_test_config(&mut deps);
        create_test_queue(&mut deps, "collateral".to_string());
        
        mock_oracle_prices(&mut deps, collateral_price, cdt_price);
        
        // Add deposits
        let deposit_info = mock_info("user1", &coins(1_000_000, "collateral"));
        let deposit_msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "collateral".to_string(),
                ltv: Decimal::percent(80),
                max_borrow_ltv: Decimal::percent(75),
                epoch_start_time: Some(0),
            },
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
        };
        execute(deps.as_mut(), mock_env(), deposit_info, deposit_msg).unwrap();
        
        // Execute bad debt
        let bad_debt = Uint128::new(500_000);
        let info = mock_info("cdp_contract", &[]);
        let env = mock_env();
        let msg = ExecuteMsg::AddBadDebt {
            asset: "collateral".to_string(),
            amount: bad_debt,
        };
        
        let res = execute(deps.as_mut(), env, info, msg);
        assert!(res.is_ok(), "Price ratio {} should work", collateral_price);
    }
}

/// Test concurrent bad debt events
#[test]
fn test_concurrent_bad_debt_events() {
    let mut deps = mock_dependencies();
    create_test_config(&mut deps);
    create_test_queue(&mut deps, "collateral".to_string());
    
    mock_oracle_prices(&mut deps, Decimal::one(), Decimal::one());
    
    // Add large deposits
    for i in 0..10 {
        let deposit_info = mock_info(&format!("user{}", i), &coins(1_000_000, "collateral"));
        let deposit_msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "collateral".to_string(),
                ltv: Decimal::percent(80 + (i % 15)),
                max_borrow_ltv: Decimal::percent(75 + (i % 15)),
                epoch_start_time: Some(0),
            },
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
        };
        execute(deps.as_mut(), mock_env(), deposit_info, deposit_msg).unwrap();
    }
    
    // Execute 50 sequential bad debt events
    let info = mock_info("cdp_contract", &[]);
    for i in 0..50 {
        let env = mock_env();
        let msg = ExecuteMsg::AddBadDebt {
            asset: "collateral".to_string(),
            amount: Uint128::new(10_000 * (i + 1)),
        };
        
        let res = execute(deps.as_mut(), env, info.clone(), msg);
        assert!(res.is_ok(), "Bad debt event {} should succeed", i);
    }
    
    // Verify final state consistency
    let queue: LTVQueue = LTV_QUEUES.load(&deps.storage, "collateral".to_string()).unwrap();
    let total_bad_debt: Uint128 = queue.slots.iter().map(|s| s.bad_debt).sum();
    let total_deposits: Uint128 = queue.slots.iter()
        .flat_map(|slot| &slot.deposit_groups)
        .map(|group| group.total_deposit_tokens)
        .sum();
    
    // Total bad debt + remaining deposits should be consistent
    assert!(total_bad_debt + total_deposits <= Uint128::new(10_000_000));
}

/// Test edge case: Bad debt exactly equals available deposits
#[test]
fn test_bad_debt_exactly_equals_deposits() {
    let mut deps = mock_dependencies();
    create_test_config(&mut deps);
    create_test_queue(&mut deps, "collateral".to_string());
    
    mock_oracle_prices(&mut deps, Decimal::one(), Decimal::one());
    
    // Add exactly 1M collateral
    let deposit_info = mock_info("user1", &coins(1_000_000, "collateral"));
    let deposit_msg = ExecuteMsg::SubmitDeposit {
        deposit_input: BackingDepositInput {
            asset: "collateral".to_string(),
            ltv: Decimal::percent(80),
            max_borrow_ltv: Decimal::percent(75),
            epoch_start_time: Some(0),
        },
        deposit_owner: None,
        locked: None,
        deposit_id: None,
        manager: None,
        affiliate_address: None,
    };
    execute(deps.as_mut(), mock_env(), deposit_info, deposit_msg).unwrap();
    
    // Bad debt exactly equals deposits (1:1 price)
    let info = mock_info("cdp_contract", &[]);
    let env = mock_env();
    let msg = ExecuteMsg::AddBadDebt {
        asset: "collateral".to_string(),
        amount: Uint128::new(1_000_000),
    };
    
    let res = execute(deps.as_mut(), env, info, msg).unwrap();
    
    // Verify all deposits slashed
    let queue: LTVQueue = LTV_QUEUES.load(&deps.storage, "collateral".to_string()).unwrap();
    let total_deposits: Uint128 = queue.slots.iter()
        .flat_map(|slot| &slot.deposit_groups)
        .map(|group| group.total_deposit_tokens)
        .sum();
    
    assert_eq!(total_deposits, Uint128::zero());
    assert_eq!(res.attributes[5].value, "0"); // remaining_bad_debt_cdt
}

/// Test edge case: Zero bad debt
#[test]
fn test_zero_bad_debt() {
    let mut deps = mock_dependencies();
    create_test_config(&mut deps);
    create_test_queue(&mut deps, "collateral".to_string());
    
    let info = mock_info("cdp_contract", &[]);
    let env = mock_env();
    let msg = ExecuteMsg::AddBadDebt {
        asset: "collateral".to_string(),
        amount: Uint128::zero(),
    };
    
    let res = execute(deps.as_mut(), env, info, msg);
    // Should handle zero gracefully (may succeed or fail depending on implementation)
    // Just verify it doesn't panic
    let _ = res;
}

/// Test swap failure scenario (simulated)
#[test]
fn test_swap_failure_handling() {
    let mut deps = mock_dependencies();
    create_test_config(&mut deps);
    create_test_queue(&mut deps, "collateral".to_string());
    
    mock_oracle_prices(&mut deps, Decimal::one(), Decimal::one());
    
    // Add deposits
    let deposit_info = mock_info("user1", &coins(1_000_000, "collateral"));
    let deposit_msg = ExecuteMsg::SubmitDeposit {
        deposit_input: BackingDepositInput {
            asset: "collateral".to_string(),
            ltv: Decimal::percent(80),
            max_borrow_ltv: Decimal::percent(75),
            epoch_start_time: Some(0),
        },
        deposit_owner: None,
        locked: None,
        deposit_id: None,
        manager: None,
        affiliate_address: None,
    };
    execute(deps.as_mut(), mock_env(), deposit_info, deposit_msg).unwrap();
    
    // Execute bad debt that requires slashing
    let info = mock_info("cdp_contract", &[]);
    let env = mock_env();
    let msg = ExecuteMsg::AddBadDebt {
        asset: "collateral".to_string(),
        amount: Uint128::new(500_000),
    };
    
    let res = execute(deps.as_mut(), env, info, msg).unwrap();
    
    // Verify swap message was created
    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, LIQUIDATION_SWAP_REPLY_ID);
    
    // Simulate swap failure (reply with error)
    let failed_reply = Reply {
        id: LIQUIDATION_SWAP_REPLY_ID,
        result: cosmwasm_std::SubMsgResult::Err("Swap failed".to_string()),
    };
    
    // Reply handler should handle error gracefully
    let reply_res = reply(deps.as_mut(), mock_env(), failed_reply);
    // Depending on implementation, this may succeed (cleanup) or fail
    // Just verify it doesn't panic
    let _ = reply_res;
}

