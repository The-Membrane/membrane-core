#![allow(non_snake_case)]

use cosmwasm_std::testing::mock_dependencies;
use cosmwasm_std::{Addr, Decimal, Uint128};

use membrane::liquidation_engine::LiquidationTimer;

use crate::reply::handle_reply;
use crate::state::*;
use crate::testing::test_helpers::*;

// ============================================================================
// Helpers
// ============================================================================

/// Set up an insolvent single-asset scenario for delay testing.
///
/// collateral: 1_000_000 uatom, price=$10, decimals=6 -> value = $10
/// debt: varies, credit_price=$1, decimals=6
/// max_ltv: 80%, borrow_ltv: 60%
///
/// To control current_ltv, adjust the debt parameter:
///   debt=8_500_000 -> ltv=85% -> excess over 80% = 5% (under 20% threshold -> delay)
///   debt=9_000_000 -> ltv=90% -> excess over 80% = 10% (under 20% threshold -> delay)
///   debt=9_700_000 -> ltv=97% -> excess over 80% = 17% -> threshold=80%*1.2=96% -> 97%>96% -> immediate
fn setup_and_run_step2(
    deps: &mut cosmwasm_std::OwnedDeps<
        cosmwasm_std::MemoryStorage,
        cosmwasm_std::testing::MockApi,
        cosmwasm_std::testing::MockQuerier,
    >,
    debt: u128,
    time_secs: u64,
    liquidation_delay: u64,
    max_threshold_to_delay: Decimal,
) -> Result<cosmwasm_std::Response, crate::error::ContractError> {
    init_config_with_delay(
        deps.as_mut().storage,
        liquidation_delay,
    );

    let prop = make_propagation_post_debt_accrue(debt, Decimal::one(), 6);
    save_propagation(deps.as_mut().storage, &prop);

    let env = mock_env_at(time_secs);
    let reply = make_collateral_assess_success_reply_with_threshold(
        vec![make_casset("uatom", 1_000_000, 60, 80)],
        vec![Decimal::one()],
        vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)],
        Decimal::percent(80),
        Decimal::percent(60),
        max_threshold_to_delay,
        false,
        vec![],
    );

    handle_reply(deps.as_mut(), env, reply)
}

// ============================================================================
// Group A: Timer Start (first call on under-threshold insolvent position)
// ============================================================================

/// First liquidation call on an under-threshold position should:
/// - Start a timer (saved to LIQUIDATION_TIMERS)
/// - Return a response with "liquidation_timer_started" method
/// - NOT proceed to liq queue / sell wall (no SubMsgs for next steps)
#[test]
fn first_call_under_threshold_starts_timer() {
    let mut deps = mock_dependencies();

    // debt=8_500_000 -> ltv = 8.5/10 = 85%
    // max_ltv=80%, threshold=80%*1.2=96%
    // 85% < 96% -> under threshold -> should start timer
    let res = setup_and_run_step2(
        &mut deps,
        8_500_000,
        1000, // block time
        3600, // 1hr delay
        Decimal::percent(20),
    )
    .unwrap();

    // Timer should be saved
    let pos_id_str = Uint128::one().to_string();
    let timer = LIQUIDATION_TIMERS
        .load(deps.as_ref().storage, ("position_owner", pos_id_str.as_str()))
        .unwrap();
    assert_eq!(timer.start_time, 1000);
    assert_eq!(timer.caller, Addr::unchecked("liquidator"));

    // Response should indicate timer started (either via attributes or by returning early)
    // For the fee-extraction path: response goes to FEE_TRANSFER_REPLY_ID
    // For the no-fee path (very small fees): response has "liquidation_timer_started"
    // In either case, the propagation should have timer_start_only = true
    let prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();
    assert!(prop.timer_start_only, "timer_start_only should be true for timer-start call");
}

/// Timer-start call should zero out protocol fees (only caller fees paid).
#[test]
fn timer_start_zeroes_protocol_fee() {
    let mut deps = mock_dependencies();

    // 85% LTV, under threshold
    let _res = setup_and_run_step2(
        &mut deps,
        8_500_000,
        1000,
        3600,
        Decimal::percent(20),
    )
    .unwrap();

    let prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();
    // Protocol fee should be zero for timer-start calls
    assert_eq!(
        prop.protocol_fee_value,
        Decimal::zero(),
        "Protocol fee should be zero on timer-start"
    );
    // But caller fee should be > 0
    // caller_fee_rate = current_ltv - max_ltv = 0.85 - 0.80 = 0.05 (5%)
    assert!(
        prop.caller_fee_value_paid > Decimal::zero(),
        "Caller fee should be positive on timer-start"
    );
}

/// Verify the timer stores the correct caller address.
#[test]
fn timer_records_correct_caller() {
    let mut deps = mock_dependencies();

    let _res = setup_and_run_step2(
        &mut deps,
        8_500_000,
        2000,
        3600,
        Decimal::percent(20),
    )
    .unwrap();

    let pos_id_str = Uint128::one().to_string();
    let timer = LIQUIDATION_TIMERS
        .load(deps.as_ref().storage, ("position_owner", pos_id_str.as_str()))
        .unwrap();
    assert_eq!(timer.caller, Addr::unchecked("liquidator"));
    assert_eq!(timer.start_time, 2000);
}

// ============================================================================
// Group B: Timer Active (second call before timer expires)
// ============================================================================

