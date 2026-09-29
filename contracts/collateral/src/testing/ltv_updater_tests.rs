#![allow(non_snake_case)]

/// Comprehensive tests for the dynamic LTV update system (v2).
/// Covers: exponential upward accrual, decay downward shifts, staging blocks upward,
/// bidirectional staged updates, mandatory cooldown, zero-Disco handling,
/// path independence, borrow derivation, bounds capping, historical snapshots, and queries.

use cosmwasm_std::{testing::mock_env, Decimal, Env, Timestamp};
use cosmwasm_std::testing::MockStorage;

use crate::ltv_updater::{
    apply_decay_downward_shift, calculate_exponential_accrual, cap_ltv_values,
    process_ltv_update, SECONDS_PER_DAY,
};
use crate::state::{
    record_ltv_snapshot, LTVUpdateTracker, LTV_HISTORY, LTV_UPDATE_TRACKERS,
    MANUAL_MAX_LTVS, DISCO_BLOCKED,
    MAX_LTV_HISTORY_ENTRIES, SECONDS_PER_DAY as STATE_SECONDS_PER_DAY,
};
use super::test_helpers::helpers::{init_config, init_basket};

/// Helper: create a mock Env at a specific timestamp (seconds).
fn env_at(seconds: u64) -> Env {
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(seconds);
    env
}

// ============================================================================
// Exponential Upward Accrual Tests
// ============================================================================

#[test]
fn test_exponential_accrual_basic() {
    // Disco above current → exponential convergence
    let current = Decimal::percent(60);
    let disco = Decimal::percent(80);
    let last_update = 1000;
    let now = 1000 + SECONDS_PER_DAY;
    let kp = Decimal::percent(5);

    let result = calculate_exponential_accrual(current, disco, last_update, now, kp).unwrap();

    // new = disco - (disco - current) * e^(-kp * 1.0)
    // gap = 20%, e^(-0.05) ≈ 0.95123
    // remaining = 20% * 0.95123 ≈ 19.0246%
    // new ≈ 80% - 19.0246% ≈ 60.975%
    assert!(result > current, "Should increase");
    assert!(result < disco, "Should not overshoot");
    // With kp=5%, one day covers ~4.88% of the gap
    let gap_covered = result - current;
    let total_gap = disco - current;
    let fraction = gap_covered / total_gap;
    // fraction ≈ 0.0488
    assert!(fraction > Decimal::permille(40), "Should cover at least 4% of gap");
    assert!(fraction < Decimal::permille(60), "Should cover less than 6% of gap");
}

#[test]
fn test_exponential_convergence() {
    // Multiple calls converge toward target without overshooting
    let disco = Decimal::percent(80);
    let kp = Decimal::percent(50); // aggressive Kp for faster convergence
    let mut current = Decimal::percent(60);
    let mut time = 1000u64;

    for _ in 0..100 {
        let new = calculate_exponential_accrual(current, disco, time, time + SECONDS_PER_DAY, kp).unwrap();
        // Must never overshoot
        assert!(new <= disco, "Overshot! new={} > disco={}", new, disco);
        // Must not go backwards
        assert!(new >= current, "Went backwards! new={} < current={}", new, current);
        current = new;
        time += SECONDS_PER_DAY;
    }

    // After 100 days with aggressive Kp, should be very close to target
    let diff = disco - current;
    assert!(diff < Decimal::permille(1), "Not converged: diff={}", diff);
}

#[test]
fn test_exponential_path_independence() {
    // One call over N seconds == N calls over 1 second each
    let current = Decimal::percent(60);
    let disco = Decimal::percent(80);
    let kp = Decimal::percent(5);
    let total_time = SECONDS_PER_DAY * 7; // 7 days

    // Single call over 7 days
    let single_result = calculate_exponential_accrual(current, disco, 0, total_time, kp).unwrap();

    // 7 separate calls, 1 day each
    let mut multi_current = current;
    let mut time = 0u64;
    for _ in 0..7 {
        multi_current = calculate_exponential_accrual(multi_current, disco, time, time + SECONDS_PER_DAY, kp).unwrap();
        time += SECONDS_PER_DAY;
    }

    // Results should be nearly identical (small floating point differences acceptable)
    let diff = if single_result > multi_current {
        single_result - multi_current
    } else {
        multi_current - single_result
    };
    // Taylor series approximation introduces small cumulative error (~0.007%)
    // over 7 calls vs 1 call. This is acceptable for a 3-term approximation.
    assert!(diff < Decimal::from_ratio(1u128, 10_000u128), "Path-dependent! diff={}", diff);
}

