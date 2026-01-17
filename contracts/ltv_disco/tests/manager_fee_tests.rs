#![allow(unused_imports)]
use cosmwasm_std::{
    testing::{mock_dependencies, mock_dependencies_with_balances, mock_env, mock_info},
    coin, coins, Decimal, Uint128, WasmMsg, CosmosMsg, BankMsg, to_json_binary, WasmQuery, QueryRequest, SystemResult, ContractResult,
    from_json, Binary, Addr,
};
use membrane::math::decimal_multiplication;
use membrane::ltv_disco::{
    InstantiateMsg, ExecuteMsg, QueryMsg, BackingDepositInput, Config, ManagerFeeResponse,
};
use membrane::types::{cAsset, Asset, AssetInfo, Basket, PendingRevenue, DepositDenom};
use membrane::oracle::PriceResponse;
use ltv_disco::contract::{instantiate, execute, query};
use ltv_disco::state::{MANAGER_FEE, BACKING_DEPOSITS, REVENUE_EVENTS, CONFIG, MANAGED_DEPOSITS};
use ltv_disco::error::ContractError;
use ltv_disco::execute::make_deposit_key;

const SECONDS_PER_DAY: u64 = 86400;

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
            individual_cost: None,
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

fn setup_instantiate(
    deps: &mut cosmwasm_std::OwnedDeps<
        cosmwasm_std::MemoryStorage,
        cosmwasm_std::testing::MockApi,
        cosmwasm_std::testing::MockQuerier<cosmwasm_std::Empty>,
    >,
) {
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
    
    let env = mock_env();
    let info = mock_info("owner", &[]);
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
        max_management_fee: Some(Decimal::percent(10)), // 10% max
        ltv_delta_minimum: Some(Decimal::percent(1)),
        emissions_voting_contract: None,
        points_system_contract: None,
        revenue_distributor: None,
    };

    instantiate(deps.as_mut(), env, info, msg).unwrap();
}

fn create_queue_and_deposit(
    deps: &mut cosmwasm_std::OwnedDeps<
        cosmwasm_std::MemoryStorage,
        cosmwasm_std::testing::MockApi,
        cosmwasm_std::testing::MockQuerier<cosmwasm_std::Empty>,
    >,
    env: &cosmwasm_std::Env,
    user: &str,
    manager: Option<&str>,
    deposit_amount: u128,
) {
    // Create queue (only if it doesn't exist - ignore error if it already exists)
    let info = mock_info("owner", &[]);
    let _ = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    );

    // Submit deposit
    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    let info = mock_info(user, &coins(deposit_amount, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input,
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: manager.map(|s| s.to_string()),
            affiliate_address: None,
        },
    ).unwrap();
}

