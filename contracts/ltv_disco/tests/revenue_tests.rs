mod test_helpers;

use cosmwasm_std::testing::{mock_env, mock_info};
use cosmwasm_std::{coins, from_json, Decimal, Uint128};
use membrane::ltv_disco::{ExecuteMsg, QueryMsg, RevenueEvent};
use ltv_disco::contract::{execute, query};
use test_helpers::*;

// =================== Slot Weights ===================

#[test]
fn test_slot_weights_single_slot_gets_all() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    let weights = query_weights(&deps, COLLATERAL);
    let non_zero: Vec<_> = weights.weights.iter().filter(|(_, w)| !w.is_zero()).collect();
    assert_eq!(non_zero.len(), 1);
    assert_eq!(non_zero[0].0, 90);
    assert_eq!(non_zero[0].1, Decimal::one()); // Only slot gets 100%
}

#[test]
fn test_slot_weights_riskier_slots_get_more() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    // Equal deposits in slots 90 (riskiest) and 50 (safest)
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    submit_deposit(&mut deps, &env, &addr("user2"), COLLATERAL, 50, 10_000);

    let weights = query_weights(&deps, COLLATERAL);
    let non_zero: Vec<_> = weights.weights.iter().filter(|(_, w)| !w.is_zero()).collect();
    assert_eq!(non_zero.len(), 2);

    // Slot 90 should have higher weight than slot 50
    let slot90_weight = weights.weights.iter().find(|(s, _)| *s == 90).unwrap().1;
    let slot50_weight = weights.weights.iter().find(|(s, _)| *s == 50).unwrap().1;
    assert!(slot90_weight > slot50_weight, "Slot 90 weight {} should be > slot 50 weight {}", slot90_weight, slot50_weight);
}

#[test]
fn test_slot_weights_three_slots() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 70, 10_000);
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 50, 10_000);

    let weights = query_weights(&deps, COLLATERAL);
    let non_zero: Vec<_> = weights.weights.iter().filter(|(_, w)| !w.is_zero()).collect();
    assert_eq!(non_zero.len(), 3);

    let w90 = weights.weights.iter().find(|(s, _)| *s == 90).unwrap().1;
    let w70 = weights.weights.iter().find(|(s, _)| *s == 70).unwrap().1;
    let w50 = weights.weights.iter().find(|(s, _)| *s == 50).unwrap().1;

    // Monotonically decreasing: slot 90 > slot 70 > slot 50
    assert!(w90 > w70, "w90={} > w70={}", w90, w70);
    assert!(w70 > w50, "w70={} > w50={}", w70, w50);

    // Weights should sum close to 1.0
    let total = w90 + w70 + w50;
    assert!(total <= Decimal::one());
    // Allow small rounding loss
    assert!(total > Decimal::percent(99));
}

#[test]
fn test_slot_weights_empty_queue() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let weights = query_weights(&deps, COLLATERAL);
    assert!(weights.weights.is_empty());
}

// =================== Add Revenue ===================

#[test]
fn test_add_revenue_distributes_to_single_slot() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    advance_time(&mut env, 100);
    add_revenue(&mut deps, &env, COLLATERAL, 1_000_000);

    // Check revenue event was created for slot 90
    let events: Vec<RevenueEvent> = from_json(query(
        deps.as_ref(), mock_env(),
        QueryMsg::GetRevenueEvents { asset: COLLATERAL.to_string(), slot: 90 },
    ).unwrap()).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].amount_to_be_claimed, Uint128::new(1_000_000));
}

#[test]
fn test_add_revenue_distributes_proportionally() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    submit_deposit(&mut deps, &env, &addr("user2"), COLLATERAL, 50, 10_000);

    advance_time(&mut env, 100);
    add_revenue(&mut deps, &env, COLLATERAL, 1_000_000);

    // Both slots should have events
    let events90: Vec<RevenueEvent> = from_json(query(
        deps.as_ref(), mock_env(),
        QueryMsg::GetRevenueEvents { asset: COLLATERAL.to_string(), slot: 90 },
    ).unwrap()).unwrap();
    let events50: Vec<RevenueEvent> = from_json(query(
        deps.as_ref(), mock_env(),
        QueryMsg::GetRevenueEvents { asset: COLLATERAL.to_string(), slot: 50 },
    ).unwrap()).unwrap();

    assert_eq!(events90.len(), 1);
    assert_eq!(events50.len(), 1);
    // Slot 90 gets more revenue
    assert!(events90[0].amount_to_be_claimed > events50[0].amount_to_be_claimed);

    // Total distributed should be close to total revenue
    let total = events90[0].amount_to_be_claimed + events50[0].amount_to_be_claimed;
    assert!(total <= Uint128::new(1_000_000));
    // Allow rounding floor loss
    assert!(total >= Uint128::new(990_000));
}

#[test]
fn test_add_revenue_wrong_denom_rejected() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    let info = mock_info(&addr("anyone"), &coins(1_000, "not_cdt"));
    let err = execute(
        deps.as_mut(), env, info,
        ExecuteMsg::AddRevenue { asset: COLLATERAL.to_string() },
    ).unwrap_err();
    assert!(err.to_string().contains("Invalid CDT"));
}

#[test]
fn test_add_revenue_no_deposits_rejected() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    let info = mock_info(&addr("anyone"), &coins(1_000, CDT));
    let err = execute(
        deps.as_mut(), env, info,
        ExecuteMsg::AddRevenue { asset: COLLATERAL.to_string() },
    ).unwrap_err();
    assert!(err.to_string().contains("No deposits"));
}