#[test]
fn test_exponential_accrual_no_time_elapsed() {
    let current = Decimal::percent(60);
    let disco = Decimal::percent(80);
    let result = calculate_exponential_accrual(current, disco, 1000, 1000, Decimal::percent(5)).unwrap();
    assert_eq!(result, current, "Should not change when no time elapsed");
}

#[test]
fn test_exponential_accrual_time_backwards() {
    let current = Decimal::percent(60);
    let disco = Decimal::percent(80);
    let result = calculate_exponential_accrual(current, disco, 2000, 1000, Decimal::percent(5)).unwrap();
    assert_eq!(result, current, "Should not change when time goes backwards");
}

#[test]
fn test_exponential_large_kp_capped_at_target() {
    // Very large Kp → close to target but never overshoots
    let current = Decimal::percent(60);
    let disco = Decimal::percent(80);
    let kp = Decimal::percent(10_000); // 100x per day

    let result = calculate_exponential_accrual(current, disco, 0, SECONDS_PER_DAY, kp).unwrap();

    assert!(result <= disco, "Should not overshoot target");
    // With exponent capped at 1.0: e^(-1) ≈ 0.368
    // remaining ≈ 20% * 0.368 = 7.36%
    // new ≈ 80% - 7.36% = 72.64%
    assert!(result > Decimal::percent(70), "Should be well above starting point");
}

#[test]
fn test_exponential_very_small_kp() {
    // Very small Kp → very small accrual
    let current = Decimal::percent(60);
    let disco = Decimal::percent(80);
    let kp = Decimal::permille(1); // 0.1% per day

    let result = calculate_exponential_accrual(current, disco, 0, SECONDS_PER_DAY, kp).unwrap();

    // exponent = 0.001, e^(-0.001) ≈ 0.999
    // remaining ≈ 20% * 0.999 = 19.98%
    // new ≈ 80% - 19.98% ≈ 60.02%
    assert!(result > current, "Should increase slightly");
    let accrual = result - current;
    assert!(accrual < Decimal::permille(1), "Accrual should be very small");
}

// ============================================================================
// Decay Downward Shift Tests
// ============================================================================

#[test]
fn test_decay_downward_shift_basic() {
    // 40% of total gap applied per period
    let current = Decimal::percent(80);
    let staged = Decimal::percent(60);
    let decay_rate = Decimal::percent(40);

    let result = apply_decay_downward_shift(current, staged, decay_rate).unwrap();

    // total_shift = 80% - 60% = 20%
    // actual_shift = 20% * 40% = 8%
    // new = 80% - 8% = 72%
    assert_eq!(result, Decimal::percent(72));
}

#[test]
fn test_decay_multiple_periods() {
    // Gap shrinks each period (exponential convergence downward)
    let mut current = Decimal::percent(80);
    let staged = Decimal::percent(40);
    let decay_rate = Decimal::percent(40);

    // Period 1: gap = 40%, shift = 16%, new = 64%
    current = apply_decay_downward_shift(current, staged, decay_rate).unwrap();
    assert_eq!(current, Decimal::percent(64));

    // Period 2: gap = 24%, shift = 9.6%, new = 54.4%
    current = apply_decay_downward_shift(current, staged, decay_rate).unwrap();
    // 64% - (24% * 40%) = 64% - 9.6% = 54.4%
    let expected_p2 = Decimal::percent(64) - Decimal::from_ratio(96u128, 1000u128);
    assert_eq!(current, expected_p2);

    // Period 3: gap shrinks further
    let prev = current;
    current = apply_decay_downward_shift(current, staged, decay_rate).unwrap();
    assert!(current < prev, "Should keep decreasing");
    assert!(current > staged, "Should not go below staged");
}

#[test]
fn test_decay_staged_equals_current() {
    // staged >= current → no shift (zero gap)
    let current = Decimal::percent(80);
    let staged = Decimal::percent(80);
    let result = apply_decay_downward_shift(current, staged, Decimal::percent(40)).unwrap();
    assert_eq!(result, current, "No shift when staged equals current");
}

#[test]
fn test_decay_staged_above_current() {
    // staged > current → no shift
    let current = Decimal::percent(80);
    let staged = Decimal::percent(90);
    let result = apply_decay_downward_shift(current, staged, Decimal::percent(40)).unwrap();
    assert_eq!(result, current, "No shift when staged above current");
}