fn add_revenue(
    deps: &mut cosmwasm_std::OwnedDeps<
        cosmwasm_std::MemoryStorage,
        cosmwasm_std::testing::MockApi,
        cosmwasm_std::testing::MockQuerier<cosmwasm_std::Empty>,
    >,
    env: &cosmwasm_std::Env,
    amount: u128,
) {
    // First, we need to send CDT to the contract to simulate revenue
    // In a real scenario, this would come from the CDP contract
    // For testing, we'll directly add revenue events
    
    // Add revenue via execute
    let info = mock_info("cdp_contract", &coins(amount, "reward_token"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::AddRevenue { asset: "uusd".to_string() },
    ).unwrap();
}

#[test]
fn test_set_manager_fee_success() {
    let mut deps = mock_dependencies();
    setup_instantiate(&mut deps);
    let env = mock_env();

    // Create deposit with manager
    create_queue_and_deposit(&mut deps, &env, "user", Some("manager"), 10000);

    // Manager sets fee to 5%
    let info = mock_info("manager", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SetManagerFee {
            fee: Decimal::percent(5),
        },
    ).unwrap();

    // Verify fee was set
    let fee = MANAGER_FEE.load(&deps.storage, Addr::unchecked("manager")).unwrap();
    assert_eq!(fee, Decimal::percent(5));
}

#[test]
fn test_set_manager_fee_without_deposits() {
    let mut deps = mock_dependencies();
    setup_instantiate(&mut deps);
    let env = mock_env();

    // Try to set fee without having any deposits
    let info = mock_info("manager", &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SetManagerFee {
            fee: Decimal::percent(5),
        },
    );

    assert!(res.is_err());
    match res.unwrap_err() {
        ContractError::CustomError { val } => {
            assert!(val.contains("Manager must have active deposits"));
        }
        _ => panic!("Expected CustomError"),
    }
}

#[test]
fn test_set_manager_fee_exceeds_max() {
    let mut deps = mock_dependencies();
    setup_instantiate(&mut deps);
    let env = mock_env();

    // Create deposit with manager
    create_queue_and_deposit(&mut deps, &env, "user", Some("manager"), 10000);

    // Try to set fee above max (max is 10%)
    let info = mock_info("manager", &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SetManagerFee {
            fee: Decimal::percent(15),
        },
    );

    assert!(res.is_err());
    match res.unwrap_err() {
        ContractError::CustomError { val } => {
            assert!(val.contains("exceeds max_management_fee"));
        }
        _ => panic!("Expected CustomError"),
    }
}

#[test]
fn test_set_manager_fee_exceeds_one() {
    let mut deps = mock_dependencies();
    setup_instantiate(&mut deps);
    let env = mock_env();

    // Create deposit with manager
    create_queue_and_deposit(&mut deps, &env, "user", Some("manager"), 10000);

    // Try to set fee above 100%
    let info = mock_info("manager", &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SetManagerFee {
            fee: Decimal::percent(150),
        },
    );

    assert!(res.is_err());
    match res.unwrap_err() {
        ContractError::CustomError { val } => {
            assert!(val.contains("less than or equal to 1") || val.contains("exceeds max_management_fee"));
        }
        _ => panic!("Expected CustomError"),
    }
}

#[test]
fn test_set_manager_fee_zero() {
    let mut deps = mock_dependencies();
    setup_instantiate(&mut deps);
    let env = mock_env();

    // Create deposit with manager
    create_queue_and_deposit(&mut deps, &env, "user", Some("manager"), 10000);

    // Set fee to 0%
    let info = mock_info("manager", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SetManagerFee {
            fee: Decimal::zero(),
        },
    ).unwrap();

    // Verify fee was set to zero
    let fee = MANAGER_FEE.load(&deps.storage, Addr::unchecked("manager")).unwrap();
    assert_eq!(fee, Decimal::zero());
}

#[test]
fn test_set_manager_fee_update() {
    let mut deps = mock_dependencies();
    setup_instantiate(&mut deps);
    let env = mock_env();

    // Create deposit with manager
    create_queue_and_deposit(&mut deps, &env, "user", Some("manager"), 10000);

    // Set fee to 3%
    let info = mock_info("manager", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::SetManagerFee {
            fee: Decimal::percent(3),
        },
    ).unwrap();

    // Update fee to 7%
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SetManagerFee {
            fee: Decimal::percent(7),
        },
    ).unwrap();

    // Verify fee was updated
    let fee = MANAGER_FEE.load(&deps.storage, Addr::unchecked("manager")).unwrap();
    assert_eq!(fee, Decimal::percent(7));
}

#[test]
fn test_query_manager_fee() {
    let mut deps = mock_dependencies();
    setup_instantiate(&mut deps);
    let env = mock_env();

    // Create deposit with manager
    create_queue_and_deposit(&mut deps, &env, "user", Some("manager"), 10000);

    // Set fee
    let info = mock_info("manager", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SetManagerFee {
            fee: Decimal::percent(5),
        },
    ).unwrap();

    // Query manager fee
    let query_msg = QueryMsg::GetManagerFee {
        manager: "manager".to_string(),
    };
    let res: ManagerFeeResponse = from_json(
        query(deps.as_ref(), env.clone(), query_msg).unwrap()
    ).unwrap();

    assert_eq!(res.manager, "manager");
    assert_eq!(res.fee, Decimal::percent(5));
}