/// Second call while timer is active should return LiquidationTimerActive error.
#[test]
fn second_call_before_timer_expires_errors() {
    let mut deps = mock_dependencies();

    // First call: start timer at t=1000
    let _res = setup_and_run_step2(
        &mut deps,
        8_500_000,
        1000,
        3600,
        Decimal::percent(20),
    )
    .unwrap();

    // Second call at t=2000 (only 1000s elapsed, timer is 3600s)
    // Need to re-save the propagation (it gets modified during step2)
    let prop = make_propagation_post_debt_accrue(8_500_000, Decimal::one(), 6);
    save_propagation(deps.as_mut().storage, &prop);

    let env = mock_env_at(2000);
    let reply = make_collateral_assess_success_reply_with_threshold(
        vec![make_casset("uatom", 1_000_000, 60, 80)],
        vec![Decimal::one()],
        vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)],
        Decimal::percent(80),
        Decimal::percent(60),
        Decimal::percent(20),
        false,
        vec![],
    );

    let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();
    let err_string = err.to_string();
    assert!(
        err_string.contains("Liquidation timer active"),
        "Expected LiquidationTimerActive error, got: {}",
        err_string
    );
    // Verify expiry is correct (start=1000 + delay=3600 = 4600)
    assert!(
        err_string.contains("4600"),
        "Expected expiry=4600 in error, got: {}",
        err_string
    );
}

/// Call 1 second before timer expires should still error.
#[test]
fn call_one_second_before_expiry_errors() {
    let mut deps = mock_dependencies();

    // Start timer at t=1000, delay=3600
    let _res = setup_and_run_step2(
        &mut deps,
        8_500_000,
        1000,
        3600,
        Decimal::percent(20),
    )
    .unwrap();

    // Call at t=4599 (expiry = 4600)
    let prop = make_propagation_post_debt_accrue(8_500_000, Decimal::one(), 6);
    save_propagation(deps.as_mut().storage, &prop);

    let env = mock_env_at(4599);
    let reply = make_collateral_assess_success_reply_with_threshold(
        vec![make_casset("uatom", 1_000_000, 60, 80)],
        vec![Decimal::one()],
        vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)],
        Decimal::percent(80),
        Decimal::percent(60),
        Decimal::percent(20),
        false,
        vec![],
    );

    let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();
    assert!(
        err.to_string().contains("Liquidation timer active"),
        "Should still be active 1s before expiry"
    );
}

// ============================================================================
// Group C: Timer Expired (second call after timer expires)
// ============================================================================

/// Call after timer expires should clear the timer and proceed with full liquidation.
#[test]
fn call_after_timer_expires_proceeds_with_liquidation() {
    let mut deps = mock_dependencies();

    // Start timer at t=1000, delay=3600 -> expires at 4600
    let _res = setup_and_run_step2(
        &mut deps,
        8_500_000,
        1000,
        3600,
        Decimal::percent(20),
    )
    .unwrap();

    // Call at t=4600 (exactly at expiry)
    let prop = make_propagation_post_debt_accrue(8_500_000, Decimal::one(), 6);
    save_propagation(deps.as_mut().storage, &prop);

    let env = mock_env_at(4600);
    let reply = make_collateral_assess_success_reply_with_threshold(
        vec![make_casset("uatom", 1_000_000, 60, 80)],
        vec![Decimal::one()],
        vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)],
        Decimal::percent(80),
        Decimal::percent(60),
        Decimal::percent(20),
        false,
        vec![],
    );

    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Timer should be cleared
    let pos_id_str = Uint128::one().to_string();
    let timer = LIQUIDATION_TIMERS
        .may_load(deps.as_ref().storage, ("position_owner", pos_id_str.as_str()))
        .unwrap();
    assert!(timer.is_none(), "Timer should be cleared after expiry");

    // Should proceed with full liquidation (timer_start_only = false)
    let prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();
    assert!(
        !prop.timer_start_only,
        "timer_start_only should be false for fulfillment call"
    );

    // Should have SubMsgs for the next step (fee extraction or LQ transfer)
    // or attributes indicating continuation of the liquidation chain
    assert!(
        !res.messages.is_empty() || res.attributes.iter().any(|a| a.key == "no_fees"),
        "Should proceed with liquidation chain"
    );
}

/// Call well after timer expires should also work.
#[test]
fn call_well_after_timer_expires_works() {
    let mut deps = mock_dependencies();

    // Start timer at t=1000, delay=3600 -> expires at 4600
    let _res = setup_and_run_step2(
        &mut deps,
        8_500_000,
        1000,
        3600,
        Decimal::percent(20),
    )
    .unwrap();

    // Call at t=100000 (way past expiry)
    let prop = make_propagation_post_debt_accrue(8_500_000, Decimal::one(), 6);
    save_propagation(deps.as_mut().storage, &prop);

    let env = mock_env_at(100_000);
    let reply = make_collateral_assess_success_reply_with_threshold(
        vec![make_casset("uatom", 1_000_000, 60, 80)],
        vec![Decimal::one()],
        vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)],
        Decimal::percent(80),
        Decimal::percent(60),
        Decimal::percent(20),
        false,
        vec![],
    );

    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Timer cleared, full liquidation
    let pos_id_str = Uint128::one().to_string();
    let timer = LIQUIDATION_TIMERS
        .may_load(deps.as_ref().storage, ("position_owner", pos_id_str.as_str()))
        .unwrap();
    assert!(timer.is_none());

    let prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();
    assert!(!prop.timer_start_only);
}

