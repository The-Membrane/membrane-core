/// Property-based tests for the Debt contract using proptest.
///
/// These tests verify invariants of the internal financial functions
/// across a wide range of random inputs, catching edge cases that
/// example-based tests might miss.
pub mod proptest_tests {
    use cosmwasm_std::{Decimal, Int128, Uint128};
    use proptest::prelude::*;

    use membrane::types::{
        AssetInfo, CreditAssetBreakdown, FixedRate, FixedRateCap, FixedRateCaps, FixedRateEnd,
        IRMConfig, RateSegment,
    };

    use crate::debt_manager::{
        apply_delta_to_caps, apply_delta_to_peg_debt, apply_delta_to_regular_debt,
        apply_discount_to_segments, combined_breakdown_for_caps, get_total_from_segments,
        reduce_segments_proportionally, repay_segments_default_order, repay_segments_targeted,
        BreakdownView, SegmentDeltas, SegmentSplit,
    };
    use crate::rates::{
        adaptive_smooth_rate, calculate_fixed_rate_accrual, SECONDS_PER_YEAR,
    };

    // ========================================================================
    // Helpers
    // ========================================================================

    fn var_segment(amount: u128) -> RateSegment {
        RateSegment {
            amount: Uint128::new(amount),
            fixed_rate: None,
        }
    }

    fn fixed_segment(amount: u128, duration: u8, rate_pct: u64, end_time: u64) -> RateSegment {
        RateSegment {
            amount: Uint128::new(amount),
            fixed_rate: Some(FixedRate {
                rate: Decimal::percent(rate_pct),
                end: FixedRateEnd {
                    end_time,
                    rollover: false,
                    duration_months: duration,
                },
            }),
        }
    }

    fn make_breakdown(
        var: u128,
        one: u128,
        three: u128,
        six: u128,
        peg_var: u128,
        peg_one: u128,
        peg_three: u128,
        peg_six: u128,
    ) -> CreditAssetBreakdown {
        CreditAssetBreakdown {
            info: AssetInfo::NativeToken {
                denom: "cdt".to_string(),
            },
            variable_amount: Uint128::new(var),
            one_month_amount: Uint128::new(one),
            three_month_amount: Uint128::new(three),
            six_month_amount: Uint128::new(six),
            peg_variable_amount: Uint128::new(peg_var),
            peg_one_month_amount: Uint128::new(peg_one),
            peg_three_month_amount: Uint128::new(peg_three),
            peg_six_month_amount: Uint128::new(peg_six),
        }
    }

    fn default_irm() -> IRMConfig {
        IRMConfig {
            adjustment_speed: Decimal::from_ratio(50u128, 1u128),
            min_adaptive_rate: Decimal::permille(1), // 0.1%
            max_adaptive_rate: Decimal::one(),        // 100%
        }
    }

    fn default_caps() -> FixedRateCaps {
        FixedRateCaps {
            one_month: FixedRateCap {
                cap: Decimal::percent(20),
                amount: Uint128::zero(),
                multiplier: Decimal::from_ratio(11u128, 10u128),
            },
            three_month: FixedRateCap {
                cap: Decimal::percent(20),
                amount: Uint128::zero(),
                multiplier: Decimal::from_ratio(13u128, 10u128),
            },
            six_month: FixedRateCap {
                cap: Decimal::percent(10),
                amount: Uint128::zero(),
                multiplier: Decimal::from_ratio(17u128, 10u128),
            },
        }
    }

    // ========================================================================
    // Strategies
    // ========================================================================

    /// Time strategy: 0 to 1 year in seconds
    fn time_strategy() -> impl Strategy<Value = u64> {
        0u64..31_536_000u64
    }

    // ========================================================================
    // Property Tests: repay_segments_default_order
    // ========================================================================

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// Repaying never increases total debt.
        #[test]
        fn repay_default_never_increases_debt(
            amounts in prop::collection::vec(1u128..100_000_000_000u128, 1..5),
            repay in 0u128..500_000_000_000u128,
        ) {
            let mut segments: Vec<RateSegment> = amounts.iter().map(|&a| var_segment(a)).collect();
            let total_before = get_total_from_segments(&segments);
            let (applied, _) = repay_segments_default_order(&mut segments, Uint128::new(repay));
            let total_after = get_total_from_segments(&segments);

            prop_assert!(total_after <= total_before);
            prop_assert!(applied <= Uint128::new(repay));
            prop_assert!(applied <= total_before);
        }

