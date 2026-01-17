use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{coins, from_json, Decimal, Uint128, to_json_binary, SystemResult, ContractResult, Binary, Addr, Storage};
use membrane::ltv_disco::*;
use membrane::types::{cAsset, Asset, AssetInfo, Basket, DepositDenom, PendingRevenue, Locked};
use membrane::oracle::PriceResponse;
use ltv_disco::contract::{instantiate, execute, query};
use ltv_disco::execute::make_deposit_key;
use ltv_disco::state::{BACKING_DEPOSITS, MANAGED_DEPOSITS, USER_DEPOSITS};

fn deposit_key(asset: &str, ltv: &str, max_borrow_ltv: &str, user: &str, deposit_id: u128) -> String {
    make_deposit_key(
        asset,
        ltv,
        max_borrow_ltv,
        user,
        &Uint128::new(deposit_id),
        0,
    )
}

fn find_user_deposit_key(
    storage: &dyn Storage,
    user: &str,
    asset: &str,
    ltv: &str,
    max_borrow_ltv: &str,
) -> String {
    let keys = USER_DEPOSITS
        .may_load(storage, (Addr::unchecked(user), asset.to_string()))
        .unwrap_or_else(|_| None)
        .unwrap_or_default();
    let pattern = format!(":{}:{}:{}:", ltv, max_borrow_ltv, user);
    keys.into_iter()
        .find(|k| k.contains(&pattern))
        .unwrap_or_else(|| panic!("Deposit key not found for {}", pattern))
}

fn deposit_id_from_key(key: &str) -> u128 {
    key.split(':')
        .nth(4)
        .expect("deposit_id missing")
        .parse()
        .expect("deposit_id parse failed")
}

fn epoch_start_from_key(key: &str) -> u64 {
    key.split(':')
        .nth(5)
        .expect("epoch_start_time missing")
        .parse()
        .expect("epoch_start_time parse failed")
}

fn setup_mock_basket() -> Basket {
    Basket {
        basket_id: Uint128::new(1),
        current_position_id: Uint128::new(1),
        collateral_types: vec![
            cAsset {
                asset: Asset {
                    info: AssetInfo::NativeToken { denom: "uusd".to_string() },
                    amount: Uint128::zero(),
                },
                max_LTV: Decimal::percent(50),
                max_borrow_LTV: Decimal::percent(30),
                rate_index: Decimal::zero(),
                pool_info: None,
                individual_cost: None,
            },
            cAsset {
                asset: Asset {
                    info: AssetInfo::NativeToken { denom: "uatom".to_string() },
                    amount: Uint128::zero(),
                },
                max_LTV: Decimal::percent(50),
                max_borrow_LTV: Decimal::percent(30),
                rate_index: Decimal::zero(),
                pool_info: None,
                individual_cost: None,
            }
        ],
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
    let basket = setup_mock_basket();
    deps.querier.update_wasm(move |query| {
        match query {
            cosmwasm_std::WasmQuery::Smart { contract_addr: _, msg } => {
                let parsed: Result<membrane::cdp::QueryMsg, _> = from_json(msg);
                if let Ok(membrane::cdp::QueryMsg::GetBasket {}) = parsed {
                    SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap()))
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
        lock_duration_ceiling: Some(30),
        affiliate_fee: Some(Decimal::percent(1)),
        max_management_fee: None,
        ltv_delta_minimum: Some(Decimal::percent(1)),
        emissions_voting_contract: None,
        points_system_contract: None,
        revenue_distributor: None,
    };

    let info = mock_info("owner", &[]);
    instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();

    (deps, env)
}

#[test]
fn test_submit_deposit_with_manager() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    // Submit deposit with manager
    let manager = "manager_contract".to_string();
    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    let info = mock_info("user", &coins(10000, "uusd"));
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: Some(manager.clone()),
            affiliate_address: None,
        },
    ).unwrap();

    // Response may include system messages; just ensure no error

    // Verify deposit was created with manager
    let deposit_key = deposit_key("uusd", "0.5", "0.3", "user", 1);
    let deposit = BACKING_DEPOSITS.load(&deps.storage, deposit_key.clone()).unwrap();
    assert_eq!(deposit.user, Addr::unchecked("user"));
    assert_eq!(deposit.manager, Some(Addr::unchecked(&manager)));
    // First deposit: vault_tokens = deposit_amount * 1_000_000 (DEFAULT_VAULT_TOKENS_PER_STAKED_BASE_TOKEN)
    assert_eq!(deposit.vault_tokens, Uint128::new(10000).checked_mul(Uint128::new(1_000_000)).unwrap());

    // Verify manager has deposit key in MANAGED_DEPOSITS
    let manager_addr = Addr::unchecked(&manager);
    let managed_keys = MANAGED_DEPOSITS.may_load(&deps.storage, manager_addr.clone()).unwrap();
    assert!(managed_keys.is_some());
    let keys = managed_keys.unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0], deposit_key);
}