/// Full liquidation (after timer) should include protocol fees.
#[test]
fn fulfillment_call_includes_protocol_fee() {
    let mut deps = mock_dependencies();

    // Start timer at t=1000
    let _res = setup_and_run_step2(
        &mut deps,
        8_500_000,
        1000,
        3600,
        Decimal::percent(20),
    )
    .unwrap();

    // Fulfillment at t=5000 (past expiry)
    let prop = make_propagation_post_debt_accrue(8_500_000, Decimal::one(), 6);
    save_propagation(deps.as_mut().storage, &prop);

    let env = mock_env_at(5000);
    let reply = make_collateral_assess_success_reply_with_threshold(
        vec![make_casset("uatom", 1_000_000, 60, 80)],
        vec![Decimal::one()],
        vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)],
        Decimal::percent(80),
        Decimal::percent(60),
        Decimal::percent(20),
        false,
        vec![],
    );

    let _res = handle_reply(deps.as_mut(), env, reply).unwrap();

    let prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();
    // Protocol fee should be non-zero for actual liquidation
    assert!(
        prop.protocol_fee_value > Decimal::zero(),
        "Protocol fee should be positive on fulfillment call"
    );
}

// ============================================================================
// Group D: Over Threshold (immediate liquidation)
// ============================================================================

/// Position over the immediate threshold should liquidate immediately (no timer).
#[test]
fn over_threshold_liquidates_immediately() {
    let mut deps = mock_dependencies();

    // debt=9_700_000 -> ltv = 9.7/10 = 97%
    // max_ltv=80%, threshold=80%*1.2=96%
    // 97% > 96% -> over threshold -> immediate liquidation
    let res = setup_and_run_step2(
        &mut deps,
        9_700_000,
        1000,
        3600,
        Decimal::percent(20),
    )
    .unwrap();

    // No timer should be saved
    let pos_id_str = Uint128::one().to_string();
    let timer = LIQUIDATION_TIMERS
        .may_load(deps.as_ref().storage, ("position_owner", pos_id_str.as_str()))
        .unwrap();
    assert!(timer.is_none(), "No timer should exist for over-threshold position");

    // Should proceed with full liquidation
    let prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();
    assert!(
        !prop.timer_start_only,
        "timer_start_only should be false for immediate liquidation"
    );

    // Should have SubMsgs or attributes for liquidation continuation
    assert!(
        !res.messages.is_empty() || res.attributes.iter().any(|a| a.key == "no_fees"),
        "Should proceed with liquidation chain"
    );
}

/// Position exactly at threshold boundary should still get delayed.
#[test]
fn at_exact_threshold_gets_delayed() {
    let mut deps = mock_dependencies();

    // threshold = 80% * 1.2 = 96%
    // debt=9_600_000 -> ltv = 9.6/10 = 96%
    // 96% is NOT > 96% (not strictly greater), so delay should apply
    let _res = setup_and_run_step2(
        &mut deps,
        9_600_000,
        1000,
        3600,
        Decimal::percent(20),
    )
    .unwrap();

    // Timer should be saved (at the boundary, delay applies)
    let pos_id_str = Uint128::one().to_string();
    let timer = LIQUIDATION_TIMERS
        .may_load(deps.as_ref().storage, ("position_owner", pos_id_str.as_str()))
        .unwrap();
    assert!(timer.is_some(), "Timer should exist at exact threshold boundary");

    let prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();
    assert!(prop.timer_start_only);
}

/// Over-threshold should clear any existing timer and proceed immediately.
#[test]
fn over_threshold_clears_existing_timer() {
    let mut deps = mock_dependencies();

    // First: start timer at t=1000 (85% LTV, under threshold)
    let _res = setup_and_run_step2(
        &mut deps,
        8_500_000,
        1000,
        3600,
        Decimal::percent(20),
    )
    .unwrap();

    // Verify timer exists
    let pos_id_str = Uint128::one().to_string();
    let timer = LIQUIDATION_TIMERS
        .may_load(deps.as_ref().storage, ("position_owner", pos_id_str.as_str()))
        .unwrap();
    assert!(timer.is_some());

    // Second: position got worse, now at 97% LTV (over threshold)
    let prop = make_propagation_post_debt_accrue(9_700_000, Decimal::one(), 6);
    save_propagation(deps.as_mut().storage, &prop);

    let env = mock_env_at(2000);
    let reply = make_collateral_assess_success_reply_with_threshold(
        vec![make_casset("uatom", 1_000_000, 60, 80)],
        vec![Decimal::one()],
        vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)],
        Decimal::percent(80),
        Decimal::percent(60),
        Decimal::percent(20),
        false,
        vec![],
    );

    let _res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Timer should be cleared
    let timer = LIQUIDATION_TIMERS
        .may_load(deps.as_ref().storage, ("position_owner", pos_id_str.as_str()))
        .unwrap();
    assert!(timer.is_none(), "Timer should be cleared when over threshold");

    // Should proceed with immediate liquidation
    let prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();
    assert!(!prop.timer_start_only);
}

// ============================================================================
// Group E: Config Variations
// ============================================================================