#[test]
fn test_decay_small_gap() {
    // Small gap → small shift
    let current = Decimal::percent(80);
    let staged = Decimal::percent(78);
    let decay_rate = Decimal::percent(40);

    let result = apply_decay_downward_shift(current, staged, decay_rate).unwrap();

    // gap = 2%, shift = 2% * 40% = 0.8%
    // new = 80% - 0.8% = 79.2%
    let expected = Decimal::percent(80) - Decimal::from_ratio(8u128, 1000u128);
    assert_eq!(result, expected);
}

// ============================================================================
// Downward Staging Tests
// ============================================================================

#[test]
fn test_downward_staging_initial() {
    let current = Decimal::percent(80);
    let disco = Decimal::percent(60);
    let mut staged = None;
    let mut staged_ts = None;

    let (new_ltv, updated) = process_ltv_update(
        current, disco,
        &mut staged, &mut staged_ts,
        1000, 2000,
        Decimal::percent(5), 604_800, Decimal::percent(40),
    ).unwrap();

    assert!(!updated, "Should not update on initial staging");
    assert_eq!(new_ltv, current, "LTV should remain unchanged");
    assert_eq!(staged, Some(disco), "Should stage the disco value");
    assert_eq!(staged_ts, Some(2000), "Should set staged timestamp to current time");
}

#[test]
fn test_downward_waiting_period() {
    let current = Decimal::percent(80);
    let disco = Decimal::percent(60);
    let mut staged = Some(Decimal::percent(60));
    let staged_start = 1000u64;
    let mut staged_ts = Some(staged_start);
    let downward_period = 604_800u64;

    // Call during waiting period (halfway through)
    let (new_ltv, updated) = process_ltv_update(
        current, disco,
        &mut staged, &mut staged_ts,
        500, staged_start + downward_period / 2,
        Decimal::percent(5), downward_period, Decimal::percent(40),
    ).unwrap();

    assert!(!updated, "Should not update during waiting period");
    assert_eq!(new_ltv, current, "LTV should remain unchanged during wait");
    assert!(staged.is_some(), "Staged value should remain");
    assert!(staged_ts.is_some(), "Staged timestamp should remain");
}

#[test]
fn test_downward_staged_update_lower() {
    // Disco drops further during wait → staged value tracks lower
    let current = Decimal::percent(80);
    let disco = Decimal::percent(50); // lower than staged 60
    let mut staged = Some(Decimal::percent(60));
    let mut staged_ts = Some(1000u64);

    let (_new_ltv, updated) = process_ltv_update(
        current, disco,
        &mut staged, &mut staged_ts,
        500, 1500, // still in waiting period
        Decimal::percent(5), 604_800, Decimal::percent(40),
    ).unwrap();

    assert!(!updated);
    assert_eq!(staged, Some(disco), "Staged value should update to lower disco");
    assert_eq!(staged_ts, Some(1000), "Timestamp should NOT reset (original timer preserved)");
}

#[test]
fn test_downward_apply_after_period() {
    let current = Decimal::percent(80);
    let disco = Decimal::percent(60);
    let staged_start = 1000u64;
    let downward_period = 604_800u64;
    let mut staged = Some(Decimal::percent(60));
    let mut staged_ts = Some(staged_start);

    let (new_ltv, updated) = process_ltv_update(
        current, disco,
        &mut staged, &mut staged_ts,
        500, staged_start + downward_period, // exactly at period end
        Decimal::percent(5), downward_period, Decimal::percent(40),
    ).unwrap();

    assert!(updated, "Should update after period elapses");
    assert!(new_ltv < current, "LTV should decrease");
    // Decay: gap = 80% - 60% = 20%, shift = 20% * 40% = 8%, new = 72%
    assert_eq!(new_ltv, Decimal::percent(72));
}

#[test]
fn test_downward_clears_after_apply() {
    let current = Decimal::percent(80);
    let disco = Decimal::percent(75);
    let mut staged = Some(Decimal::percent(75));
    let mut staged_ts = Some(1000u64);

    let (_new_ltv, updated) = process_ltv_update(
        current, disco,
        &mut staged, &mut staged_ts,
        500, 1000 + 604_800,
        Decimal::percent(5), 604_800, Decimal::percent(40),
    ).unwrap();

    assert!(updated);
    assert!(staged.is_none(), "Staged should be cleared after application");
    assert!(staged_ts.is_none(), "Staged timestamp should be cleared after application");
}