        /// Amount applied equals total_before - total_after.
        #[test]
        fn repay_default_applied_equals_reduction(
            amounts in prop::collection::vec(1u128..100_000_000_000u128, 1..5),
            repay in 0u128..500_000_000_000u128,
        ) {
            let mut segments: Vec<RateSegment> = amounts.iter().map(|&a| var_segment(a)).collect();
            let total_before = get_total_from_segments(&segments);
            let (applied, _) = repay_segments_default_order(&mut segments, Uint128::new(repay));
            let total_after = get_total_from_segments(&segments);

            prop_assert_eq!(applied, total_before - total_after);
        }

        /// Zero repayment is a no-op.
        #[test]
        fn repay_default_zero_is_noop(
            amounts in prop::collection::vec(1u128..100_000_000_000u128, 1..5),
        ) {
            let mut segments: Vec<RateSegment> = amounts.iter().map(|&a| var_segment(a)).collect();
            let total_before = get_total_from_segments(&segments);
            let (applied, _) = repay_segments_default_order(&mut segments, Uint128::zero());
            let total_after = get_total_from_segments(&segments);

            prop_assert_eq!(applied, Uint128::zero());
            prop_assert_eq!(total_before, total_after);
        }

        /// Repay is idempotent when repaying full debt twice.
        #[test]
        fn repay_default_full_then_zero(
            amounts in prop::collection::vec(1u128..100_000_000_000u128, 1..4),
        ) {
            let mut segments: Vec<RateSegment> = amounts.iter().map(|&a| var_segment(a)).collect();
            let total = get_total_from_segments(&segments);
            // First repay: clear all debt
            let (applied1, _) = repay_segments_default_order(&mut segments, total);
            prop_assert_eq!(applied1, total);
            prop_assert_eq!(get_total_from_segments(&segments), Uint128::zero());
            // Second repay: nothing to repay
            let (applied2, _) = repay_segments_default_order(&mut segments, Uint128::new(1000));
            prop_assert_eq!(applied2, Uint128::zero());
        }

        // ====================================================================
        // Property Tests: repay_segments_targeted
        // ====================================================================

        /// Targeted repay never applies more than the repay amount.
        #[test]
        fn repay_targeted_bounded_by_amount(
            var_amount in 1u128..100_000_000_000u128,
            repay in 0u128..200_000_000_000u128,
            var_pct in 0u64..100u64,
        ) {
            let mut segments = vec![var_segment(var_amount)];
            let split = SegmentSplit {
                variable: Some(Decimal::percent(var_pct)),
                ..SegmentSplit::default()
            };
            let (applied, _) = repay_segments_targeted(
                &mut segments,
                Uint128::new(repay),
                &split,
            ).unwrap();

            prop_assert!(applied <= Uint128::new(repay));
        }

        /// Targeted repay never increases debt.
        #[test]
        fn repay_targeted_never_increases_debt(
            var_amount in 1u128..100_000_000_000u128,
            fixed_amount in 1u128..100_000_000_000u128,
            repay in 0u128..200_000_000_000u128,
            var_pct in 0u64..50u64,
            fixed_pct in 0u64..50u64,
        ) {
            let mut segments = vec![
                var_segment(var_amount),
                fixed_segment(fixed_amount, 3, 5, 9999999),
            ];
            let total_before = get_total_from_segments(&segments);
            let split = SegmentSplit {
                variable: Some(Decimal::percent(var_pct)),
                three_month: Some(Decimal::percent(fixed_pct)),
                ..SegmentSplit::default()
            };
            let _ = repay_segments_targeted(
                &mut segments,
                Uint128::new(repay),
                &split,
            ).unwrap();
            let total_after = get_total_from_segments(&segments);

            prop_assert!(total_after <= total_before);
        }

        // ====================================================================
        // Property Tests: reduce_segments_proportionally
        // ====================================================================

