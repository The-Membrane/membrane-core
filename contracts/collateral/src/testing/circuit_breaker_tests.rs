use cosmwasm_std::testing::mock_dependencies;
use cosmwasm_std::{Decimal, Env, Timestamp};

use crate::circuit_breaker::check_and_update_circuit_breaker;
use crate::state::ASSET_CIRCUIT_BREAKERS;
use crate::testing::test_helpers::helpers::*;

fn mock_env_at(time_secs: u64) -> Env {
    let mut env = cosmwasm_std::testing::mock_env();
    env.block.time = Timestamp::from_seconds(time_secs);
    env
}

// ========================================================================
// Group A: Reference price behavior (tested via circuit breaker) (5 tests)
// ========================================================================

#[test]
fn no_history_uses_current_price_as_reference() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // No historical prices seeded
    // current_price = 10, threshold = 10%
    // Reference will be current_price. Deviation = 0. Not frozen.
    let frozen = check_and_update_circuit_breaker(
        deps.as_mut().storage,
        &env,
        "uatom".to_string(),
        Decimal::from_ratio(10u128, 1u128),
        Decimal::percent(10),
    ).unwrap();
    assert!(!frozen);
}

#[test]
fn uses_average_of_last_5_prices() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // Seed 5 historical prices averaging to 10.0
    init_historical_prices(deps.as_mut().storage, "uatom", vec![
        ("8".to_string(), 100),
        ("9".to_string(), 200),
        ("10".to_string(), 300),
        ("11".to_string(), 400),
        ("12".to_string(), 500),
    ]);

    // Current price = 10 (matches average), threshold 10%
    let frozen = check_and_update_circuit_breaker(
        deps.as_mut().storage,
        &env,
        "uatom".to_string(),
        Decimal::from_ratio(10u128, 1u128),
        Decimal::percent(10),
    ).unwrap();
    assert!(!frozen);
}

#[test]
fn fewer_than_5_entries_averages_available() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // Only 2 prices: average = (8 + 12) / 2 = 10
    init_historical_prices(deps.as_mut().storage, "uatom", vec![
        ("8".to_string(), 100),
        ("12".to_string(), 200),
    ]);

    // Current = 10, ref = 10, deviation = 0
    let frozen = check_and_update_circuit_breaker(
        deps.as_mut().storage,
        &env,
        "uatom".to_string(),
        Decimal::from_ratio(10u128, 1u128),
        Decimal::percent(10),
    ).unwrap();
    assert!(!frozen);
}

#[test]
fn invalid_price_strings_skipped() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // Mix of valid and invalid
    init_historical_prices(deps.as_mut().storage, "uatom", vec![
        ("not_a_number".to_string(), 100),
        ("10".to_string(), 200),
        ("also_bad".to_string(), 300),
        ("10".to_string(), 400),
    ]);

    // Only 2 valid prices: avg = (10 + 10) / 2 = 10
    let frozen = check_and_update_circuit_breaker(
        deps.as_mut().storage,
        &env,
        "uatom".to_string(),
        Decimal::from_ratio(10u128, 1u128),
        Decimal::percent(10),
    ).unwrap();
    assert!(!frozen);
}

#[test]
fn all_invalid_falls_back_to_current() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_historical_prices(deps.as_mut().storage, "uatom", vec![
        ("bad1".to_string(), 100),
        ("bad2".to_string(), 200),
    ]);

    // Falls back to current price = 10, deviation = 0
    let frozen = check_and_update_circuit_breaker(
        deps.as_mut().storage,
        &env,
        "uatom".to_string(),
        Decimal::from_ratio(10u128, 1u128),
        Decimal::percent(10),
    ).unwrap();
    assert!(!frozen);
}

// ========================================================================
// Group B: check_and_update_circuit_breaker (10 tests)
// ========================================================================