// =================== Claiming Revenue ===================

#[test]
fn test_pending_claims_calculation() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    advance_time(&mut env, 100);
    add_revenue(&mut deps, &env, COLLATERAL, 1_000_000);

    let claims = query_claims(&deps, &addr("user1"), COLLATERAL);
    assert_eq!(claims.claims.len(), 1);
    assert_eq!(claims.claims[0].slot, 90);
    assert_eq!(claims.claims[0].deposit_id, Uint128::zero());
    // Single depositor gets all revenue
    assert_eq!(claims.claims[0].pending_amount, Uint128::new(1_000_000));
}

#[test]
fn test_pending_claims_two_users_same_slot() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    submit_deposit(&mut deps, &env, &addr("user2"), COLLATERAL, 90, 10_000);

    advance_time(&mut env, 100);
    add_revenue(&mut deps, &env, COLLATERAL, 1_000_000);

    let claims1 = query_claims(&deps, &addr("user1"), COLLATERAL);
    let claims2 = query_claims(&deps, &addr("user2"), COLLATERAL);

    // Each gets half (both have 10k of 20k total vault tokens)
    assert_eq!(claims1.claims[0].pending_amount, Uint128::new(500_000));
    assert_eq!(claims2.claims[0].pending_amount, Uint128::new(500_000));
}

#[test]
fn test_claim_revenue_sends_cdt() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    advance_time(&mut env, 100);
    add_revenue(&mut deps, &env, COLLATERAL, 1_000_000);

    advance_time(&mut env, 10);

    let info = mock_info(&addr("user1"), &[]);
    let res = execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::ClaimRevenueForUser {
            user: addr("user1"),
            asset: COLLATERAL.to_string(),
            limit: None,
            compound_action: None,
        },
    ).unwrap();

    // Should have a bank send message
    assert!(!res.messages.is_empty(), "Expected bank send msg for revenue claim");

    // After claiming, pending should be zero
    let claims = query_claims(&deps, &addr("user1"), COLLATERAL);
    assert!(claims.claims.is_empty() || claims.claims[0].pending_amount.is_zero());
}

#[test]
fn test_deposit_after_revenue_does_not_claim_past() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // Add revenue
    advance_time(&mut env, 100);
    add_revenue(&mut deps, &env, COLLATERAL, 1_000_000);

    // New user deposits AFTER revenue was added
    advance_time(&mut env, 100);
    submit_deposit(&mut deps, &env, &addr("user2"), COLLATERAL, 90, 10_000);

    // user2 should have 0 pending (their last_claimed is set to deposit time)
    let claims2 = query_claims(&deps, &addr("user2"), COLLATERAL);
    assert!(claims2.claims.is_empty());

    // user1 still has pending
    let claims1 = query_claims(&deps, &addr("user1"), COLLATERAL);
    assert!(!claims1.claims.is_empty());
    assert!(claims1.claims[0].pending_amount > Uint128::zero());
}

#[test]
fn test_multiple_revenue_events_accumulated() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    advance_time(&mut env, 100);
    add_revenue(&mut deps, &env, COLLATERAL, 1_000_000);

    advance_time(&mut env, 100);
    add_revenue(&mut deps, &env, COLLATERAL, 2_000_000);

    let claims = query_claims(&deps, &addr("user1"), COLLATERAL);
    assert_eq!(claims.claims[0].pending_amount, Uint128::new(3_000_000));
}

// =================== Cumulative Revenue Query ===================

#[test]
fn test_cumulative_revenue_per_slot() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    advance_time(&mut env, 100);
    add_revenue(&mut deps, &env, COLLATERAL, 500_000);

    advance_time(&mut env, 100);
    add_revenue(&mut deps, &env, COLLATERAL, 500_000);

    let entries: Vec<membrane::ltv_disco::RevenueTrackingEntry> = from_json(query(
        deps.as_ref(), mock_env(),
        QueryMsg::GetCumulativeRevenue {
            asset: COLLATERAL.to_string(),
            slot: Some(90),
        },
    ).unwrap()).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].total_revenue, Uint128::new(500_000));
    assert_eq!(entries[1].total_revenue, Uint128::new(1_000_000)); // cumulative
}

// =================== Auto-Claim on Top-Up ===================

#[test]
fn test_top_up_auto_claims_revenue() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    advance_time(&mut env, 100);
    add_revenue(&mut deps, &env, COLLATERAL, 1_000_000);

    // Top up - should auto-claim
    advance_time(&mut env, 10);
    let info = mock_info(&addr("user1"), &coins(5_000, COLLATERAL));
    let res = execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: membrane::ltv_disco::BackingDepositInput {
                asset: COLLATERAL.to_string(),
                slot: 90,
            },
            deposit_owner: None,
            deposit_id: Some(Uint128::zero()),
            manager: None,
            affiliate_address: None,
            revenue_destination: None,
        },
    ).unwrap();

    // Should have a bank send for the auto-claimed revenue
    let has_bank_msg = res.messages.iter().any(|m| {
        matches!(m.msg, cosmwasm_std::CosmosMsg::Bank(_))
    });
    assert!(has_bank_msg, "Auto-claim should send bank msg");

    // Pending should now be zero
    let claims = query_claims(&deps, &addr("user1"), COLLATERAL);
    assert!(claims.claims.is_empty());
}