        /// Proportional reduction never increases total debt.
        #[test]
        fn reduce_proportionally_never_increases(
            amounts in prop::collection::vec(1u128..50_000_000_000u128, 1..4),
            repay in 0u128..200_000_000_000u128,
        ) {
            let mut segments: Vec<RateSegment> = amounts.iter().map(|&a| var_segment(a)).collect();
            let pool_total = get_total_from_segments(&segments);
            let total_debt = pool_total + Uint128::new(1_000_000); // some extra debt in other pools

            let total_before = get_total_from_segments(&segments);
            let _ = reduce_segments_proportionally(
                &mut segments,
                pool_total,
                total_debt,
                Uint128::new(repay),
            );
            let total_after = get_total_from_segments(&segments);

            prop_assert!(total_after <= total_before);
        }

        /// Proportional reduction with zero repay is a no-op.
        #[test]
        fn reduce_proportionally_zero_is_noop(
            amounts in prop::collection::vec(1u128..50_000_000_000u128, 1..4),
        ) {
            let mut segments: Vec<RateSegment> = amounts.iter().map(|&a| var_segment(a)).collect();
            let pool_total = get_total_from_segments(&segments);
            let total_before = get_total_from_segments(&segments);
            let _ = reduce_segments_proportionally(
                &mut segments,
                pool_total,
                pool_total,
                Uint128::zero(),
            );
            let total_after = get_total_from_segments(&segments);

            prop_assert_eq!(total_before, total_after);
        }

        // ====================================================================
        // Property Tests: apply_discount_to_segments
        // ====================================================================

        /// Discount never increases any segment.
        #[test]
        fn discount_never_increases_segment(
            amounts in prop::collection::vec(1u128..50_000_000_000u128, 1..4),
            discount_pct in 0u64..100u64,
        ) {
            let mut segments: Vec<RateSegment> = amounts.iter().map(|&a| var_segment(a)).collect();
            let amounts_before: Vec<Uint128> = segments.iter().map(|s| s.amount).collect();
            let total_debt = get_total_from_segments(&segments);
            let discount_amount = Decimal::percent(discount_pct)
                .checked_mul(Decimal::from_ratio(total_debt, Uint128::one()))
                .unwrap_or(Decimal::zero())
                .to_uint_floor();

            let _ = apply_discount_to_segments(&mut segments, discount_amount, total_debt);

            for (i, seg) in segments.iter().enumerate() {
                prop_assert!(seg.amount <= amounts_before[i]);
            }
        }

        /// Discount with zero amount is a no-op.
        #[test]
        fn discount_zero_is_noop(
            amounts in prop::collection::vec(1u128..50_000_000_000u128, 1..4),
        ) {
            let mut segments: Vec<RateSegment> = amounts.iter().map(|&a| var_segment(a)).collect();
            let total_before = get_total_from_segments(&segments);
            let total_debt = total_before;
            let _ = apply_discount_to_segments(&mut segments, Uint128::zero(), total_debt);
            let total_after = get_total_from_segments(&segments);

            prop_assert_eq!(total_before, total_after);
        }

        /// Discount total reduction is bounded by discount_amount.
        #[test]
        fn discount_reduction_bounded(
            amounts in prop::collection::vec(1u128..50_000_000_000u128, 1..4),
            discount in 0u128..100_000_000_000u128,
        ) {
            let mut segments: Vec<RateSegment> = amounts.iter().map(|&a| var_segment(a)).collect();
            let total_before = get_total_from_segments(&segments);
            let _ = apply_discount_to_segments(&mut segments, Uint128::new(discount), total_before);
            let total_after = get_total_from_segments(&segments);
            let reduction = total_before - total_after;

            prop_assert!(reduction <= Uint128::new(discount));
        }

        // ====================================================================
        // Property Tests: adaptive_smooth_rate
        // ====================================================================

        /// Result is always within [min_adaptive_rate, max_adaptive_rate].
        #[test]
        fn adaptive_rate_within_bounds(
            current_pct in 1u64..200u64,
            dest_pct in 1u64..200u64,
            time in time_strategy(),
        ) {
            let irm = default_irm();
            let current = Decimal::percent(current_pct);
            let dest = Decimal::percent(dest_pct);

            let result = adaptive_smooth_rate(current, dest, time, &irm);

            prop_assert!(result >= irm.min_adaptive_rate);
            prop_assert!(result <= irm.max_adaptive_rate);
        }