/// With liquidation_delay=0, positions should always liquidate immediately
/// (timer starts and immediately expires, effectively no delay).
#[test]
fn zero_delay_allows_immediate_liquidation() {
    let mut deps = mock_dependencies();

    // delay=0 means timer expires immediately
    let _res = setup_and_run_step2(
        &mut deps,
        8_500_000,
        1000,
        0,    // zero delay
        Decimal::percent(20),
    )
    .unwrap();

    // Timer is saved with start_time=1000 and delay=0, so expiry=1000
    // Since we're at t=1000 and expiry=1000, env.block.time >= expiry
    // But actually, the timer was just created in THIS call - it won't be
    // checked until the NEXT call. The first call still starts the timer.
    // So with delay=0, the first call starts a timer, and ANY subsequent
    // call will find it expired and proceed.
    let pos_id_str = Uint128::one().to_string();
    let timer = LIQUIDATION_TIMERS
        .may_load(deps.as_ref().storage, ("position_owner", pos_id_str.as_str()))
        .unwrap();
    assert!(timer.is_some(), "Timer is still created even with delay=0");

    // Now a second call at the same time should find the timer expired
    let prop = make_propagation_post_debt_accrue(8_500_000, Decimal::one(), 6);
    save_propagation(deps.as_mut().storage, &prop);

    let env = mock_env_at(1000);
    let reply = make_collateral_assess_success_reply_with_threshold(
        vec![make_casset("uatom", 1_000_000, 60, 80)],
        vec![Decimal::one()],
        vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)],
        Decimal::percent(80),
        Decimal::percent(60),
        Decimal::percent(20),
        false,
        vec![],
    );

    let _res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Timer cleared, full liquidation
    let timer = LIQUIDATION_TIMERS
        .may_load(deps.as_ref().storage, ("position_owner", pos_id_str.as_str()))
        .unwrap();
    assert!(timer.is_none());
}

/// With max_threshold_to_delay=0%, every insolvent position should be liquidated immediately.
#[test]
fn zero_threshold_means_always_immediate() {
    let mut deps = mock_dependencies();

    // threshold = 80% * (1 + 0%) = 80%
    // ltv = 85% > 80% -> over threshold -> immediate
    let _res = setup_and_run_step2(
        &mut deps,
        8_500_000,
        1000,
        3600,
        Decimal::zero(), // 0% threshold
    )
    .unwrap();

    // No timer (immediate liquidation)
    let pos_id_str = Uint128::one().to_string();
    let timer = LIQUIDATION_TIMERS
        .may_load(deps.as_ref().storage, ("position_owner", pos_id_str.as_str()))
        .unwrap();
    assert!(timer.is_none(), "No timer with 0% threshold");

    let prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();
    assert!(!prop.timer_start_only);
}

/// With max_threshold_to_delay=100%, only positions over 2*LLTV get immediate liquidation.
#[test]
fn large_threshold_delays_most_positions() {
    let mut deps = mock_dependencies();

    // threshold = 80% * (1 + 100%) = 160%
    // ltv = 97% < 160% -> under threshold -> delay
    let _res = setup_and_run_step2(
        &mut deps,
        9_700_000,
        1000,
        3600,
        Decimal::percent(100), // 100% threshold
    )
    .unwrap();

    // Timer should exist (under very large threshold)
    let pos_id_str = Uint128::one().to_string();
    let timer = LIQUIDATION_TIMERS
        .may_load(deps.as_ref().storage, ("position_owner", pos_id_str.as_str()))
        .unwrap();
    assert!(timer.is_some(), "Timer should exist with 100% threshold");
}

// ============================================================================
// Group F: Per-Position Isolation
// ============================================================================

/// Timers for different positions should be independent.
#[test]
fn different_positions_have_independent_timers() {
    let mut deps = mock_dependencies();

    // Start timer for position 1
    init_config_with_delay(
        deps.as_mut().storage,
        3600,
    );

    let mut prop = make_propagation_post_debt_accrue(8_500_000, Decimal::one(), 6);
    prop.position_id = Uint128::new(1);
    prop.position_owner = Addr::unchecked("owner_a");
    save_propagation(deps.as_mut().storage, &prop);

    let env = mock_env_at(1000);
    let reply = make_collateral_assess_success_reply_with_threshold(
        vec![make_casset("uatom", 1_000_000, 60, 80)],
        vec![Decimal::one()],
        vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)],
        Decimal::percent(80),
        Decimal::percent(60),
        Decimal::percent(20),
        false,
        vec![],
    );
    let _res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Start timer for position 2 (different owner)
    let mut prop2 = make_propagation_post_debt_accrue(8_500_000, Decimal::one(), 6);
    prop2.position_id = Uint128::new(2);
    prop2.position_owner = Addr::unchecked("owner_b");
    save_propagation(deps.as_mut().storage, &prop2);

    let env2 = mock_env_at(2000);
    let reply2 = make_collateral_assess_success_reply_with_threshold(
        vec![make_casset("uatom", 1_000_000, 60, 80)],
        vec![Decimal::one()],
        vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)],
        Decimal::percent(80),
        Decimal::percent(60),
        Decimal::percent(20),
        false,
        vec![],
    );
    let _res2 = handle_reply(deps.as_mut(), env2, reply2).unwrap();

    // Both timers should exist independently
    let timer1 = LIQUIDATION_TIMERS
        .may_load(deps.as_ref().storage, ("owner_a", "1"))
        .unwrap();
    let timer2 = LIQUIDATION_TIMERS
        .may_load(deps.as_ref().storage, ("owner_b", "2"))
        .unwrap();

    assert!(timer1.is_some(), "Timer for position 1 should exist");
    assert!(timer2.is_some(), "Timer for position 2 should exist");
    assert_eq!(timer1.unwrap().start_time, 1000);
    assert_eq!(timer2.unwrap().start_time, 2000);
}