#[test]
fn test_query_manager_fee_not_set() {
    let mut deps = mock_dependencies();
    setup_instantiate(&mut deps);
    let env = mock_env();

    // Query manager fee for manager who hasn't set a fee
    let query_msg = QueryMsg::GetManagerFee {
        manager: "manager".to_string(),
    };
    let res: ManagerFeeResponse = from_json(
        query(deps.as_ref(), env.clone(), query_msg).unwrap()
    ).unwrap();

    assert_eq!(res.manager, "manager");
    assert_eq!(res.fee, Decimal::zero());
}

#[test]
fn test_manager_fee_on_revenue_claim() {
    let mut deps = mock_dependencies();
    setup_instantiate(&mut deps);
    let mut env = mock_env();

    // Create deposit with manager
    create_queue_and_deposit(&mut deps, &env, "user", Some("manager"), 10000);

    // Set manager fee to 5%
    let info = mock_info("manager", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SetManagerFee {
            fee: Decimal::percent(5),
        },
    ).unwrap();

    // Add revenue (1000 CDT)
    env.block.time = env.block.time.plus_seconds(100);
    add_revenue(&mut deps, &env, 1000);

    // Claim revenue
    let info = mock_info("user", &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ClaimRevenueForUser {
            user: "user".to_string(),
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            limit: None,
            compound_action: None,
        },
    ).unwrap();

    // Check that manager fee was sent to manager (5% of claimed amount)
    // Revenue is distributed based on locked_vault_tokens, so we check that manager gets a fee
    let manager_msg = res.messages.iter().find(|msg| {
        if let CosmosMsg::Bank(BankMsg::Send { to_address, amount }) = &msg.msg {
            to_address == "manager" && amount[0].amount > Uint128::zero()
        } else {
            false
        }
    });
    assert!(manager_msg.is_some(), "Manager should receive 5% fee");
    
    // Verify the fee is approximately 5% by checking attributes
    let claimed_attr = res.attributes.iter().find(|a| a.key == "revenue_claimed");
    let manager_fees_attr = res.attributes.iter().find(|a| a.key == "manager_fees");
    if let (Some(claimed), Some(fees)) = (claimed_attr, manager_fees_attr) {
        let claimed_val: Uint128 = claimed.value.parse().unwrap();
        let fees_val: Uint128 = fees.value.parse().unwrap();
        // Fee should be approximately 5% (allow for rounding)
        let expected_fee = claimed_val.multiply_ratio(5u128, 100u128);
        assert!(fees_val >= expected_fee - Uint128::new(1) && fees_val <= expected_fee + Uint128::new(1),
            "Manager fee should be approximately 5%: got {}, expected ~{}", fees_val, expected_fee);
    }

    // Check that user received the remaining amount
    let user_msg = res.messages.iter().find(|msg| {
        if let CosmosMsg::Bank(BankMsg::Send { to_address, amount }) = &msg.msg {
            to_address == "user" && amount[0].amount > Uint128::zero()
        } else {
            false
        }
    });
    assert!(user_msg.is_some(), "User should receive remaining amount");
}

#[test]
fn test_manager_fee_multiple_deposits() {
    let mut deps = mock_dependencies();
    setup_instantiate(&mut deps);
    let mut env = mock_env();

    // Create two deposits with same manager
    create_queue_and_deposit(&mut deps, &env, "user1", Some("manager"), 10000);
    create_queue_and_deposit(&mut deps, &env, "user2", Some("manager"), 10000);

    // Set manager fee to 5%
    let info = mock_info("manager", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SetManagerFee {
            fee: Decimal::percent(5),
        },
    ).unwrap();

    // Add revenue
    env.block.time = env.block.time.plus_seconds(100);
    add_revenue(&mut deps, &env, 1000);

    // Claim revenue for user1
    let info = mock_info("user1", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ClaimRevenueForUser {
            user: "user1".to_string(),
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            limit: None,
            compound_action: None,
        },
    ).unwrap();

    // Claim revenue for user2
    let info = mock_info("user2", &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ClaimRevenueForUser {
            user: "user2".to_string(),
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            limit: None,
            compound_action: None,
        },
    ).unwrap();

    // Manager should receive fee from both claims
    // Each deposit gets ~500 CDT (1000 / 2), manager gets 5% = 25 from each
    let manager_msgs: Vec<_> = res.messages.iter()
        .filter(|msg| {
            if let CosmosMsg::Bank(BankMsg::Send { to_address, .. }) = &msg.msg {
                to_address == "manager"
            } else {
                false
            }
        })
        .collect();
    
    // Manager should receive fee from user2's claim
    assert!(!manager_msgs.is_empty(), "Manager should receive fee");
}