        /// Bootstrap: zero current jumps to destination (clamped).
        #[test]
        fn adaptive_rate_bootstrap(
            dest_pct in 1u64..200u64,
            time in time_strategy(),
        ) {
            let irm = default_irm();
            let dest = Decimal::percent(dest_pct);

            let result = adaptive_smooth_rate(Decimal::zero(), dest, time, &irm);

            // Should be clamped destination
            let expected = std::cmp::min(
                std::cmp::max(dest, irm.min_adaptive_rate),
                irm.max_adaptive_rate,
            );
            prop_assert_eq!(result, expected);
        }

        /// Zero time elapsed means rate doesn't change (for non-zero current).
        #[test]
        fn adaptive_rate_zero_time_unchanged(
            current_pct in 1u64..100u64,
            dest_pct in 1u64..100u64,
        ) {
            let irm = default_irm();
            let current = Decimal::percent(current_pct);
            let dest = Decimal::percent(dest_pct);

            let result = adaptive_smooth_rate(current, dest, 0, &irm);

            // With zero time, exponent = 0, exp(0) = 1, new_rate = current * 1 = current
            // Then clamped between current and dest... but also clamped to global bounds.
            // The result should be current (if within bounds).
            if current >= irm.min_adaptive_rate && current <= irm.max_adaptive_rate {
                prop_assert_eq!(result, current);
            }
        }

        /// Rate moves toward destination (directional correctness).
        #[test]
        fn adaptive_rate_moves_toward_destination(
            current_pct in 2u64..50u64,
            dest_pct in 2u64..50u64,
            time in 86400u64..SECONDS_PER_YEAR,
        ) {
            let irm = default_irm();
            let current = Decimal::percent(current_pct);
            let dest = Decimal::percent(dest_pct);

            let result = adaptive_smooth_rate(current, dest, time, &irm);

            if current < dest {
                // Should increase (or stay if at max)
                prop_assert!(result >= current);
            } else if current > dest {
                // Should decrease (or stay if at min)
                prop_assert!(result <= current);
            } else {
                // current == dest: no change
                prop_assert_eq!(result, current);
            }
        }

        /// Rate never overshoots the destination.
        #[test]
        fn adaptive_rate_no_overshoot(
            current_pct in 2u64..80u64,
            dest_pct in 2u64..80u64,
            time in 0u64..SECONDS_PER_YEAR,
        ) {
            let irm = default_irm();
            let current = Decimal::percent(current_pct);
            let dest = Decimal::percent(dest_pct);

            let result = adaptive_smooth_rate(current, dest, time, &irm);

            if current < dest {
                prop_assert!(result <= dest);
            } else if current > dest {
                prop_assert!(result >= dest);
            }
        }

        // ====================================================================
        // Property Tests: calculate_fixed_rate_accrual
        // ====================================================================

        /// Fixed rate accrual is monotonic in time.
        #[test]
        fn fixed_accrual_monotonic_in_time(
            amount in 1u128..100_000_000_000u128,
            rate_pct in 1u64..100u64,
            t1 in 1u64..SECONDS_PER_YEAR / 2,
        ) {
            let t2 = t1 + 1; // strictly larger time
            let accrual1 = calculate_fixed_rate_accrual(
                Uint128::new(amount),
                Decimal::percent(rate_pct),
                0,
                t1,
            ).unwrap();
            let accrual2 = calculate_fixed_rate_accrual(
                Uint128::new(amount),
                Decimal::percent(rate_pct),
                0,
                t2,
            ).unwrap();

            prop_assert!(accrual2 >= accrual1);
        }

        /// Fixed rate accrual is monotonic in rate.
        #[test]
        fn fixed_accrual_monotonic_in_rate(
            amount in 1u128..100_000_000_000u128,
            r1 in 1u64..99u64,
            time in 1u64..SECONDS_PER_YEAR,
        ) {
            let r2 = r1 + 1;
            let accrual1 = calculate_fixed_rate_accrual(
                Uint128::new(amount),
                Decimal::percent(r1),
                0,
                time,
            ).unwrap();
            let accrual2 = calculate_fixed_rate_accrual(
                Uint128::new(amount),
                Decimal::percent(r2),
                0,
                time,
            ).unwrap();

            prop_assert!(accrual2 >= accrual1);
        }