/// Same owner, different position IDs should have independent timers.
#[test]
fn same_owner_different_positions_independent() {
    let mut deps = mock_dependencies();

    init_config_with_delay(
        deps.as_mut().storage,
        3600,
    );

    // Position 1 for owner_a
    let mut prop = make_propagation_post_debt_accrue(8_500_000, Decimal::one(), 6);
    prop.position_id = Uint128::new(1);
    prop.position_owner = Addr::unchecked("owner_a");
    save_propagation(deps.as_mut().storage, &prop);

    let env = mock_env_at(1000);
    let reply = make_collateral_assess_success_reply_with_threshold(
        vec![make_casset("uatom", 1_000_000, 60, 80)],
        vec![Decimal::one()],
        vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)],
        Decimal::percent(80),
        Decimal::percent(60),
        Decimal::percent(20),
        false,
        vec![],
    );
    let _res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Position 5 for same owner_a (no timer yet)
    let mut prop2 = make_propagation_post_debt_accrue(8_500_000, Decimal::one(), 6);
    prop2.position_id = Uint128::new(5);
    prop2.position_owner = Addr::unchecked("owner_a");
    save_propagation(deps.as_mut().storage, &prop2);

    let env2 = mock_env_at(1500);
    let reply2 = make_collateral_assess_success_reply_with_threshold(
        vec![make_casset("uatom", 1_000_000, 60, 80)],
        vec![Decimal::one()],
        vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)],
        Decimal::percent(80),
        Decimal::percent(60),
        Decimal::percent(20),
        false,
        vec![],
    );
    let _res2 = handle_reply(deps.as_mut(), env2, reply2).unwrap();

    // Both should have timers
    let timer1 = LIQUIDATION_TIMERS
        .may_load(deps.as_ref().storage, ("owner_a", "1"))
        .unwrap();
    let timer5 = LIQUIDATION_TIMERS
        .may_load(deps.as_ref().storage, ("owner_a", "5"))
        .unwrap();

    assert!(timer1.is_some());
    assert!(timer5.is_some());
    assert_eq!(timer1.unwrap().start_time, 1000);
    assert_eq!(timer5.unwrap().start_time, 1500);
}

// ============================================================================
// Group G: Step 9 (Fee Transfer Reply) Timer Early Return
// ============================================================================

/// When timer_start_only is true in propagation, Step 9 should return early
/// after sending caller fees (no SubMsgs to continue the chain).
#[test]
fn step9_returns_early_for_timer_start() {
    let mut deps = mock_dependencies();

    init_config_with_delay(
        deps.as_mut().storage,
        3600,
    );

    // Set up propagation as if Step 2 set timer_start_only = true
    let mut prop = default_propagation();
    prop.timer_start_only = true;
    prop.caller_fee_assets = vec![cosmwasm_std::Coin {
        denom: "uatom".to_string(),
        amount: Uint128::new(1000),
    }];
    prop.caller_fee_value_paid = Decimal::from_ratio(10u128, 1u128);
    save_propagation(deps.as_mut().storage, &prop);

    let env = mock_env_at(1000);
    let reply = make_success_reply(FEE_TRANSFER_REPLY_ID, vec![]);

    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Should have BankMsg::Send for caller fees but NO SubMsgs for next step
    assert!(
        res.messages.iter().all(|m| {
            matches!(m.msg, cosmwasm_std::CosmosMsg::Bank(_))
        }),
        "Should only have Bank messages (caller fees), no Wasm SubMsgs for next step"
    );

    // Should have timer-started attribute
    assert!(
        res.attributes.iter().any(|a| a.key == "method" && a.value == "liquidation_timer_started"),
        "Should have liquidation_timer_started method attribute"
    );
}

/// When timer_start_only is false, Step 9 should proceed normally to next step.
#[test]
fn step9_proceeds_normally_when_not_timer_start() {
    let mut deps = mock_dependencies();

    init_config_with_delay(
        deps.as_mut().storage,
        3600,
    );

    // Propagation with timer_start_only = false (normal liquidation)
    let mut prop = make_propagation_post_assess(
        8_500_000,
        Decimal::one(),
        6,
        vec![make_casset("uatom", 950_000, 60, 80)], // fee-deducted
        vec![Decimal::one()],
        vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)],
        vec![],
        Decimal::from_ratio(1u128, 1u128),
        Decimal::from_ratio(1u128, 10u128),
        Decimal::zero(),
    );
    prop.timer_start_only = false;
    prop.caller_fee_assets = vec![cosmwasm_std::Coin {
        denom: "uatom".to_string(),
        amount: Uint128::new(1000),
    }];
    save_propagation(deps.as_mut().storage, &prop);

    let env = mock_env_at(1000);
    let reply = make_success_reply(FEE_TRANSFER_REPLY_ID, vec![]);

    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Should have the fee_transfer_reply_success method (not timer_started)
    assert!(
        res.attributes.iter().any(|a| a.key == "method" && a.value == "fee_transfer_reply_success"),
        "Should have fee_transfer_reply_success method for normal liquidation"
    );
}