#[test]
fn test_manager_fee_no_fee_set() {
    let mut deps = mock_dependencies();
    setup_instantiate(&mut deps);
    let mut env = mock_env();

    // Create deposit with manager but don't set fee
    create_queue_and_deposit(&mut deps, &env, "user", Some("manager"), 10000);

    // Add revenue
    env.block.time = env.block.time.plus_seconds(100);
    add_revenue(&mut deps, &env, 1000);

    // Claim revenue
    let info = mock_info("user", &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ClaimRevenueForUser {
            user: "user".to_string(),
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            limit: None,
            compound_action: None,
        },
    ).unwrap();

    // Manager should not receive any fee (no fee set)
    let manager_msg = res.messages.iter().find(|msg| {
        if let CosmosMsg::Bank(BankMsg::Send { to_address, .. }) = &msg.msg {
            to_address == "manager"
        } else {
            false
        }
    });
    assert!(manager_msg.is_none(), "Manager should not receive fee when not set");

    // User should receive full amount
    let user_msg = res.messages.iter().find(|msg| {
        if let CosmosMsg::Bank(BankMsg::Send { to_address, amount }) = &msg.msg {
            to_address == "user" && amount[0].amount > Uint128::zero()
        } else {
            false
        }
    });
    assert!(user_msg.is_some(), "User should receive full amount");
}

#[test]
fn test_manager_fee_on_manager_change() {
    let mut deps = mock_dependencies();
    setup_instantiate(&mut deps);
    let mut env = mock_env();

    // Create deposit with manager
    create_queue_and_deposit(&mut deps, &env, "user", Some("manager1"), 10000);

    // Set manager1 fee to 5%
    let info = mock_info("manager1", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SetManagerFee {
            fee: Decimal::percent(5),
        },
    ).unwrap();

    // Add revenue
    env.block.time = env.block.time.plus_seconds(100);
    add_revenue(&mut deps, &env, 1000);

    // Change manager - should claim revenue and pay manager1 their fee
    let info = mock_info("user", &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::UpdateManager {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::new(1),
            manager: Some("manager2".to_string()),
            epoch_start_time: 0,
        },
    ).unwrap();

    // Check that manager1 received their fee
    let manager1_msg = res.messages.iter().find(|msg| {
        if let CosmosMsg::Bank(BankMsg::Send { to_address, amount }) = &msg.msg {
            to_address == "manager1" && amount[0].amount > Uint128::zero()
        } else {
            false
        }
    });
    assert!(manager1_msg.is_some(), "Manager1 should receive fee when changed");

    // Check that user received remaining amount
    let user_msg = res.messages.iter().find(|msg| {
        if let CosmosMsg::Bank(BankMsg::Send { to_address, amount }) = &msg.msg {
            to_address == "user" && amount[0].amount > Uint128::zero()
        } else {
            false
        }
    });
    assert!(user_msg.is_some(), "User should receive remaining amount");

    // Verify manager was changed
    let deposit_key = make_deposit_key(
        "uusd",
        "0.5",
        "0.3",
        "user",
        &Uint128::new(1),
        0,
    );
    let deposit = BACKING_DEPOSITS.load(&deps.storage, deposit_key).unwrap();
    assert_eq!(deposit.manager, Some(Addr::unchecked("manager2")));
}

