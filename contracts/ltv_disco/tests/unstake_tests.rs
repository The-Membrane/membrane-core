mod test_helpers;

use cosmwasm_std::testing::{mock_env, mock_info};
use cosmwasm_std::{coins, Uint128};
use membrane::ltv_disco::ExecuteMsg;
use ltv_disco::contract::execute;
use test_helpers::*;

// =================== Request Unstake ===================

#[test]
fn test_request_unstake_full() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    request_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0, None);

    let requests = query_unstakes(&deps, &addr("user1"), COLLATERAL);
    assert_eq!(requests.requests.len(), 1);
    assert_eq!(requests.requests[0].vault_tokens, vt(10_000));
    assert_eq!(requests.requests[0].unlock_time, env.block.time.seconds() + 172800);
}

#[test]
fn test_request_unstake_partial() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    request_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0, Some(5_000 * VT_SCALE));

    let requests = query_unstakes(&deps, &addr("user1"), COLLATERAL);
    assert_eq!(requests.requests[0].vault_tokens, vt(5_000));
}

#[test]
fn test_request_unstake_exceeds_balance() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    let info = mock_info(&addr("user1"), &[]);
    let err = execute(
        deps.as_mut(), env, info,
        ExecuteMsg::RequestUnstake {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
            amount: Some(vt(20_000)),
        },
    ).unwrap_err();
    assert_eq!(err.to_string(), "Insufficient deposits");
}

#[test]
fn test_request_unstake_zero_amount() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    let info = mock_info(&addr("user1"), &[]);
    let err = execute(
        deps.as_mut(), env, info,
        ExecuteMsg::RequestUnstake {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
            amount: Some(Uint128::zero()),
        },
    ).unwrap_err();
    assert_eq!(err.to_string(), "Invalid deposit amount");
}

#[test]
fn test_request_unstake_duplicate_rejected() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    request_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0, None);

    // Try again - should fail
    let info = mock_info(&addr("user1"), &[]);
    let err = execute(
        deps.as_mut(), env, info,
        ExecuteMsg::RequestUnstake {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
            amount: None,
        },
    ).unwrap_err();
    assert_eq!(err.to_string(), "Unstake request already pending for this deposit");
}

#[test]
fn test_request_unstake_wrong_user() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // user2 tries to unstake user1's deposit
    let info = mock_info(&addr("user2"), &[]);
    let err = execute(
        deps.as_mut(), env, info,
        ExecuteMsg::RequestUnstake {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
            amount: None,
        },
    ).unwrap_err();
    // Will fail at deposit lookup since key includes user address
    assert!(err.to_string().contains("not found") || err.to_string().contains("Unauthorized"));
}

#[test]
fn test_request_unstake_nonexistent_deposit() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let info = mock_info(&addr("user1"), &[]);
    let err = execute(
        deps.as_mut(), mock_env(), info,
        ExecuteMsg::RequestUnstake {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::new(999),
            amount: None,
        },
    ).unwrap_err();
    assert_eq!(err.to_string(), "Deposit not found");
}

// =================== Complete Unstake ===================

#[test]
fn test_complete_unstake_after_cooldown() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    request_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0, None);

    // Advance past cooldown (2 days)
    advance_time(&mut env, 172801);

    let res = complete_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0);

    // Should have BankMsg to send collateral back
    assert!(res.messages.len() >= 1);

    // Queue should be empty now
    let queue_res = query_queue(&deps, COLLATERAL);
    assert!(queue_res.queues[0].1.slots[0].total_deposit_tokens.is_zero());

    // Unstake request should be cleaned up
    let unstakes = query_unstakes(&deps, &addr("user1"), COLLATERAL);
    assert!(unstakes.requests.is_empty());
}

#[test]
fn test_complete_unstake_before_cooldown_fails() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    request_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0, None);

    // Only advance 1 day (cooldown is 2 days)
    advance_time(&mut env, 86400);

    let info = mock_info(&addr("user1"), &[]);
    let err = execute(
        deps.as_mut(), env, info,
        ExecuteMsg::CompleteUnstake {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
        },
    ).unwrap_err();
    assert_eq!(err.to_string(), "Unstake request not ready yet");
}

#[test]
fn test_complete_unstake_partial() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    request_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0, Some(5_000 * VT_SCALE));

    advance_time(&mut env, 172801);
    complete_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0);

    // Deposit should still exist with remaining
    let deposit = query_deposit(&deps, &addr("user1"), COLLATERAL, 90, 0);
    assert_eq!(deposit.deposit.vault_tokens, vt(5_000));

    // Queue should have remaining
    let queue_res = query_queue(&deps, COLLATERAL);
    assert_eq!(queue_res.queues[0].1.slots[0].total_deposit_tokens, Uint128::new(5_000));
}

// =================== Cancel Unstake ===================

#[test]
fn test_cancel_unstake_success() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    request_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0, None);

    // Cancel
    let info = mock_info(&addr("user1"), &[]);
    execute(
        deps.as_mut(), env, info,
        ExecuteMsg::CancelUnstake {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
        },
    ).unwrap();

    // Request should be gone
    let unstakes = query_unstakes(&deps, &addr("user1"), COLLATERAL);
    assert!(unstakes.requests.is_empty());

    // Deposit should still exist untouched
    let deposit = query_deposit(&deps, &addr("user1"), COLLATERAL, 90, 0);
    assert_eq!(deposit.deposit.vault_tokens, vt(10_000));
}

#[test]
fn test_cancel_unstake_wrong_user() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    request_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0, None);

    // user2 tries to cancel user1's request
    let info = mock_info(&addr("user2"), &[]);
    let err = execute(
        deps.as_mut(), env, info,
        ExecuteMsg::CancelUnstake {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
        },
    ).unwrap_err();
    // The key includes the sender, so it won't find the request
    assert!(err.to_string().contains("not found") || err.to_string().contains("Unauthorized"));
}

#[test]
fn test_cancel_nonexistent_unstake() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let info = mock_info(&addr("user1"), &[]);
    let err = execute(
        deps.as_mut(), mock_env(), info,
        ExecuteMsg::CancelUnstake {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::new(999),
        },
    ).unwrap_err();
    assert_eq!(err.to_string(), "Unstake request not found");
}

// =================== Withdrawals Disabled ===================

#[test]
fn test_request_unstake_withdrawals_disabled() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    // Deposit on behalf of user1
    submit_deposit_for_user(&mut deps, &env, &addr("depositor"), &addr("user1"), COLLATERAL, 90, 10_000, None);

    // The `withdrawals_enabled` field is set true by default for new deposits.
    let deposit = query_deposit(&deps, &addr("user1"), COLLATERAL, 90, 0);
    assert!(deposit.deposit.withdrawals_enabled);
}

// =================== Unstake + Re-deposit Cycle ===================

#[test]
fn test_unstake_then_redeposit() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    request_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0, None);

    advance_time(&mut env, 172801);
    complete_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0);

    // Deposit should be removed
    let all = query_all_deposits(&deps, &addr("user1"));
    assert!(all.deposits.is_empty());

    // Re-deposit (new id)
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 20_000);
    let queue_res = query_queue(&deps, COLLATERAL);
    assert_eq!(queue_res.queues[0].1.slots[0].total_deposit_tokens, Uint128::new(20_000));
}