// ============================================================================
// Group H: Query Tests
// ============================================================================

/// GetLiquidationTimer query returns timer info when timer exists.
#[test]
fn query_timer_returns_existing_timer() {
    let mut deps = mock_dependencies();

    init_config_with_delay(
        deps.as_mut().storage,
        3600,
    );

    // Manually save a timer
    LIQUIDATION_TIMERS
        .save(
            deps.as_mut().storage,
            ("owner_a", "1"),
            &LiquidationTimer {
                start_time: 5000,
                caller: Addr::unchecked("liquidator"),
            },
        )
        .unwrap();

    let res = crate::query::query_liquidation_timer(
        deps.as_ref(),
        Uint128::new(1),
        "owner_a".to_string(),
    )
    .unwrap();

    let response: membrane::liquidation_engine::LiquidationTimerResponse =
        cosmwasm_std::from_json(res).unwrap();
    assert!(response.has_timer);
    assert_eq!(response.start_time, Some(5000));
    assert_eq!(response.expiry_time, Some(5000 + 3600));
    assert_eq!(response.caller, Some("liquidator".to_string()));
}

/// GetLiquidationTimer query returns empty when no timer exists.
#[test]
fn query_timer_returns_none_when_no_timer() {
    let mut deps = mock_dependencies();

    init_config_with_delay(
        deps.as_mut().storage,
        3600,
    );

    let res = crate::query::query_liquidation_timer(
        deps.as_ref(),
        Uint128::new(99),
        "nonexistent_owner".to_string(),
    )
    .unwrap();

    let response: membrane::liquidation_engine::LiquidationTimerResponse =
        cosmwasm_std::from_json(res).unwrap();
    assert!(!response.has_timer);
    assert_eq!(response.start_time, None);
    assert_eq!(response.expiry_time, None);
    assert_eq!(response.caller, None);
}

// ============================================================================
// Group I: Config Update Tests
// ============================================================================

/// UpdateConfig can modify liquidation_delay.
#[test]
fn update_config_changes_delay() {
    let mut deps = mock_dependencies();

    init_config_with_delay(
        deps.as_mut().storage,
        3600,
    );

    let info = cosmwasm_std::testing::message_info(&Addr::unchecked("admin"), &[]);
    let update = membrane::liquidation_engine::UpdateConfig {
        owner: None,
        collateral_contract: None,
        debt_contract: None,
        liq_queue: None,
        chain_proxy: None,
        debt_auction: None,
        staking_contract: None,
        liq_fee: None,
        credit_denom: None,
        liquidation_delay: Some(7200),          // Change to 2hr
    };

    crate::contract::execute(
        deps.as_mut(),
        cosmwasm_std::testing::mock_env(),
        info,
        membrane::liquidation_engine::ExecuteMsg::UpdateConfig(update),
    )
    .unwrap();

    let config = CONFIG.load(deps.as_ref().storage).unwrap();
    assert_eq!(config.liquidation_delay, 7200);
}

/// Instantiate with None delay/threshold uses defaults.
#[test]
fn instantiate_defaults() {
    let mut deps = mock_dependencies();
    let env = cosmwasm_std::testing::mock_env();
    let info = cosmwasm_std::testing::message_info(&Addr::unchecked("admin"), &[]);

    let valid_collateral = deps.api.addr_make("collateral_contract");
    let valid_debt = deps.api.addr_make("debt_contract");

    let msg = membrane::liquidation_engine::InstantiateMsg {
        owner: None,
        collateral_contract: valid_collateral.to_string(),
        debt_contract: valid_debt.to_string(),
        liq_queue: None,
        chain_proxy: None,
        debt_auction: None,
        staking_contract: None,
        liq_fee: Decimal::percent(1),
        credit_denom: None,
        liquidation_delay: None,          // Should default to 28800
    };

    crate::contract::instantiate(deps.as_mut(), env, info, msg).unwrap();

    let config = CONFIG.load(deps.as_ref().storage).unwrap();
    assert_eq!(config.liquidation_delay, 28800);
}

/// Instantiate with explicit values uses those values.
#[test]
fn instantiate_with_custom_delay() {
    let mut deps = mock_dependencies();
    let env = cosmwasm_std::testing::mock_env();
    let info = cosmwasm_std::testing::message_info(&Addr::unchecked("admin"), &[]);

    let valid_collateral = deps.api.addr_make("collateral_contract");
    let valid_debt = deps.api.addr_make("debt_contract");

    let msg = membrane::liquidation_engine::InstantiateMsg {
        owner: None,
        collateral_contract: valid_collateral.to_string(),
        debt_contract: valid_debt.to_string(),
        liq_queue: None,
        chain_proxy: None,
        debt_auction: None,
        staking_contract: None,
        liq_fee: Decimal::percent(2),
        credit_denom: None,
        liquidation_delay: Some(1800),          // 30 min
    };

    crate::contract::instantiate(deps.as_mut(), env, info, msg).unwrap();

    let config = CONFIG.load(deps.as_ref().storage).unwrap();
    assert_eq!(config.liquidation_delay, 1800);
}

// ============================================================================
// Group J: Migration
// ============================================================================