#[test]
fn test_submit_deposit_without_manager() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    // Submit deposit without manager
    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    let info = mock_info("user", &coins(10000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
        },
    ).unwrap();

    // Verify deposit was created without manager
    let deposit_key = deposit_key("uusd", "0.5", "0.3", "user", 1);
    let deposit = BACKING_DEPOSITS.load(&deps.storage, deposit_key.clone()).unwrap();
    assert_eq!(deposit.user, Addr::unchecked("user"));
    assert_eq!(deposit.manager, None);

    // Verify manager does NOT have deposit key
    let manager_addr = Addr::unchecked("manager_contract");
    let managed_keys = MANAGED_DEPOSITS.may_load(&deps.storage, manager_addr).unwrap();
    assert!(managed_keys.is_none() || managed_keys.unwrap().is_empty());
}

#[test]
fn test_query_managed_deposit_keys() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    let manager = "manager_contract".to_string();
    
    // Create multiple deposits with same manager
    for i in 1..=3 {
        let deposit_input = BackingDepositInput {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            epoch_start_time: Some(0),
        };

        let info = mock_info(&format!("user{}", i), &coins(10000, "uusd"));
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::SubmitDeposit {
                deposit_input: deposit_input.clone(),
                deposit_owner: None,
                locked: None,
                deposit_id: None,
                manager: Some(manager.clone()),
                affiliate_address: None,
            },
        ).unwrap();
    }

    // Query managed deposit keys
    let response: ManagedDepositKeysResponse = from_json(
        query(
            deps.as_ref(),
            env.clone(),
            QueryMsg::GetManagedDepositKeys {
                manager: manager.clone(),
                limit: None,
                start_after: None,
            },
        ).unwrap()
    ).unwrap();

    assert_eq!(response.total, 3);
    assert_eq!(response.keys.len(), 3);
    // Deposit IDs are per group, not per user - each new deposit in the same group gets next ID
    assert_eq!(response.keys[0], deposit_key("uusd", "0.5", "0.3", "user1", 1));
    assert_eq!(response.keys[1], deposit_key("uusd", "0.5", "0.3", "user2", 2));
    assert_eq!(response.keys[2], deposit_key("uusd", "0.5", "0.3", "user3", 3));
    assert!(response.next_start_after.is_none());
}

