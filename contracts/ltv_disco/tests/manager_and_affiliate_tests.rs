mod test_helpers;

use cosmwasm_std::testing::{mock_env, mock_info};
use cosmwasm_std::{coins, from_json, Decimal, Uint128};
use membrane::ltv_disco::{ExecuteMsg, QueryMsg, ManagerFeeResponse, ManagedDepositKeysResponse};
use ltv_disco::contract::{execute, query};
use test_helpers::*;

// =================== Manager Deposits ===================

#[test]
fn test_manager_can_move_deposit() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit_for_user(&mut deps, &env, &addr("depositor"), &addr("user1"), COLLATERAL, 90, 10_000, Some(addr("manager1")));

    // Manager moves the deposit
    let info = mock_info(&addr("manager1"), &[]);
    execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::MoveDeposit {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
            destination: membrane::ltv_disco::BackingDepositInput {
                asset: COLLATERAL.to_string(),
                slot: 70,
            },
            amount: None,
            user: Some(addr("user1")),
        },
    ).unwrap();

    let queue = query_queue(&deps, COLLATERAL);
    // Slot 90 (index 0) should be empty
    assert!(queue.queues[0].1.slots[0].total_deposit_tokens.is_zero());
    // Slot 70 (index 20) should have the deposit
    assert!(queue.queues[0].1.slots[20].total_deposit_tokens > Uint128::zero());
}

#[test]
fn test_manager_cannot_unstake() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit_for_user(&mut deps, &env, &addr("depositor"), &addr("user1"), COLLATERAL, 90, 10_000, Some(addr("manager1")));

    // Manager tries to request unstake - should fail (only user can)
    let info = mock_info(&addr("manager1"), &[]);
    let err = execute(
        deps.as_mut(), env, info,
        ExecuteMsg::RequestUnstake {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
            amount: None,
        },
    ).unwrap_err();
    assert!(err.to_string().contains("not found") || err.to_string().contains("Unauthorized"));
}

// =================== Manager Fee ===================

#[test]
fn test_set_manager_fee() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit_for_user(&mut deps, &env, &addr("depositor"), &addr("user1"), COLLATERAL, 90, 10_000, Some(addr("manager1")));

    let info = mock_info(&addr("manager1"), &[]);
    execute(
        deps.as_mut(), env, info,
        ExecuteMsg::SetManagerFee { fee: Decimal::percent(3) },
    ).unwrap();

    let res: ManagerFeeResponse = from_json(query(
        deps.as_ref(), mock_env(),
        QueryMsg::GetManagerFee { manager: addr("manager1") },
    ).unwrap()).unwrap();
    assert_eq!(res.fee, Decimal::percent(3));
}

#[test]
fn test_clean_manager_fee() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit_for_user(&mut deps, &env, &addr("depositor"), &addr("user1"), COLLATERAL, 90, 10_000, Some(addr("manager1")));

    let info = mock_info(&addr("manager1"), &[]);
    execute(deps.as_mut(), env.clone(), info, ExecuteMsg::SetManagerFee { fee: Decimal::percent(3) }).unwrap();

    // Remove the deposit by unstaking
    let mut env = env;
    request_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0, None);
    advance_time(&mut env, 172801);
    complete_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0);

    // Now clean the manager fee (permissionless since no deposits)
    let info = mock_info(&addr("anyone"), &[]);
    execute(
        deps.as_mut(), env, info,
        ExecuteMsg::CleanManagerFee { manager: addr("manager1") },
    ).unwrap();

    let res: ManagerFeeResponse = from_json(query(
        deps.as_ref(), mock_env(),
        QueryMsg::GetManagerFee { manager: addr("manager1") },
    ).unwrap()).unwrap();
    assert_eq!(res.fee, Decimal::zero());
}

// =================== Managed Deposit Keys Query ===================

#[test]
fn test_query_managed_deposit_keys() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit_for_user(&mut deps, &env, &addr("depositor"), &addr("user1"), COLLATERAL, 90, 10_000, Some(addr("manager1")));
    submit_deposit_for_user(&mut deps, &env, &addr("depositor"), &addr("user2"), COLLATERAL, 80, 20_000, Some(addr("manager1")));

    let res: ManagedDepositKeysResponse = from_json(query(
        deps.as_ref(), mock_env(),
        QueryMsg::GetManagedDepositKeys {
            manager: addr("manager1"),
            limit: None,
            start_after: None,
        },
    ).unwrap()).unwrap();
    assert_eq!(res.keys.len(), 2);
    assert_eq!(res.total, 2);
}

// =================== Affiliate ===================

#[test]
fn test_set_affiliate() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);

    let env = mock_env();
    let info = mock_info(&addr("owner"), &[]);
    execute(
        deps.as_mut(), env, info,
        ExecuteMsg::SetAffiliate {
            user: addr("user1"),
            affiliate_address: addr("affiliate1"),
            label: Some("Test Affiliate".to_string()),
        },
    ).unwrap();

    let affiliates: Vec<membrane::types::AffiliateData> = from_json(query(
        deps.as_ref(), mock_env(),
        QueryMsg::GetAffiliates { user: addr("user1") },
    ).unwrap()).unwrap();
    assert!(!affiliates.is_empty());
}

#[test]
fn test_deposit_with_affiliate() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    let info = mock_info(&addr("user1"), &coins(10_000, COLLATERAL));
    execute(
        deps.as_mut(), env, info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: membrane::ltv_disco::BackingDepositInput {
                asset: COLLATERAL.to_string(),
                slot: 90,
            },
            deposit_owner: None,
            deposit_id: None,
            manager: None,
            affiliate_address: Some(addr("affiliate1")),
            revenue_destination: None,
        },
    ).unwrap();

    // Verify affiliate was added
    let affiliates: Vec<membrane::types::AffiliateData> = from_json(query(
        deps.as_ref(), mock_env(),
        QueryMsg::GetAffiliates { user: addr("user1") },
    ).unwrap()).unwrap();
    assert!(!affiliates.is_empty());
}