        /// Fixed rate accrual is monotonic in amount.
        #[test]
        fn fixed_accrual_monotonic_in_amount(
            a1 in 1u128..100_000_000_000u128,
            rate_pct in 1u64..100u64,
            time in 1u64..SECONDS_PER_YEAR,
        ) {
            let a2 = a1 + 1;
            let accrual1 = calculate_fixed_rate_accrual(
                Uint128::new(a1),
                Decimal::percent(rate_pct),
                0,
                time,
            ).unwrap();
            let accrual2 = calculate_fixed_rate_accrual(
                Uint128::new(a2),
                Decimal::percent(rate_pct),
                0,
                time,
            ).unwrap();

            prop_assert!(accrual2 >= accrual1);
        }

        /// Zero inputs yield zero accrual.
        #[test]
        fn fixed_accrual_zero_for_zero_inputs(
            rate_pct in 0u64..100u64,
            time in 0u64..SECONDS_PER_YEAR,
        ) {
            // Zero amount
            let accrual = calculate_fixed_rate_accrual(
                Uint128::zero(),
                Decimal::percent(rate_pct),
                0,
                time,
            ).unwrap();
            prop_assert_eq!(accrual, Uint128::zero());

            // Zero rate
            let accrual = calculate_fixed_rate_accrual(
                Uint128::new(1_000_000),
                Decimal::zero(),
                0,
                time,
            ).unwrap();
            prop_assert_eq!(accrual, Uint128::zero());

            // Zero time
            let accrual = calculate_fixed_rate_accrual(
                Uint128::new(1_000_000),
                Decimal::percent(rate_pct),
                1000,
                1000, // start == end
            ).unwrap();
            prop_assert_eq!(accrual, Uint128::zero());
        }

        // ====================================================================
        // Property Tests: get_total_from_segments
        // ====================================================================

        /// Total is always >= 0 (trivially true for Uint128, but also checks summation).
        #[test]
        fn total_segments_nonnegative(
            amounts in prop::collection::vec(0u128..100_000_000_000u128, 0..6),
        ) {
            let segments: Vec<RateSegment> = amounts.iter().map(|&a| var_segment(a)).collect();
            let total = get_total_from_segments(&segments);
            let expected: Uint128 = amounts.iter().map(|&a| Uint128::new(a)).sum();
            prop_assert_eq!(total, expected);
        }

        /// Empty segments yield zero total.
        #[test]
        fn total_segments_empty_is_zero(_dummy in 0u32..1u32) {
            let segments: Vec<RateSegment> = vec![];
            prop_assert_eq!(get_total_from_segments(&segments), Uint128::zero());
        }

        // ====================================================================
        // Property Tests: combined_breakdown_for_caps
        // ====================================================================

        /// Each field sums regular + peg correctly.
        #[test]
        fn combined_breakdown_sums_match(
            var in 0u128..10_000_000_000u128,
            one in 0u128..10_000_000_000u128,
            three in 0u128..10_000_000_000u128,
            six in 0u128..10_000_000_000u128,
            peg_var in 0u128..10_000_000_000u128,
            peg_one in 0u128..10_000_000_000u128,
            peg_three in 0u128..10_000_000_000u128,
            peg_six in 0u128..10_000_000_000u128,
        ) {
            let bd = make_breakdown(var, one, three, six, peg_var, peg_one, peg_three, peg_six);
            let view = combined_breakdown_for_caps(&bd);

            prop_assert_eq!(view.variable, Uint128::new(var + peg_var));
            prop_assert_eq!(view.one_month, Uint128::new(one + peg_one));
            prop_assert_eq!(view.three_month, Uint128::new(three + peg_three));
            prop_assert_eq!(view.six_month, Uint128::new(six + peg_six));
        }

        /// Combined breakdown total_regular_debt + total_peg_debt equals sum of all fields.
        #[test]
        fn combined_breakdown_total_consistency(
            var in 0u128..10_000_000_000u128,
            one in 0u128..10_000_000_000u128,
            three in 0u128..10_000_000_000u128,
            six in 0u128..10_000_000_000u128,
            peg_var in 0u128..10_000_000_000u128,
            peg_one in 0u128..10_000_000_000u128,
            peg_three in 0u128..10_000_000_000u128,
            peg_six in 0u128..10_000_000_000u128,
        ) {
            let bd = make_breakdown(var, one, three, six, peg_var, peg_one, peg_three, peg_six);
            let view = combined_breakdown_for_caps(&bd);

            let view_total = view.variable + view.one_month + view.three_month + view.six_month;
            let bd_total = bd.total_regular_debt() + bd.total_peg_debt();

            prop_assert_eq!(view_total, bd_total);
        }