#[test]
fn test_query_managed_deposit_keys_pagination() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    let manager = "manager_contract".to_string();
    
    // Create 5 deposits with same manager
    for i in 1..=5 {
        let deposit_input = BackingDepositInput {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            epoch_start_time: Some(0),
        };

        let info = mock_info(&format!("user{}", i), &coins(10000, "uusd"));
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::SubmitDeposit {
                deposit_input: deposit_input.clone(),
                deposit_owner: None,
                locked: None,
                deposit_id: None,
                manager: Some(manager.clone()),
                affiliate_address: None,
            },
        ).unwrap();
    }

    // Query first page (limit 2)
    let response: ManagedDepositKeysResponse = from_json(
        query(
            deps.as_ref(),
            env.clone(),
            QueryMsg::GetManagedDepositKeys {
                manager: manager.clone(),
                limit: Some(2),
                start_after: None,
            },
        ).unwrap()
    ).unwrap();

    assert_eq!(response.total, 5);
    assert_eq!(response.keys.len(), 2);
    // Deposit IDs are per group - each new deposit gets next ID
    assert_eq!(response.keys[0], deposit_key("uusd", "0.5", "0.3", "user1", 1));
    assert_eq!(response.keys[1], deposit_key("uusd", "0.5", "0.3", "user2", 2));
    assert_eq!(response.next_start_after, Some(deposit_key("uusd", "0.5", "0.3", "user2", 2).to_string()));

    // Query next page
    let response2: ManagedDepositKeysResponse = from_json(
        query(
            deps.as_ref(),
            env.clone(),
            QueryMsg::GetManagedDepositKeys {
                manager: manager.clone(),
                limit: Some(2),
                start_after: response.next_start_after,
            },
        ).unwrap()
    ).unwrap();

    assert_eq!(response2.total, 5);
    assert_eq!(response2.keys.len(), 2);
    assert_eq!(response2.keys[0], deposit_key("uusd", "0.5", "0.3", "user3", 3));
    assert_eq!(response2.keys[1], deposit_key("uusd", "0.5", "0.3", "user4", 4));
}

#[test]
fn test_manager_can_move_deposit() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    let manager = "manager_contract".to_string();
    
    // User submits deposit with manager
    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    let info = mock_info("user", &coins(10000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: Some(manager.clone()),
            affiliate_address: None,
        },
    ).unwrap();

    // Manager moves deposit to new LTV slot
    let destination = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        epoch_start_time: Some(0),
    };

    let info = mock_info(&manager, &[]);
    let _res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::MoveDeposit {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::new(1),
            destination,
            amount: None,
            user: Some("user".to_string()), // Manager must specify user
            epoch_start_time: 0,
        },
    ).unwrap();

    // Verify old deposit key is removed from MANAGED_DEPOSITS
    let manager_addr = Addr::unchecked(&manager);
    let managed_keys = MANAGED_DEPOSITS.may_load(&deps.storage, manager_addr.clone()).unwrap();
    
    // When moving to a new slot, the old key should be removed and a new one created
    if let Some(ref keys) = managed_keys {
        assert!(!keys.contains(&deposit_key("uusd", "0.5", "0.3", "user", 1)));
        // Find new deposit key for destination group (epoch suffix may differ)
        let new_key = keys.iter()
            .find(|k| k.contains(":0.6:0.4:"))
            .expect("Expected destination key not found");
        // Verify new deposit exists (with new deposit_id)
        let new_deposit = BACKING_DEPOSITS.load(&deps.storage, new_key.clone()).unwrap();
        assert_eq!(new_deposit.user, Addr::unchecked("user"));
        assert_eq!(new_deposit.manager, Some(Addr::unchecked(&manager)));
        // First deposit: vault_tokens = deposit_amount * 1_000_000
        assert_eq!(new_deposit.vault_tokens, Uint128::new(10000).checked_mul(Uint128::new(1_000_000)).unwrap());
    } else {
        panic!("Manager should have managed keys after move");
    }

    // Verify old deposit no longer exists
    assert!(BACKING_DEPOSITS.may_load(&deps.storage, deposit_key("uusd", "0.5", "0.3", "user", 1)).unwrap().is_none());
}

#[test]
fn test_manager_cannot_withdraw_deposit() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    let manager = "manager_contract".to_string();
    
    // User submits deposit with manager
    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    let info = mock_info("user", &coins(10000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: Some(manager.clone()),
            affiliate_address: None,
        },
    ).unwrap();

    // Manager tries to withdraw (should fail)
    let info = mock_info(&manager, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::WithdrawDeposit {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::new(1),
            amount: None,
            epoch_start_time: 0,
        },
    );

    assert!(res.is_err());
    // Manager cannot withdraw - the contract will return NotFound or Unauthorized
    // depending on how it checks authorization
    let err = res.unwrap_err();
    match err {
        ltv_disco::error::ContractError::Unauthorized {} => {}
        ltv_disco::error::ContractError::Std(cosmwasm_std::StdError::NotFound { .. }) => {}
        e => panic!("Expected Unauthorized or NotFound error, got: {:?}", e),
    }
}

