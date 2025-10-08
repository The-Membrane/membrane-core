#[cfg(test)]
mod tests {
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{coins, from_json, Addr, Decimal, Uint128, DepsMut, Env, Response, to_json_binary, SystemResult, ContractResult, CosmosMsg, WasmMsg, Reply, SubMsgResult, SubMsgResponse, Event, Binary};
    use membrane::ltv_disco::*;
    use membrane::types::{AssetInfo, Basket, cAsset, Asset, DepositDenom, VaultTokenInfo};
    use membrane::transmuter::QueryMsg as Transmuter_QueryMsg;
    use membrane::oracle::PriceResponse;
    use crate::contract::{instantiate, execute, query};
    use crate::state::PENDING_BAD_DEBT;
    use crate::error::ContractError;

    fn setup_mock_basket() -> Basket {
        Basket {
            basket_id: Uint128::new(1),
            current_position_id: Uint128::new(1),
            collateral_types: vec![cAsset {
                asset: Asset {
                    info: AssetInfo::NativeToken { denom: "uusd".to_string() },
                    amount: Uint128::zero(),
                },
                max_LTV: Decimal::percent(50),
                max_borrow_LTV: Decimal::percent(30),
                rate_index: Decimal::zero(),
                pool_info: None,
                hike_rates: Some(false),
            }, cAsset {
                asset: Asset {
                    info: AssetInfo::NativeToken { denom: "untrn".to_string() },
                    amount: Uint128::zero(),
                },
                max_LTV: Decimal::percent(50),
                max_borrow_LTV: Decimal::percent(30),
                rate_index: Decimal::zero(),
                pool_info: None,
                hike_rates: Some(false),
            }],
            collateral_supply_caps: vec![],
            lastest_collateral_rates: vec![],
            multi_asset_supply_caps: vec![],
            credit_asset: Asset {
                info: AssetInfo::NativeToken { denom: "debt".to_string() },
                amount: Uint128::zero(),
            },
            credit_price: PriceResponse {
                prices: vec![],
                price: Decimal::one(),
                decimals: 6,
            },
            base_interest_rate: Decimal::percent(5),
            pending_revenue: Uint128::zero(),
            pending_bad_debt: Uint128::zero(),
            credit_last_accrued: 0,
            rates_last_accrued: 0,
            oracle_set: true,
            negative_rates: false,
            frozen: false,
            distribute_revenue: true,
            cpc_margin_of_error: Decimal::percent(100),
            liq_queue: None,
        }
    }

    fn instantiate_contract(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
        let msg = InstantiateMsg {
            owner: Some("owner".to_string()),
            cdp_contract: "cdp_contract".to_string(),
            deposit_denom: DepositDenom { denom: "uusd".to_string(), vault_info: None },
            minimum_deposit: Uint128::new(1000),
            waiting_period: 86400, // 1 day
            max_ltv: Decimal::percent(80),
            percent_to_disperse: Decimal::percent(10),
            dispersal_window: 2, // small window (hours) to make dispersal amounts clear in tests
            activation_window: 2, // hours
        };
        
        let info = mock_info("owner", &[]);
        instantiate(deps, env, info, msg)
    }

    fn instantiate_contract_with_vault(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
        let msg = InstantiateMsg {
            owner: Some("owner".to_string()),
            cdp_contract: "cdp_contract".to_string(),
            deposit_denom: DepositDenom { 
                denom: "vtkn".to_string(), 
                vault_info: Some(VaultTokenInfo { 
                    vault_contract: "vault_contract".to_string(), 
                    underlying_token: "uusd".to_string(),
                })
            },
            minimum_deposit: Uint128::new(1000),
            waiting_period: 86400, // 1 day
            max_ltv: Decimal::percent(80),
            percent_to_disperse: Decimal::percent(10),
            dispersal_window: 2, // hours
            activation_window: 2, // hours
        };

        let info = mock_info("owner", &[]);
        instantiate(deps, env, info, msg)
    }