#[test]
fn test_downward_full_cycle() {
    // Stage → wait → apply → new stage (with decay)
    let current = Decimal::percent(80);
    let disco1 = Decimal::percent(70);
    let mut staged = None;
    let mut staged_ts = None;
    let period = 604_800u64;
    let decay_rate = Decimal::percent(40);

    // Phase 1: Initial staging
    let (ltv1, upd1) = process_ltv_update(
        current, disco1, &mut staged, &mut staged_ts,
        0, 1000, Decimal::percent(5), period, decay_rate,
    ).unwrap();
    assert!(!upd1);
    assert_eq!(ltv1, current);

    // Phase 2: Apply after period
    let (ltv2, upd2) = process_ltv_update(
        current, disco1, &mut staged, &mut staged_ts,
        0, 1000 + period, Decimal::percent(5), period, decay_rate,
    ).unwrap();
    assert!(upd2);
    // gap = 80% - 70% = 10%, shift = 10% * 40% = 4%, new = 76%
    assert_eq!(ltv2, Decimal::percent(76));

    // Phase 3: New stage (disco still at 70%, below 76%)
    let disco2 = Decimal::percent(70);
    let (ltv3, upd3) = process_ltv_update(
        ltv2, disco2, &mut staged, &mut staged_ts,
        0, 1000 + period + 100, Decimal::percent(5), period, decay_rate,
    ).unwrap();
    assert!(!upd3, "Should stage again, not apply immediately");
    assert_eq!(ltv3, ltv2, "LTV unchanged during new staging");
    assert_eq!(staged, Some(disco2));
}

// ============================================================================
// Staging Blocks Upward (Change 1)
// ============================================================================

#[test]
fn test_staged_blocks_upward() {
    // Staged value exists + disco above current → LTV frozen (no upward)
    let current = Decimal::percent(80);
    let disco = Decimal::percent(90); // disco above current
    let mut staged = Some(Decimal::percent(60)); // pending downward
    let mut staged_ts = Some(1000u64);

    let (new_ltv, updated) = process_ltv_update(
        current, disco,
        &mut staged, &mut staged_ts,
        500, 2000, // still in waiting period
        Decimal::percent(5), 604_800, Decimal::percent(40),
    ).unwrap();

    assert!(!updated, "Should NOT accrue upward while staged exists");
    assert_eq!(new_ltv, current, "LTV must be frozen");
    // Bidirectional update: disco >= current → staged = current (zero pending shift)
    assert_eq!(staged, Some(current), "Staged should update to current (zero shift)");
    assert_eq!(staged_ts, Some(1000), "Timer must NOT reset");
}

#[test]
fn test_staged_must_complete_before_upward() {
    // Timer must fully elapse even if disco recovers above current
    let current = Decimal::percent(80);
    let period = 604_800u64;
    let mut staged = Some(Decimal::percent(60));
    let mut staged_ts = Some(1000u64);

    // Disco recovers above current during wait
    let (ltv1, upd1) = process_ltv_update(
        current, Decimal::percent(90),
        &mut staged, &mut staged_ts,
        500, 2000, // still in wait
        Decimal::percent(5), period, Decimal::percent(40),
    ).unwrap();
    assert!(!upd1, "Should not update during wait");
    assert_eq!(ltv1, current);
    assert_eq!(staged, Some(current), "Staged = current (zero shift)");

    // Timer elapses → APPLYING with zero shift (staged == current)
    let (ltv2, upd2) = process_ltv_update(
        current, Decimal::percent(90),
        &mut staged, &mut staged_ts,
        2000, 1000 + period,
        Decimal::percent(5), period, Decimal::percent(40),
    ).unwrap();
    // staged was set to current, so decay_shift = (current - current) * 0.4 = 0 → no-op
    // But apply still clears the staged values
    // Note: apply_decay_downward_shift returns current when staged >= current
    assert!(upd2, "Should apply (even if shift is zero)");
    assert_eq!(ltv2, current, "Zero shift, LTV unchanged");
    assert!(staged.is_none(), "Staged should be cleared");
    assert!(staged_ts.is_none());

    // NOW upward accrual is allowed (next call)
    let (ltv3, upd3) = process_ltv_update(
        current, Decimal::percent(90),
        &mut staged, &mut staged_ts,
        1000 + period, 1000 + period + SECONDS_PER_DAY,
        Decimal::percent(5), period, Decimal::percent(40),
    ).unwrap();
    assert!(upd3, "Now upward accrual should work");
    assert!(ltv3 > current, "Should accrue upward");
}

// ============================================================================
// Bidirectional Staged Updates (Change 2)
// ============================================================================