#[test]
fn test_owner_can_still_move_their_deposit() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    let manager = "manager_contract".to_string();
    
    // User submits deposit with manager
    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    let info = mock_info("user", &coins(10000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: Some(manager.clone()),
            affiliate_address: None,
        },
    ).unwrap();

    // Owner (user) can still move their own deposit
    let destination = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        epoch_start_time: Some(0),
    };

    let info = mock_info("user", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::MoveDeposit {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::new(1),
            destination,
            amount: None,
            user: None, // User doesn't need to specify, defaults to sender
            epoch_start_time: 0,
        },
    ).unwrap();

    // Verify move succeeded (creates new deposit with new deposit_id)
    let new_key = find_user_deposit_key(&deps.storage, "user", "uusd", "0.6", "0.4");
    let new_deposit = BACKING_DEPOSITS.load(&deps.storage, new_key).unwrap();
    assert_eq!(new_deposit.user, Addr::unchecked("user"));
    assert_eq!(new_deposit.manager, Some(Addr::unchecked(&manager))); // Manager preserved
    // Vault tokens = deposit_amount * 1_000_000 for first deposit in slot
    assert_eq!(new_deposit.vault_tokens, Uint128::new(10000).checked_mul(Uint128::new(1_000_000)).unwrap());
}

#[test]
fn test_manager_can_move_partial_deposit() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    let manager = "manager_contract".to_string();
    
    // User submits deposit with manager
    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    let info = mock_info("user", &coins(10000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: Some(manager.clone()),
            affiliate_address: None,
        },
    ).unwrap();

    // Manager moves partial deposit (5000 vault tokens)
    let destination = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        epoch_start_time: Some(0),
    };

    let info = mock_info(&manager, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::MoveDeposit {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::new(1),
            destination,
            // amount is in vault tokens, not base tokens
            // Move half: 10_000_000_000 / 2 = 5_000_000_000 vault tokens
            amount: Some(Uint128::new(10_000_000_000) / Uint128::new(2)),
            user: Some("user".to_string()), // Manager must specify user
            epoch_start_time: 0,
        },
    ).unwrap();

    // Verify original deposit still exists with reduced amount
    // Original had 10_000_000_000 vault tokens
    // Moving 5_000_000_000 vault tokens
    // Remaining: 10_000_000_000 - 5_000_000_000 = 5_000_000_000
    let original_deposit = BACKING_DEPOSITS.load(&deps.storage, deposit_key("uusd", "0.5", "0.3", "user", 1).to_string()).unwrap();
    let expected_remaining = Uint128::new(10_000_000_000) / Uint128::new(2);
    assert_eq!(original_deposit.vault_tokens, expected_remaining);
    assert_eq!(original_deposit.manager, Some(Addr::unchecked(&manager)));

    // Verify new deposit exists with moved amount
    // New deposit in empty slot: 5_000_000_000 vault tokens = 5_000_000_000 / 1_000_000 = 5000 base tokens
    let new_key = find_user_deposit_key(&deps.storage, "user", "uusd", "0.6", "0.4");
    let new_deposit = BACKING_DEPOSITS.load(&deps.storage, new_key).unwrap();
    let expected_moved = Uint128::new(10_000_000_000) / Uint128::new(2);
    assert_eq!(new_deposit.vault_tokens, expected_moved);
    assert_eq!(new_deposit.manager, Some(Addr::unchecked(&manager)));

    // Verify both keys are in MANAGED_DEPOSITS
    // Original deposit (reduced) and new deposit (with new deposit_id)
    let manager_addr = Addr::unchecked(&manager);
    let managed_keys = MANAGED_DEPOSITS.may_load(&deps.storage, manager_addr).unwrap().unwrap();
    assert_eq!(managed_keys.len(), 2);
    assert!(managed_keys.contains(&deposit_key("uusd", "0.5", "0.3", "user", 1).to_string()));
    assert!(managed_keys.iter().any(|k| k.contains(":0.6:0.4:")));
}

