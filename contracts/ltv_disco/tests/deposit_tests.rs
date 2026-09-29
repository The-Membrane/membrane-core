mod test_helpers;

use cosmwasm_std::testing::{mock_env, mock_info};
use cosmwasm_std::{coins, Decimal, Uint128};
use membrane::ltv_disco::{BackingDepositInput, ExecuteMsg, QueryMsg};
use ltv_disco::contract::execute;
use test_helpers::*;

// =================== Basic Deposits ===================

#[test]
fn test_submit_deposit_new() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // Verify queue updated
    let res = query_queue(&deps, COLLATERAL);
    let slot = &res.queues[0].1.slots[0]; // 90% LTV slot (highest, index 0)
    assert_eq!(slot.total_deposit_tokens, Uint128::new(10_000));
    assert_eq!(slot.total_vault_tokens, vt(10_000)); // 1:1 for first deposit

    // Verify deposit stored
    let deposit_res = query_deposit(&deps, &addr("user1"), COLLATERAL, 90, 0);
    assert_eq!(deposit_res.deposit.vault_tokens, vt(10_000));
    assert_eq!(deposit_res.deposit.user.as_str(), addr("user1"));
    assert!(deposit_res.deposit.withdrawals_enabled);
    assert!(!deposit_res.deposit.compound_claims);
}

#[test]
fn test_submit_deposit_multiple_slots() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 70, 20_000);
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 50, 30_000);

    let res = query_queue(&deps, COLLATERAL);
    // slots[0] = 90% LTV, slots[20] = 70% LTV, slots[40] = 50% LTV
    assert_eq!(res.queues[0].1.slots[0].total_deposit_tokens, Uint128::new(10_000));
    assert_eq!(res.queues[0].1.slots[20].total_deposit_tokens, Uint128::new(20_000));
    assert_eq!(res.queues[0].1.slots[40].total_deposit_tokens, Uint128::new(30_000));
}

#[test]
fn test_submit_deposit_multiple_users_same_slot() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 80, 10_000);
    submit_deposit(&mut deps, &env, &addr("user2"), COLLATERAL, 80, 20_000);

    let res = query_queue(&deps, COLLATERAL);
    // slots[10] = 80% LTV (90 - 80 = 10)
    assert_eq!(res.queues[0].1.slots[10].total_deposit_tokens, Uint128::new(30_000));

    // Both deposits exist
    let d1 = query_deposit(&deps, &addr("user1"), COLLATERAL, 80, 0);
    assert_eq!(d1.deposit.vault_tokens, vt(10_000));
    let d2 = query_deposit(&deps, &addr("user2"), COLLATERAL, 80, 1);
    assert_eq!(d2.deposit.vault_tokens, vt(20_000));
}

#[test]
fn test_submit_deposit_top_up_existing() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // Top up existing deposit (deposit_id=0)
    let info = mock_info(&addr("user1"), &coins(5_000, COLLATERAL));
    execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput { asset: COLLATERAL.to_string(), slot: 90 },
            deposit_owner: None,
            deposit_id: Some(Uint128::zero()),
            manager: None,
            affiliate_address: None,
            revenue_destination: None,
        },
    ).unwrap();

    let deposit = query_deposit(&deps, &addr("user1"), COLLATERAL, 90, 0);
    assert_eq!(deposit.deposit.vault_tokens, vt(15_000));
}

#[test]
fn test_submit_deposit_on_behalf_of_user() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit_for_user(&mut deps, &env, &addr("depositor"), &addr("user1"), COLLATERAL, 90, 10_000, None);

    let deposit = query_deposit(&deps, &addr("user1"), COLLATERAL, 90, 0);
    assert_eq!(deposit.deposit.user.as_str(), addr("user1"));
    assert_eq!(deposit.deposit.depositor.unwrap().as_str(), addr("depositor"));
}

#[test]
fn test_submit_deposit_with_manager() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit_for_user(&mut deps, &env, &addr("depositor"), &addr("user1"), COLLATERAL, 90, 10_000, Some(addr("manager1")));

    let deposit = query_deposit(&deps, &addr("user1"), COLLATERAL, 90, 0);
    assert_eq!(deposit.deposit.manager.unwrap().as_str(), addr("manager1"));
}

#[test]
fn test_submit_deposit_with_revenue_destination() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let info = mock_info(&addr("user1"), &coins(10_000, COLLATERAL));
    execute(
        deps.as_mut(), mock_env(), info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput { asset: COLLATERAL.to_string(), slot: 90 },
            deposit_owner: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
            revenue_destination: Some(addr("revenue_dest")),
        },
    ).unwrap();

    let deposit = query_deposit(&deps, &addr("user1"), COLLATERAL, 90, 0);
    assert_eq!(deposit.deposit.revenue_destination.unwrap().as_str(), addr("revenue_dest"));
}

// =================== Deposit ID Incrementing ===================

#[test]
fn test_deposit_ids_auto_increment() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000); // id=0
    submit_deposit(&mut deps, &env, &addr("user2"), COLLATERAL, 90, 10_000); // id=1
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 89, 10_000); // id=2

    let res = query_queue(&deps, COLLATERAL);
    assert_eq!(res.queues[0].1.current_deposit_id, Uint128::new(3));
}

// =================== Deposit Validation ===================