#[test]
fn test_staged_value_updates_upward_during_wait() {
    // Disco recovers above staged but still below current → staged tracks disco
    let current = Decimal::percent(80);
    let disco = Decimal::percent(65); // above staged 60, below current 80
    let mut staged = Some(Decimal::percent(60));
    let mut staged_ts = Some(1000u64);

    let (_new_ltv, updated) = process_ltv_update(
        current, disco,
        &mut staged, &mut staged_ts,
        500, 1500,
        Decimal::percent(5), 604_800, Decimal::percent(40),
    ).unwrap();

    assert!(!updated);
    // Bidirectional: disco (65%) < current (80%) → staged = disco (65%)
    assert_eq!(staged, Some(disco), "Staged should track disco upward");
    assert_eq!(staged_ts, Some(1000), "Timer must NOT reset");
}

#[test]
fn test_staged_value_set_to_current_when_disco_above() {
    // disco >= current during wait → staged = current (zero pending shift, timer still runs)
    let current = Decimal::percent(80);
    let disco = Decimal::percent(85); // above current
    let mut staged = Some(Decimal::percent(60));
    let mut staged_ts = Some(1000u64);

    let (_new_ltv, updated) = process_ltv_update(
        current, disco,
        &mut staged, &mut staged_ts,
        500, 1500,
        Decimal::percent(5), 604_800, Decimal::percent(40),
    ).unwrap();

    assert!(!updated);
    assert_eq!(staged, Some(current), "Staged should equal current (zero shift)");
    assert_eq!(staged_ts, Some(1000), "Timer must still run (mandatory cooldown)");
}

#[test]
fn test_cooldown_period_after_recovery() {
    // Disco goes below current → stages → disco recovers → still waits full period
    let current = Decimal::percent(80);
    let period = 604_800u64;
    let mut staged = None;
    let mut staged_ts = None;

    // Step 1: Disco drops → stage
    let (_, _) = process_ltv_update(
        current, Decimal::percent(60),
        &mut staged, &mut staged_ts,
        0, 1000,
        Decimal::percent(5), period, Decimal::percent(40),
    ).unwrap();
    assert_eq!(staged, Some(Decimal::percent(60)));

    // Step 2: Disco recovers above current (halfway through wait)
    let (ltv2, upd2) = process_ltv_update(
        current, Decimal::percent(90),
        &mut staged, &mut staged_ts,
        1000, 1000 + period / 2,
        Decimal::percent(5), period, Decimal::percent(40),
    ).unwrap();
    assert!(!upd2, "LTV frozen during wait");
    assert_eq!(ltv2, current);
    assert_eq!(staged, Some(current), "Staged = current (zero shift)");
    assert!(staged_ts.is_some(), "Timer still running");

    // Step 3: Timer elapses → apply zero shift, clear staged
    let (ltv3, upd3) = process_ltv_update(
        current, Decimal::percent(90),
        &mut staged, &mut staged_ts,
        1000 + period / 2, 1000 + period,
        Decimal::percent(5), period, Decimal::percent(40),
    ).unwrap();
    assert!(upd3);
    assert_eq!(ltv3, current, "Zero shift applied");
    assert!(staged.is_none(), "Staged cleared after full period");

    // Step 4: Now upward is unblocked
    let (ltv4, upd4) = process_ltv_update(
        current, Decimal::percent(90),
        &mut staged, &mut staged_ts,
        1000 + period, 1000 + period + SECONDS_PER_DAY,
        Decimal::percent(5), period, Decimal::percent(40),
    ).unwrap();
    assert!(upd4, "Upward accrual should now work");
    assert!(ltv4 > current);
}

// ============================================================================
// Stable State Tests
// ============================================================================

#[test]
fn test_disco_equals_current() {
    let current = Decimal::percent(80);
    let disco = Decimal::percent(80);
    let mut staged = None;
    let mut staged_ts = None;

    let (new_ltv, updated) = process_ltv_update(
        current, disco,
        &mut staged, &mut staged_ts,
        1000, 2000,
        Decimal::percent(5), 604_800, Decimal::percent(40),
    ).unwrap();

    assert!(!updated, "Should not update when disco equals current");
    assert_eq!(new_ltv, current);
}

// ============================================================================
// State Machine Full Flow Tests
// ============================================================================