#[test]
fn initializes_on_first_call() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // No breaker exists yet
    let frozen = check_and_update_circuit_breaker(
        deps.as_mut().storage,
        &env,
        "uatom".to_string(),
        Decimal::from_ratio(10u128, 1u128),
        Decimal::percent(10),
    ).unwrap();
    assert!(!frozen);

    // Breaker should now exist in state
    let breaker = ASSET_CIRCUIT_BREAKERS
        .load(deps.as_ref().storage, "uatom".to_string())
        .unwrap();
    assert!(!breaker.frozen);
    assert!(breaker.reference_price.is_some());
}

#[test]
fn stays_unfrozen_within_threshold() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // Seed history at price=10
    init_historical_prices(deps.as_mut().storage, "uatom", vec![
        ("10".to_string(), 100),
        ("10".to_string(), 200),
        ("10".to_string(), 300),
        ("10".to_string(), 400),
        ("10".to_string(), 500),
    ]);

    // Current = 10.5 -> deviation = 0.5/10 = 5% <= 10%
    let frozen = check_and_update_circuit_breaker(
        deps.as_mut().storage,
        &env,
        "uatom".to_string(),
        Decimal::from_ratio(105u128, 10u128),
        Decimal::percent(10),
    ).unwrap();
    assert!(!frozen);
}

#[test]
fn freezes_when_deviation_exceeds_threshold() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_historical_prices(deps.as_mut().storage, "uatom", vec![
        ("10".to_string(), 100),
        ("10".to_string(), 200),
        ("10".to_string(), 300),
        ("10".to_string(), 400),
        ("10".to_string(), 500),
    ]);

    // Current = 12 -> deviation = 2/10 = 20% > 10%
    let frozen = check_and_update_circuit_breaker(
        deps.as_mut().storage,
        &env,
        "uatom".to_string(),
        Decimal::from_ratio(12u128, 1u128),
        Decimal::percent(10),
    ).unwrap();
    assert!(frozen);

    let breaker = ASSET_CIRCUIT_BREAKERS
        .load(deps.as_ref().storage, "uatom".to_string())
        .unwrap();
    assert!(breaker.frozen);
    assert_eq!(breaker.frozen_at, 1000);
}

#[test]
fn auto_unfreezes_when_price_returns() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(2000);

    // Seed as frozen with reference_price = 10
    init_circuit_breaker(
        deps.as_mut().storage,
        "uatom",
        true,
        1000,
        Decimal::percent(10),
        Some(Decimal::from_ratio(10u128, 1u128)),
    );

    // Current = 10 -> deviation = 0/10 = 0% <= 10% -> unfreezes
    let frozen = check_and_update_circuit_breaker(
        deps.as_mut().storage,
        &env,
        "uatom".to_string(),
        Decimal::from_ratio(10u128, 1u128),
        Decimal::percent(10),
    ).unwrap();
    assert!(!frozen);

    let breaker = ASSET_CIRCUIT_BREAKERS
        .load(deps.as_ref().storage, "uatom".to_string())
        .unwrap();
    assert!(!breaker.frozen);
    assert_eq!(breaker.frozen_at, 0);
}

#[test]
fn stays_frozen_while_still_deviated() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(2000);

    init_circuit_breaker(
        deps.as_mut().storage,
        "uatom",
        true,
        1000,
        Decimal::percent(10),
        Some(Decimal::from_ratio(10u128, 1u128)),
    );

    // Current = 12 -> deviation = 2/10 = 20% > 10% -> stays frozen
    let frozen = check_and_update_circuit_breaker(
        deps.as_mut().storage,
        &env,
        "uatom".to_string(),
        Decimal::from_ratio(12u128, 1u128),
        Decimal::percent(10),
    ).unwrap();
    assert!(frozen);
}

