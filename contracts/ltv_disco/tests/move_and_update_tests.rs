mod test_helpers;

use cosmwasm_std::testing::{mock_env, mock_info};
use cosmwasm_std::{coins, Uint128};
use membrane::ltv_disco::{BackingDepositInput, ExecuteMsg};
use ltv_disco::contract::execute;
use test_helpers::*;

// =================== Move Deposit ===================

#[test]
fn test_move_deposit_between_slots() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // Move from slot 90 to slot 70
    let info = mock_info(&addr("user1"), &[]);
    execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::MoveDeposit {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
            destination: BackingDepositInput { asset: COLLATERAL.to_string(), slot: 70 },
            amount: None, // Move all
            user: None,
        },
    ).unwrap();

    // Slot 90 (index 0) should be empty
    let queue = query_queue(&deps, COLLATERAL);
    assert!(queue.queues[0].1.slots[0].total_deposit_tokens.is_zero());

    // Slot 70 (index 20) should have the deposit
    assert_eq!(queue.queues[0].1.slots[20].total_deposit_tokens, Uint128::new(10_000));

    // Old deposit key should be gone, new one should exist
    let all = query_all_deposits(&deps, &addr("user1"));
    assert_eq!(all.deposits.len(), 1);
    assert_eq!(all.deposits[0].slot, 70);
}

#[test]
fn test_move_deposit_partial() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    let info = mock_info(&addr("user1"), &[]);
    execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::MoveDeposit {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
            destination: BackingDepositInput { asset: COLLATERAL.to_string(), slot: 70 },
            amount: Some(vt(5_000)),
            user: None,
        },
    ).unwrap();

    let queue = query_queue(&deps, COLLATERAL);
    // Slot 90 (index 0) should have 5000 remaining
    assert_eq!(queue.queues[0].1.slots[0].total_deposit_tokens, Uint128::new(5_000));
    // Slot 70 (index 20) should have ~5000
    assert!(queue.queues[0].1.slots[20].total_deposit_tokens > Uint128::zero());
}

#[test]
fn test_move_deposit_unauthorized() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // user2 tries to move user1's deposit
    let info = mock_info(&addr("user2"), &[]);
    let err = execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::MoveDeposit {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
            destination: BackingDepositInput { asset: COLLATERAL.to_string(), slot: 70 },
            amount: None,
            user: None,
        },
    ).unwrap_err();
    assert!(err.to_string().contains("not found") || err.to_string().contains("Unauthorized"));
}

#[test]
fn test_move_deposit_by_manager() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit_for_user(&mut deps, &env, &addr("depositor"), &addr("user1"), COLLATERAL, 90, 10_000, Some(addr("manager1")));

    // Manager moves deposit on behalf of user
    let info = mock_info(&addr("manager1"), &[]);
    execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::MoveDeposit {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
            destination: BackingDepositInput { asset: COLLATERAL.to_string(), slot: 80 },
            amount: None,
            user: Some(addr("user1")),
        },
    ).unwrap();

    let queue = query_queue(&deps, COLLATERAL);
    // Slot 90 (index 0) should be empty
    assert!(queue.queues[0].1.slots[0].total_deposit_tokens.is_zero());
    // Slot 80 (index 10) should have the deposit
    assert_eq!(queue.queues[0].1.slots[10].total_deposit_tokens, Uint128::new(10_000));
}

// =================== Update Deposit ===================

#[test]
fn test_update_deposit_change_manager() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    let info = mock_info(&addr("user1"), &[]);
    execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::UpdateDeposit {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
            deposit_owner: None,
            manager: Some(addr("manager1")),
            revenue_destination: None,
        },
    ).unwrap();

    let deposit = query_deposit(&deps, &addr("user1"), COLLATERAL, 90, 0);
    assert_eq!(deposit.deposit.manager.unwrap().as_str(), addr("manager1"));
}

#[test]
fn test_update_deposit_change_revenue_destination() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    let info = mock_info(&addr("user1"), &[]);
    execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::UpdateDeposit {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
            deposit_owner: None,
            manager: None,
            revenue_destination: Some(addr("rev_dest")),
        },
    ).unwrap();

    let deposit = query_deposit(&deps, &addr("user1"), COLLATERAL, 90, 0);
    assert_eq!(deposit.deposit.revenue_destination.unwrap().as_str(), addr("rev_dest"));
}

#[test]
fn test_update_deposit_unauthorized() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    let info = mock_info(&addr("user2"), &[]);
    let err = execute(
        deps.as_mut(), env, info,
        ExecuteMsg::UpdateDeposit {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
            deposit_owner: None,
            manager: None,
            revenue_destination: None,
        },
    ).unwrap_err();
    assert!(err.to_string().contains("not found") || err.to_string().contains("Unauthorized"));
}