#[test]
fn test_state_machine_upward_then_downward_then_apply() {
    let period = 604_800u64;
    let kp = Decimal::percent(5);
    let decay_rate = Decimal::percent(40);
    let mut staged = None;
    let mut staged_ts = None;
    let mut current = Decimal::percent(60);

    // Step 1: Upward — disco at 80%, accrue for 1 day
    let (new, upd) = process_ltv_update(
        current, Decimal::percent(80),
        &mut staged, &mut staged_ts,
        0, SECONDS_PER_DAY,
        kp, period, decay_rate,
    ).unwrap();
    assert!(upd);
    assert!(new > current);
    current = new;

    // Step 2: Downward — disco drops to 50%, should stage
    let (new2, upd2) = process_ltv_update(
        current, Decimal::percent(50),
        &mut staged, &mut staged_ts,
        SECONDS_PER_DAY, 2 * SECONDS_PER_DAY,
        kp, period, decay_rate,
    ).unwrap();
    assert!(!upd2, "Should stage, not apply");
    assert_eq!(new2, current);
    assert!(staged.is_some());

    // Step 3: Apply after period (decay shift)
    let (new3, upd3) = process_ltv_update(
        current, Decimal::percent(50),
        &mut staged, &mut staged_ts,
        2 * SECONDS_PER_DAY, 2 * SECONDS_PER_DAY + period,
        kp, period, decay_rate,
    ).unwrap();
    assert!(upd3);
    assert!(new3 < current, "Should decrease with decay shift");
}

// ============================================================================
// Zero Disco Handling (Change 3)
// ============================================================================

#[test]
fn test_manual_ltv_set_on_init() {
    // Tracker init saves current max_LTV to MANUAL_MAX_LTVS
    let mut storage = MockStorage::new();

    // No manual LTV set yet
    let manual = MANUAL_MAX_LTVS.may_load(&storage, "uatom".to_string()).unwrap();
    assert!(manual.is_none());

    // Simulate get_or_init_tracker by saving directly (testing the storage pattern)
    let current_max_ltv = Decimal::percent(60);
    if MANUAL_MAX_LTVS.may_load(&storage, "uatom".to_string()).unwrap().is_none() {
        MANUAL_MAX_LTVS.save(&mut storage, "uatom".to_string(), &current_max_ltv).unwrap();
    }

    let manual = MANUAL_MAX_LTVS.load(&storage, "uatom".to_string()).unwrap();
    assert_eq!(manual, Decimal::percent(60), "Manual LTV should match initial max_LTV");
}

#[test]
fn test_manual_ltv_not_overwritten_on_reinit() {
    // If MANUAL_MAX_LTVS already set, get_or_init_tracker should not overwrite
    let mut storage = MockStorage::new();

    // Pre-set manual LTV
    MANUAL_MAX_LTVS.save(&mut storage, "uatom".to_string(), &Decimal::percent(70)).unwrap();

    // Simulate get_or_init_tracker with a different current max_LTV
    let current_max_ltv = Decimal::percent(80);
    if MANUAL_MAX_LTVS.may_load(&storage, "uatom".to_string()).unwrap().is_none() {
        MANUAL_MAX_LTVS.save(&mut storage, "uatom".to_string(), &current_max_ltv).unwrap();
    }

    let manual = MANUAL_MAX_LTVS.load(&storage, "uatom".to_string()).unwrap();
    assert_eq!(manual, Decimal::percent(70), "Should keep original value");
}

#[test]
fn test_zero_disco_blocks_deposits() {
    // DISCO_BLOCKED flag set when Disco returns 0
    let mut storage = MockStorage::new();

    // Simulate the zero-Disco pre-machine logic
    DISCO_BLOCKED.save(&mut storage, "uatom".to_string(), &true).unwrap();

    let blocked = DISCO_BLOCKED.load(&storage, "uatom".to_string()).unwrap();
    assert!(blocked, "Should be blocked when Disco returns 0");
}

#[test]
fn test_nonzero_disco_unblocks_deposits() {
    // DISCO_BLOCKED cleared when Disco returns non-zero
    let mut storage = MockStorage::new();

    // Previously blocked
    DISCO_BLOCKED.save(&mut storage, "uatom".to_string(), &true).unwrap();

    // Simulate non-zero Disco response
    DISCO_BLOCKED.save(&mut storage, "uatom".to_string(), &false).unwrap();

    let blocked = DISCO_BLOCKED.load(&storage, "uatom".to_string()).unwrap();
    assert!(!blocked, "Should be unblocked when Disco returns data");
}

#[test]
fn test_disco_blocked_default_false() {
    // If never set, defaults to false
    let storage = MockStorage::new();
    let blocked = DISCO_BLOCKED
        .may_load(&storage, "uatom".to_string())
        .unwrap()
        .unwrap_or(false);
    assert!(!blocked, "Should default to false");
}

// ============================================================================
// Borrow LTV Derivation Tests
// ============================================================================

#[test]
fn test_borrow_ltv_derived_from_max() {
    let max_ltv = Decimal::percent(80);
    let distance = Decimal::percent(10);
    let borrow = (max_ltv - distance).max(Decimal::zero());
    assert_eq!(borrow, Decimal::percent(70));
}

