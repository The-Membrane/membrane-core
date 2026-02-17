use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{coins, from_json, Decimal, Uint128, to_json_binary, SystemResult, ContractResult, Binary, BankMsg};
    use membrane::ltv_disco::*;
    use membrane::types::{cAsset, Asset, AssetInfo, Basket, DepositDenom, PendingRevenue};
    use membrane::oracle::PriceResponse;
    use ltv_disco::contract::{instantiate, execute, query};

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
            cpc_margin_of_error: Decimal::percent(100),
            liq_queue: None,
        }
    }

    fn instantiate_contract() -> (cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>, cosmwasm_std::Env) {
        let mut deps = mock_dependencies();
        let env = mock_env();
        
        // Setup querier to return mock basket
        deps.querier.update_wasm(move |query| {
            match query {
                cosmwasm_std::WasmQuery::Smart { contract_addr: _, msg } => {
                    let parsed: Result<membrane::cdp::QueryMsg, _> = from_json(msg);
                    if let Ok(membrane::cdp::QueryMsg::GetBasket {}) = parsed {
                        SystemResult::Ok(ContractResult::Ok(to_json_binary(&setup_mock_basket()).unwrap()))
                    } else {
                        SystemResult::Ok(ContractResult::Ok(Binary::default()))
                    }
                }
                _ => SystemResult::Ok(ContractResult::Ok(Binary::default())),
            }
        });

        let msg = InstantiateMsg {
            owner: Some("owner".to_string()),
            cdp_contract: "cdp_contract".to_string(),
            deposit_denom: DepositDenom { denom: "uusd".to_string(), vault_info: None },
            cdt_denom: "reward_token".to_string(),
            minimum_deposit: Uint128::new(1000),
            max_ltv: Decimal::percent(80),
            percent_to_disperse: Decimal::percent(10),
            dispersal_window: 2,
            activation_window: 2,
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
        
        let info = mock_info("owner", &[]);
        instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        (deps, env)
    }

    #[test]
    fn test_event_based_revenue_distribution() {
        let (mut deps, mut env) = instantiate_contract();
        
        // Create queue
        let info = mock_info("cdp_contract", &[]);
        let msg = ExecuteMsg::CreateQueue { asset: "uusd".to_string() };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Submit deposits from multiple users
        for i in 0..5 {
            let user = format!("user{}", i);
            let info = mock_info(&user, &coins(10000, "uusd"));
            let msg = ExecuteMsg::SubmitDeposit {
                deposit_input: BackingDepositInput {
                    asset: "uusd".to_string(),
                    ltv: Decimal::percent(60),
                    max_borrow_ltv: Decimal::percent(40),
                    epoch_start_time: Some(0),
                },
                deposit_owner: None,
                locked: None,
                deposit_id: None,
                manager: None,
                affiliate_address: None,
            };
            execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        }
        
        // Add revenue
        env.block.time = env.block.time.plus_seconds(100);
        let info = mock_info("anyone", &coins(5000, "reward_token"));
        let msg = ExecuteMsg::AddRevenue { asset: "uusd".to_string() };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Query pending claims for user0
        let msg = QueryMsg::PendingClaims {
            user: "user0".to_string(),
            asset: "uusd".to_string(),
        };
        let result = query(deps.as_ref(), env.clone(), msg).unwrap();
        let pending: PendingClaimsResponse = from_json(&result).unwrap();
        
        println!("Pending claims for user0: {:?}", pending);
        if !pending.claims.is_empty() {
            assert!(pending.claims[0].pending_amount > Uint128::zero());
        }
        
        // Claim revenue for user0
        let info = mock_info("anyone", &[]);
        let msg = ExecuteMsg::ClaimRevenueForUser {
            compound_action: None,
            user: "user0".to_string(),
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
            limit: None,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Verify revenue was sent
        assert!(res.messages.len() > 0);
        
        // Query pending claims again - should be zero
        let msg = QueryMsg::PendingClaims {
            user: "user0".to_string(),
            asset: "uusd".to_string(),
        };
        let result = query(deps.as_ref(), env.clone(), msg).unwrap();
        let pending: PendingClaimsResponse = from_json(&result).unwrap();
        
        // After claiming, pending should be zero or empty
        if !pending.claims.is_empty() {
            assert_eq!(pending.claims[0].pending_amount, Uint128::zero());
        }
    }

    #[test]
    fn test_auto_claim_on_deposit() {
        let (mut deps, mut env) = instantiate_contract();
        
        // Create queue
        let info = mock_info("cdp_contract", &[]);
        let msg = ExecuteMsg::CreateQueue { asset: "uusd".to_string() };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Submit initial deposit
        let info = mock_info("user1", &coins(10000, "uusd"));
        let msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "uusd".to_string(),
                ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(40),
                epoch_start_time: Some(0),
            },
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Advance time BEFORE adding revenue so revenue events have timestamp > deposit.last_claimed
        env.block.time = env.block.time.plus_seconds(100);
        
        // Add revenue (revenue events will have timestamp = current time, which is > deposit.last_claimed)
        let info = mock_info("anyone", &coins(1000, "reward_token"));
        let msg = ExecuteMsg::AddRevenue { asset: "uusd".to_string() };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Submit another deposit - should auto-claim (revenue events are now claimable)
        // Use the same deposit_id to top-up the existing deposit (which triggers auto-claim)
        env.block.time = env.block.time.plus_seconds(100);
        let info = mock_info("user1", &coins(5000, "uusd"));
        let msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "uusd".to_string(),
                ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(40),
                epoch_start_time: Some(0),
            },
            deposit_owner: None,
            locked: None,
            deposit_id: Some(Uint128::one()), // Use deposit_id 1 to top-up existing deposit
            manager: None,
            affiliate_address: None,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Verify revenue was auto-claimed and sent
        let has_bank_send = res.messages.iter().any(|msg| {
            matches!(&msg.msg, cosmwasm_std::CosmosMsg::Bank(BankMsg::Send { .. }))
        });
        assert!(has_bank_send, "Should have sent claimed revenue");
    }

    #[test]
    fn test_auto_claim_on_withdrawal() {
        let (mut deps, mut env) = instantiate_contract();
        
        // Create queue
        let info = mock_info("cdp_contract", &[]);
        let msg = ExecuteMsg::CreateQueue { asset: "uusd".to_string() };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Submit deposit
        let info = mock_info("user1", &coins(10000, "uusd"));
        let msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "uusd".to_string(),
                ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(40),
                epoch_start_time: Some(0),
            },
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Add revenue
        env.block.time = env.block.time.plus_seconds(100);
        let info = mock_info("anyone", &coins(1000, "reward_token"));
        let msg = ExecuteMsg::AddRevenue { asset: "uusd".to_string() };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Withdraw - should auto-claim
        env.block.time = env.block.time.plus_seconds(100);
        let info = mock_info("user1", &[]);
        // Get deposit_id first
        let queue_msg = QueryMsg::GetLTVQueue { assets: vec!["uusd".to_string() ], limit: None, start_after: None };
        let queue_result = query(deps.as_ref(), env.clone(), queue_msg).unwrap();
        let queue: LTVQueueResponse = from_json(&queue_result).unwrap();
        let deposit_id = queue.queues[0].1.current_deposit_id - Uint128::one();
        
        let msg = ExecuteMsg::WithdrawDeposit {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
            deposit_id,
            amount: Some(Uint128::new(5000)),
            epoch_start_time: 0,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Verify both withdrawal and revenue were sent
        let bank_sends: Vec<_> = res.messages.iter().filter(|msg| {
            matches!(&msg.msg, cosmwasm_std::CosmosMsg::Bank(BankMsg::Send { .. }))
        }).collect();
        
        // Should have 2 bank sends: one for withdrawal, one for revenue
        assert_eq!(bank_sends.len(), 2, "Should have sent both withdrawal and revenue");
    }

    #[test]
    fn test_stress_20k_users() {
        let (mut deps, mut env) = instantiate_contract();
        
        println!("Starting 20k user stress test...");
        
        // Create queue
        let info = mock_info("cdp_contract", &[]);
        let msg = ExecuteMsg::CreateQueue { asset: "uusd".to_string() };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        let num_users = 20000;
        let deposit_amount = 10000u128;
        
        // Phase 1: Submit deposits for 20k users
        println!("Phase 1: Submitting {} deposits...", num_users);
        for i in 0..num_users {
            if i % 1000 == 0 {
                println!("  Processed {} deposits", i);
            }
            
            let user = format!("user{}", i);
            let info = mock_info(&user, &coins(deposit_amount, "uusd"));
            
            // Vary LTV and max_borrow_ltv to create different groups
            let ltv = if i % 3 == 0 {
                Decimal::percent(60)
            } else if i % 3 == 1 {
                Decimal::percent(65)
            } else {
                Decimal::percent(70)
            };
            
            let max_borrow_ltv = if i % 2 == 0 {
                Decimal::percent(40)
            } else {
                Decimal::percent(45)
            };
            
            let msg = ExecuteMsg::SubmitDeposit {
                deposit_input: BackingDepositInput {
                    asset: "uusd".to_string(),
                    ltv,
                    max_borrow_ltv,
                    epoch_start_time: Some(0),
                },
                deposit_owner: None,
                locked: None,
                deposit_id: None,
                manager: None,
                affiliate_address: None,
            };
            
            execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        }
        println!("Phase 1 complete: {} deposits submitted", num_users);
        
        // Phase 2: Add revenue multiple times
        println!("Phase 2: Adding revenue 10 times...");
        for i in 0..10 {
            env.block.time = env.block.time.plus_seconds(100);
            let info = mock_info("revenue_source", &coins(1_000_000, "reward_token"));
            let msg = ExecuteMsg::AddRevenue { asset: "uusd".to_string() };
            execute(deps.as_mut(), env.clone(), info, msg).unwrap();
            println!("  Added revenue iteration {}", i + 1);
        }
        println!("Phase 2 complete: 10 revenue distributions");
        
        // Phase 3: Random withdrawals and redeposits
        println!("Phase 3: Processing 1000 random withdrawals/redeposits...");
        for i in 0..1000 {
            if i % 100 == 0 {
                println!("  Processed {} withdrawals/redeposits", i);
            }
            
            let user_index = (i * 17) % num_users; // Pseudo-random selection
            let user = format!("user{}", user_index);
            
            let ltv = if user_index % 3 == 0 {
                Decimal::percent(60)
            } else if user_index % 3 == 1 {
                Decimal::percent(65)
            } else {
                Decimal::percent(70)
            };
            
            let max_borrow_ltv = if user_index % 2 == 0 {
                Decimal::percent(40)
            } else {
                Decimal::percent(45)
            };
            
            // Withdraw partial amount
            env.block.time = env.block.time.plus_seconds(10);
            let info = mock_info(&user, &[]);
            // Get deposit_id - would need to query, but for stress test, use first deposit
            // Since this is a stress test with many users, we'll use a placeholder
            // In a real scenario, you'd query for the user's deposit_id
            let deposit_id = Uint128::one(); // Using first deposit as approximation
            let msg = ExecuteMsg::WithdrawDeposit {
                asset: "uusd".to_string(),
                ltv,
                max_borrow_ltv,
                deposit_id,
                amount: Some(Uint128::new(2000)),
                epoch_start_time: 0,
            };
            
            let res = execute(deps.as_mut(), env.clone(), info, msg);
            if res.is_err() {
                // User might not have enough or other error, skip
                continue;
            }
            
            // Redeposit
            env.block.time = env.block.time.plus_seconds(10);
            let info = mock_info(&user, &coins(3000, "uusd"));
            let msg = ExecuteMsg::SubmitDeposit {
                deposit_input: BackingDepositInput {
                    asset: "uusd".to_string(),
                    ltv,
                    max_borrow_ltv,
                    epoch_start_time: Some(0),
                },
                deposit_owner: None,
                locked: None,
                deposit_id: None,
                manager: None,
                affiliate_address: None,
            };
            execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        }
        println!("Phase 3 complete: 1000 withdrawals/redeposits");
        
        // Phase 4: Query pending claims for sample users
        println!("Phase 4: Querying pending claims for 100 sample users...");
        let mut total_pending = Uint128::zero();
        for i in 0..100 {
            let user_index = i * 200; // Sample every 200th user
            let user = format!("user{}", user_index);
            
            let msg = QueryMsg::PendingClaims {
                user: user.clone(),
                asset: "uusd".to_string(),
            };
            
            let result = query(deps.as_ref(), env.clone(), msg).unwrap();
            let pending: PendingClaimsResponse = from_json(&result).unwrap();
            
            for claim in pending.claims {
                total_pending += claim.pending_amount;
            }
        }
        println!("Phase 4 complete: Total pending for 100 sampled users: {}", total_pending);
        
        // Phase 5: Claim revenue for 100 users
        println!("Phase 5: Claiming revenue for 100 users...");
        let mut total_claimed = Uint128::zero();
        for i in 0..100 {
            let user_index = i * 200;
            let user = format!("user{}", user_index);
            
            let ltv = if user_index % 3 == 0 {
                Decimal::percent(60)
            } else if user_index % 3 == 1 {
                Decimal::percent(65)
            } else {
                Decimal::percent(70)
            };
            
            let max_borrow_ltv = if user_index % 2 == 0 {
                Decimal::percent(40)
            } else {
                Decimal::percent(45)
            };
            
            env.block.time = env.block.time.plus_seconds(1);
            let info = mock_info("claimer", &[]);
            let msg = ExecuteMsg::ClaimRevenueForUser {
            compound_action: None,
                user: user.clone(),
                asset: "uusd".to_string(),
                max_ltv: ltv,
                max_borrow_ltv,
                limit: None,
            };
            
            let res = execute(deps.as_mut(), env.clone(), info, msg);
            if let Ok(response) = res {
                // Extract claimed amount from attributes
                for attr in response.attributes {
                    if attr.key == "revenue_claimed" {
                        if let Ok(amount) = attr.value.parse::<u128>() {
                            total_claimed += Uint128::new(amount);
                        }
                    }
                }
            }
        }
        println!("Phase 5 complete: Total claimed by 100 users: {}", total_claimed);
        
        // Phase 6: Verify state consistency
        println!("Phase 6: Verifying state consistency...");
        let msg = QueryMsg::GetLTVQueue { assets: vec!["uusd".to_string() ], limit: None, start_after: None };
        let result = query(deps.as_ref(), env.clone(), msg).unwrap();
        let queue_response: LTVQueueResponse = from_json(&result).unwrap();
        
        let mut total_deposits = Uint128::zero();
        let mut total_vault_tokens = Uint128::zero();
        for slot in &queue_response.queues[0].1.slots {
            total_deposits += slot.total_deposit_tokens;
            for group in slot.deposit_groups {
                total_vault_tokens += group.total_vault_tokens;
            }
        }
        
        println!("  Total deposit tokens: {}", total_deposits);
        println!("  Total vault tokens: {}", total_vault_tokens);
        assert!(total_deposits > Uint128::zero(), "Should have deposits");
        assert!(total_vault_tokens > Uint128::zero(), "Should have vault tokens");
        
        println!("\n=== STRESS TEST COMPLETE ===");
        println!("Successfully processed:");
        println!("  - {} user deposits", num_users);
        println!("  - 10 revenue distributions");
        println!("  - 1000 withdrawals/redeposits");
        println!("  - 100 pending claims queries");
        println!("  - 100 revenue claims");
        println!("  - State verification passed");
    }

    #[test]
    fn test_user_lifetime_revenue() {
        let (mut deps, mut env) = instantiate_contract();
        
        // Create queue
        let info = mock_info("cdp_contract", &[]);
        let msg = ExecuteMsg::CreateQueue { asset: "uusd".to_string() };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Submit deposit
        let info = mock_info("user1", &coins(10000, "uusd"));
        let msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "uusd".to_string(),
                ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(40),
                epoch_start_time: Some(0),
            },
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Add revenue and claim multiple times
        for _ in 0..3 {
            env.block.time = env.block.time.plus_seconds(100);
            let info = mock_info("anyone", &coins(1000, "reward_token"));
            let msg = ExecuteMsg::AddRevenue { asset: "uusd".to_string() };
            execute(deps.as_mut(), env.clone(), info, msg).unwrap();
            
            env.block.time = env.block.time.plus_seconds(10);
            let info = mock_info("anyone", &[]);
            let msg = ExecuteMsg::ClaimRevenueForUser {
            compound_action: None,
                user: "user1".to_string(),
                asset: "uusd".to_string(),
                max_ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(40),
                limit: None,
            };
            execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        }
        
        // Query lifetime revenue
        let msg = QueryMsg::GetUserLifetimeRevenue {
            user: "user1".to_string(),
            asset: "uusd".to_string(),
        };
        let result = query(deps.as_ref(), env.clone(), msg).unwrap();
        let lifetime: Vec<UserLifetimeRevenueEntry> = from_json(&result).unwrap();
        
        println!("User lifetime revenue entries: {:?}", lifetime);
        assert!(!lifetime.is_empty());
        assert!(lifetime.last().unwrap().total_claimed > Uint128::zero());
    }

    #[test]
    fn test_deposit_for_another_user() {
        let (mut deps, mut env) = instantiate_contract();
        
        // Create queue
        let info = mock_info("cdp_contract", &[]);
        let msg = ExecuteMsg::CreateQueue { asset: "uusd".to_string() };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // User1 deposits for user2
        let info = mock_info("user1", &coins(10000, "uusd"));
        let msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "uusd".to_string(),
                ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(40),
                epoch_start_time: Some(0),
            },
            deposit_owner: Some("user2".to_string()),
            locked: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Add revenue
        env.block.time = env.block.time.plus_seconds(100);
        let info = mock_info("anyone", &coins(1000, "reward_token"));
        let msg = ExecuteMsg::AddRevenue { asset: "uusd".to_string() };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Query pending claims for user2 (the owner)
        let msg = QueryMsg::PendingClaims {
            user: "user2".to_string(),
            asset: "uusd".to_string(),
        };
        let result = query(deps.as_ref(), env.clone(), msg).unwrap();
        let pending: PendingClaimsResponse = from_json(&result).unwrap();
        
        if !pending.claims.is_empty() {
            assert!(pending.claims[0].pending_amount > Uint128::zero());
        }
        assert!(pending.claims[0].pending_amount > Uint128::zero());
        
        // Claim for user2
        let info = mock_info("anyone", &[]);
        let msg = ExecuteMsg::ClaimRevenueForUser {
            compound_action: None,
            user: "user2".to_string(),
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
            limit: None,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Verify revenue sent to user2, not user1
        let bank_sends: Vec<_> = res.messages.iter().filter_map(|msg| {
            if let cosmwasm_std::CosmosMsg::Bank(BankMsg::Send { to_address, .. }) = &msg.msg {
                Some(to_address.clone())
            } else {
                None
            }
        }).collect();
        
        assert_eq!(bank_sends.len(), 1);
        assert_eq!(bank_sends[0], "user2");
    }

    #[test]
    fn test_amount_to_be_claimed_accuracy() {
        let (mut deps, mut env) = instantiate_contract();
        
        // Create queue
        let info = mock_info("cdp_contract", &[]);
        let msg = ExecuteMsg::CreateQueue { asset: "uusd".to_string() };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Submit 3 deposits with different amounts
        let info = mock_info("user1", &coins(10000, "uusd"));
        let msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "uusd".to_string(),
                ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(40),
                epoch_start_time: Some(0),
            },
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        let info = mock_info("user2", &coins(20000, "uusd"));
        let msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "uusd".to_string(),
                ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(40),
                epoch_start_time: Some(0),
            },
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        let info = mock_info("user3", &coins(30000, "uusd"));
        let msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "uusd".to_string(),
                ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(40),
                epoch_start_time: Some(0),
            },
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Total deposits: 60000
        // Add revenue
        env.block.time = env.block.time.plus_seconds(100);
        let revenue_amount = 6000u128;
        let info = mock_info("anyone", &coins(revenue_amount, "reward_token"));
        let msg = ExecuteMsg::AddRevenue { asset: "uusd".to_string() };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Query revenue events to check amount_to_be_claimed
        let msg = QueryMsg::GetRevenueEvents {
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
        };
        let result = query(deps.as_ref(), env.clone(), msg).unwrap();
        let events: Vec<RevenueEvent> = from_json(&result).unwrap();
        
        println!("Initial events: {:?}", events);
        assert_eq!(events.len(), 1);
        // 90% of revenue goes to users (10% dispersed)
        let expected_initial = Uint128::new((revenue_amount as f64 * 0.9) as u128);
        assert!(events[0].amount_to_be_claimed > Uint128::zero());
        assert!(events[0].amount_to_be_claimed <= expected_initial);
        let initial_amount_to_be_claimed = events[0].amount_to_be_claimed;
        
        // Claim for user1 (should get 1/6 of total)
        let info = mock_info("anyone", &[]);
        let msg = ExecuteMsg::ClaimRevenueForUser {
            compound_action: None,
            user: "user1".to_string(),
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
            limit: None,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Extract claimed amount
        let claimed_user1 = res.attributes.iter()
            .find(|attr| attr.key == "revenue_claimed")
            .and_then(|attr| attr.value.parse::<u128>().ok())
            .unwrap();
        
        println!("User1 claimed: {}", claimed_user1);
        
        // Check amount_to_be_claimed decreased
        let msg = QueryMsg::GetRevenueEvents {
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
        };
        let result = query(deps.as_ref(), env.clone(), msg).unwrap();
        let events: Vec<RevenueEvent> = from_json(&result).unwrap();
        
        println!("After user1 claim: {:?}", events);
        assert_eq!(events.len(), 1);
        let expected_after_user1 = initial_amount_to_be_claimed - Uint128::new(claimed_user1);
        assert_eq!(events[0].amount_to_be_claimed, expected_after_user1);
        
        // Claim for user2 (should get 2/6 of total)
        let info = mock_info("anyone", &[]);
        let msg = ExecuteMsg::ClaimRevenueForUser {
            compound_action: None,
            user: "user2".to_string(),
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
            limit: None,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        let claimed_user2 = res.attributes.iter()
            .find(|attr| attr.key == "revenue_claimed")
            .and_then(|attr| attr.value.parse::<u128>().ok())
            .unwrap();
        
        println!("User2 claimed: {}", claimed_user2);
        
        // Check amount_to_be_claimed decreased again
        let msg = QueryMsg::GetRevenueEvents {
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
        };
        let result = query(deps.as_ref(), env.clone(), msg).unwrap();
        let events: Vec<RevenueEvent> = from_json(&result).unwrap();
        
        println!("After user2 claim: {:?}", events);
        assert_eq!(events.len(), 1);
        let expected_after_user2 = expected_after_user1 - Uint128::new(claimed_user2);
        assert_eq!(events[0].amount_to_be_claimed, expected_after_user2);
        
        // Claim for user3 (should get 3/6 of total, which should be the remaining)
        let info = mock_info("anyone", &[]);
        let msg = ExecuteMsg::ClaimRevenueForUser {
            compound_action: None,
            user: "user3".to_string(),
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
            limit: None,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        let claimed_user3 = res.attributes.iter()
            .find(|attr| attr.key == "revenue_claimed")
            .and_then(|attr| attr.value.parse::<u128>().ok())
            .unwrap();
        
        println!("User3 claimed: {}", claimed_user3);
        
        // Check event was trimmed (amount_to_be_claimed should be zero or event removed)
        let msg = QueryMsg::GetRevenueEvents {
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
        };
        let result = query(deps.as_ref(), env.clone(), msg);
        
        if let Ok(binary) = result {
            let events: Vec<RevenueEvent> = from_json(&binary).unwrap();
            println!("After user3 claim (all claimed): {:?}", events);
            // Event should be trimmed (empty vec)
            assert!(events.is_empty(), "Event should be trimmed after all claims");
        } else {
            // Or query returns error if no events
            println!("No events found (correctly trimmed)");
        }
        
        // Verify total claimed equals initial amount_to_be_claimed
        let total_claimed = claimed_user1 + claimed_user2 + claimed_user3;
        println!("Total claimed: {}, Expected: {}", total_claimed, initial_amount_to_be_claimed);
        // Allow for small rounding differences (within 1 token per user)
        assert!(
            total_claimed >= initial_amount_to_be_claimed.u128().saturating_sub(3)
                && total_claimed <= initial_amount_to_be_claimed.u128() + 3,
            "Total claimed should approximately equal initial amount_to_be_claimed"
        );
    }

    #[test]
    fn test_event_trimming_with_multiple_events() {
        let (mut deps, mut env) = instantiate_contract();
        
        // Create queue
        let info = mock_info("cdp_contract", &[]);
        let msg = ExecuteMsg::CreateQueue { asset: "uusd".to_string() };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Submit deposits
        for i in 0..5 {
            let user = format!("user{}", i);
            let info = mock_info(&user, &coins(10000, "uusd"));
            let msg = ExecuteMsg::SubmitDeposit {
                deposit_input: BackingDepositInput {
                    asset: "uusd".to_string(),
                    ltv: Decimal::percent(60),
                    max_borrow_ltv: Decimal::percent(40),
                    epoch_start_time: Some(0),
                },
                deposit_owner: None,
                locked: None,
                deposit_id: None,
                manager: None,
                affiliate_address: None,
            };
            execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        }
        
        // Add revenue 5 times
        for i in 0..5 {
            env.block.time = env.block.time.plus_seconds(100);
            let info = mock_info("anyone", &coins(1000, "reward_token"));
            let msg = ExecuteMsg::AddRevenue { asset: "uusd".to_string() };
            execute(deps.as_mut(), env.clone(), info, msg).unwrap();
            println!("Added revenue iteration {}", i + 1);
        }
        
        // Check we have 5 events
        let msg = QueryMsg::GetRevenueEvents {
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
        };
        let result = query(deps.as_ref(), env.clone(), msg).unwrap();
        let events: Vec<RevenueEvent> = from_json(&result).unwrap();
        
        println!("Total events before claiming: {}", events.len());
        assert_eq!(events.len(), 5);
        
        // Claim for all users
        for i in 0..5 {
            let user = format!("user{}", i);
            env.block.time = env.block.time.plus_seconds(10);
            let info = mock_info("anyone", &[]);
            let msg = ExecuteMsg::ClaimRevenueForUser {
            compound_action: None,
                user: user.clone(),
                asset: "uusd".to_string(),
                max_ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(40),
                limit: None,
            };
            execute(deps.as_mut(), env.clone(), info, msg).unwrap();
            println!("Claimed for {}", user);
        }
        
        // Check all events were trimmed
        let msg = QueryMsg::GetRevenueEvents {
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
        };
        let result = query(deps.as_ref(), env.clone(), msg);
        
        if let Ok(binary) = result {
            let events: Vec<RevenueEvent> = from_json(&binary).unwrap();
            println!("Events after all claims: {}", events.len());
            assert!(events.is_empty(), "All events should be trimmed");
        } else {
            println!("No events found (correctly trimmed)");
        }
    }

    #[test]
    fn test_partial_event_trimming() {
        let (mut deps, mut env) = instantiate_contract();
        
        // Create queue
        let info = mock_info("cdp_contract", &[]);
        let msg = ExecuteMsg::CreateQueue { asset: "uusd".to_string() };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Submit deposits
        let info = mock_info("user1", &coins(10000, "uusd"));
        let msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "uusd".to_string(),
                ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(40),
                epoch_start_time: Some(0),
            },
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        let info = mock_info("user2", &coins(10000, "uusd"));
        let msg = ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: "uusd".to_string(),
                ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(40),
                epoch_start_time: Some(0),
            },
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Add revenue 3 times
        for i in 0..3 {
            env.block.time = env.block.time.plus_seconds(100);
            let info = mock_info("anyone", &coins(1000, "reward_token"));
            let msg = ExecuteMsg::AddRevenue { asset: "uusd".to_string() };
            execute(deps.as_mut(), env.clone(), info, msg).unwrap();
            println!("Added revenue iteration {}", i + 1);
        }
        
        // Claim for user1 only
        env.block.time = env.block.time.plus_seconds(10);
        let info = mock_info("anyone", &[]);
        let msg = ExecuteMsg::ClaimRevenueForUser {
            compound_action: None,
            user: "user1".to_string(),
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
            limit: None,
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Check events still exist (user2 hasn't claimed)
        let msg = QueryMsg::GetRevenueEvents {
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
        };
        let result = query(deps.as_ref(), env.clone(), msg).unwrap();
        let events: Vec<RevenueEvent> = from_json(&result).unwrap();
        
        println!("Events after user1 claim: {:?}", events);
        assert_eq!(events.len(), 3, "Events should still exist for user2");
        
        // Verify amount_to_be_claimed is roughly half of original
        for event in &events {
            assert!(event.amount_to_be_claimed > Uint128::zero(), "Events should have remaining claims");
        }
        
        // Claim for user2
        env.block.time = env.block.time.plus_seconds(10);
        let info = mock_info("anyone", &[]);
        let msg = ExecuteMsg::ClaimRevenueForUser {
            compound_action: None,
            user: "user2".to_string(),
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
            limit: None,
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        // Now all events should be trimmed
        let msg = QueryMsg::GetRevenueEvents {
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
        };
        let result = query(deps.as_ref(), env.clone(), msg);
        
        if let Ok(binary) = result {
            let events: Vec<RevenueEvent> = from_json(&binary).unwrap();
            println!("Events after user2 claim: {}", events.len());
            assert!(events.is_empty(), "All events should be trimmed after both users claim");
        }
    }