#[test]
fn test_unauthorized_cannot_move_deposit() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    let manager = "manager_contract".to_string();
    
    // User submits deposit with manager
    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    let info = mock_info("user", &coins(10000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: Some(manager.clone()),
            affiliate_address: None,
        },
    ).unwrap();

    // Unauthorized user tries to move deposit (should fail)
    let destination = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        epoch_start_time: Some(0),
    };

    let info = mock_info("unauthorized", &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::MoveDeposit {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::new(1),
            destination,
            amount: None,
            user: None, // Unauthorized user tries without specifying user
            epoch_start_time: 0,
        },
    );

    assert!(res.is_err());
    match res.unwrap_err() {
        // When user is None, it defaults to info.sender, so unauthorized user won't find the deposit
        ltv_disco::error::ContractError::Std(cosmwasm_std::StdError::NotFound { .. }) |
        ltv_disco::error::ContractError::Unauthorized {} | 
        ltv_disco::error::ContractError::CustomError { .. } => {}
        e => panic!("Expected NotFound, Unauthorized, or CustomError, got: {:?}", e),
    }
}

#[test]
fn test_multiple_managers_different_deposits() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    let manager1 = "manager1".to_string();
    let manager2 = "manager2".to_string();
    
    // User1 submits deposit with manager1
    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    let info = mock_info("user1", &coins(10000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: Some(manager1.clone()),
            affiliate_address: None,
        },
    ).unwrap();

    // User2 submits deposit with manager2
    let info = mock_info("user2", &coins(10000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: Some(manager2.clone()),
            affiliate_address: None,
        },
    ).unwrap();

    // Verify each manager only has their own deposit
    let manager1_addr = Addr::unchecked(&manager1);
    let manager1_keys = MANAGED_DEPOSITS.may_load(&deps.storage, manager1_addr).unwrap().unwrap();
    assert_eq!(manager1_keys.len(), 1);
    assert_eq!(manager1_keys[0], deposit_key("uusd", "0.5", "0.3", "user1", 1));

    let manager2_addr = Addr::unchecked(&manager2);
    let manager2_keys = MANAGED_DEPOSITS.may_load(&deps.storage, manager2_addr).unwrap().unwrap();
    assert_eq!(manager2_keys.len(), 1);
    // User2 gets deposit_id 2 since user1 already has deposit_id 1 in the same group
    assert_eq!(manager2_keys[0], deposit_key("uusd", "0.5", "0.3", "user2", 2));

    // Verify manager1 cannot move manager2's deposit
    let destination = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        epoch_start_time: Some(0),
    };

    let info = mock_info(&manager1, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::MoveDeposit {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::new(2), // user2's deposit (which manager1 doesn't manage)
            destination,
            amount: None,
            user: Some("user2".to_string()), // Manager1 tries to move user2's deposit
            epoch_start_time: 0,
        },
    );

    // Should fail because manager1 is trying to move user2's deposit (which manager2 manages)
    assert!(res.is_err());
}

#[test]
fn test_query_managed_deposit_keys_empty() {
    let (mut deps, env) = instantiate_contract();
    
    // Query managed deposit keys for manager with no deposits
    let response: ManagedDepositKeysResponse = from_json(
        query(
            deps.as_ref(),
            env.clone(),
            QueryMsg::GetManagedDepositKeys {
                manager: "manager_contract".to_string(),
                limit: None,
                start_after: None,
            },
        ).unwrap()
    ).unwrap();

    assert_eq!(response.total, 0);
    assert_eq!(response.keys.len(), 0);
    assert!(response.next_start_after.is_none());
}

#[test]
fn test_manager_cannot_move_non_existent_deposit() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    let manager = "manager_contract".to_string();
    
    // Manager tries to move non-existent deposit
    let destination = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        epoch_start_time: Some(0),
    };

    let info = mock_info(&manager, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::MoveDeposit {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::new(999),
            destination,
            amount: None,
            user: Some("user".to_string()), // Manager tries to move non-existent deposit
            epoch_start_time: 0,
        },
    );

    assert!(res.is_err());
}