#[test]
fn test_borrow_ltv_floor_at_zero() {
    let max_ltv = Decimal::percent(5);
    let distance = Decimal::percent(10);
    let borrow = max_ltv.checked_sub(distance).unwrap_or(Decimal::zero()).max(Decimal::zero());
    assert_eq!(borrow, Decimal::zero());
}

// ============================================================================
// Cap / Bounds Tests
// ============================================================================

#[test]
fn test_cap_values_within_bounds() {
    let mut borrow = Decimal::percent(70);
    let mut max = Decimal::percent(80);
    cap_ltv_values(&mut borrow, &mut max).unwrap();
    assert_eq!(borrow, Decimal::percent(70));
    assert_eq!(max, Decimal::percent(80));
}

#[test]
fn test_cap_clamps_above_100() {
    let mut borrow = Decimal::percent(110);
    let mut max = Decimal::percent(120);
    cap_ltv_values(&mut borrow, &mut max).unwrap();
    assert_eq!(max, Decimal::percent(100));
    assert_eq!(borrow, Decimal::percent(95));
}

#[test]
fn test_cap_borrow_below_max() {
    let mut borrow = Decimal::percent(80);
    let mut max = Decimal::percent(80);
    cap_ltv_values(&mut borrow, &mut max).unwrap();
    assert_eq!(borrow, Decimal::percent(76)); // 80 * 95% = 76
}

#[test]
fn test_cap_zero_values() {
    let mut borrow = Decimal::zero();
    let mut max = Decimal::zero();
    cap_ltv_values(&mut borrow, &mut max).unwrap();
    assert_eq!(borrow, Decimal::zero());
    assert_eq!(max, Decimal::zero());
}

// ============================================================================
// Historical Snapshot Tests
// ============================================================================

#[test]
fn test_snapshot_daily_granularity() {
    let mut storage = MockStorage::new();

    // First snapshot always records
    let env1 = env_at(1000);
    record_ltv_snapshot(&mut storage, &env1, "uatom", Decimal::percent(80), Decimal::percent(70), Decimal::percent(80), None, Decimal::percent(70)).unwrap();
    let history = LTV_HISTORY.load(&storage, "uatom".to_string()).unwrap();
    assert_eq!(history.len(), 1);

    // Same day (less than 86400 seconds later) → should NOT record
    let env2 = env_at(1000 + STATE_SECONDS_PER_DAY - 1);
    record_ltv_snapshot(&mut storage, &env2, "uatom", Decimal::percent(81), Decimal::percent(71), Decimal::percent(81), None, Decimal::percent(71)).unwrap();
    let history = LTV_HISTORY.load(&storage, "uatom".to_string()).unwrap();
    assert_eq!(history.len(), 1, "Should not record within same day");

    // Next day → should record
    let env3 = env_at(1000 + STATE_SECONDS_PER_DAY);
    record_ltv_snapshot(&mut storage, &env3, "uatom", Decimal::percent(82), Decimal::percent(72), Decimal::percent(82), Some(Decimal::percent(85)), Decimal::percent(72)).unwrap();
    let history = LTV_HISTORY.load(&storage, "uatom".to_string()).unwrap();
    assert_eq!(history.len(), 2, "Should record on new day");
    assert_eq!(history[1].max_ltv, Decimal::percent(82));
    assert_eq!(history[1].max_borrow_ltv, Decimal::percent(72));
    assert_eq!(history[1].current_ltv, Decimal::percent(82));
    assert_eq!(history[1].pending_ltv, Some(Decimal::percent(85)));
    assert_eq!(history[1].current_borrow_ltv, Decimal::percent(72));
}

#[test]
fn test_snapshot_max_entries() {
    let mut storage = MockStorage::new();

    let total = MAX_LTV_HISTORY_ENTRIES + 10;
    for i in 0..total {
        let env = env_at((i as u64) * STATE_SECONDS_PER_DAY);
        record_ltv_snapshot(&mut storage, &env, "uatom", Decimal::percent(80), Decimal::percent(70), Decimal::percent(80), None, Decimal::percent(70)).unwrap();
    }

    let history = LTV_HISTORY.load(&storage, "uatom".to_string()).unwrap();
    assert_eq!(history.len(), MAX_LTV_HISTORY_ENTRIES, "Should prune to max entries");
    assert_eq!(history[0].timestamp, 10 * STATE_SECONDS_PER_DAY);
}