#[test]
fn zero_reference_price_guard() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // Seed historical prices with "0"
    init_historical_prices(deps.as_mut().storage, "uatom", vec![
        ("0".to_string(), 100),
        ("0".to_string(), 200),
        ("0".to_string(), 300),
        ("0".to_string(), 400),
        ("0".to_string(), 500),
    ]);

    // Zero reference price -> guard returns false (not frozen)
    let frozen = check_and_update_circuit_breaker(
        deps.as_mut().storage,
        &env,
        "uatom".to_string(),
        Decimal::from_ratio(10u128, 1u128),
        Decimal::percent(10),
    ).unwrap();
    assert!(!frozen);
}

#[test]
fn downward_deviation_freezes() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_historical_prices(deps.as_mut().storage, "uatom", vec![
        ("10".to_string(), 100),
        ("10".to_string(), 200),
        ("10".to_string(), 300),
        ("10".to_string(), 400),
        ("10".to_string(), 500),
    ]);

    // Current = 8 -> deviation = |8-10|/10 = 20% > 10%
    let frozen = check_and_update_circuit_breaker(
        deps.as_mut().storage,
        &env,
        "uatom".to_string(),
        Decimal::from_ratio(8u128, 1u128),
        Decimal::percent(10),
    ).unwrap();
    assert!(frozen);
}

#[test]
fn exact_threshold_boundary_does_not_freeze() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_historical_prices(deps.as_mut().storage, "uatom", vec![
        ("10".to_string(), 100),
        ("10".to_string(), 200),
        ("10".to_string(), 300),
        ("10".to_string(), 400),
        ("10".to_string(), 500),
    ]);

    // Current = 11 -> deviation = 1/10 = 10% = threshold
    // Strict `>` means exactly at threshold does NOT freeze
    let frozen = check_and_update_circuit_breaker(
        deps.as_mut().storage,
        &env,
        "uatom".to_string(),
        Decimal::from_ratio(11u128, 1u128),
        Decimal::percent(10),
    ).unwrap();
    assert!(!frozen);
}

#[test]
fn frozen_at_timestamp_recorded() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(5000);

    init_historical_prices(deps.as_mut().storage, "uatom", vec![
        ("10".to_string(), 100),
    ]);

    // Large deviation to trigger freeze
    let _ = check_and_update_circuit_breaker(
        deps.as_mut().storage,
        &env,
        "uatom".to_string(),
        Decimal::from_ratio(20u128, 1u128), // 100% deviation
        Decimal::percent(10),
    ).unwrap();

    let breaker = ASSET_CIRCUIT_BREAKERS
        .load(deps.as_ref().storage, "uatom".to_string())
        .unwrap();
    assert_eq!(breaker.frozen_at, 5000);
}

#[test]
fn multiple_sequential_updates() {
    let mut deps = mock_dependencies();

    // Start: seed history at price 10
    init_historical_prices(deps.as_mut().storage, "uatom", vec![
        ("10".to_string(), 100),
        ("10".to_string(), 200),
        ("10".to_string(), 300),
    ]);

    // Update 1: price=10 -> not frozen
    let env1 = mock_env_at(1000);
    let frozen = check_and_update_circuit_breaker(
        deps.as_mut().storage,
        &env1,
        "uatom".to_string(),
        Decimal::from_ratio(10u128, 1u128),
        Decimal::percent(10),
    ).unwrap();
    assert!(!frozen);

    // Update 2: price=12 -> 20% deviation -> frozen
    let env2 = mock_env_at(2000);
    let frozen = check_and_update_circuit_breaker(
        deps.as_mut().storage,
        &env2,
        "uatom".to_string(),
        Decimal::from_ratio(12u128, 1u128),
        Decimal::percent(10),
    ).unwrap();
    assert!(frozen);

    // Update 3: price=10 -> back to normal -> unfreezes
    let env3 = mock_env_at(3000);
    let frozen = check_and_update_circuit_breaker(
        deps.as_mut().storage,
        &env3,
        "uatom".to_string(),
        Decimal::from_ratio(10u128, 1u128),
        Decimal::percent(10),
    ).unwrap();
    assert!(!frozen);
}
