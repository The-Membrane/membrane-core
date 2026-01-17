use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{coins, from_json, Decimal, Uint128};
use membrane::ltv_disco::*;
use membrane::types::{cAsset, Asset, AssetInfo, Basket, DepositDenom, Locked};

use ltv_disco::contract::{instantiate, execute, query};

const SECONDS_PER_DAY: u64 = 86400;

fn setup_contract() -> (cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>, cosmwasm_std::Env) {
    let mut deps = mock_dependencies();
    let env = mock_env();
    
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
    instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();
    
    (deps, env)
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
            },
        ],
        collateral_supply_caps: vec![],
        lastest_collateral_rates: vec![],
        multi_asset_supply_caps: vec![],
        credit_asset: Asset {
            info: AssetInfo::NativeToken { denom: "debt".to_string() },
            amount: Uint128::zero(),
        },
        credit_price: membrane::oracle::PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6,
        },
        base_interest_rate: Decimal::zero(),
        pending_revenue: membrane::types::PendingRevenue {
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
    }
}

fn create_queue_and_deposit(
    deps: &mut cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>,
    env: &cosmwasm_std::Env,
    asset: &str,
    user: &str,
    amount: u128,
    ltv: Decimal,
    max_borrow_ltv: Decimal,
    locked: Option<Locked>,
) -> Uint128 {
    // Setup mock basket query
    let basket = setup_mock_basket();
    deps.querier.update_wasm(move |query| -> cosmwasm_std::SystemResult<cosmwasm_std::ContractResult<cosmwasm_std::Binary>> {
        match query {
            cosmwasm_std::WasmQuery::Smart { contract_addr: _, msg: _ } => {
                cosmwasm_std::SystemResult::Ok(cosmwasm_std::ContractResult::Ok(
                    cosmwasm_std::to_json_binary(&basket.clone()).unwrap()
                ))
            }
            _ => cosmwasm_std::SystemResult::Ok(cosmwasm_std::ContractResult::Ok(
                cosmwasm_std::to_json_binary(&basket.clone()).unwrap()
            )),
        }
    });

    // Create queue (CreateQueue only needs asset - LTVs are determined from CDP basket)
    let info = mock_info("owner", &[]);
    let msg = ExecuteMsg::CreateQueue {
        asset: asset.to_string(),
    };
    let _ = execute(deps.as_mut(), env.clone(), info, msg); // Ignore error if queue already exists

    // Submit deposit
    let info = mock_info(user, &coins(amount, asset));
    let msg = ExecuteMsg::SubmitDeposit {
        deposit_input: BackingDepositInput {
            asset: asset.to_string(),
            ltv,
            max_borrow_ltv,
            epoch_start_time: Some(0),
        },
        deposit_owner: None,
        locked,
        deposit_id: None,
        manager: None,
        affiliate_address: None,
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Get the deposit_id by querying the queue
    let queue: LTVQueueResponse = from_json(
        query(deps.as_ref(), env.clone(), QueryMsg::GetLTVQueue { asset: asset.to_string() }).unwrap()
    ).unwrap();
    queue.queue.current_deposit_id - Uint128::one()
}

#[test]
fn test_refresh_lock_with_perpetual_lock() {
    let (mut deps, mut env) = setup_contract();
    let user = "user1";
    
    // Create deposit with perpetual lock
    let locked_until = env.block.time.plus_seconds(30 * SECONDS_PER_DAY).seconds();
    let deposit_id = create_queue_and_deposit(
        &mut deps,
        &env,
        "uusd",
        user,
        10000,
        Decimal::percent(60),
        Decimal::percent(40),
        Some(Locked {
            locked_until,
            perpetual_lock: Some(30), // 30 day perpetual lock
            intended_lock_days: None,
        }),
    );

    // Get initial locked_until
    let response: BackingDepositResponse = from_json(
        query(deps.as_ref(), env.clone(), QueryMsg::GetBackingDeposit {
            user: user.to_string(),
            asset: "uusd".to_string(),
            ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
            deposit_id,
        }).unwrap()
    ).unwrap();
    let initial_locked_until = response.deposit.locked.as_ref().unwrap().locked_until;
    
    // Advance time by 10 days
    env.block.time = env.block.time.plus_seconds(10 * SECONDS_PER_DAY);
    
    // Refresh lock (permissionless - anyone can call, but need to specify user to find deposit)
    let msg = ExecuteMsg::RefreshLock {
        user: Some(user.to_string()), // Must specify user to find the deposit
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        deposit_id,
        epoch_start_time: 0,
    };
    let info = mock_info("anyone", &[]); // Anyone can call, but user parameter finds the deposit
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Check that locked_until was extended
    let response: BackingDepositResponse = from_json(
        query(deps.as_ref(), env.clone(), QueryMsg::GetBackingDeposit {
            user: user.to_string(),
            asset: "uusd".to_string(),
            ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
            deposit_id,
        }).unwrap()
    ).unwrap();
    let new_locked_until = response.deposit.locked.as_ref().unwrap().locked_until;
    
    // Should be extended by perpetual_lock duration from current time
    let expected_locked_until = env.block.time.plus_seconds(30 * SECONDS_PER_DAY).seconds();
    assert_eq!(new_locked_until, expected_locked_until);
    assert!(new_locked_until > initial_locked_until, "Lock should be extended");
}

#[test]
fn test_refresh_lock_permissionless_for_any_user() {
    let (mut deps, mut env) = setup_contract();
    let user = "user1";
    let other_user = "user2";
    
    // User1 creates deposit with perpetual lock
    let locked_until = env.block.time.plus_seconds(30 * SECONDS_PER_DAY).seconds();
    let deposit_id = create_queue_and_deposit(
        &mut deps,
        &env,
        "uusd",
        user,
        10000,
        Decimal::percent(60),
        Decimal::percent(40),
        Some(Locked {
            locked_until,
            perpetual_lock: Some(30),
            intended_lock_days: None,
        }),
    );

    let initial_locked_until = {
        let response: BackingDepositResponse = from_json(
            query(deps.as_ref(), env.clone(), QueryMsg::GetBackingDeposit {
                user: user.to_string(),
                asset: "uusd".to_string(),
                ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(40),
                deposit_id,
            }).unwrap()
        ).unwrap();
        response.deposit.locked.as_ref().unwrap().locked_until
    };
    
    // Advance time
    env.block.time = env.block.time.plus_seconds(10 * SECONDS_PER_DAY);
    
    // User2 (different user) refreshes lock for user1 (permissionless)
    let msg = ExecuteMsg::RefreshLock {
        user: Some(user.to_string()), // Refresh for user1
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        deposit_id,
        epoch_start_time: 0,
    };
    let info = mock_info(other_user, &[]);
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Check that lock was refreshed
    let response: BackingDepositResponse = from_json(
        query(deps.as_ref(), env.clone(), QueryMsg::GetBackingDeposit {
            user: user.to_string(),
            asset: "uusd".to_string(),
            ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
            deposit_id,
        }).unwrap()
    ).unwrap();
    let new_locked_until = response.deposit.locked.as_ref().unwrap().locked_until;
    
    assert!(new_locked_until > initial_locked_until, "Lock should be refreshed by anyone");
}

#[test]
fn test_refresh_lock_on_claim_revenue() {
    let (mut deps, mut env) = setup_contract();
    let user = "user1";
    
    // Create deposit with perpetual lock
    let locked_until = env.block.time.plus_seconds(30 * SECONDS_PER_DAY).seconds();
    let deposit_id = create_queue_and_deposit(
        &mut deps,
        &env,
        "uusd",
        user,
        10000,
        Decimal::percent(60),
        Decimal::percent(40),
        Some(Locked {
            locked_until,
            perpetual_lock: Some(30),
            intended_lock_days: None,
        }),
    );

    let initial_locked_until = {
        let response: BackingDepositResponse = from_json(
            query(deps.as_ref(), env.clone(), QueryMsg::GetBackingDeposit {
                user: user.to_string(),
                asset: "uusd".to_string(),
                ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(40),
                deposit_id,
            }).unwrap()
        ).unwrap();
        response.deposit.locked.as_ref().unwrap().locked_until
    };
    
    // Advance time
    env.block.time = env.block.time.plus_seconds(10 * SECONDS_PER_DAY);
    
    // Claim revenue (this should auto-refresh the lock)
    let msg = ExecuteMsg::ClaimRevenueForUser {
        compound_action: None,
        user: user.to_string(),
        asset: "uusd".to_string(),
        max_ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        limit: None,
    };
    let info = mock_info(user, &[]);
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Check that lock was auto-refreshed during claim
    let response: BackingDepositResponse = from_json(
        query(deps.as_ref(), env.clone(), QueryMsg::GetBackingDeposit {
            user: user.to_string(),
            asset: "uusd".to_string(),
            ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
            deposit_id,
        }).unwrap()
    ).unwrap();
    let new_locked_until = response.deposit.locked.as_ref().unwrap().locked_until;
    
    assert!(new_locked_until > initial_locked_until, "Lock should be auto-refreshed on claim");
}


#[test]
fn test_refresh_lock_on_move_deposit() {
    let (mut deps, mut env) = setup_contract();
    let user = "user1";
    
    // Setup second asset
    let basket = setup_mock_basket();
    deps.querier.update_wasm(move |query| -> cosmwasm_std::SystemResult<cosmwasm_std::ContractResult<cosmwasm_std::Binary>> {
        match query {
            cosmwasm_std::WasmQuery::Smart { contract_addr: _, msg: _ } => {
                cosmwasm_std::SystemResult::Ok(cosmwasm_std::ContractResult::Ok(
                    cosmwasm_std::to_json_binary(&basket).unwrap()
                ))
            }
            _ => cosmwasm_std::SystemResult::Ok(cosmwasm_std::ContractResult::Ok(
                cosmwasm_std::to_json_binary(&basket).unwrap()
            )),
        }
    });

    // Create deposit with perpetual lock
    let locked_until = env.block.time.plus_seconds(30 * SECONDS_PER_DAY).seconds();
    let deposit_id = create_queue_and_deposit(
        &mut deps,
        &env,
        "uusd",
        user,
        10000,
        Decimal::percent(60),
        Decimal::percent(40),
        Some(Locked {
            locked_until,
            perpetual_lock: Some(30),
            intended_lock_days: None,
        }),
    );

    let initial_locked_until = {
        let response: BackingDepositResponse = from_json(
            query(deps.as_ref(), env.clone(), QueryMsg::GetBackingDeposit {
                user: user.to_string(),
                asset: "uusd".to_string(),
                ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(40),
                deposit_id,
            }).unwrap()
        ).unwrap();
        response.deposit.locked.as_ref().unwrap().locked_until
    };
    
    // Advance time
    env.block.time = env.block.time.plus_seconds(10 * SECONDS_PER_DAY);
    
    // Create queue for destination (CreateQueue only needs asset)
    let info = mock_info("owner", &[]);
    let msg = ExecuteMsg::CreateQueue {
        asset: "uatom".to_string(),
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    
    // Move deposit (this should auto-refresh the lock)
    let msg = ExecuteMsg::MoveDeposit {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        deposit_id,
        destination: BackingDepositInput {
            asset: "uatom".to_string(),
            ltv: Decimal::percent(70),
            max_borrow_ltv: Decimal::percent(50),
            epoch_start_time: Some(0),
        },
        amount: None,
        user: None,
        epoch_start_time: 0,
    };
    let info = mock_info(user, &[]);
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Check that lock was preserved and refreshed in the new deposit
    let deposits: BackingDepositsByUserResponse = from_json(
        query(deps.as_ref(), env.clone(), QueryMsg::GetBackingDepositsByUser {
            user: user.to_string(),
            asset: "uatom".to_string(),
            limit: None,
            start_after: None,
        }).unwrap()
    ).unwrap();
    
    let moved_deposit = &deposits.deposits[0];
    assert!(moved_deposit.locked.is_some(), "Lock should be preserved");
    let new_locked_until = moved_deposit.locked.as_ref().unwrap().locked_until;
    assert!(new_locked_until > initial_locked_until, "Lock should be refreshed on move");
}

#[test]
fn test_refresh_lock_respects_ceiling() {
    let (mut deps, mut env) = setup_contract();
    let user = "user1";
    
    // Create deposit with perpetual lock, near the ceiling
    let lock_ceiling_days = 365u64;
    let days_already_elapsed = 300u64;
    let locked_until = env.block.time.plus_seconds(days_already_elapsed * SECONDS_PER_DAY).seconds();
    
    let deposit_id = create_queue_and_deposit(
        &mut deps,
        &env,
        "uusd",
        user,
        10000,
        Decimal::percent(60),
        Decimal::percent(40),
        Some(Locked {
            locked_until,
            perpetual_lock: Some(100), // Would exceed ceiling if fully applied
            intended_lock_days: None,
        }),
    );

    // Advance time significantly
    env.block.time = env.block.time.plus_seconds(50 * SECONDS_PER_DAY);
    
    // Refresh lock
    let msg = ExecuteMsg::RefreshLock {
        user: Some(user.to_string()), // Must specify user to find the deposit
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        deposit_id,
        epoch_start_time: 0,
    };
    let info = mock_info("anyone", &[]); // Anyone can call, but user parameter finds the deposit
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Check that lock respects ceiling (start_time + ceiling days)
    let response: BackingDepositResponse = from_json(
        query(deps.as_ref(), env.clone(), QueryMsg::GetBackingDeposit {
            user: user.to_string(),
            asset: "uusd".to_string(),
            ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
            deposit_id,
        }).unwrap()
    ).unwrap();
    let new_locked_until = response.deposit.locked.as_ref().unwrap().locked_until;
    let start_time = response.deposit.start_time;
    let max_lock_time = start_time + (lock_ceiling_days * SECONDS_PER_DAY);
    
    assert!(new_locked_until <= max_lock_time, "Lock should not exceed ceiling");
}

#[test]
fn test_refresh_lock_without_perpetual_lock_does_nothing() {
    let (mut deps, mut env) = setup_contract();
    let user = "user1";
    
    // Create deposit without perpetual lock
    let locked_until = env.block.time.plus_seconds(30 * SECONDS_PER_DAY).seconds();
    let deposit_id = create_queue_and_deposit(
        &mut deps,
        &env,
        "uusd",
        user,
        10000,
        Decimal::percent(60),
        Decimal::percent(40),
        Some(Locked {
            locked_until,
            perpetual_lock: None, // No perpetual lock
            intended_lock_days: None,
        }),
    );

    let initial_locked_until = {
        let response: BackingDepositResponse = from_json(
            query(deps.as_ref(), env.clone(), QueryMsg::GetBackingDeposit {
                user: user.to_string(),
                asset: "uusd".to_string(),
                ltv: Decimal::percent(60),
                max_borrow_ltv: Decimal::percent(40),
                deposit_id,
            }).unwrap()
        ).unwrap();
        response.deposit.locked.as_ref().unwrap().locked_until
    };
    
    // Advance time
    env.block.time = env.block.time.plus_seconds(10 * SECONDS_PER_DAY);
    
    // Refresh lock (should do nothing without perpetual_lock)
    let msg = ExecuteMsg::RefreshLock {
        user: Some(user.to_string()), // Must specify user to find the deposit
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        deposit_id,
        epoch_start_time: 0,
    };
    let info = mock_info("anyone", &[]); // Anyone can call, but user parameter finds the deposit
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Check that locked_until didn't change
    let response: BackingDepositResponse = from_json(
        query(deps.as_ref(), env.clone(), QueryMsg::GetBackingDeposit {
            user: user.to_string(),
            asset: "uusd".to_string(),
            ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
            deposit_id,
        }).unwrap()
    ).unwrap();
    let new_locked_until = response.deposit.locked.as_ref().unwrap().locked_until;
    
    assert_eq!(new_locked_until, initial_locked_until, "Lock without perpetual_lock should not be refreshed");
}