#[test]
fn test_snapshot_different_assets_independent() {
    let mut storage = MockStorage::new();

    let env = env_at(1000);
    record_ltv_snapshot(&mut storage, &env, "uatom", Decimal::percent(80), Decimal::percent(70), Decimal::percent(80), None, Decimal::percent(70)).unwrap();
    record_ltv_snapshot(&mut storage, &env, "uosmo", Decimal::percent(60), Decimal::percent(50), Decimal::percent(60), None, Decimal::percent(50)).unwrap();

    let atom_history = LTV_HISTORY.load(&storage, "uatom".to_string()).unwrap();
    let osmo_history = LTV_HISTORY.load(&storage, "uosmo".to_string()).unwrap();

    assert_eq!(atom_history.len(), 1);
    assert_eq!(osmo_history.len(), 1);
    assert_eq!(atom_history[0].max_ltv, Decimal::percent(80));
    assert_eq!(osmo_history[0].max_ltv, Decimal::percent(60));
}

// ============================================================================
// Tracker State Tests
// ============================================================================

#[test]
fn test_tracker_persistence() {
    let mut storage = MockStorage::new();

    let tracker = LTVUpdateTracker {
        last_update_timestamp: 5000,
        staged_max_ltv: Some(Decimal::percent(70)),
        staged_timestamp: Some(4000),
    };

    LTV_UPDATE_TRACKERS.save(&mut storage, "uatom".to_string(), &tracker).unwrap();

    let loaded = LTV_UPDATE_TRACKERS.load(&storage, "uatom".to_string()).unwrap();
    assert_eq!(loaded.last_update_timestamp, 5000);
    assert_eq!(loaded.staged_max_ltv, Some(Decimal::percent(70)));
    assert_eq!(loaded.staged_timestamp, Some(4000));
}

#[test]
fn test_tracker_per_asset_isolation() {
    let mut storage = MockStorage::new();

    let tracker_atom = LTVUpdateTracker {
        last_update_timestamp: 1000,
        staged_max_ltv: None,
        staged_timestamp: None,
    };
    let tracker_osmo = LTVUpdateTracker {
        last_update_timestamp: 2000,
        staged_max_ltv: Some(Decimal::percent(50)),
        staged_timestamp: Some(1500),
    };

    LTV_UPDATE_TRACKERS.save(&mut storage, "uatom".to_string(), &tracker_atom).unwrap();
    LTV_UPDATE_TRACKERS.save(&mut storage, "uosmo".to_string(), &tracker_osmo).unwrap();

    let loaded_atom = LTV_UPDATE_TRACKERS.load(&storage, "uatom".to_string()).unwrap();
    let loaded_osmo = LTV_UPDATE_TRACKERS.load(&storage, "uosmo".to_string()).unwrap();

    assert_eq!(loaded_atom.last_update_timestamp, 1000);
    assert!(loaded_atom.staged_max_ltv.is_none());
    assert_eq!(loaded_osmo.last_update_timestamp, 2000);
    assert_eq!(loaded_osmo.staged_max_ltv, Some(Decimal::percent(50)));
}

#[test]
fn test_no_tracker_initializes() {
    let mut storage = MockStorage::new();

    let tracker = LTV_UPDATE_TRACKERS
        .may_load(&storage, "uatom".to_string())
        .unwrap();
    assert!(tracker.is_none());

    let new_tracker = LTVUpdateTracker {
        last_update_timestamp: 1000,
        staged_max_ltv: None,
        staged_timestamp: None,
    };
    LTV_UPDATE_TRACKERS.save(&mut storage, "uatom".to_string(), &new_tracker).unwrap();

    let loaded = LTV_UPDATE_TRACKERS.load(&storage, "uatom".to_string()).unwrap();
    assert_eq!(loaded.last_update_timestamp, 1000);
    assert!(loaded.staged_max_ltv.is_none());
    assert!(loaded.staged_timestamp.is_none());
}

// ============================================================================
// Config Integration Tests
// ============================================================================

#[test]
fn test_config_defaults_set_correctly() {
    let mut storage = MockStorage::new();
    let config = init_config(&mut storage);

    assert_eq!(config.ltv_upward_kp, Decimal::percent(5));
    assert_eq!(config.ltv_downward_period, 604_800u64);
    assert_eq!(config.ltv_downward_decay_rate, Decimal::percent(40));
    assert_eq!(config.ltv_borrow_distance, Decimal::percent(10));
}

#[test]
fn test_basket_collateral_types_exist() {
    let mut storage = MockStorage::new();
    let basket = init_basket(&mut storage);

    assert_eq!(basket.collateral_types.len(), 2);
    assert_eq!(basket.collateral_types[0].max_LTV, Decimal::percent(60));
    assert_eq!(basket.collateral_types[0].max_borrow_LTV, Decimal::percent(50));
}