/// Migration is a no-op (no fields to migrate).
#[test]
fn migrate_is_noop() {
    let mut deps = mock_dependencies();
    let env = cosmwasm_std::testing::mock_env();

    init_config_with_delay(
        deps.as_mut().storage,
        3600,
    );

    crate::contract::migrate(
        deps.as_mut(),
        env,
        membrane::liquidation_engine::MigrateMsg {},
    )
    .unwrap();

    let config = CONFIG.load(deps.as_ref().storage).unwrap();
    assert_eq!(config.liquidation_delay, 3600, "Should preserve existing delay");
}

/// Migration preserves non-zero values.
#[test]
fn migrate_preserves_existing_values() {
    let mut deps = mock_dependencies();
    let env = cosmwasm_std::testing::mock_env();

    init_config_with_delay(
        deps.as_mut().storage,
        7200,
    );

    crate::contract::migrate(
        deps.as_mut(),
        env,
        membrane::liquidation_engine::MigrateMsg {},
    )
    .unwrap();

    let config = CONFIG.load(deps.as_ref().storage).unwrap();
    assert_eq!(config.liquidation_delay, 7200, "Should preserve existing delay");
}

// ============================================================================
// Group K: Propagation Initialization
// ============================================================================

/// liquidate() initializes timer_start_only to false.
#[test]
fn liquidate_initializes_timer_start_only_false() {
    let mut deps = mock_dependencies();
    let env = cosmwasm_std::testing::mock_env();

    init_config_with_delay(
        deps.as_mut().storage,
        3600,
    );

    let valid_owner = deps.api.addr_make("position_owner");
    let info = cosmwasm_std::testing::message_info(&Addr::unchecked("liquidator"), &[]);

    let _res = crate::liquidations::liquidate(
        deps.as_mut(),
        env,
        info,
        Uint128::one(),
        valid_owner.to_string(),
    )
    .unwrap();

    let prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();
    assert!(!prop.timer_start_only, "timer_start_only should be false at initialization");
}

// ============================================================================
// Group L: Per-Collateral Threshold Tests
// ============================================================================

/// Low per-collateral threshold (5%) means immediate liquidation at lower LTV.
/// Position: 1_000_000 uatom, price=$10 -> value=$10, max_ltv=80%, threshold=5%
/// Immediate threshold = 80% * 1.05 = 84%
/// debt=8_500_000 -> ltv=85% > 84% -> immediate liquidation (no timer)
#[test]
fn low_per_collateral_threshold_triggers_immediate_liquidation() {
    let mut deps = mock_dependencies();

    init_config_with_delay(
        deps.as_mut().storage,
        3600,
    );

    let prop = make_propagation_post_debt_accrue(8_500_000, Decimal::one(), 6);
    save_propagation(deps.as_mut().storage, &prop);

    let env = mock_env_at(1000);
    // Use 5% threshold instead of default 15%
    let reply = make_collateral_assess_success_reply_with_threshold(
        vec![make_casset_with_threshold("uatom", 1_000_000, 60, 80, 5)],
        vec![Decimal::one()],
        vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)],
        Decimal::percent(80),
        Decimal::percent(60),
        Decimal::percent(5), // avg threshold = 5%
        false,
        vec![],
    );

    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Should NOT start a timer — 85% > 84% immediate threshold
    let timer = LIQUIDATION_TIMERS
        .may_load(deps.as_ref().storage, ("position_owner", "1"))
        .unwrap();
    assert!(timer.is_none(), "No timer should exist — immediate liquidation");

    // Should proceed to full liquidation (not timer_start_only)
    let prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();
    assert!(!prop.timer_start_only, "Should be full liquidation, not timer start");
}

/// High per-collateral threshold (30%) gives delay at the same LTV.
/// Position: 1_000_000 uatom, price=$10 -> value=$10, max_ltv=80%, threshold=30%
/// Immediate threshold = 80% * 1.30 = 104%
/// debt=8_500_000 -> ltv=85% < 104% -> timer started (delay)
#[test]
fn high_per_collateral_threshold_gives_delay() {
    let mut deps = mock_dependencies();

    init_config_with_delay(
        deps.as_mut().storage,
        3600,
    );

    let prop = make_propagation_post_debt_accrue(8_500_000, Decimal::one(), 6);
    save_propagation(deps.as_mut().storage, &prop);

    let env = mock_env_at(1000);
    // Use 30% threshold
    let reply = make_collateral_assess_success_reply_with_threshold(
        vec![make_casset_with_threshold("uatom", 1_000_000, 60, 80, 30)],
        vec![Decimal::one()],
        vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)],
        Decimal::percent(80),
        Decimal::percent(60),
        Decimal::percent(30), // avg threshold = 30%
        false,
        vec![],
    );

    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Timer should be started — 85% < 104% threshold
    let timer = LIQUIDATION_TIMERS
        .may_load(deps.as_ref().storage, ("position_owner", "1"))
        .unwrap();
    assert!(timer.is_some(), "Timer should exist — under immediate threshold");
    assert_eq!(timer.unwrap().start_time, 1000);
}