#[test]
fn test_manager_fee_on_manager_removal() {
    let mut deps = mock_dependencies();
    setup_instantiate(&mut deps);
    let mut env = mock_env();

    // Create deposit with manager
    create_queue_and_deposit(&mut deps, &env, "user", Some("manager"), 10000);

    // Set manager fee to 5%
    let info = mock_info("manager", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SetManagerFee {
            fee: Decimal::percent(5),
        },
    ).unwrap();

    // Add revenue
    env.block.time = env.block.time.plus_seconds(100);
    add_revenue(&mut deps, &env, 1000);

    // Remove manager (set to None)
    let info = mock_info("user", &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::UpdateManager {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::new(1),
            manager: None,
            epoch_start_time: 0,
        },
    ).unwrap();

    // Check that manager received their fee before removal
    let manager_msg = res.messages.iter().find(|msg| {
        if let CosmosMsg::Bank(BankMsg::Send { to_address, .. }) = &msg.msg {
            to_address == "manager"
        } else {
            false
        }
    });
    assert!(manager_msg.is_some(), "Manager should receive fee when removed");

    // Verify manager was removed
    let deposit_key = make_deposit_key(
        "uusd",
        "0.5",
        "0.3",
        "user",
        &Uint128::new(1),
        0,
    );
    let deposit = BACKING_DEPOSITS.load(&deps.storage, deposit_key).unwrap();
    assert_eq!(deposit.manager, None);
}

#[test]
fn test_clean_manager_fee_success() {
    let mut deps = mock_dependencies();
    setup_instantiate(&mut deps);
    let env = mock_env();

    // Create deposit with manager
    create_queue_and_deposit(&mut deps, &env, "user", Some("manager"), 10000);

    // Set manager fee
    let info = mock_info("manager", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SetManagerFee {
            fee: Decimal::percent(5),
        },
    ).unwrap();

    // Remove manager from deposit
    let info = mock_info("user", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::UpdateManager {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::new(1),
            manager: None,
            epoch_start_time: 0,
        },
    ).unwrap();

    // Clean manager fee (permissionless)
    let info = mock_info("anyone", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CleanManagerFee {
            manager: "manager".to_string(),
        },
    ).unwrap();

    // Verify fee was removed
    let fee = MANAGER_FEE.may_load(&deps.storage, Addr::unchecked("manager")).unwrap();
    assert_eq!(fee, None);
}

#[test]
fn test_clean_manager_fee_with_active_deposits() {
    let mut deps = mock_dependencies();
    setup_instantiate(&mut deps);
    let env = mock_env();

    // Create deposit with manager
    create_queue_and_deposit(&mut deps, &env, "user", Some("manager"), 10000);

    // Set manager fee
    let info = mock_info("manager", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SetManagerFee {
            fee: Decimal::percent(5),
        },
    ).unwrap();

    // Try to clean manager fee while manager still has deposits - should fail
    let info = mock_info("anyone", &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CleanManagerFee {
            manager: "manager".to_string(),
        },
    );

    assert!(res.is_err());
    match res.unwrap_err() {
        ContractError::CustomError { val } => {
            assert!(val.contains("Manager still has active deposits"));
        }
        _ => panic!("Expected CustomError"),
    }
}