#[test]
fn test_manager_cannot_lock_deposit() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    let manager = "manager_contract".to_string();
    
    // User submits deposit with manager
    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    let info = mock_info("user", &coins(10000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: Some(manager.clone()),
            affiliate_address: None,
        },
    ).unwrap();

    // Manager tries to lock deposit (should fail)
    let locked = Locked {
        locked_until: env.block.time.seconds() + 86400 * 7, // 7 days
        perpetual_lock: None,
        intended_lock_days: None,
    };

    let info = mock_info(&manager, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::Lock {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::new(1),
            locked,
            amount: None,
            epoch_start_time: 0,
        },
    );

    // Manager cannot lock - only the user can
    // The function constructs deposit key with info.sender, so manager won't find the deposit
    assert!(res.is_err());
    match res.unwrap_err() {
        ltv_disco::error::ContractError::Std(cosmwasm_std::StdError::NotFound { .. }) => {}
        e => panic!("Expected NotFound error (deposit key uses sender as user), got: {:?}", e),
    }
}

#[test]
fn test_manager_cannot_update_manager() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    let manager = "manager_contract".to_string();
    let new_manager = "new_manager".to_string();
    
    // User submits deposit with manager
    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    let info = mock_info("user", &coins(10000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: Some(manager.clone()),
            affiliate_address: None,
        },
    ).unwrap();

    // Manager tries to update manager (should fail)
    let info = mock_info(&manager, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::UpdateManager {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::new(1),
            manager: Some(new_manager.clone()),
            epoch_start_time: 0,
        },
    );

    // Manager cannot update manager - only the user can
    // The function constructs deposit key with info.sender, so manager won't find the deposit
    assert!(res.is_err());
    match res.unwrap_err() {
        ltv_disco::error::ContractError::Std(cosmwasm_std::StdError::NotFound { .. }) => {}
        e => panic!("Expected NotFound error (deposit key uses sender as user), got: {:?}", e),
    }
}

#[test]
fn test_user_can_move_deposit_with_manager() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    let manager = "manager_contract".to_string();
    
    // User submits deposit with manager
    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    let info = mock_info("user", &coins(10000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: Some(manager.clone()),
            affiliate_address: None,
        },
    ).unwrap();

    // User moves their own deposit (without specifying user parameter - defaults to sender)
    let destination = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        epoch_start_time: Some(0),
    };

    let info = mock_info("user", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::MoveDeposit {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::new(1),
            destination,
            amount: None,
            user: None, // Defaults to sender (user)
            epoch_start_time: 0,
        },
    ).unwrap();

    // Verify move succeeded
    let new_key = find_user_deposit_key(&deps.storage, "user", "uusd", "0.6", "0.4");
    let new_deposit = BACKING_DEPOSITS.load(&deps.storage, new_key).unwrap();
    assert_eq!(new_deposit.user, Addr::unchecked("user"));
    assert_eq!(new_deposit.manager, Some(Addr::unchecked(&manager))); // Manager preserved
}

#[test]
fn test_manager_can_move_deposit_with_user_parameter() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    let manager = "manager_contract".to_string();
    
    // User submits deposit with manager
    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    let info = mock_info("user", &coins(10000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: Some(manager.clone()),
            affiliate_address: None,
        },
    ).unwrap();

    // Manager moves deposit by specifying the user parameter
    let destination = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        epoch_start_time: Some(0),
    };

    let info = mock_info(&manager, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::MoveDeposit {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::new(1),
            destination,
            amount: None,
            user: Some("user".to_string()), // Manager specifies user
            epoch_start_time: 0,
        },
    ).unwrap();

    // Verify move succeeded
    let new_key = find_user_deposit_key(&deps.storage, "user", "uusd", "0.6", "0.4");
    let new_deposit = BACKING_DEPOSITS.load(&deps.storage, new_key).unwrap();
    assert_eq!(new_deposit.user, Addr::unchecked("user"));
    assert_eq!(new_deposit.manager, Some(Addr::unchecked(&manager))); // Manager preserved
    
    // Verify manager still has the new deposit key in MANAGED_DEPOSITS
    let manager_addr = Addr::unchecked(&manager);
    let managed_keys = MANAGED_DEPOSITS.may_load(&deps.storage, manager_addr).unwrap().unwrap();
    assert!(managed_keys.iter().any(|k| k.contains(":0.6:0.4:")));
}

