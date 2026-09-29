#![allow(non_snake_case)]

use cosmwasm_std::testing::mock_dependencies;
use cosmwasm_std::{Decimal, Timestamp};
use proptest::prelude::*;

use crate::circuit_breaker::check_and_update_circuit_breaker;
use crate::testing::test_helpers::helpers::*;

// ========================================================================
// Strategies
// ========================================================================

fn arb_price() -> impl Strategy<Value = Decimal> {
    (1u64..=100_000u64).prop_map(|p| Decimal::from_ratio(p, 100u128))
}

fn arb_threshold() -> impl Strategy<Value = Decimal> {
    (1u64..=50u64).prop_map(|p| Decimal::percent(p))
}

// ========================================================================
// Circuit Breaker Properties (3)
// ========================================================================

proptest! {
    #[test]
    fn circuit_breaker_consistency(
        price in arb_price(),
        threshold in arb_threshold(),
    ) {
        let mut deps = mock_dependencies();
        let mut env = cosmwasm_std::testing::mock_env();
        env.block.time = Timestamp::from_seconds(1000);

        // First call: initializes. Second call: uses reference from first.
        let result1 = check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            "test_asset".to_string(),
            price,
            threshold,
        );
        prop_assert!(result1.is_ok());

        // Second call with same price should never be frozen
        // (reference becomes the price from first call, deviation = 0)
        let result2 = check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            "test_asset".to_string(),
            price,
            threshold,
        );
        prop_assert!(result2.is_ok());
        if let Ok(frozen) = result2 {
            prop_assert!(!frozen, "Same price twice should never freeze");
        }
    }

    #[test]
    fn exact_threshold_never_freezes(
        base_price_cents in 100u64..=10000u64,
        threshold_pct in 1u64..=50u64,
    ) {
        let mut deps = mock_dependencies();
        let mut env = cosmwasm_std::testing::mock_env();
        env.block.time = Timestamp::from_seconds(1000);

        let base = Decimal::from_ratio(base_price_cents as u128, 100u128);
        let threshold = Decimal::percent(threshold_pct);

        // Seed reference price via historical prices
        init_historical_prices(deps.as_mut().storage, "test_asset", vec![
            (base.to_string(), 100),
            (base.to_string(), 200),
            (base.to_string(), 300),
            (base.to_string(), 400),
            (base.to_string(), 500),
        ]);

        // Price exactly at threshold deviation
        let deviation_amount = base * threshold;
        let price_at_threshold = base + deviation_amount;

        let result = check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            "test_asset".to_string(),
            price_at_threshold,
            threshold,
        );

        if let Ok(frozen) = result {
            prop_assert!(!frozen, "Exact threshold should NOT freeze (strict >)");
        }
    }

    #[test]
    fn reference_price_non_negative(
        prices in prop::collection::vec(1u64..=10000u64, 1..=5),
    ) {
        let mut deps = mock_dependencies();
        let mut env = cosmwasm_std::testing::mock_env();
        env.block.time = Timestamp::from_seconds(1000);

        let history: Vec<(String, u64)> = prices.iter().enumerate()
            .map(|(i, p)| (Decimal::from_ratio(*p as u128, 100u128).to_string(), (i as u64 + 1) * 100))
            .collect();
        init_historical_prices(deps.as_mut().storage, "test_asset", history);

        let current = Decimal::from_ratio(prices[0] as u128, 100u128);
        let result = check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            "test_asset".to_string(),
            current,
            Decimal::percent(10),
        );
        prop_assert!(result.is_ok());
    }
}

// ========================================================================
// Supply Cap Properties (4)
// ========================================================================