/// Mixed collateral: weighted average threshold determines behavior.
/// Asset A: 50% of value, threshold=5%
/// Asset B: 50% of value, threshold=25%
/// Weighted avg threshold = 50%*5% + 50%*25% = 15%
/// max_ltv=80%, immediate threshold = 80% * 1.15 = 92%
/// debt=9_000_000 -> ltv=90% < 92% -> timer started
/// debt=9_500_000 -> ltv=95% > 92% -> immediate
#[test]
fn mixed_collateral_weighted_threshold_under() {
    let mut deps = mock_dependencies();

    init_config_with_delay(
        deps.as_mut().storage,
        3600,
    );

    // ltv = 90%, threshold = 15%, immediate at 92% -> 90% < 92% -> delay
    let prop = make_propagation_post_debt_accrue(9_000_000, Decimal::one(), 6);
    save_propagation(deps.as_mut().storage, &prop);

    let env = mock_env_at(1000);
    let reply = make_collateral_assess_success_reply_with_threshold(
        vec![
            make_casset_with_threshold("uatom", 500_000, 60, 80, 5),
            make_casset_with_threshold("uosmo", 500_000, 60, 80, 25),
        ],
        vec![Decimal::percent(50), Decimal::percent(50)],
        vec![
            make_price(Decimal::from_ratio(10u128, 1u128), 6),
            make_price(Decimal::from_ratio(10u128, 1u128), 6),
        ],
        Decimal::percent(80),
        Decimal::percent(60),
        Decimal::percent(15), // weighted avg: 50%*5% + 50%*25% = 15%
        false,
        vec![],
    );

    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    let timer = LIQUIDATION_TIMERS
        .may_load(deps.as_ref().storage, ("position_owner", "1"))
        .unwrap();
    assert!(timer.is_some(), "Timer should exist — 90% < 92% immediate threshold");
}

/// Same mixed collateral but higher LTV exceeds weighted threshold -> immediate.
#[test]
fn mixed_collateral_weighted_threshold_over() {
    let mut deps = mock_dependencies();

    init_config_with_delay(
        deps.as_mut().storage,
        3600,
    );

    // ltv = 95%, threshold = 15%, immediate at 92% -> 95% > 92% -> immediate
    let prop = make_propagation_post_debt_accrue(9_500_000, Decimal::one(), 6);
    save_propagation(deps.as_mut().storage, &prop);

    let env = mock_env_at(1000);
    let reply = make_collateral_assess_success_reply_with_threshold(
        vec![
            make_casset_with_threshold("uatom", 500_000, 60, 80, 5),
            make_casset_with_threshold("uosmo", 500_000, 60, 80, 25),
        ],
        vec![Decimal::percent(50), Decimal::percent(50)],
        vec![
            make_price(Decimal::from_ratio(10u128, 1u128), 6),
            make_price(Decimal::from_ratio(10u128, 1u128), 6),
        ],
        Decimal::percent(80),
        Decimal::percent(60),
        Decimal::percent(15), // weighted avg: 50%*5% + 50%*25% = 15%
        false,
        vec![],
    );

    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    let timer = LIQUIDATION_TIMERS
        .may_load(deps.as_ref().storage, ("position_owner", "1"))
        .unwrap();
    assert!(timer.is_none(), "No timer — 95% > 92% immediate threshold");
}

/// Full delay flow with per-collateral threshold:
/// 1. Start timer (under threshold)
/// 2. Advance past delay
/// 3. Full liquidation proceeds
#[test]
fn per_collateral_threshold_full_delay_flow() {
    let mut deps = mock_dependencies();

    // Step 1: Start timer at t=1000
    let res = setup_and_run_step2(
        &mut deps,
        8_500_000,  // ltv=85%, with 20% threshold -> immediate at 96% -> 85%<96% -> delay
        1000,
        3600,
        Decimal::percent(20),
    );
    assert!(res.is_ok());

    let timer = LIQUIDATION_TIMERS
        .may_load(deps.as_ref().storage, ("position_owner", "1"))
        .unwrap();
    assert!(timer.is_some(), "Timer should be started");
    assert_eq!(timer.unwrap().start_time, 1000);

    // Step 2: Try before expiry -> should error
    let res = setup_and_run_step2(
        &mut deps,
        8_500_000,
        2000,   // only 1000s elapsed, need 3600
        3600,
        Decimal::percent(20),
    );
    assert!(res.is_err());

    // Step 3: Try after expiry (t=1000+3600=4600) -> full liquidation
    let res = setup_and_run_step2(
        &mut deps,
        8_500_000,
        4700,   // 3700s elapsed > 3600 delay
        3600,
        Decimal::percent(20),
    );
    assert!(res.is_ok());

    // Timer should be cleared after successful liquidation
    let timer = LIQUIDATION_TIMERS
        .may_load(deps.as_ref().storage, ("position_owner", "1"))
        .unwrap();
    assert!(timer.is_none(), "Timer should be cleared after liquidation");
}

/// Position over per-collateral threshold gets immediate liquidation (no timer).
#[test]
fn per_collateral_threshold_immediate_when_over() {
    let mut deps = mock_dependencies();

    // threshold=10%, max_ltv=80% -> immediate at 88%
    // debt=9_000_000 -> ltv=90% > 88% -> immediate
    let res = setup_and_run_step2(
        &mut deps,
        9_000_000,
        1000,
        3600,
        Decimal::percent(10),
    );
    assert!(res.is_ok());

    let timer = LIQUIDATION_TIMERS
        .may_load(deps.as_ref().storage, ("position_owner", "1"))
        .unwrap();
    assert!(timer.is_none(), "No timer — immediate liquidation above threshold");
}
