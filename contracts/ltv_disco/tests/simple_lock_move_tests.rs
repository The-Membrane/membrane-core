// Simple focused tests for lock and move operations with effective locked tokens
// These tests use direct storage access to avoid query complications

mod effective_locked_tokens_tests; // Import the setup helpers

use cosmwasm_std::testing::{mock_info};
use cosmwasm_std::{coins, Decimal, Uint128};
use ltv_disco::contract::{execute};
use ltv_disco::state::LTV_QUEUES;
use membrane::ltv_disco::{ExecuteMsg, BackingDepositInput, QueryMsg, LTVQueueResponse};
use membrane::types::Locked;
use cosmwasm_std::from_json;
use ltv_disco::contract::query;

fn get_effective_total_from_storage(
    deps: &cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>,
    asset: &str,
    ltv: Decimal,
    max_borrow_ltv: Decimal,
) -> Uint128 {
    let queue = LTV_QUEUES.load(&deps.storage, asset.to_string()).unwrap();

    for slot in queue.slots {
        if slot.ltv == ltv {
            for group in slot.deposit_groups {
                if group.max_borrow_ltv == max_borrow_ltv {
                    return group.total_unused_locked_vault_tokens;
                }
            }
        }
    }
    Uint128::zero()
}

fn get_total_locked_from_storage(
    deps: &cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>,
    asset: &str,
    ltv: Decimal,
    max_borrow_ltv: Decimal,
) -> Uint128 {
    let queue = LTV_QUEUES.load(&deps.storage, asset.to_string()).unwrap();

    for slot in queue.slots {
        if slot.ltv == ltv {
            for group in slot.deposit_groups {
                if group.max_borrow_ltv == max_borrow_ltv {
                    return group.total_locked_vault_tokens;
                }
            }
        }
    }
    Uint128::zero()
}

#[test]
fn test_simple_lock_increases_total_locked() {
    use effective_locked_tokens_tests::instantiate_contract_with_epoch;

    let (mut deps, env, _epoch_state) = instantiate_contract_with_epoch(0, 86400, 1000);

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
            epoch_start_time: None,
        },
        deposit_owner: None,
        locked: None,
        deposit_id: None,
        manager: None,
        affiliate_address: None,
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Get deposit_id
    let queue: LTVQueueResponse = from_json(
        query(deps.as_ref(), env.clone(), QueryMsg::GetLTVQueue { asset: "uusd".to_string() }).unwrap()
    ).unwrap();
    let deposit_id = queue.queue.current_deposit_id - Uint128::one();

    // Get initial total_locked_vault_tokens from storage
    let initial_locked = get_total_locked_from_storage(&deps, "uusd", Decimal::percent(60), Decimal::percent(40));
    println!("Initial total_locked: {}", initial_locked);

    // Lock for 30 days
    let info = mock_info("user1", &[]);
    let msg = ExecuteMsg::Lock {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        deposit_id,
        locked: Locked {
            locked_until: env.block.time.plus_seconds(30 * 86400).seconds(),
            perpetual_lock: None,
            intended_lock_days: Some(30),
        },
        amount: None,
        epoch_start_time: 0,
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Get new total_locked from storage
    let new_locked = get_total_locked_from_storage(&deps, "uusd", Decimal::percent(60), Decimal::percent(40));
    println!("New total_locked: {}", new_locked);

    // Should increase by factor of 31 (30 lock days + 1)
    assert!(new_locked > initial_locked, "Total locked should increase");
    assert_eq!(new_locked, initial_locked * Uint128::new(31), "Should be 31x for 30-day lock");
}

#[test]
fn test_simple_lock_increases_effective_total() {
    use effective_locked_tokens_tests::instantiate_contract_with_epoch;

    let (mut deps, env, _epoch_state) = instantiate_contract_with_epoch(0, 86400, 1000);

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
            epoch_start_time: None,
        },
        deposit_owner: None,
        locked: None,
        deposit_id: None,
        manager: None,
        affiliate_address: None,
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Get deposit_id
    let queue: LTVQueueResponse = from_json(
        query(deps.as_ref(), env.clone(), QueryMsg::GetLTVQueue { asset: "uusd".to_string() }).unwrap()
    ).unwrap();
    let deposit_id = queue.queue.current_deposit_id - Uint128::one();

    // Get initial effective total from storage
    let initial_effective = get_effective_total_from_storage(&deps, "uusd", Decimal::percent(60), Decimal::percent(40));
    println!("Initial effective: {}", initial_effective);
    assert!(initial_effective > Uint128::zero(), "Should have initial effective total");

    // Lock for 30 days
    let info = mock_info("user1", &[]);
    let msg = ExecuteMsg::Lock {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        deposit_id,
        locked: Locked {
            locked_until: env.block.time.plus_seconds(30 * 86400).seconds(),
            perpetual_lock: None,
            intended_lock_days: Some(30),
        },
        amount: None,
        epoch_start_time: 0,
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Get new effective total from storage
    let new_effective = get_effective_total_from_storage(&deps, "uusd", Decimal::percent(60), Decimal::percent(40));
    println!("New effective: {}", new_effective);

    // Should increase significantly (proportional to the 31x increase in locked vault tokens)
    assert!(new_effective > initial_effective, "Effective total should increase after locking");

    // The increase should be approximately 31x (accounting for time weighting)
    let ratio = new_effective.u128() as f64 / initial_effective.u128() as f64;
    println!("Increase ratio: {:.2}x", ratio);
    assert!(ratio > 25.0 && ratio < 35.0, "Ratio should be close to 31x");
}

#[test]
fn test_simple_move_updates_both_groups() {
    use effective_locked_tokens_tests::instantiate_contract_with_epoch;

    let (mut deps, env, _epoch_state) = instantiate_contract_with_epoch(0, 86400, 1000);

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
            epoch_start_time: None,
        },
        deposit_owner: None,
        locked: None,
        deposit_id: None,
        manager: None,
        affiliate_address: None,
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Get deposit_id
    let queue: LTVQueueResponse = from_json(
        query(deps.as_ref(), env.clone(), QueryMsg::GetLTVQueue { asset: "uusd".to_string() }).unwrap()
    ).unwrap();
    let deposit_id = queue.queue.current_deposit_id - Uint128::one();

    // Get initial effective total for source group
    let initial_source_effective = get_effective_total_from_storage(&deps, "uusd", Decimal::percent(60), Decimal::percent(40));
    println!("Initial source effective: {}", initial_source_effective);
    assert!(initial_source_effective > Uint128::zero(), "Should have initial effective total");

    // Move deposit to different group (different max_borrow_ltv)
    let info = mock_info("user1", &[]);
    let msg = ExecuteMsg::MoveDeposit {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        deposit_id,
        destination: BackingDepositInput {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(50), // Different max_borrow_ltv
            epoch_start_time: None,
        },
        amount: None,
        user: None,
        epoch_start_time: 0,
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Source group should have zero effective total
    let source_effective_after = get_effective_total_from_storage(&deps, "uusd", Decimal::percent(60), Decimal::percent(40));
    println!("Source effective after: {}", source_effective_after);
    assert_eq!(source_effective_after, Uint128::zero(), "Source group should have zero effective total");

    // Destination group should have non-zero effective total
    let dest_effective_after = get_effective_total_from_storage(&deps, "uusd", Decimal::percent(60), Decimal::percent(50));
    println!("Dest effective after: {}", dest_effective_after);
    assert!(dest_effective_after > Uint128::zero(), "Destination group should have effective total");
}