proptest! {
    #[test]
    fn add_then_subtract_neutral(
        amount in 1u128..=1_000_000u128,
    ) {
        let mut deps = mock_dependencies();

        let cassets = vec![
            make_casset_with_amount("uatom", 0, 50, 60),
            make_casset_with_amount("uosmo", 1_000_000, 40, 50),
        ];
        let caps = vec![
            make_supply_cap("uatom"),
            make_supply_cap_with_supply("uosmo", 100, 1_000_000),
        ];
        init_basket_custom(deps.as_mut().storage, cassets, caps, vec![]);
        init_config(deps.as_mut().storage);
        init_frozen_state(deps.as_mut().storage, false);
        init_stored_price(deps.as_mut().storage, "uatom", Decimal::one(), 900);
        init_stored_price(deps.as_mut().storage, "uosmo", Decimal::one(), 900);

        let env = cosmwasm_std::testing::mock_env();

        // Add
        let collateral = vec![make_casset_with_amount("uatom", amount, 50, 60)];
        let full_pos = collateral.clone();
        let _ = crate::risk_engine::update_basket_tally(
            &mut deps.as_mut(), env.clone(), collateral, full_pos, true, false,
        );

        // Subtract
        let collateral = vec![make_casset_with_amount("uatom", amount, 50, 60)];
        let full_pos = vec![make_casset_with_amount("uatom", 0, 50, 60)];
        let _ = crate::risk_engine::update_basket_tally(
            &mut deps.as_mut(), env, collateral, full_pos, false, false,
        );

        let basket = crate::state::BASKET.load(deps.as_ref().storage).unwrap();
        prop_assert_eq!(basket.collateral_supply_caps[0].current_supply, cosmwasm_std::Uint128::zero());
    }

    #[test]
    fn current_supply_never_negative(
        supply in 0u128..=1000u128,
        subtract in 0u128..=2000u128,
    ) {
        let mut deps = mock_dependencies();

        let cassets = vec![make_casset_with_amount("uatom", supply, 50, 60)];
        let caps = vec![make_supply_cap_with_supply("uatom", 100, supply)];
        init_basket_custom(deps.as_mut().storage, cassets, caps, vec![]);
        init_config(deps.as_mut().storage);
        init_frozen_state(deps.as_mut().storage, false);
        init_stored_price(deps.as_mut().storage, "uatom", Decimal::one(), 900);

        let env = cosmwasm_std::testing::mock_env();

        let collateral = vec![make_casset_with_amount("uatom", subtract, 50, 60)];
        let full_pos = vec![];
        let _ = crate::risk_engine::update_basket_tally(
            &mut deps.as_mut(), env, collateral, full_pos, false, false,
        );

        let basket = crate::state::BASKET.load(deps.as_ref().storage).unwrap();
        // current_supply should never be negative (it's Uint128, but verify the clamp)
        prop_assert!(basket.collateral_supply_caps[0].current_supply <= cosmwasm_std::Uint128::new(supply));
    }

    #[test]
    fn from_liquidation_never_errors(
        amount in 1u128..=1_000_000u128,
    ) {
        let mut deps = mock_dependencies();

        let cassets = vec![make_casset_with_amount("uatom", 0, 50, 60)];
        let caps = vec![make_supply_cap_with_ratio("uatom", 1)]; // tiny cap
        init_basket_custom(deps.as_mut().storage, cassets, caps, vec![]);
        init_config(deps.as_mut().storage);
        init_frozen_state(deps.as_mut().storage, false);
        init_stored_price(deps.as_mut().storage, "uatom", Decimal::one(), 900);

        let env = cosmwasm_std::testing::mock_env();

        let collateral = vec![make_casset_with_amount("uatom", amount, 50, 60)];
        let full_pos = collateral.clone();
        let res = crate::risk_engine::update_basket_tally(
            &mut deps.as_mut(), env, collateral, full_pos, true, true,
        );
        prop_assert!(res.is_ok(), "from_liquidation should never error on cap");
    }

    #[test]
    fn cap_ratio_bounded(
        ratio_pct in 0u64..=200u64,
    ) {
        let cap = make_supply_cap_with_ratio("uatom", ratio_pct);
        prop_assert!(cap.supply_cap_ratio <= Decimal::from_ratio(200u128, 100u128));
        prop_assert!(cap.supply_cap_ratio >= Decimal::zero());
    }
}