#[test]
fn test_manager_fee_with_compound() {
    let mut deps = mock_dependencies();
    setup_instantiate(&mut deps);
    let mut env = mock_env();

    // Create deposit with manager
    create_queue_and_deposit(&mut deps, &env, "user", Some("manager"), 10000);

    // Set manager fee to 5%
    let info = mock_info("manager", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SetManagerFee {
            fee: Decimal::percent(5),
        },
    ).unwrap();

    // Add revenue
    env.block.time = env.block.time.plus_seconds(100);
    add_revenue(&mut deps, &env, 1000);

    // Claim revenue with compound
    let info = mock_info("user", &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ClaimRevenueForUser {
            user: "user".to_string(),
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            limit: None,
            compound_action: Some(membrane::ltv_disco::CompoundAction {
                compound_now: true,
                set_ongoing: false,
                recipient_address: None,
            }),
        },
    ).unwrap();

    // Manager should receive fee (5% of claimed amount)
    let manager_msg = res.messages.iter().find(|msg| {
        if let CosmosMsg::Bank(BankMsg::Send { to_address, amount }) = &msg.msg {
            to_address == "manager" && amount[0].amount > Uint128::zero()
        } else {
            false
        }
    });
    assert!(manager_msg.is_some(), "Manager should receive fee even with compound");

    // Compound should use remaining amount (950)
    // This would be in a submessage for the swap
    let has_swap = res.messages.iter().any(|msg| {
        matches!(&msg.msg, CosmosMsg::Wasm(WasmMsg::Execute { .. }))
    });
    assert!(has_swap, "Should have compound swap message");
}

#[test]
fn test_manager_fee_different_managers() {
    let mut deps = mock_dependencies();
    setup_instantiate(&mut deps);
    let mut env = mock_env();

    // Create deposits with different managers
    create_queue_and_deposit(&mut deps, &env, "user1", Some("manager1"), 10000);
    create_queue_and_deposit(&mut deps, &env, "user2", Some("manager2"), 10000);

    // Set different fees
    let info = mock_info("manager1", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SetManagerFee {
            fee: Decimal::percent(3),
        },
    ).unwrap();

    let info = mock_info("manager2", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SetManagerFee {
            fee: Decimal::percent(7),
        },
    ).unwrap();

    // Add revenue
    env.block.time = env.block.time.plus_seconds(100);
    add_revenue(&mut deps, &env, 1000);

    // Claim for user1
    let info = mock_info("user1", &[]);
    let res1 = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ClaimRevenueForUser {
            user: "user1".to_string(),
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            limit: None,
            compound_action: None,
        },
    ).unwrap();

    // Manager1 should receive 3% fee
    let manager1_msg = res1.messages.iter().find(|msg| {
        if let CosmosMsg::Bank(BankMsg::Send { to_address, .. }) = &msg.msg {
            to_address == "manager1"
        } else {
            false
        }
    });
    assert!(manager1_msg.is_some(), "Manager1 should receive 3% fee");

    // Claim for user2
    let info = mock_info("user2", &[]);
    let res2 = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ClaimRevenueForUser {
            user: "user2".to_string(),
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            limit: None,
            compound_action: None,
        },
    ).unwrap();

    // Manager2 should receive 7% fee
    let manager2_msg = res2.messages.iter().find(|msg| {
        if let CosmosMsg::Bank(BankMsg::Send { to_address, .. }) = &msg.msg {
            to_address == "manager2"
        } else {
            false
        }
    });
    assert!(manager2_msg.is_some(), "Manager2 should receive 7% fee");
}

#[test]
fn test_manager_fee_zero_fee() {
    let mut deps = mock_dependencies();
    setup_instantiate(&mut deps);
    let mut env = mock_env();

    // Create deposit with manager
    create_queue_and_deposit(&mut deps, &env, "user", Some("manager"), 10000);

    // Set manager fee to 0%
    let info = mock_info("manager", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SetManagerFee {
            fee: Decimal::zero(),
        },
    ).unwrap();

    // Add revenue
    env.block.time = env.block.time.plus_seconds(100);
    add_revenue(&mut deps, &env, 1000);

    // Claim revenue
    let info = mock_info("user", &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ClaimRevenueForUser {
            user: "user".to_string(),
            asset: "uusd".to_string(),
            max_ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            limit: None,
            compound_action: None,
        },
    ).unwrap();

    // Manager should not receive any fee (0%)
    let manager_msg = res.messages.iter().find(|msg| {
        if let CosmosMsg::Bank(BankMsg::Send { to_address, .. }) = &msg.msg {
            to_address == "manager"
        } else {
            false
        }
    });
    assert!(manager_msg.is_none(), "Manager should not receive fee when set to 0%");
}