    #[test]
    fn test_instantiate() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        
        // Mock CDP contract response
        let basket = setup_mock_basket();
        deps.querier.update_wasm(move |_| SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())));
        
        // Test successful instantiation
        let result = instantiate_contract(deps.as_mut(), env.clone());
        assert!(result.is_ok());
        
        // Test instantiation without owner (uses sender)
        let msg = InstantiateMsg {
            owner: None,
            cdp_contract: "cdp_contract".to_string(),
            deposit_denom: DepositDenom { denom: "uusd".to_string(), vault_info: None },
            minimum_deposit: Uint128::new(1000),
            waiting_period: 86400,
            max_ltv: Decimal::percent(80),
            percent_to_disperse: Decimal::percent(10),
            dispersal_window: 2, // hours
            activation_window: 2, // hours
        };
        
        let info = mock_info("sender", &[]);
        let result = instantiate(deps.as_mut(), env, info, msg);
        assert!(result.is_ok());
    }

    #[test]
    fn test_add_revenue_reserves_percent_for_dispersal() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        // Mock CDP contract response for GetBasket only
        let basket = setup_mock_basket();
        deps.querier.update_wasm(move |_| SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())));

        instantiate_contract(deps.as_mut(), env.clone()).unwrap();

        // Create queue and single deposit (one slot/group for deterministic distribution)
        let msg = ExecuteMsg::CreateQueue { asset: "uusd".to_string() };
        let info = mock_info("owner", &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        let msg = ExecuteMsg::SubmitDeposit { deposit_input: BackingDepositInput { asset: "uusd".to_string(), ltv: Decimal::percent(60), max_borrow_ltv: Decimal::percent(40) }, deposit_owner: None };
        let info = mock_info("user1", &coins(1000, "uusd"));
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Snapshot before revenue
        let before: LTVQueueResponse = from_json(&query(deps.as_ref(), env.clone(), QueryMsg::GetLTVQueue { asset: "uusd".to_string() }).unwrap()).unwrap();
        let before_total = before.queue.slots[0].deposit_groups[0].total_deposit_tokens;
        println!("before_total: {}", before_total);

        // Add revenue 100 uusd; percent_to_disperse=10% -> 10 withheld, 90 immediately distributed
        let msg = ExecuteMsg::AddRevenue { asset: "uusd".to_string() };
        let info = mock_info("anyone", &coins(100, "uusd"));
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        let after: LTVQueueResponse = from_json(&query(deps.as_ref(), env.clone(), QueryMsg::GetLTVQueue { asset: "uusd".to_string() }).unwrap()).unwrap();
        let after_total = after.queue.slots[0].deposit_groups[0].total_deposit_tokens;
        println!("after_total: {}", after_total);

        //10 is saved for the dispersal
        assert_eq!(after_total, before_total + Uint128::new(90));
    }

    #[test]
    fn test_disperse_revenue_linear_and_pending_rollover() {
        use membrane::cdp::{QueryMsg as CDP_QueryMsg, LiquidationStatResponse};

        let mut deps = mock_dependencies();
        let env = mock_env();

        // Prepare dynamic wasm mock to handle both GetBasket and GetLiquidationStats
        let basket = setup_mock_basket();
        let asset = "uusd".to_string();
        let activation_event_time;
        {
            // Capture initial time for liquidation stat within activation window (1 hour ago)
            let now = env.block.time.seconds();
            activation_event_time = now.saturating_sub(3600);
        }
        deps.querier.update_wasm(move |wasm_query| {
            match wasm_query {
                cosmwasm_std::WasmQuery::Smart { msg, .. } => {
                    if let Ok(q) = from_json::<CDP_QueryMsg>(msg) {
                        match q {
                            CDP_QueryMsg::GetBasket { .. } => SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())),
                            CDP_QueryMsg::GetLiquidationStats { .. } => {
                                // Return one liquidation including our asset inside the activation window
                                let entry = LiquidationStatResponse {
                                    block_time: activation_event_time,
                                    position_id: Uint128::new(1),
                                    collateral_assets: vec![Asset { info: AssetInfo::NativeToken { denom: asset.clone() }, amount: Uint128::zero() }],
                                    amount_liquidated: Uint128::zero(),
                                };
                                SystemResult::Ok(ContractResult::Ok(to_json_binary(&vec![entry]).unwrap()))
                            }
                            _ => SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())),
                        }
                    } else {
                        SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap()))
                    }
                }
                _ => SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())),
            }
        });

        instantiate_contract(deps.as_mut(), env.clone()).unwrap();

        // Create queue and single deposit
        let msg = ExecuteMsg::CreateQueue { asset: "uusd".to_string() };
        let info = mock_info("owner", &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        let msg = ExecuteMsg::SubmitDeposit { deposit_input: BackingDepositInput { asset: "uusd".to_string(), ltv: Decimal::percent(60), max_borrow_ltv: Decimal::percent(40) }, deposit_owner: None };
        let info = mock_info("user1", &coins(1000, "uusd"));
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Baseline (no need to store values yet)
        let _baseline: LTVQueueResponse = from_json(&query(deps.as_ref(), env.clone(), QueryMsg::GetLTVQueue { asset: "uusd".to_string() }).unwrap()).unwrap();

        // Add revenue 100 -> immediately distribute 90 and reserve 10 for dispersal
        let msg = ExecuteMsg::AddRevenue { asset: "uusd".to_string() };
        let info = mock_info("anyone", &coins(100, "uusd"));
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        let mut q: LTVQueueResponse = from_json(&query(deps.as_ref(), env.clone(), QueryMsg::GetLTVQueue { asset: "uusd".to_string() }).unwrap()).unwrap();
        let mut total = q.queue.slots[0].deposit_groups[0].total_deposit_tokens;
        //90 distributed, 10% saved for dispersal
        assert_eq!(total, Uint128::new(1000 + 90));

        // First disperse call at current time (elapsed 1h since event) -> disperse 10/2 = 5
        let msg = ExecuteMsg::DisperseRevenue { asset: "uusd".to_string() };
        let info = mock_info("cdp_contract", &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        q = from_json(&query(deps.as_ref(), env.clone(), QueryMsg::GetLTVQueue { asset: "uusd".to_string() }).unwrap()).unwrap();
        total = q.queue.slots[0].deposit_groups[0].total_deposit_tokens;
        assert_eq!(total, Uint128::new(1000 + 90 + 5));

        // Add revenue 50 during active dispersal -> 45 immediately distributed ( 5 set to pending)
        let msg = ExecuteMsg::AddRevenue { asset: "uusd".to_string() };
        let info = mock_info("anyone", &coins(50, "uusd"));
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        q = from_json(&query(deps.as_ref(), env.clone(), QueryMsg::GetLTVQueue { asset: "uusd".to_string() }).unwrap()).unwrap();
        total = q.queue.slots[0].deposit_groups[0].total_deposit_tokens;
        assert_eq!(total, Uint128::new(1000 + 90 + 5 + 45)); // 1145

        // Advance time by 1 hour; finish first dispersal (another 5)
        let mut env2 = env.clone();
        env2.block.time = env2.block.time.plus_seconds(3600);
        let msg = ExecuteMsg::DisperseRevenue { asset: "uusd".to_string() };
        let info = mock_info("cdp_contract", &[]);
        execute(deps.as_mut(), env2.clone(), info, msg).unwrap();
        q = from_json(&query(deps.as_ref(), env2.clone(), QueryMsg::GetLTVQueue { asset: "uusd".to_string() }).unwrap()).unwrap();
        total = q.queue.slots[0].deposit_groups[0].total_deposit_tokens;
        assert_eq!(total, Uint128::new(1000 + 90 + 5 + 45 + 5)); // 1150

        // Next cycle should pick up pending (5) and disperse it; call again to trigger new activation and full disperse
        let mut env3 = env2.clone();
        env3.block.time = env3.block.time.plus_seconds(60); // small bump; auto-activation still within window
        let msg = ExecuteMsg::DisperseRevenue { asset: "uusd".to_string() };
        let info = mock_info("cdp_contract", &[]);
        execute(deps.as_mut(), env3.clone(), info, msg).unwrap();
        q = from_json(&query(deps.as_ref(), env3.clone(), QueryMsg::GetLTVQueue { asset: "uusd".to_string() }).unwrap()).unwrap();
        total = q.queue.slots[0].deposit_groups[0].total_deposit_tokens;
        assert_eq!(total, Uint128::new(1150));
    }

    #[test]
    fn test_create_queue() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        
        // Mock CDP contract response
        let basket = setup_mock_basket();
        deps.querier.update_wasm(move |_| SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())));
        
        instantiate_contract(deps.as_mut(), env.clone()).unwrap();
        
        // Test successful queue creation by owner
        let msg = ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        };
        let info = mock_info("owner", &[]);
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_ok());
        
        // Test queue creation by CDP contract
        let msg = ExecuteMsg::CreateQueue {
            asset: "untrn".to_string(),
        };
        let info = mock_info("cdp_contract", &[]);
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_ok());
        
        // Test unauthorized queue creation
        let msg = ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        };
        let info = mock_info("unauthorized", &[]);
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_err());
        
        // Test asset not found in basket
        let msg = ExecuteMsg::CreateQueue {
            asset: "unknown_asset".to_string(),
        };
        let info = mock_info("owner", &[]);
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_err());
    }

    #[test]
    fn test_update_queue() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        
        // Mock CDP contract response
        let basket = setup_mock_basket();
        deps.querier.update_wasm(move |_| SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())));
        
        instantiate_contract(deps.as_mut(), env.clone()).unwrap();
        
        // Create queue first
        let msg = ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        };
        let info = mock_info("owner", &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Test successful update by owner with max_ltv
        let msg = ExecuteMsg::UpdateQueue {
            asset: "uusd".to_string(),
            max_ltv: Some(Decimal::percent(80)),
        };
        let info = mock_info("owner", &[]);
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_ok());
        
        // Test update by non-owner (should not set max_ltv)
        let msg = ExecuteMsg::UpdateQueue {
            asset: "uusd".to_string(),
            max_ltv: Some(Decimal::percent(90)),
        };
        let info = mock_info("cdp_contract", &[]);
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_ok());
        
        // Test update with asset not found
        let msg = ExecuteMsg::UpdateQueue {
            asset: "unknown_asset".to_string(),
            max_ltv: None,
        };
        let info = mock_info("owner", &[]);
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_err());
    }

    #[test]
    fn test_submit_deposit() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        
        // Mock CDP contract response
        let basket = setup_mock_basket();
        deps.querier.update_wasm(move |_| SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())));
        
        instantiate_contract(deps.as_mut(), env.clone()).unwrap();
        
        // Create queue first
        let msg = ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        };
        let info = mock_info("owner", &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Test successful deposit
        let msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "uusd".to_string(),
                ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(40),
            },
            deposit_owner: None,
        };
        let info = mock_info("user1", &coins(1000, "uusd"));
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_ok());
        
        // Test deposit with deposit_owner
        let msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "uusd".to_string(),
                ltv: Decimal::percent(70),
                max_borrow_ltv: Decimal::percent(50),
            },
            deposit_owner: Some("deposit_owner".to_string()),
        };
        let info = mock_info("user2", &coins(2000, "uusd"));
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_ok());
        
        // Test adding to existing deposit
        let msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "uusd".to_string(),
                ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(40),
            },
            deposit_owner: None,
        };
        let info = mock_info("user1", &coins(500, "uusd"));
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        // println!("result: {:?}", result);
        assert!(result.is_ok());
        
        // Test invalid asset
        let msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "unknown_asset".to_string(),
                ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(40),
            },
            deposit_owner: None,
        };
        let info = mock_info("user1", &coins(1000, "uusd"));
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_err());
        
        // Test invalid LTV
        let msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "uusd".to_string(),
                ltv: Decimal::percent(20), // Below minimum
                max_borrow_ltv: Decimal::percent(40),
            },
            deposit_owner: None,
        };
        let info = mock_info("user1", &coins(1000, "uusd"));
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_err());
        
        // Test invalid max_borrow_ltv
        let msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "uusd".to_string(),
                ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(20), // Below minimum
            },
            deposit_owner: None,
        };
        let info = mock_info("user1", &coins(1000, "uusd"));
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_err());
        
        // Test insufficient funds
        let msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "uusd".to_string(),
                ltv: Decimal::percent(61),
                max_borrow_ltv: Decimal::percent(40),
            },
            deposit_owner: None,
        };
        let info = mock_info("user1", &coins(500, "uusd")); // Below minimum
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_err());
        
        // Test wrong denomination
        let msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "uusd".to_string(),
                ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(40),
            },
            deposit_owner: None,
        };
        let info = mock_info("user1", &coins(1000, "wrong_denom"));
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_err());
    }

    #[test]
    fn test_withdraw_deposit() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        
        // Mock CDP contract response
        let basket = setup_mock_basket();
        deps.querier.update_wasm(move |_| SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())));
        
        instantiate_contract(deps.as_mut(), env.clone()).unwrap();
        
        // Create queue and deposit first
        let msg = ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        };
        let info = mock_info("owner", &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        let msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "uusd".to_string(),
                ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(40),
            },
            deposit_owner: None,
        };
        let info = mock_info("user1", &coins(1500, "uusd"));
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Test successful partial withdrawal
        let msg = ExecuteMsg::WithdrawDeposit {
            deposit_id: Uint128::new(1),
            asset: "uusd".to_string(),
            amount: Some(Uint128::new(500)),
        };
        let info = mock_info("user1", &[]);
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_ok());
        
        // Test successful full withdrawal
        let msg = ExecuteMsg::WithdrawDeposit {
            deposit_id: Uint128::new(1),
            asset: "uusd".to_string(),
            amount: None,
        };
        let info = mock_info("user1", &[]);
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_ok());
        
        // Test unauthorized withdrawal
        let msg = ExecuteMsg::WithdrawDeposit {
            deposit_id: Uint128::new(1),
            asset: "uusd".to_string(),
            amount: Some(Uint128::new(100)),
        };
        let info = mock_info("unauthorized", &[]);
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_err());
        
        // Test deposit not found
        let msg = ExecuteMsg::WithdrawDeposit {
            deposit_id: Uint128::new(999),
            asset: "uusd".to_string(),
            amount: Some(Uint128::new(100)),
        };
        let info = mock_info("user1", &[]);
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_err());
        
        // Test invalid withdrawal amount
        let msg = ExecuteMsg::WithdrawDeposit {
            deposit_id: Uint128::new(1),
            asset: "uusd".to_string(),
            amount: Some(Uint128::new(1000000)), // More than available
        };
        let info = mock_info("user1", &[]);
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_err());
    }

    #[test]
    fn test_add_bad_debt() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        
        // Mock CDP contract response
        let basket = setup_mock_basket();
        deps.querier.update_wasm(move |_| SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())));
        
        instantiate_contract(deps.as_mut(), env.clone()).unwrap();
        
        // Create queue and deposits first
        let msg = ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        };
        let info = mock_info("owner", &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Add some deposits
        for i in 0..3 {
            let msg = ExecuteMsg::SubmitDeposit {
                deposit_input: BackingDepositInput {
                    asset: "uusd".to_string(),
                    ltv: Decimal::percent(60 + i * 5),
                    max_borrow_ltv: Decimal::percent(40 + i * 5),
                },
                deposit_owner: None,
            };
            let info = mock_info(&format!("user{}", i), &coins(1000, "uusd"));
            execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        }
        
        // Test successful bad debt addition by CDP contract
        let msg = ExecuteMsg::AddBadDebt {
            asset: "uusd".to_string(),
            amount: Uint128::new(500),
        };
        let info = mock_info("cdp_contract", &[]);
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_ok());
        
        // Verify bad debt was applied to groups (total_deposit_tokens should decrease)
        let queue_res: LTVQueueResponse = from_json(&query(deps.as_ref(), env.clone(), QueryMsg::GetLTVQueue { 
            asset: "uusd".to_string() 
        }).unwrap()).unwrap();
        
        // Check that total_deposit_tokens decreased in groups due to bad debt waterfall
        for slot in queue_res.queue.slots {
            for group in slot.deposit_groups {
                println!("Slot LTV: {}, Group max_borrow_ltv: {}, Group total_deposit_tokens: {}", slot.ltv, group.max_borrow_ltv, group.total_deposit_tokens);
                // Groups should have less than the original 1000 deposit tokens due to bad debt
                // But only if they actually received bad debt (highest LTV slot with highest max_borrow_ltv)
                if slot.ltv == Decimal::percent(70) && group.max_borrow_ltv == Decimal::percent(50) {
                    // This group should have received bad debt
                    assert!(group.total_deposit_tokens < Uint128::new(1000), 
                        "Group total_deposit_tokens should have decreased from bad debt application");
                } else {
                    // Other groups should be unchanged
                    assert_eq!(group.total_deposit_tokens, Uint128::new(1000), 
                        "Group total_deposit_tokens should be unchanged");
                }
            }
        }
        
        // Test unauthorized bad debt addition
        let msg = ExecuteMsg::AddBadDebt {
            asset: "uusd".to_string(),
            amount: Uint128::new(100),
        };
        let info = mock_info("unauthorized", &[]);
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_err());
        
        // Test bad debt with no deposits
        let msg = ExecuteMsg::AddBadDebt {
            asset: "unknown_asset".to_string(),
            amount: Uint128::new(100),
        };
        let info = mock_info("cdp_contract", &[]);
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_err());
    }

    #[test]
    fn test_add_bad_debt_with_vault_token_converts_and_sends_underlying() {
        use membrane::cdp::QueryMsg as CDP_QueryMsg;

        let mut deps = mock_dependencies();
        let env = mock_env();

        // Prepare mocks for GetBasket and Transmuter conversions
        let basket = setup_mock_basket();
        deps.querier.update_wasm(move |wasm_query| {
            match wasm_query {
                cosmwasm_std::WasmQuery::Smart { contract_addr: _, msg } => {
                    if let Ok(q) = from_json::<CDP_QueryMsg>(msg) {
                        match q {
                            CDP_QueryMsg::GetBasket { .. } => SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())),
                            _ => SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())),
                        }
                    } else if let Ok(tq) = from_json::<Transmuter_QueryMsg>(msg) {
                        match tq {
                            Transmuter_QueryMsg::DepositTokenConversion { deposit_token_amount } => {
                                // 1:1 conversion for simplicity in test
                                SystemResult::Ok(ContractResult::Ok(to_json_binary(&deposit_token_amount).unwrap()))
                            }
                            Transmuter_QueryMsg::VaultTokenUnderlying { vault_token_amount } => {
                                // 1:1 conversion back to underlying
                                SystemResult::Ok(ContractResult::Ok(to_json_binary(&vault_token_amount).unwrap()))
                            }
                            _ => SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())),
                        }
                    } else {
                        SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap()))
                    }
                }
                _ => SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())),
            }
        });

        // Instantiate with vault token deposit denom
        instantiate_contract_with_vault(deps.as_mut(), env.clone()).unwrap();

        // Create queue and a single deposit to have balance to write bad debt against
        let msg = ExecuteMsg::CreateQueue { asset: "uusd".to_string() };
        let info = mock_info("owner", &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        let msg = ExecuteMsg::SubmitDeposit { 
            deposit_input: BackingDepositInput { 
                asset: "uusd".to_string(), 
                ltv: Decimal::percent(60), 
                max_borrow_ltv: Decimal::percent(40) 
            }, 
            deposit_owner: None 
        };
        let info = mock_info("user1", &coins(1000, "uusd"));
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Add bad debt of 100 (base units). With 1:1 conversions, we expect:
        // - One submessage to vault_contract with funds denom "vtkn" and amount 100
        // - One submessage to cdp_contract with funds denom "uusd" and amount 100
        let msg = ExecuteMsg::AddBadDebt { asset: "uusd".to_string(), amount: Uint128::new(100) };
        let info = mock_info("cdp_contract", &[]);
        let result = execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Assert we produced two submessages
        assert_eq!(result.messages.len(), 2);

        // First: ExitVault to vault contract with vt funds
        if let CosmosMsg::Wasm(WasmMsg::Execute { contract_addr, funds, .. }) = &result.messages[0].msg {
            assert_eq!(contract_addr, "vault_contract");
            assert_eq!(funds.len(), 1);
            assert_eq!(funds[0].denom, "vtkn");
            assert_eq!(funds[0].amount, Uint128::new(100));
        } else {
            panic!("expected WasmMsg::Execute for ExitVault");
        }

        // Second: FulfillBadDebt to cdp_contract with underlying funds
        if let CosmosMsg::Wasm(WasmMsg::Execute { contract_addr, funds, .. }) = &result.messages[1].msg {
            assert_eq!(contract_addr, "cdp_contract");
            assert_eq!(funds.len(), 1);
            assert_eq!(funds[0].denom, "uusd");
            assert_eq!(funds[0].amount, Uint128::new(100));
        } else {
            panic!("expected WasmMsg::Execute for FulfillBadDebt");
        }
    }

    #[test]
    fn test_bad_debt_transmuter_exit_error_and_retry_flow() {
        use membrane::cdp::QueryMsg as CDP_QueryMsg;

        let mut deps = mock_dependencies();
        let env = mock_env();

        // Mock GetBasket and Transmuter conversions (1:1)
        let basket = setup_mock_basket();
        deps.querier.update_wasm(move |wasm_query| {
            match wasm_query {
                cosmwasm_std::WasmQuery::Smart { msg, .. } => {
                    if let Ok(q) = from_json::<CDP_QueryMsg>(msg) {
                        match q {
                            CDP_QueryMsg::GetBasket { .. } => SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())),
                            _ => SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())),
                        }
                    } else if let Ok(tq) = from_json::<Transmuter_QueryMsg>(msg) {
                        match tq {
                            Transmuter_QueryMsg::DepositTokenConversion { deposit_token_amount } => SystemResult::Ok(ContractResult::Ok(to_json_binary(&deposit_token_amount).unwrap())),
                            Transmuter_QueryMsg::VaultTokenUnderlying { vault_token_amount } => SystemResult::Ok(ContractResult::Ok(to_json_binary(&vault_token_amount).unwrap())),
                            _ => SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())),
                        }
                    } else {
                        SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap()))
                    }
                }
                _ => SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())),
            }
        });

        // Instantiate with vault token denom
        instantiate_contract_with_vault(deps.as_mut(), env.clone()).unwrap();

        // Create queue and deposit
        execute(
            deps.as_mut(),
            env.clone(),
            mock_info("owner", &[]),
            ExecuteMsg::CreateQueue { asset: "uusd".to_string() },
        ).unwrap();

        execute(
            deps.as_mut(),
            env.clone(),
            mock_info("user1", &coins(1000, "uusd")),
            ExecuteMsg::SubmitDeposit { deposit_input: BackingDepositInput { asset: "uusd".to_string(), ltv: Decimal::percent(60), max_borrow_ltv: Decimal::percent(40) }, deposit_owner: None }
        ).unwrap();

        // Add bad debt -> will attempt ExitVault; we'll simulate error via reply
        let result = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("cdp_contract", &[]),
            ExecuteMsg::AddBadDebt { asset: "uusd".to_string(), amount: Uint128::new(100) }
        ).unwrap();

        // Ensure ExitVault submsg is present
        assert!(!result.messages.is_empty());

        // Simulate error reply from ExitVault (record pending bad debt)
        let err_reply = Reply { id: crate::contract::TRANSMUTER_REPLY_ID, result: SubMsgResult::Err("exit error".to_string()) };
        let _ = crate::contract::reply(deps.as_mut(), env.clone(), err_reply).unwrap();

        // Retry path: should produce ExitVault (reply_on_success) and a FulfillBadDebt with underlying funds
        let retry_res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("anyone", &[]),
            ExecuteMsg::RetryFailedBadDebt { asset: "uusd".to_string() }
        ).unwrap();
        assert_eq!(retry_res.messages.len(), 2);
        // First: ExitVault with vtkn funds
        if let CosmosMsg::Wasm(WasmMsg::Execute { contract_addr, funds, .. }) = &retry_res.messages[0].msg {
            assert_eq!(contract_addr, "vault_contract");
            assert_eq!(funds.len(), 1);
            assert_eq!(funds[0].denom, "vtkn");
            assert_eq!(funds[0].amount, Uint128::new(100));
        } else {
            panic!("expected ExitVault execute msg in retry");
        }
        // Second: FulfillBadDebt to cdp_contract with underlying funds
        if let CosmosMsg::Wasm(WasmMsg::Execute { contract_addr, funds, .. }) = &retry_res.messages[1].msg {
            assert_eq!(contract_addr, "cdp_contract");
            assert_eq!(funds.len(), 1);
            assert_eq!(funds[0].denom, "uusd");
            assert_eq!(funds[0].amount, Uint128::new(100));
        } else {
            panic!("expected FulfillBadDebt execute msg in retry");
        }

        // Simulate success reply from ExitVault -> should clear pending
        let ok_reply = Reply { id: crate::contract::TRANSMUTER_REPLY_ID, result: SubMsgResult::Ok(SubMsgResponse { events: vec![Event::new("ok")], data: Some(Binary::default()) }) };
        let _ = crate::contract::reply(deps.as_mut(), env.clone(), ok_reply).unwrap();

        // Calling retry again should now error (no pending bad debt)
        let retry_again = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("anyone", &[]),
            ExecuteMsg::RetryFailedBadDebt { asset: "uusd".to_string() }
        );
        assert!(retry_again.is_err());
    }

    #[test]
    fn test_retry_errors_and_pending_remains_retryable() {
        use membrane::cdp::QueryMsg as CDP_QueryMsg;

        let mut deps = mock_dependencies();
        let env = mock_env();

        // Mock GetBasket and Transmuter conversions (1:1)
        let basket = setup_mock_basket();
        deps.querier.update_wasm(move |wasm_query| {
            match wasm_query {
                cosmwasm_std::WasmQuery::Smart { msg, .. } => {
                    if let Ok(q) = from_json::<CDP_QueryMsg>(msg) {
                        match q {
                            CDP_QueryMsg::GetBasket { .. } => SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())),
                            _ => SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())),
                        }
                    } else if let Ok(tq) = from_json::<Transmuter_QueryMsg>(msg) {
                        match tq {
                            Transmuter_QueryMsg::DepositTokenConversion { deposit_token_amount } => SystemResult::Ok(ContractResult::Ok(to_json_binary(&deposit_token_amount).unwrap())),
                            Transmuter_QueryMsg::VaultTokenUnderlying { vault_token_amount } => SystemResult::Ok(ContractResult::Ok(to_json_binary(&vault_token_amount).unwrap())),
                            _ => SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())),
                        }
                    } else {
                        SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap()))
                    }
                }
                _ => SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())),
            }
        });

        // Instantiate with vault token denom
        instantiate_contract_with_vault(deps.as_mut(), env.clone()).unwrap();

        // Create queue and deposit
        execute(
            deps.as_mut(),
            env.clone(),
            mock_info("owner", &[]),
            ExecuteMsg::CreateQueue { asset: "uusd".to_string() },
        ).unwrap();

        execute(
            deps.as_mut(),
            env.clone(),
            mock_info("user1", &coins(1000, "uusd")),
            ExecuteMsg::SubmitDeposit { deposit_input: BackingDepositInput { asset: "uusd".to_string(), ltv: Decimal::percent(60), max_borrow_ltv: Decimal::percent(40) }, deposit_owner: None }
        ).unwrap();

        // Add bad debt and simulate initial ExitVault error to populate pending
        let add_res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("cdp_contract", &[]),
            ExecuteMsg::AddBadDebt { asset: "uusd".to_string(), amount: Uint128::new(100) }
        ).unwrap();
        assert!(!add_res.messages.is_empty());
        let err_reply = Reply { id: crate::contract::TRANSMUTER_REPLY_ID, result: SubMsgResult::Err("exit error".to_string()) };
        let _ = crate::contract::reply(deps.as_mut(), env.clone(), err_reply).unwrap();

        // Check pending is set to 100
        let pending = PENDING_BAD_DEBT.load(&deps.storage, "uusd".to_string()).unwrap();
        assert_eq!(pending, Uint128::new(100));

        // Call retry (but do not simulate any reply). Pending should remain unchanged and another retry should be possible
        let retry1 = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("anyone", &[]),
            ExecuteMsg::RetryFailedBadDebt { asset: "uusd".to_string() }
        ).unwrap();
        assert_eq!(retry1.messages.len(), 2);

        // Pending remains
        let pending_after_retry1 = PENDING_BAD_DEBT.load(&deps.storage, "uusd".to_string()).unwrap();
        assert_eq!(pending_after_retry1, Uint128::new(100));

        // Another retry still works
        let retry2 = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("anyone", &[]),
            ExecuteMsg::RetryFailedBadDebt { asset: "uusd".to_string() }
        ).unwrap();
        assert_eq!(retry2.messages.len(), 2);

        // Pending still unchanged (not auto-cleared on error since reply_on_success wasn't triggered)
        let pending_after_retry2 = PENDING_BAD_DEBT.load(&deps.storage, "uusd".to_string()).unwrap();
        assert_eq!(pending_after_retry2, Uint128::new(100));
    }

    #[test]
    fn test_add_revenue() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        
        // Mock CDP contract response
        let basket = setup_mock_basket();
        deps.querier.update_wasm(move |_| SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())));
        
        instantiate_contract(deps.as_mut(), env.clone()).unwrap();
        
        // Create queue and deposits first
        let msg = ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        };
        let info = mock_info("owner", &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Add some deposits
        for i in 0..2 {
            let msg = ExecuteMsg::SubmitDeposit {
                deposit_input: BackingDepositInput {
                    asset: "uusd".to_string(),
                    ltv: Decimal::percent(60 + i * 10),
                    max_borrow_ltv: Decimal::percent(40 + i * 10),
                },
                deposit_owner: None,
            };
            let info = mock_info(&format!("user{}", i), &coins(1000, "uusd"));
            execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        }
        
        // Test successful revenue addition
        let msg = ExecuteMsg::AddRevenue {
            asset: "uusd".to_string(),
        };
        let info = mock_info("anyone", &coins(100, "uusd"));
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_ok());
        
        // Verify revenue was distributed to groups (total_deposit_tokens should increase)
        let queue_res: LTVQueueResponse = from_json(&query(deps.as_ref(), env.clone(), QueryMsg::GetLTVQueue { 
            asset: "uusd".to_string() 
        }).unwrap()).unwrap();
        
        // Check that total_deposit_tokens increased in groups
        for slot in queue_res.queue.slots {
            for group in slot.deposit_groups {
                println!("Group max_borrow_ltv: {}, Group total_deposit_tokens: {}", group.max_borrow_ltv, group.total_deposit_tokens);
                // Each group should have more than the original 1000 deposit tokens due to revenue distribution
                assert!(group.total_deposit_tokens > Uint128::new(1000), 
                    "Group total_deposit_tokens should have increased from revenue distribution");
            }
        }
        
        // Test revenue with wrong denomination
        let msg = ExecuteMsg::AddRevenue {
            asset: "uusd".to_string(),
        };
        let info = mock_info("anyone", &coins(100, "wrong_denom"));
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_err());
        
        // Test revenue with no funds
        let msg = ExecuteMsg::AddRevenue {
            asset: "uusd".to_string(),
        };
        let info = mock_info("anyone", &[]);
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_err());
    }

    #[test]
    fn test_update_config() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        
        // Mock CDP contract response
        let basket = setup_mock_basket();
        deps.querier.update_wasm(move |_| SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())));
        
        instantiate_contract(deps.as_mut(), env.clone()).unwrap();
        
        // Test successful config update by owner
        let msg = ExecuteMsg::UpdateConfig {
            owner: Some("new_owner".to_string()),
            cdp_contract: Some("new_cdp".to_string()),
            deposit_denom: Some(DepositDenom { denom: "new_denom".to_string(), vault_info: None }),
            minimum_deposit: Some(Uint128::new(2000)),
            waiting_period: Some(172800),
            percent_to_disperse: Some(Decimal::percent(20)),
            dispersal_window: Some(48), // 48 hours
            activation_window: Some(48),
        };
        let info = mock_info("owner", &[]);
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_ok());
        
        // Test unauthorized config update
        let msg = ExecuteMsg::UpdateConfig {
            owner: Some("hacker".to_string()),
            cdp_contract: None,
            deposit_denom: None,
            minimum_deposit: None,
            waiting_period: None,
            percent_to_disperse: None,
            dispersal_window: None,
            activation_window: None,
        };
        let info = mock_info("unauthorized", &[]);
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_err());
    }

    #[test]
    fn test_query_config() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        
        // Mock CDP contract response
        let basket = setup_mock_basket();
        deps.querier.update_wasm(move |_| SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())));
        
        instantiate_contract(deps.as_mut(), env.clone()).unwrap();
        
        let msg = QueryMsg::Config {};
        let result = query(deps.as_ref(), env.clone(), msg);
        assert!(result.is_ok());
        
        let config: Config = from_json(result.unwrap()).unwrap();
        assert_eq!(config.owner, Addr::unchecked("owner"));
        assert_eq!(config.cdp_contract, Addr::unchecked("cdp_contract"));
        assert_eq!(config.deposit_denom.denom, "uusd");
        assert_eq!(config.minimum_deposit, Uint128::new(1000));
        assert_eq!(config.waiting_period, 86400);
    }

    #[test]
    fn test_query_ltv_queue() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        
        // Mock CDP contract response
        let basket = setup_mock_basket();
        deps.querier.update_wasm(move |_| SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())));
        
        instantiate_contract(deps.as_mut(), env.clone()).unwrap();
        
        // Create queue first
        let msg = ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        };
        let info = mock_info("owner", &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        let msg = QueryMsg::GetLTVQueue {
            asset: "uusd".to_string(),
        };
        let result = query(deps.as_ref(), env.clone(), msg);
        assert!(result.is_ok());
        
        let response: LTVQueueResponse = from_json(result.unwrap()).unwrap();
        assert_eq!(response.queue.current_deposit_id, Uint128::new(1));
        
        // Test query for non-existent queue
        let msg = QueryMsg::GetLTVQueue {
            asset: "unknown_asset".to_string(),
        };
        let result = query(deps.as_ref(), env.clone(), msg);
        assert!(result.is_err());
    }

    #[test]
    fn test_query_can_handle_bad_debt_true_and_boundary() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        // Mock CDP contract response
        let basket = setup_mock_basket();
        deps.querier.update_wasm(move |_| SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())));

        // Instantiate and create queue with a single 1000 deposit
        instantiate_contract(deps.as_mut(), env.clone()).unwrap();
        execute(
            deps.as_mut(),
            env.clone(),
            mock_info("owner", &[]),
            ExecuteMsg::CreateQueue { asset: "uusd".to_string() },
        ).unwrap();
        execute(
            deps.as_mut(),
            env.clone(),
            mock_info("user1", &coins(1000, "uusd")),
            ExecuteMsg::SubmitDeposit { deposit_input: BackingDepositInput { asset: "uusd".to_string(), ltv: Decimal::percent(60), max_borrow_ltv: Decimal::percent(40) }, deposit_owner: None }
        ).unwrap();

        // amount below total -> true
        let res_small = query(
            deps.as_ref(),
            env.clone(),
            QueryMsg::CanHandleBadDebt { asset: "uusd".to_string(), amount: Uint128::new(500) }
        ).unwrap();
        let can_small: bool = from_json(&res_small).unwrap();
        assert!(can_small);

        // amount equal to total -> true (>= check)
        let res_equal = query(
            deps.as_ref(),
            env.clone(),
            QueryMsg::CanHandleBadDebt { asset: "uusd".to_string(), amount: Uint128::new(1000) }
        ).unwrap();
        let can_equal: bool = from_json(&res_equal).unwrap();
        assert!(can_equal);


        // amount above total -> false
        let res = query(
            deps.as_ref(),
            env.clone(),
            QueryMsg::CanHandleBadDebt { asset: "uusd".to_string(), amount: Uint128::new(1001) }
        ).unwrap();
        let can_handle: bool = from_json(&res).unwrap();
        assert!(!can_handle);
    }

    #[test]
    fn test_query_can_handle_bad_debt_zero_amount_empty_queue() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        // Mock CDP contract response
        let basket = setup_mock_basket();
        deps.querier.update_wasm(move |_| SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())));

        // Instantiate and create queue but add no deposits
        instantiate_contract(deps.as_mut(), env.clone()).unwrap();
        execute(
            deps.as_mut(),
            env.clone(),
            mock_info("owner", &[]),
            ExecuteMsg::CreateQueue { asset: "uusd".to_string() },
        ).unwrap();

        // Zero amount should be considered handleable even with zero total
        let res = query(
            deps.as_ref(),
            env.clone(),
            QueryMsg::CanHandleBadDebt { asset: "uusd".to_string(), amount: Uint128::zero() }
        ).unwrap();
        let can_handle: bool = from_json(&res).unwrap();
        assert!(can_handle);
    }

    #[test]
    fn test_query_can_handle_bad_debt_unknown_asset_false() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        // Mock CDP contract response
        let basket = setup_mock_basket();
        deps.querier.update_wasm(move |_| SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())));

        // Instantiate but do not create the requested queue
        instantiate_contract(deps.as_mut(), env.clone()).unwrap();

        let res = query(
            deps.as_ref(),
            env.clone(),
            QueryMsg::CanHandleBadDebt { asset: "unknown_asset".to_string(), amount: Uint128::new(1) }
        ).unwrap();
        let can_handle: bool = from_json(&res).unwrap();
        assert!(!can_handle);
    }

    #[test]
    fn test_query_backing_deposit() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        
        // Mock CDP contract response
        let basket = setup_mock_basket();
        deps.querier.update_wasm(move |_| SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())));
        
        instantiate_contract(deps.as_mut(), env.clone()).unwrap();
        
        // Create queue and deposit first
        let msg = ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        };
        let info = mock_info("owner", &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        let msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "uusd".to_string(),
                ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(40),
            },
            deposit_owner: None,
        };
        let info = mock_info("user1", &coins(1000, "uusd"));
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        let msg = QueryMsg::GetBackingDeposit {
            deposit_id: Uint128::new(1),
            asset: "uusd".to_string(),
        };
        let result = query(deps.as_ref(), env.clone(), msg);
        assert!(result.is_ok());
        
        let response: BackingDepositResponse = from_json(result.unwrap()).unwrap();
        assert_eq!(response.deposit.user, Addr::unchecked("user1"));
        assert_eq!(response.deposit.id, Uint128::new(1));
        
        // Test query for non-existent deposit
        let msg = QueryMsg::GetBackingDeposit {
            deposit_id: Uint128::new(999),
            asset: "uusd".to_string(),
        };
        let result = query(deps.as_ref(), env.clone(), msg);
        assert!(result.is_err());
    }

    #[test]
    fn test_query_backing_deposits_by_user() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        
        // Mock CDP contract response
        let basket = setup_mock_basket();
        deps.querier.update_wasm(move |_| SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())));
        
        instantiate_contract(deps.as_mut(), env.clone()).unwrap();
        
        // Create queue and deposits first
        let msg = ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        };
        let info = mock_info("owner", &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Add multiple deposits for same user
        for i in 0..3 {
            let msg = ExecuteMsg::SubmitDeposit {
                deposit_input: BackingDepositInput {
                    asset: "uusd".to_string(),
                    ltv: Decimal::percent(60 + i * 5),
                    max_borrow_ltv: Decimal::percent(40 + i * 5),
                },
                deposit_owner: None,
            };
            let info = mock_info("user1", &coins(1000, "uusd"));
            execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        }
        
        let msg = QueryMsg::GetBackingDepositsByUser {
            user: "user1".to_string(),
            asset: "uusd".to_string(),
            limit: Some(10),
            start_after: None,
        };
        let result = query(deps.as_ref(), env.clone(), msg);
        assert!(result.is_ok());
        
        let response: BackingDepositsByUserResponse = from_json(result.unwrap()).unwrap();
        assert_eq!(response.deposits.len(), 3);
        
        // Test with limit
        let msg = QueryMsg::GetBackingDepositsByUser {
            user: "user1".to_string(),
            asset: "uusd".to_string(),
            limit: Some(2),
            start_after: None,
        };
        let result = query(deps.as_ref(), env.clone(), msg);
        assert!(result.is_ok());
        
        let response: BackingDepositsByUserResponse = from_json(result.unwrap()).unwrap();
        assert_eq!(response.deposits.len(), 2);
        
        // Test with start_after
        let msg = QueryMsg::GetBackingDepositsByUser {
            user: "user1".to_string(),
            asset: "uusd".to_string(),
            limit: Some(10),
            start_after: Some(Uint128::new(1)),
        };
        let result = query(deps.as_ref(), env.clone(), msg);
        assert!(result.is_ok());
        
        let response: BackingDepositsByUserResponse = from_json(result.unwrap()).unwrap();
        assert_eq!(response.deposits.len(), 2);
    }

    #[test]
    fn test_query_average_ltvs_multi_asset() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        
        // Mock CDP contract response
        let basket = setup_mock_basket();
        deps.querier.update_wasm(move |_| SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())));
        
        instantiate_contract(deps.as_mut(), env.clone()).unwrap();
        
        // Create queues and deposits first
        for asset in ["uusd", "untrn"] {
            let msg = ExecuteMsg::CreateQueue {
                asset: asset.to_string(),
            };
            let info = mock_info("owner", &[]);
            execute(deps.as_mut(), env.clone(), info, msg).unwrap();
            
            let msg = ExecuteMsg::SubmitDeposit {
                deposit_input: BackingDepositInput {
                    asset: asset.to_string(),
                    ltv: Decimal::percent(60),
                    max_borrow_ltv: Decimal::percent(40),
                },
                deposit_owner: None,
            };
            let info = mock_info("user1", &coins(1000, asset));
            execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        }
        
        let msg = QueryMsg::GetAverageLTVs {
            assets: vec!["uusd".to_string(), "untrn".to_string()],
        };
        let result = query(deps.as_ref(), env.clone(), msg);
        println!("result: {:?}", result);
        assert!(result.is_ok());
        
        let response: AverageLTVsResponse = from_json(result.unwrap()).unwrap();
        println!("response: {:?}", response);
        assert!(response.average_max_ltv > Decimal::zero());
        assert!(response.average_max_borrow_ltv > Decimal::zero());
        
        // Test with empty assets list
        let msg = QueryMsg::GetAverageLTVs {
            assets: vec![],
        };
        let result = query(deps.as_ref(), env.clone(), msg);
        assert!(result.is_ok());
        
        let response: AverageLTVsResponse = from_json(result.unwrap()).unwrap();
        assert_eq!(response.average_max_ltv, Decimal::zero());
        assert_eq!(response.average_max_borrow_ltv, Decimal::zero());
    }

    #[test]
    fn test_query_average_ltvs() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        
        // Mock CDP contract response
        let basket = setup_mock_basket();
        deps.querier.update_wasm(move |_| SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())));
        
        instantiate_contract(deps.as_mut(), env.clone()).unwrap();
        
        // Create queue and deposits first
        let msg = ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        };
        let info = mock_info("owner", &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        for i in 0..3 {
            let msg = ExecuteMsg::SubmitDeposit {
                deposit_input: BackingDepositInput {
                    asset: "uusd".to_string(),
                    ltv: Decimal::percent(60),
                    max_borrow_ltv: Decimal::percent(40 + i * 10),
                },
                deposit_owner: None,
            };
            let info = mock_info(&format!("user{}", i), &coins(1000, "uusd"));
            execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        }
        
        let msg = QueryMsg::GetAverageLTVs {
            assets: vec!["uusd".to_string()],
        };
        let result = query(deps.as_ref(), env.clone(), msg);
        assert!(result.is_ok());
        
        let response: AverageLTVsResponse = from_json(result.unwrap()).unwrap();
        assert!(response.average_max_ltv > Decimal::zero());
        assert!(response.average_max_borrow_ltv > Decimal::zero());
        
        // Test with empty assets list
        let msg = QueryMsg::GetAverageLTVs {
            assets: vec![],
        };
        let result = query(deps.as_ref(), env.clone(), msg);
        assert!(result.is_ok());
        
        let response: AverageLTVsResponse = from_json(result.unwrap()).unwrap();
        assert_eq!(response.average_max_ltv, Decimal::zero());
        assert_eq!(response.average_max_borrow_ltv, Decimal::zero());
    }

    #[test]
    fn test_edge_cases() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        
        // Mock CDP contract response
        let basket = setup_mock_basket();
        deps.querier.update_wasm(move |_| SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap())));
        
        instantiate_contract(deps.as_mut(), env.clone()).unwrap();
        
        // Test revenue distribution with no deposits
        let msg = ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        };
        let info = mock_info("owner", &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        let msg = ExecuteMsg::AddRevenue {
            asset: "uusd".to_string(),
        };
        let info = mock_info("anyone", &coins(100, "uusd"));
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_ok()); // Should succeed but do nothing
        
        // Test bad debt with no deposits
        let msg = ExecuteMsg::AddBadDebt {
            asset: "uusd".to_string(),
            amount: Uint128::new(100),
        };
        let info = mock_info("cdp_contract", &[]);
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_ok()); // Should succeed but do nothing
        
        // Test withdrawal from empty queue
        let msg = ExecuteMsg::WithdrawDeposit {
            deposit_id: Uint128::new(1),
            asset: "uusd".to_string(),
            amount: Some(Uint128::new(100)),
        };
        let info = mock_info("user1", &[]);
        let result = execute(deps.as_mut(), env.clone(), info, msg);
        assert!(result.is_err());
    }
}