        // ====================================================================
        // Property Tests: apply_delta_to_regular_debt / peg_debt
        // ====================================================================

        /// Positive deltas always increase breakdown amounts.
        #[test]
        fn apply_regular_deltas_positive_increases(
            base in 0u128..10_000_000_000u128,
            delta_var in 0i128..1_000_000_000i128,
            delta_one in 0i128..1_000_000_000i128,
        ) {
            let mut bd = make_breakdown(base, base, base, base, 0, 0, 0, 0);
            let deltas = SegmentDeltas {
                variable: Int128::from(delta_var),
                one_month: Int128::from(delta_one),
                three_month: Int128::zero(),
                six_month: Int128::zero(),
            };
            let var_before = bd.variable_amount;
            let one_before = bd.one_month_amount;

            apply_delta_to_regular_debt(&mut bd, &deltas);

            prop_assert!(bd.variable_amount >= var_before);
            prop_assert!(bd.one_month_amount >= one_before);
        }

        /// Negative deltas clamp to zero, never underflow.
        #[test]
        fn apply_regular_deltas_clamp_to_zero(
            base in 0u128..1_000_000u128,
            delta in 1i128..10_000_000i128,
        ) {
            let mut bd = make_breakdown(base, base, base, base, 0, 0, 0, 0);
            let deltas = SegmentDeltas {
                variable: Int128::from(-delta),
                one_month: Int128::from(-delta),
                three_month: Int128::from(-delta),
                six_month: Int128::from(-delta),
            };

            // This should not panic
            apply_delta_to_regular_debt(&mut bd, &deltas);

            // All should be >= 0 (guaranteed by Uint128)
            prop_assert!(bd.variable_amount >= Uint128::zero());
            prop_assert!(bd.one_month_amount >= Uint128::zero());
            prop_assert!(bd.three_month_amount >= Uint128::zero());
            prop_assert!(bd.six_month_amount >= Uint128::zero());
        }

        /// apply_delta_to_peg_debt doesn't touch regular fields.
        #[test]
        fn apply_peg_deltas_preserves_regular(
            base in 0u128..10_000_000_000u128,
            delta in 0i128..1_000_000_000i128,
        ) {
            let mut bd = make_breakdown(base, base, base, base, base, base, base, base);
            let deltas = SegmentDeltas {
                variable: Int128::from(delta),
                one_month: Int128::from(delta),
                three_month: Int128::from(delta),
                six_month: Int128::from(delta),
            };
            let var_before = bd.variable_amount;
            let one_before = bd.one_month_amount;

            apply_delta_to_peg_debt(&mut bd, &deltas);

            // Regular fields unchanged
            prop_assert_eq!(bd.variable_amount, var_before);
            prop_assert_eq!(bd.one_month_amount, one_before);
        }

        // ====================================================================
        // Property Tests: apply_delta_to_caps
        // ====================================================================

        /// Caps amounts never go below zero after negative deltas.
        #[test]
        fn apply_caps_deltas_no_underflow(
            base in 0u128..10_000_000u128,
            delta in 0i128..100_000_000i128,
        ) {
            let mut caps = default_caps();
            caps.one_month.amount = Uint128::new(base);
            caps.three_month.amount = Uint128::new(base);
            caps.six_month.amount = Uint128::new(base);

            let deltas = SegmentDeltas {
                variable: Int128::from(-delta), // ignored for caps
                one_month: Int128::from(-delta),
                three_month: Int128::from(-delta),
                six_month: Int128::from(-delta),
            };

            apply_delta_to_caps(&mut caps, &deltas);

            prop_assert!(caps.one_month.amount >= Uint128::zero());
            prop_assert!(caps.three_month.amount >= Uint128::zero());
            prop_assert!(caps.six_month.amount >= Uint128::zero());
        }
    }
}