#[test]
fn test_manager_cannot_move_deposit_with_wrong_user_parameter() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    let manager = "manager_contract".to_string();
    
    // User submits deposit with manager
    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    let info = mock_info("user", &coins(10000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: Some(manager.clone()),
            affiliate_address: None,
        },
    ).unwrap();

    // Manager tries to move deposit with wrong user parameter (should fail)
    let destination = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        epoch_start_time: Some(0),
    };

    let info = mock_info(&manager, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::MoveDeposit {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::new(1),
            destination,
            amount: None,
            user: Some("wrong_user".to_string()), // Wrong user - manager doesn't manage this user's deposit
            epoch_start_time: 0,
        },
    );

    // Should fail because manager doesn't manage wrong_user's deposit
    assert!(res.is_err());
    match res.unwrap_err() {
        ltv_disco::error::ContractError::CustomError { val } => {
            assert!(val.contains("Deposit not found in manager's managed deposits"));
        }
        ltv_disco::error::ContractError::Std(_) => {}
        e => panic!("Expected CustomError about deposit not found, got: {:?}", e),
    }
}

#[test]
fn test_both_user_and_manager_can_move_same_deposit() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    let manager = "manager_contract".to_string();
    
    // User submits deposit with manager
    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    let info = mock_info("user", &coins(20000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: Some(manager.clone()),
            affiliate_address: None,
        },
    ).unwrap();

    // First, user moves deposit to new slot
    let destination1 = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        epoch_start_time: Some(0),
    };

    let info = mock_info("user", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::MoveDeposit {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::new(1),
            destination: destination1.clone(),
            amount: None,
            user: None,
            epoch_start_time: 0,
        },
    ).unwrap();

    // Verify user's move succeeded
    let user_move_key = find_user_deposit_key(&deps.storage, "user", "uusd", "0.6", "0.4");
    let user_move_deposit_id = deposit_id_from_key(&user_move_key);
    let user_move_epoch_start = epoch_start_from_key(&user_move_key);
    let deposit_after_user_move = BACKING_DEPOSITS.load(&deps.storage, user_move_key).unwrap();
    assert_eq!(deposit_after_user_move.user, Addr::unchecked("user"));
    assert_eq!(deposit_after_user_move.manager, Some(Addr::unchecked(&manager)));

    // Then, manager moves the same deposit (now in new slot) to another slot
    let destination2 = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(70),
        max_borrow_ltv: Decimal::percent(50),
        epoch_start_time: Some(0),
    };

    let info = mock_info(&manager, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::MoveDeposit {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
            deposit_id: Uint128::new(user_move_deposit_id),
            destination: destination2,
            amount: None,
            user: Some("user".to_string()), // Manager specifies user
            epoch_start_time: user_move_epoch_start,
        },
    ).unwrap();

    // Verify manager's move also succeeded
    let manager_move_key = find_user_deposit_key(&deps.storage, "user", "uusd", "0.7", "0.5");
    let deposit_after_manager_move = BACKING_DEPOSITS.load(&deps.storage, manager_move_key).unwrap();
    assert_eq!(deposit_after_manager_move.user, Addr::unchecked("user"));
    assert_eq!(deposit_after_manager_move.manager, Some(Addr::unchecked(&manager)));
    
    // Verify old deposits are gone
    assert!(BACKING_DEPOSITS.may_load(&deps.storage, deposit_key("uusd", "0.5", "0.3", "user", 1).to_string()).unwrap().is_none());
    let user_keys = USER_DEPOSITS
        .may_load(&deps.storage, (Addr::unchecked("user"), "uusd".to_string()))
        .unwrap_or_else(|_| None)
        .unwrap_or_default();
    assert!(user_keys.iter().all(|k| !k.contains(":0.6:0.4:")));
}