#[test]
fn test_deposit_invalid_slot_below_min() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let info = mock_info(&addr("user1"), &coins(10_000, COLLATERAL));
    let err = execute(
        deps.as_mut(), mock_env(), info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput { asset: COLLATERAL.to_string(), slot: 49 },
            deposit_owner: None, deposit_id: None, manager: None,
            affiliate_address: None, revenue_destination: None,
        },
    ).unwrap_err();
    assert!(err.to_string().contains("Slot not found") || err.to_string().contains("Invalid slot"));
}

#[test]
fn test_deposit_invalid_slot_above_max() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let info = mock_info(&addr("user1"), &coins(10_000, COLLATERAL));
    let err = execute(
        deps.as_mut(), mock_env(), info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput { asset: COLLATERAL.to_string(), slot: 91 },
            deposit_owner: None, deposit_id: None, manager: None,
            affiliate_address: None, revenue_destination: None,
        },
    ).unwrap_err();
    assert!(err.to_string().contains("Slot not found") || err.to_string().contains("Invalid slot"));
}

#[test]
fn test_deposit_below_minimum() {
    let mut deps = setup_deps();
    setup_contract(&mut deps); // minimum is 1000
    create_queue(&mut deps, COLLATERAL);

    let info = mock_info(&addr("user1"), &coins(999, COLLATERAL));
    let err = execute(
        deps.as_mut(), mock_env(), info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput { asset: COLLATERAL.to_string(), slot: 90 },
            deposit_owner: None, deposit_id: None, manager: None,
            affiliate_address: None, revenue_destination: None,
        },
    ).unwrap_err();
    assert_eq!(err.to_string(), "Invalid deposit amount");
}

#[test]
fn test_deposit_wrong_denom() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let info = mock_info(&addr("user1"), &coins(10_000, "wrong_denom"));
    let err = execute(
        deps.as_mut(), mock_env(), info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput { asset: COLLATERAL.to_string(), slot: 90 },
            deposit_owner: None, deposit_id: None, manager: None,
            affiliate_address: None, revenue_destination: None,
        },
    ).unwrap_err();
    assert!(err.to_string().contains("mismatch"));
}

#[test]
fn test_deposit_no_funds() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let info = mock_info(&addr("user1"), &[]);
    let err = execute(
        deps.as_mut(), mock_env(), info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput { asset: COLLATERAL.to_string(), slot: 90 },
            deposit_owner: None, deposit_id: None, manager: None,
            affiliate_address: None, revenue_destination: None,
        },
    ).unwrap_err();
    assert!(err.to_string().contains("only"));
}

#[test]
fn test_deposit_nonexistent_queue() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    // Don't create a queue

    let info = mock_info(&addr("user1"), &coins(10_000, "unknown_asset"));
    let err = execute(
        deps.as_mut(), mock_env(), info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput { asset: "unknown_asset".to_string(), slot: 90 },
            deposit_owner: None, deposit_id: None, manager: None,
            affiliate_address: None, revenue_destination: None,
        },
    ).unwrap_err();
    assert_eq!(err.to_string(), "Invalid asset");
}

#[test]
fn test_deposit_top_up_nonexistent_id() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let info = mock_info(&addr("user1"), &coins(10_000, COLLATERAL));
    let err = execute(
        deps.as_mut(), mock_env(), info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput { asset: COLLATERAL.to_string(), slot: 90 },
            deposit_owner: None,
            deposit_id: Some(Uint128::new(999)),
            manager: None,
            affiliate_address: None,
            revenue_destination: None,
        },
    ).unwrap_err();
    assert!(err.to_string().contains("not found"));
}

// =================== Vault Token Math ===================

#[test]
fn test_vault_token_ratio_first_deposit_is_one_to_one() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 50_000);

    let deposit = query_deposit(&deps, &addr("user1"), COLLATERAL, 90, 0);
    assert_eq!(deposit.deposit.vault_tokens, vt(50_000));
}

#[test]
fn test_vault_token_ratio_second_deposit_proportional() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // With 1:1 ratio still (no bad debt yet), second deposit gets same tokens
    submit_deposit(&mut deps, &env, &addr("user2"), COLLATERAL, 90, 10_000);

    let d2 = query_deposit(&deps, &addr("user2"), COLLATERAL, 90, 1);
    assert_eq!(d2.deposit.vault_tokens, vt(10_000));
}

// =================== All User Deposits Query ===================

#[test]
fn test_query_all_user_deposits() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 70, 20_000);

    let all = query_all_deposits(&deps, &addr("user1"));
    assert_eq!(all.deposits.len(), 2);

    // Verify contains both slots
    let slots: Vec<u8> = all.deposits.iter().map(|d| d.slot).collect();
    assert!(slots.contains(&90));
    assert!(slots.contains(&70));
}

// =================== User Total Deposits ===================

#[test]
fn test_user_total_deposits_tracked() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 70, 20_000);

    let res: membrane::ltv_disco::UserTotalDepositsResponse = cosmwasm_std::from_json(
        ltv_disco::contract::query(
            deps.as_ref(), mock_env(),
            QueryMsg::UserTotalDeposits { user: addr("user1") },
        ).unwrap()
    ).unwrap();
    assert_eq!(res.total_deposits, Uint128::new(30_000));
}
