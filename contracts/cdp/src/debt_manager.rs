use cosmwasm_std::{Decimal, Env, Int128, StdError, StdResult, Uint128};
use membrane::math::decimal_multiplication;
use membrane::types::{
    Basket, CreditAssetBreakdown, FixedRate, FixedRateEnd, FixedRateCap, FixedRateCaps, Rates, RateSegment,
};

use crate::rates::{calculate_fixed_rate_accrual, check_and_handle_fixed_rate_expiration};
use crate::ContractError;

/// Per-segment changes for basket tracking
#[derive(Default, Clone, Debug)]
pub struct SegmentDeltas {
    pub variable: Int128,
    pub one_month: Int128,
    pub three_month: Int128,
    pub six_month: Int128,
}

/// Fractions for one pool (extracted from DebtSplit)
#[derive(Default, Clone, Debug)]
pub struct SegmentSplit {
    pub variable: Option<Decimal>,
    pub one_month: Option<Decimal>,
    pub three_month: Option<Decimal>,
    pub six_month: Option<Decimal>,
    pub one_month_rollover: Option<bool>,
    pub three_month_rollover: Option<bool>,
    pub six_month_rollover: Option<bool>,
}

/// Read-only view of breakdown amounts for cap checking
#[derive(Default, Clone, Debug)]
pub struct BreakdownView {
    pub variable: Uint128,
    pub one_month: Uint128,
    pub three_month: Uint128,
    pub six_month: Uint128,
}

/// Get total debt from segments
pub fn get_total_from_segments(segments: &[RateSegment]) -> Uint128 {
    segments.iter().map(|s| s.amount).sum()
}

/// Accrue interest on a set of rate segments.
/// `rate_of_change` is the weighted avg index ratio for variable rate segments.
/// Fixed rate segments use their own fixed rate for time-based accrual.
/// Returns (total_accrued, deltas for basket tracking).
pub fn accrue_segments(
    segments: &mut Vec<RateSegment>,
    rate_of_change: Decimal,
    last_accrued_time: u64,
    current_time: u64,
    rates: &Rates,
    collateral_weighted_rate: Decimal,
) -> StdResult<(Uint128, SegmentDeltas)> {
    let mut total_accrued = Uint128::zero();
    let mut deltas = SegmentDeltas::default();

    for segment in segments.iter_mut() {
        let amount_before = segment.amount;
        let was_fixed = segment.fixed_rate.is_some();
        let duration_before = segment
            .fixed_rate
            .as_ref()
            .map(|fr| fr.end.duration_months)
            .unwrap_or(0);

        let segment_accrued = if let Some(ref fixed_rate) = segment.fixed_rate {
            calculate_fixed_rate_accrual(
                segment.amount,
                fixed_rate.rate,
                last_accrued_time,
                current_time,
            )?
        } else {
            let new_amount = decimal_multiplication(
                Decimal::from_ratio(segment.amount, Uint128::new(1)),
                rate_of_change,
            )?
            .to_uint_floor();
            if new_amount > segment.amount {
                new_amount - segment.amount
            } else {
                Uint128::zero()
            }
        };

        if segment_accrued > Uint128::zero() {
            segment.amount += segment_accrued;
            total_accrued += segment_accrued;
        }

        // Check and handle fixed rate expiration
        let (_was_fixed_after, _duration_after, converted_to_variable) =
            check_and_handle_fixed_rate_expiration(segment, rates, current_time, collateral_weighted_rate)?;

        let amount_after = segment.amount;
        let segment_delta = amount_after
            .checked_sub(amount_before)
            .unwrap_or(Uint128::zero());

        // Track deltas for basket
        if was_fixed && converted_to_variable {
            // Moved from fixed to variable
            match duration_before {
                1 => deltas.one_month -= Int128::from(amount_before.u128() as i128),
                3 => deltas.three_month -= Int128::from(amount_before.u128() as i128),
                6 => deltas.six_month -= Int128::from(amount_before.u128() as i128),
                _ => {}
            }
            deltas.variable += Int128::from(amount_after.u128() as i128);
        } else if was_fixed {
            match duration_before {
                1 => deltas.one_month += Int128::from(segment_delta.u128() as i128),
                3 => deltas.three_month += Int128::from(segment_delta.u128() as i128),
                6 => deltas.six_month += Int128::from(segment_delta.u128() as i128),
                _ => {}
            }
        } else {
            deltas.variable += Int128::from(segment_delta.u128() as i128);
        }
    }

    Ok((total_accrued, deltas))
}

/// Repay segments in default priority order: variable -> 1mo -> 3mo -> 6mo
/// Returns (amount_applied, deltas for basket tracking)
pub fn repay_segments_default_order(
    segments: &mut Vec<RateSegment>,
    amount: Uint128,
) -> (Uint128, SegmentDeltas) {
    let mut remaining = amount;
    let mut deltas = SegmentDeltas::default();

    // Variable first
    for segment in segments.iter_mut() {
        if remaining.is_zero() {
            break;
        }
        if segment.fixed_rate.is_none() && segment.amount > Uint128::zero() {
            let repay = std::cmp::min(remaining, segment.amount);
            segment.amount -= repay;
            deltas.variable -= Int128::from(repay.u128() as i128);
            remaining -= repay;
        }
    }

    // Fixed by duration (1, 3, 6)
    for duration in [1u8, 3, 6] {
        if remaining.is_zero() {
            break;
        }
        for segment in segments.iter_mut() {
            if remaining.is_zero() {
                break;
            }
            if let Some(ref fr) = segment.fixed_rate {
                if fr.end.duration_months == duration && segment.amount > Uint128::zero() {
                    let repay = std::cmp::min(remaining, segment.amount);
                    segment.amount -= repay;
                    match duration {
                        1 => deltas.one_month -= Int128::from(repay.u128() as i128),
                        3 => deltas.three_month -= Int128::from(repay.u128() as i128),
                        6 => deltas.six_month -= Int128::from(repay.u128() as i128),
                        _ => {}
                    }
                    remaining -= repay;
                }
            }
        }
    }

    // Clean up zero segments
    segments.retain(|s| s.amount > Uint128::zero());

    (amount - remaining, deltas)
}

/// Repay segments according to targeted split fractions
pub fn repay_segments_targeted(
    segments: &mut Vec<RateSegment>,
    amount: Uint128,
    split: &SegmentSplit,
) -> StdResult<(Uint128, SegmentDeltas)> {
    let mut remaining = amount;
    let mut deltas = SegmentDeltas::default();

    // Variable
    if let Some(var_ratio) = split.variable {
        if !var_ratio.is_zero() {
            let var_repay = decimal_multiplication(
                var_ratio,
                Decimal::from_ratio(amount, Uint128::new(1)),
            )?
            .to_uint_floor();
            let var_repay = std::cmp::min(var_repay, remaining);
            for segment in segments.iter_mut() {
                if segment.fixed_rate.is_none() && segment.amount > Uint128::zero() {
                    let repay = std::cmp::min(var_repay, segment.amount);
                    segment.amount -= repay;
                    deltas.variable -= Int128::from(repay.u128() as i128);
                    remaining -= repay;
                    break; // One variable segment
                }
            }
        }
    }

    // Fixed by duration
    let durations = [
        (1u8, split.one_month),
        (3, split.three_month),
        (6, split.six_month),
    ];
    for (duration, ratio_opt) in durations {
        if let Some(ratio) = ratio_opt {
            if !ratio.is_zero() && remaining > Uint128::zero() {
                let fixed_repay = decimal_multiplication(
                    ratio,
                    Decimal::from_ratio(amount, Uint128::new(1)),
                )?
                .to_uint_floor();
                let fixed_repay = std::cmp::min(fixed_repay, remaining);
                for segment in segments.iter_mut() {
                    if let Some(ref fr) = segment.fixed_rate {
                        if fr.end.duration_months == duration && segment.amount > Uint128::zero() {
                            let repay = std::cmp::min(fixed_repay, segment.amount);
                            segment.amount -= repay;
                            match duration {
                                1 => deltas.one_month -= Int128::from(repay.u128() as i128),
                                3 => deltas.three_month -= Int128::from(repay.u128() as i128),
                                6 => deltas.six_month -= Int128::from(repay.u128() as i128),
                                _ => {}
                            }
                            remaining -= repay;
                            break;
                        }
                    }
                }
            }
        }
    }

    segments.retain(|s| s.amount > Uint128::zero());

    Ok((amount - remaining, deltas))
}

/// Create segments from a debt split allocation.
/// Appends new segments to `existing_segments`.
/// `base_rate` is the variable rate used as the basis for fixed rate calculations.
/// `combined_breakdown` is used for shared cap checking (regular + peg totals).
pub fn create_segments_from_split(
    amount: Uint128,
    split: &SegmentSplit,
    fixed_rate_caps: &FixedRateCaps,
    base_rate: Decimal,
    existing_segments: &mut Vec<RateSegment>,
    combined_breakdown: &BreakdownView,
    total_debt_after: Uint128,
    env: &Env,
) -> Result<SegmentDeltas, ContractError> {
    if amount.is_zero() {
        return Ok(SegmentDeltas::default());
    }

    let mut deltas = SegmentDeltas::default();

    let variable_amount = decimal_multiplication(
        split.variable.unwrap_or(Decimal::zero()),
        Decimal::from_ratio(amount, Uint128::new(1)),
    )
    .map_err(|e| ContractError::Std(e))?
    .to_uint_floor();
    let one_month_amount = decimal_multiplication(
        split.one_month.unwrap_or(Decimal::zero()),
        Decimal::from_ratio(amount, Uint128::new(1)),
    )
    .map_err(|e| ContractError::Std(e))?
    .to_uint_floor();
    let three_month_amount = decimal_multiplication(
        split.three_month.unwrap_or(Decimal::zero()),
        Decimal::from_ratio(amount, Uint128::new(1)),
    )
    .map_err(|e| ContractError::Std(e))?
    .to_uint_floor();
    let six_month_amount = decimal_multiplication(
        split.six_month.unwrap_or(Decimal::zero()),
        Decimal::from_ratio(amount, Uint128::new(1)),
    )
    .map_err(|e| ContractError::Std(e))?
    .to_uint_floor();

    // Check shared caps (combined regular + peg)
    let new_one_month_total = combined_breakdown.one_month + one_month_amount;
    let new_three_month_total = combined_breakdown.three_month + three_month_amount;
    let new_six_month_total = combined_breakdown.six_month + six_month_amount;

    if !fixed_rate_caps.one_month.cap.is_zero() {
        let cap_amount = decimal_multiplication(
            fixed_rate_caps.one_month.cap,
            Decimal::from_ratio(total_debt_after, Uint128::new(1)),
        )
        .map_err(|e| ContractError::Std(e))?
        .to_uint_floor();
        if new_one_month_total > cap_amount {
            return Err(ContractError::CustomError {
                val: String::from("1-month fixed rate cap exceeded"),
            });
        }
    }
    if !fixed_rate_caps.three_month.cap.is_zero() {
        let cap_amount = decimal_multiplication(
            fixed_rate_caps.three_month.cap,
            Decimal::from_ratio(total_debt_after, Uint128::new(1)),
        )
        .map_err(|e| ContractError::Std(e))?
        .to_uint_floor();
        if new_three_month_total > cap_amount {
            return Err(ContractError::CustomError {
                val: String::from("3-month fixed rate cap exceeded"),
            });
        }
    }
    if !fixed_rate_caps.six_month.cap.is_zero() {
        let cap_amount = decimal_multiplication(
            fixed_rate_caps.six_month.cap,
            Decimal::from_ratio(total_debt_after, Uint128::new(1)),
        )
        .map_err(|e| ContractError::Std(e))?
        .to_uint_floor();
        if new_six_month_total > cap_amount {
            return Err(ContractError::CustomError {
                val: String::from("6-month fixed rate cap exceeded"),
            });
        }
    }

    // Add variable amount
    if variable_amount > Uint128::zero() {
        if let Some(segment) = existing_segments.iter_mut().find(|s| s.fixed_rate.is_none()) {
            segment.amount += variable_amount;
        } else {
            existing_segments.push(RateSegment {
                amount: variable_amount,
                fixed_rate: None,
            });
        }
        deltas.variable += Int128::from(variable_amount.u128() as i128);
    }

    // Add fixed segments
    let fixed_durations = [
        (1u8, one_month_amount, &fixed_rate_caps.one_month, split.one_month_rollover.unwrap_or(false)),
        (3, three_month_amount, &fixed_rate_caps.three_month, split.three_month_rollover.unwrap_or(false)),
        (6, six_month_amount, &fixed_rate_caps.six_month, split.six_month_rollover.unwrap_or(false)),
    ];

    for (duration_months, amt, cap, rollover) in fixed_durations {
        if amt > Uint128::zero() {
            let rate = decimal_multiplication(base_rate, cap.multiplier)
                .map_err(|e| ContractError::Std(e))?;
            let seconds_per_month = 2_592_000u64;
            let end_time = env
                .block
                .time
                .seconds()
                .checked_add(seconds_per_month * duration_months as u64)
                .ok_or_else(|| ContractError::Std(StdError::generic_err("End time overflow")))?;

            existing_segments.push(RateSegment {
                amount: amt,
                fixed_rate: Some(FixedRate {
                    rate,
                    end: FixedRateEnd {
                        end_time,
                        rollover,
                        duration_months,
                    },
                }),
            });

            match duration_months {
                1 => deltas.one_month += Int128::from(amt.u128() as i128),
                3 => deltas.three_month += Int128::from(amt.u128() as i128),
                6 => deltas.six_month += Int128::from(amt.u128() as i128),
                _ => {}
            }
        }
    }

    Ok(deltas)
}

/// Reduce segments proportionally during liquidation
pub fn reduce_segments_proportionally(
    segments: &mut Vec<RateSegment>,
    pool_total: Uint128,
    total_debt: Uint128,
    repay_amount: Uint128,
) -> SegmentDeltas {
    if pool_total.is_zero() || total_debt.is_zero() || repay_amount.is_zero() {
        return SegmentDeltas::default();
    }

    // Pool's share of the repayment
    let pool_repay = decimal_multiplication(
        Decimal::from_ratio(repay_amount, Uint128::new(1)),
        Decimal::from_ratio(pool_total, total_debt),
    )
    .unwrap_or(Decimal::zero())
    .to_uint_floor();

    let mut remaining = pool_repay;
    let mut deltas = SegmentDeltas::default();

    // Variable first
    for segment in segments.iter_mut() {
        if remaining.is_zero() {
            break;
        }
        if segment.fixed_rate.is_none() && segment.amount > Uint128::zero() {
            let repay = std::cmp::min(remaining, segment.amount);
            segment.amount -= repay;
            deltas.variable -= Int128::from(repay.u128() as i128);
            remaining -= repay;
        }
    }

    // Peg debt second (skipped here - peg is its own pool)
    // Fixed by duration
    for duration in [1u8, 3, 6] {
        if remaining.is_zero() {
            break;
        }
        for segment in segments.iter_mut() {
            if remaining.is_zero() {
                break;
            }
            if let Some(ref fr) = segment.fixed_rate {
                if fr.end.duration_months == duration && segment.amount > Uint128::zero() {
                    let repay = std::cmp::min(remaining, segment.amount);
                    segment.amount -= repay;
                    match duration {
                        1 => deltas.one_month -= Int128::from(repay.u128() as i128),
                        3 => deltas.three_month -= Int128::from(repay.u128() as i128),
                        6 => deltas.six_month -= Int128::from(repay.u128() as i128),
                        _ => {}
                    }
                    remaining -= repay;
                }
            }
        }
    }

    segments.retain(|s| s.amount > Uint128::zero());
    deltas
}

/// Apply segment deltas to CreditAssetBreakdown (regular pool)
pub fn apply_delta_to_regular_debt(breakdown: &mut CreditAssetBreakdown, deltas: &SegmentDeltas) {
    apply_int128_delta(&mut breakdown.variable_amount, deltas.variable);
    apply_int128_delta(&mut breakdown.one_month_amount, deltas.one_month);
    apply_int128_delta(&mut breakdown.three_month_amount, deltas.three_month);
    apply_int128_delta(&mut breakdown.six_month_amount, deltas.six_month);
}

/// Apply segment deltas to CreditAssetBreakdown (peg pool)
pub fn apply_delta_to_peg_debt(breakdown: &mut CreditAssetBreakdown, deltas: &SegmentDeltas) {
    apply_int128_delta(&mut breakdown.peg_variable_amount, deltas.variable);
    apply_int128_delta(&mut breakdown.peg_one_month_amount, deltas.one_month);
    apply_int128_delta(&mut breakdown.peg_three_month_amount, deltas.three_month);
    apply_int128_delta(&mut breakdown.peg_six_month_amount, deltas.six_month);
}

/// Apply segment deltas to FixedRateCaps
pub fn apply_delta_to_caps(caps: &mut FixedRateCaps, deltas: &SegmentDeltas) {
    apply_int128_delta(&mut caps.one_month.amount, deltas.one_month);
    apply_int128_delta(&mut caps.three_month.amount, deltas.three_month);
    apply_int128_delta(&mut caps.six_month.amount, deltas.six_month);
}

/// Apply an Int128 delta to a Uint128 value (clamp to zero on negative)
fn apply_int128_delta(value: &mut Uint128, delta: Int128) {
    if delta.is_negative() {
        let abs = (-delta).unsigned_abs();
        *value = value.checked_sub(abs).unwrap_or(Uint128::zero());
    } else {
        let pos = delta.unsigned_abs();
        *value = value.checked_add(pos).unwrap_or(*value);
    }
}

/// Apply discount proportionally across segments
pub fn apply_discount_to_segments(
    segments: &mut Vec<RateSegment>,
    discount_amount: Uint128,
    total_debt: Uint128,
) -> SegmentDeltas {
    let mut deltas = SegmentDeltas::default();
    if discount_amount.is_zero() || total_debt.is_zero() {
        return deltas;
    }

    for segment in segments.iter_mut() {
        let proportion = if total_debt > Uint128::zero() {
            membrane::math::decimal_division(
                Decimal::from_ratio(segment.amount, Uint128::new(1)),
                Decimal::from_ratio(total_debt, Uint128::new(1)),
            )
            .unwrap_or(Decimal::zero())
        } else {
            Decimal::zero()
        };

        let segment_discount = decimal_multiplication(
            proportion,
            Decimal::from_ratio(discount_amount, Uint128::new(1)),
        )
        .unwrap_or(Decimal::zero())
        .to_uint_floor();

        segment.amount = segment
            .amount
            .checked_sub(segment_discount)
            .unwrap_or(Uint128::zero());

        if segment.fixed_rate.is_none() {
            deltas.variable -= Int128::from(segment_discount.u128() as i128);
        } else {
            match segment
                .fixed_rate
                .as_ref()
                .unwrap()
                .end
                .duration_months
            {
                1 => deltas.one_month -= Int128::from(segment_discount.u128() as i128),
                3 => deltas.three_month -= Int128::from(segment_discount.u128() as i128),
                6 => deltas.six_month -= Int128::from(segment_discount.u128() as i128),
                _ => {}
            }
        }
    }

    deltas
}

/// Build a combined BreakdownView for shared cap checking (regular + peg)
pub fn combined_breakdown_for_caps(breakdown: &CreditAssetBreakdown) -> BreakdownView {
    BreakdownView {
        variable: breakdown.variable_amount + breakdown.peg_variable_amount,
        one_month: breakdown.one_month_amount + breakdown.peg_one_month_amount,
        three_month: breakdown.three_month_amount + breakdown.peg_three_month_amount,
        six_month: breakdown.six_month_amount + breakdown.peg_six_month_amount,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::mock_env;
    use std::str::FromStr;

    // ========== Helpers ==========

    fn var_segment(amount: u128) -> RateSegment {
        RateSegment {
            amount: Uint128::new(amount),
            fixed_rate: None,
        }
    }

    fn fixed_segment(amount: u128, duration: u8, rate_pct: u64, end_time: u64, rollover: bool) -> RateSegment {
        RateSegment {
            amount: Uint128::new(amount),
            fixed_rate: Some(FixedRate {
                rate: Decimal::percent(rate_pct),
                end: FixedRateEnd {
                    end_time,
                    rollover,
                    duration_months: duration,
                },
            }),
        }
    }

    fn default_caps() -> FixedRateCaps {
        FixedRateCaps {
            one_month: FixedRateCap {
                cap: Decimal::percent(20),
                amount: Uint128::zero(),
                multiplier: Decimal::from_str("1.1").unwrap(),
            },
            three_month: FixedRateCap {
                cap: Decimal::percent(20),
                amount: Uint128::zero(),
                multiplier: Decimal::from_str("1.3").unwrap(),
            },
            six_month: FixedRateCap {
                cap: Decimal::percent(10),
                amount: Uint128::zero(),
                multiplier: Decimal::from_str("1.7").unwrap(),
            },
        }
    }

    fn default_rates() -> Rates {
        Rates {
            current_adaptive_rate: vec![],
            peg_current_adaptive_rate: vec![],
            lastest_collateral_rates: vec![],
            base_interest_rate: Decimal::percent(1), // 1% default
            fixed_rate_caps: default_caps(),
            rates_last_accrued: 1000,
            credit_last_accrued: 1000,
            cpc_margin_of_error: Decimal::one(),
            negative_rates: false,
            acquisition_bump_rate: Decimal::zero(),
        }
    }

    fn zero_caps() -> FixedRateCaps {
        FixedRateCaps {
            one_month: FixedRateCap { cap: Decimal::zero(), amount: Uint128::zero(), multiplier: Decimal::one() },
            three_month: FixedRateCap { cap: Decimal::zero(), amount: Uint128::zero(), multiplier: Decimal::one() },
            six_month: FixedRateCap { cap: Decimal::zero(), amount: Uint128::zero(), multiplier: Decimal::one() },
        }
    }

    fn make_breakdown(
        var: u128, one: u128, three: u128, six: u128,
        peg_var: u128, peg_one: u128, peg_three: u128, peg_six: u128,
    ) -> CreditAssetBreakdown {
        CreditAssetBreakdown {
            info: membrane::types::AssetInfo::NativeToken { denom: "cdt".to_string() },
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

    // ========== get_total_from_segments ==========

    #[test]
    fn test_get_total_empty() {
        assert_eq!(get_total_from_segments(&[]), Uint128::zero());
    }

    #[test]
    fn test_get_total_single_variable() {
        let segs = vec![var_segment(1000)];
        assert_eq!(get_total_from_segments(&segs), Uint128::new(1000));
    }

    #[test]
    fn test_get_total_mixed() {
        let segs = vec![
            var_segment(500),
            fixed_segment(300, 1, 5, 9999999, false),
            fixed_segment(200, 3, 7, 9999999, false),
        ];
        assert_eq!(get_total_from_segments(&segs), Uint128::new(1000));
    }

    // ========== repay_segments_default_order ==========

    #[test]
    fn test_repay_default_variable_first() {
        let mut segs = vec![
            var_segment(500),
            fixed_segment(300, 1, 5, 9999999, false),
            fixed_segment(200, 3, 7, 9999999, false),
        ];
        let (applied, deltas) = repay_segments_default_order(&mut segs, Uint128::new(400));
        assert_eq!(applied, Uint128::new(400));
        // Variable should be reduced first: 500 - 400 = 100
        assert_eq!(deltas.variable, Int128::from(-400i128));
        assert_eq!(deltas.one_month, Int128::zero());
        assert_eq!(deltas.three_month, Int128::zero());
        // Variable segment reduced to 100
        assert_eq!(get_total_from_segments(&segs), Uint128::new(600));
    }

    #[test]
    fn test_repay_default_spills_to_fixed() {
        let mut segs = vec![
            var_segment(200),
            fixed_segment(300, 1, 5, 9999999, false),
            fixed_segment(400, 3, 7, 9999999, false),
        ];
        let (applied, deltas) = repay_segments_default_order(&mut segs, Uint128::new(600));
        assert_eq!(applied, Uint128::new(600));
        // 200 from variable, 300 from 1-month, 100 from 3-month
        assert_eq!(deltas.variable, Int128::from(-200i128));
        assert_eq!(deltas.one_month, Int128::from(-300i128));
        assert_eq!(deltas.three_month, Int128::from(-100i128));
        assert_eq!(get_total_from_segments(&segs), Uint128::new(300)); // only 300 of 3mo left
    }

    #[test]
    fn test_repay_default_full_repay() {
        let mut segs = vec![
            var_segment(100),
            fixed_segment(200, 3, 7, 9999999, false),
        ];
        let (applied, deltas) = repay_segments_default_order(&mut segs, Uint128::new(500));
        // Can only repay 300 total
        assert_eq!(applied, Uint128::new(300));
        assert_eq!(deltas.variable, Int128::from(-100i128));
        assert_eq!(deltas.three_month, Int128::from(-200i128));
        assert!(segs.is_empty()); // all zero segments removed
    }

    #[test]
    fn test_repay_default_zero_amount() {
        let mut segs = vec![var_segment(500)];
        let (applied, deltas) = repay_segments_default_order(&mut segs, Uint128::zero());
        assert_eq!(applied, Uint128::zero());
        assert_eq!(deltas.variable, Int128::zero());
        assert_eq!(segs.len(), 1);
    }

    #[test]
    fn test_repay_default_empty_segments() {
        let mut segs: Vec<RateSegment> = vec![];
        let (applied, _deltas) = repay_segments_default_order(&mut segs, Uint128::new(100));
        assert_eq!(applied, Uint128::zero());
    }

    #[test]
    fn test_repay_default_priority_1mo_before_3mo_before_6mo() {
        let mut segs = vec![
            fixed_segment(100, 6, 10, 9999999, false),
            fixed_segment(100, 1, 5, 9999999, false),
            fixed_segment(100, 3, 7, 9999999, false),
        ];
        let (applied, deltas) = repay_segments_default_order(&mut segs, Uint128::new(250));
        assert_eq!(applied, Uint128::new(250));
        // Priority: 1mo (100), then 3mo (100), then 6mo (50)
        assert_eq!(deltas.one_month, Int128::from(-100i128));
        assert_eq!(deltas.three_month, Int128::from(-100i128));
        assert_eq!(deltas.six_month, Int128::from(-50i128));
    }

    // ========== repay_segments_targeted ==========

    #[test]
    fn test_repay_targeted_split() {
        let mut segs = vec![
            var_segment(1000),
            fixed_segment(1000, 3, 7, 9999999, false),
        ];
        let split = SegmentSplit {
            variable: Some(Decimal::percent(60)),
            three_month: Some(Decimal::percent(40)),
            ..SegmentSplit::default()
        };
        let (applied, deltas) = repay_segments_targeted(&mut segs, Uint128::new(500), &split).unwrap();
        assert_eq!(applied, Uint128::new(500));
        // 60% of 500 = 300 from variable, 40% of 500 = 200 from 3mo
        assert_eq!(deltas.variable, Int128::from(-300i128));
        assert_eq!(deltas.three_month, Int128::from(-200i128));
    }

    #[test]
    fn test_repay_targeted_exceeds_segment() {
        let mut segs = vec![
            var_segment(100),    // only 100 available
            fixed_segment(1000, 1, 5, 9999999, false),
        ];
        let split = SegmentSplit {
            variable: Some(Decimal::percent(80)),  // wants 800 but only 100 available
            one_month: Some(Decimal::percent(20)),
            ..SegmentSplit::default()
        };
        let (applied, deltas) = repay_segments_targeted(&mut segs, Uint128::new(1000), &split).unwrap();
        // Variable: min(800, 100) = 100; 1mo: min(200, 900) = 200
        assert_eq!(deltas.variable, Int128::from(-100i128));
        assert_eq!(deltas.one_month, Int128::from(-200i128));
        assert_eq!(applied, Uint128::new(300));
    }

    #[test]
    fn test_repay_targeted_all_none() {
        let mut segs = vec![var_segment(500)];
        let split = SegmentSplit::default(); // all None
        let (applied, _deltas) = repay_segments_targeted(&mut segs, Uint128::new(100), &split).unwrap();
        assert_eq!(applied, Uint128::zero()); // nothing targeted
    }

    // ========== create_segments_from_split ==========

    #[test]
    fn test_create_all_variable() {
        let env = mock_env();
        let caps = zero_caps();
        let mut segs = vec![];
        let split = SegmentSplit {
            variable: Some(Decimal::one()),
            ..SegmentSplit::default()
        };
        let view = BreakdownView::default();
        let deltas = create_segments_from_split(
            Uint128::new(1000), &split, &caps, Decimal::percent(5),
            &mut segs, &view, Uint128::new(1000), &env,
        ).unwrap();
        assert_eq!(segs.len(), 1);
        assert!(segs[0].fixed_rate.is_none());
        assert_eq!(segs[0].amount, Uint128::new(1000));
        assert_eq!(deltas.variable, Int128::from(1000i128));
    }

    #[test]
    fn test_create_mixed_split() {
        let env = mock_env();
        let caps = zero_caps(); // no cap enforcement
        let mut segs = vec![];
        let split = SegmentSplit {
            variable: Some(Decimal::percent(50)),
            one_month: Some(Decimal::percent(30)),
            three_month: Some(Decimal::percent(20)),
            ..SegmentSplit::default()
        };
        let view = BreakdownView::default();
        let deltas = create_segments_from_split(
            Uint128::new(1000), &split, &caps, Decimal::percent(5),
            &mut segs, &view, Uint128::new(1000), &env,
        ).unwrap();
        assert_eq!(segs.len(), 3); // variable + 1mo + 3mo
        assert_eq!(deltas.variable, Int128::from(500i128));
        assert_eq!(deltas.one_month, Int128::from(300i128));
        assert_eq!(deltas.three_month, Int128::from(200i128));
        // Fixed rate should be base_rate * multiplier
        assert!(segs[1].fixed_rate.is_some());
        assert!(segs[2].fixed_rate.is_some());
    }

    #[test]
    fn test_create_appends_to_existing_variable() {
        let env = mock_env();
        let caps = zero_caps();
        let mut segs = vec![var_segment(500)]; // existing variable
        let split = SegmentSplit {
            variable: Some(Decimal::one()),
            ..SegmentSplit::default()
        };
        let view = BreakdownView::default();
        create_segments_from_split(
            Uint128::new(300), &split, &caps, Decimal::percent(5),
            &mut segs, &view, Uint128::new(800), &env,
        ).unwrap();
        assert_eq!(segs.len(), 1); // still 1 segment, merged
        assert_eq!(segs[0].amount, Uint128::new(800));
    }

    #[test]
    fn test_create_zero_amount() {
        let env = mock_env();
        let caps = zero_caps();
        let mut segs = vec![];
        let split = SegmentSplit {
            variable: Some(Decimal::one()),
            ..SegmentSplit::default()
        };
        let view = BreakdownView::default();
        let deltas = create_segments_from_split(
            Uint128::zero(), &split, &caps, Decimal::percent(5),
            &mut segs, &view, Uint128::zero(), &env,
        ).unwrap();
        assert!(segs.is_empty());
        assert_eq!(deltas.variable, Int128::zero());
    }

    #[test]
    fn test_create_cap_exceeded() {
        let env = mock_env();
        let caps = default_caps(); // 20% cap on 1-month
        let mut segs = vec![];
        let split = SegmentSplit {
            one_month: Some(Decimal::one()), // all into 1-month
            ..SegmentSplit::default()
        };
        // Existing breakdown already has 200 in 1mo out of 1000 total = 20%
        let view = BreakdownView {
            one_month: Uint128::new(200),
            ..BreakdownView::default()
        };
        // Adding 100 more at total_debt_after=1100 → new_total=300, cap=220 (20% of 1100) → exceeds
        let result = create_segments_from_split(
            Uint128::new(100), &split, &caps, Decimal::percent(5),
            &mut segs, &view, Uint128::new(1100), &env,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_create_cap_not_exceeded() {
        let env = mock_env();
        let caps = default_caps(); // 20% cap on 1-month
        let mut segs = vec![];
        let split = SegmentSplit {
            one_month: Some(Decimal::one()),
            ..SegmentSplit::default()
        };
        let view = BreakdownView::default(); // 0 existing
        // Adding 100 at total_debt_after=1000 → new_total=100, cap=200 (20% of 1000) → OK
        let result = create_segments_from_split(
            Uint128::new(100), &split, &caps, Decimal::percent(5),
            &mut segs, &view, Uint128::new(1000), &env,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_fixed_rate_calculation() {
        let env = mock_env();
        let caps = default_caps(); // 1mo multiplier = 1.1
        let mut segs = vec![];
        let split = SegmentSplit {
            one_month: Some(Decimal::one()),
            one_month_rollover: Some(true),
            ..SegmentSplit::default()
        };
        let view = BreakdownView::default();
        create_segments_from_split(
            Uint128::new(100), &split, &caps, Decimal::percent(10), // 10% base
            &mut segs, &view, Uint128::new(1000), &env,
        ).unwrap();
        assert_eq!(segs.len(), 1);
        let fr = segs[0].fixed_rate.as_ref().unwrap();
        // rate = 10% * 1.1 = 11%
        assert_eq!(fr.rate, Decimal::percent(11));
        assert!(fr.end.rollover);
        assert_eq!(fr.end.duration_months, 1);
        // end_time = env.block.time + 2_592_000
        let expected_end = env.block.time.seconds() + 2_592_000;
        assert_eq!(fr.end.end_time, expected_end);
    }

    // ========== reduce_segments_proportionally ==========

    #[test]
    fn test_reduce_proportionally_single_pool() {
        let mut segs = vec![
            var_segment(600),
            fixed_segment(400, 3, 7, 9999999, false),
        ];
        // pool_total = 1000, total_debt = 1000, repay = 500
        let deltas = reduce_segments_proportionally(&mut segs, Uint128::new(1000), Uint128::new(1000), Uint128::new(500));
        // pool_repay = 500 * (1000/1000) = 500
        // variable first: min(500, 600) = 500
        assert_eq!(deltas.variable, Int128::from(-500i128));
        assert_eq!(deltas.three_month, Int128::zero());
        assert_eq!(get_total_from_segments(&segs), Uint128::new(500));
    }

    #[test]
    fn test_reduce_proportionally_partial_pool() {
        let mut segs = vec![
            var_segment(300),
            fixed_segment(200, 1, 5, 9999999, false),
        ];
        // pool_total = 500, total_debt = 1000 (other pool has 500), repay = 400
        let deltas = reduce_segments_proportionally(&mut segs, Uint128::new(500), Uint128::new(1000), Uint128::new(400));
        // pool_repay = 400 * (500/1000) = 200
        // variable first: min(200, 300) = 200
        assert_eq!(deltas.variable, Int128::from(-200i128));
        assert_eq!(deltas.one_month, Int128::zero());
    }

    #[test]
    fn test_reduce_proportionally_zero_inputs() {
        let mut segs = vec![var_segment(100)];
        let deltas = reduce_segments_proportionally(&mut segs, Uint128::zero(), Uint128::new(100), Uint128::new(50));
        assert_eq!(deltas.variable, Int128::zero());

        let deltas = reduce_segments_proportionally(&mut segs, Uint128::new(100), Uint128::zero(), Uint128::new(50));
        assert_eq!(deltas.variable, Int128::zero());

        let deltas = reduce_segments_proportionally(&mut segs, Uint128::new(100), Uint128::new(100), Uint128::zero());
        assert_eq!(deltas.variable, Int128::zero());
    }

    // ========== apply_delta_to_regular_debt ==========

    #[test]
    fn test_apply_regular_deltas_positive() {
        let mut bd = make_breakdown(100, 200, 300, 400, 0, 0, 0, 0);
        let deltas = SegmentDeltas {
            variable: Int128::from(50i128),
            one_month: Int128::from(25i128),
            three_month: Int128::from(10i128),
            six_month: Int128::from(5i128),
        };
        apply_delta_to_regular_debt(&mut bd, &deltas);
        assert_eq!(bd.variable_amount, Uint128::new(150));
        assert_eq!(bd.one_month_amount, Uint128::new(225));
        assert_eq!(bd.three_month_amount, Uint128::new(310));
        assert_eq!(bd.six_month_amount, Uint128::new(405));
    }

    #[test]
    fn test_apply_regular_deltas_negative() {
        let mut bd = make_breakdown(100, 200, 300, 400, 0, 0, 0, 0);
        let deltas = SegmentDeltas {
            variable: Int128::from(-50i128),
            one_month: Int128::from(-200i128), // exact match
            three_month: Int128::from(-500i128), // exceeds → clamp to 0
            six_month: Int128::from(-1i128),
        };
        apply_delta_to_regular_debt(&mut bd, &deltas);
        assert_eq!(bd.variable_amount, Uint128::new(50));
        assert_eq!(bd.one_month_amount, Uint128::zero());
        assert_eq!(bd.three_month_amount, Uint128::zero()); // clamped
        assert_eq!(bd.six_month_amount, Uint128::new(399));
    }

    // ========== apply_delta_to_peg_debt ==========

    #[test]
    fn test_apply_peg_deltas() {
        let mut bd = make_breakdown(0, 0, 0, 0, 100, 200, 300, 400);
        let deltas = SegmentDeltas {
            variable: Int128::from(10i128),
            one_month: Int128::from(-50i128),
            three_month: Int128::from(-300i128),
            six_month: Int128::from(100i128),
        };
        apply_delta_to_peg_debt(&mut bd, &deltas);
        assert_eq!(bd.peg_variable_amount, Uint128::new(110));
        assert_eq!(bd.peg_one_month_amount, Uint128::new(150));
        assert_eq!(bd.peg_three_month_amount, Uint128::zero());
        assert_eq!(bd.peg_six_month_amount, Uint128::new(500));
        // Regular amounts unchanged
        assert_eq!(bd.variable_amount, Uint128::zero());
    }

    // ========== apply_delta_to_caps ==========

    #[test]
    fn test_apply_caps_deltas() {
        let mut caps = default_caps();
        caps.one_month.amount = Uint128::new(100);
        caps.three_month.amount = Uint128::new(200);
        caps.six_month.amount = Uint128::new(300);

        let deltas = SegmentDeltas {
            variable: Int128::from(999i128), // ignored for caps
            one_month: Int128::from(-50i128),
            three_month: Int128::from(25i128),
            six_month: Int128::from(-400i128), // exceeds → clamp
        };
        apply_delta_to_caps(&mut caps, &deltas);
        assert_eq!(caps.one_month.amount, Uint128::new(50));
        assert_eq!(caps.three_month.amount, Uint128::new(225));
        assert_eq!(caps.six_month.amount, Uint128::zero()); // clamped
    }

    // ========== apply_discount_to_segments ==========

    #[test]
    fn test_discount_proportional() {
        let mut segs = vec![
            var_segment(600),    // 60%
            fixed_segment(400, 3, 7, 9999999, false), // 40%
        ];
        let deltas = apply_discount_to_segments(&mut segs, Uint128::new(100), Uint128::new(1000));
        // variable discount: 60/1000 * 100 = 6 (using segment proportion: 600/1000=0.6, 0.6*100=60)
        // Wait, total_debt is 1000, discount is 100. Proportions: 600/1000=0.6 → 60, 400/1000=0.4 → 40
        assert_eq!(segs[0].amount, Uint128::new(540)); // 600 - 60
        assert_eq!(segs[1].amount, Uint128::new(360)); // 400 - 40
        assert_eq!(deltas.variable, Int128::from(-60i128));
        assert_eq!(deltas.three_month, Int128::from(-40i128));
    }

    #[test]
    fn test_discount_zero_amount() {
        let mut segs = vec![var_segment(500)];
        let deltas = apply_discount_to_segments(&mut segs, Uint128::zero(), Uint128::new(500));
        assert_eq!(segs[0].amount, Uint128::new(500)); // unchanged
        assert_eq!(deltas.variable, Int128::zero());
    }

    #[test]
    fn test_discount_zero_total_debt() {
        let mut segs = vec![var_segment(500)];
        let deltas = apply_discount_to_segments(&mut segs, Uint128::new(100), Uint128::zero());
        assert_eq!(segs[0].amount, Uint128::new(500)); // unchanged
        assert_eq!(deltas.variable, Int128::zero());
    }

    // ========== combined_breakdown_for_caps ==========

    #[test]
    fn test_combined_breakdown_adds_pools() {
        let bd = make_breakdown(100, 200, 300, 400, 10, 20, 30, 40);
        let view = combined_breakdown_for_caps(&bd);
        assert_eq!(view.variable, Uint128::new(110));
        assert_eq!(view.one_month, Uint128::new(220));
        assert_eq!(view.three_month, Uint128::new(330));
        assert_eq!(view.six_month, Uint128::new(440));
    }

    #[test]
    fn test_combined_breakdown_empty() {
        let bd = make_breakdown(0, 0, 0, 0, 0, 0, 0, 0);
        let view = combined_breakdown_for_caps(&bd);
        assert_eq!(view.variable, Uint128::zero());
        assert_eq!(view.one_month, Uint128::zero());
    }

    // ========== accrue_segments (variable only, no expiration) ==========

    #[test]
    fn test_accrue_variable_rate_of_change() {
        // rate_of_change = 1.05 means 5% increase
        let mut segs = vec![var_segment(1000)];
        let rates = default_rates();

        let (accrued, deltas) = accrue_segments(
            &mut segs,
            Decimal::from_str("1.05").unwrap(), // 5% increase
            1000,
            2000,
            &rates,
            Decimal::percent(5),
        ).unwrap();

        // 1000 * 1.05 = 1050, accrued = 50
        assert_eq!(accrued, Uint128::new(50));
        assert_eq!(deltas.variable, Int128::from(50i128));
        assert_eq!(segs[0].amount, Uint128::new(1050));
    }

    #[test]
    fn test_accrue_no_change() {
        let mut segs = vec![var_segment(1000)];
        let rates = default_rates();

        let (accrued, deltas) = accrue_segments(
            &mut segs,
            Decimal::one(), // no change
            1000,
            2000,
            &rates,
            Decimal::percent(5),
        ).unwrap();
        assert_eq!(accrued, Uint128::zero());
        assert_eq!(deltas.variable, Int128::zero());
    }

    #[test]
    fn test_accrue_empty_segments() {
        let mut segs: Vec<RateSegment> = vec![];
        let rates = default_rates();

        let (accrued, _deltas) = accrue_segments(&mut segs, Decimal::one(), 1000, 2000, &rates, Decimal::percent(5)).unwrap();
        assert_eq!(accrued, Uint128::zero());
    }

    // ========== accrue_segments (fixed rate expiration) ==========

    #[test]
    fn test_accrue_fixed_expired_no_rollover() {
        // Fixed segment with end_time=1500, no rollover. Current time=2000 → expires → convert to variable
        let mut segs = vec![fixed_segment(1000, 3, 7, 1500, false)];
        let rates = default_rates();

        let (_accrued, deltas) = accrue_segments(&mut segs, Decimal::one(), 1000, 2000, &rates, Decimal::percent(5)).unwrap();
        // Fixed segment expired and converted to variable
        assert!(segs[0].fixed_rate.is_none());
        // Deltas: -original 3mo amount, +new variable amount (original + accrued)
        assert!(deltas.three_month.is_negative());
        assert!(deltas.variable > Int128::zero());
    }

    #[test]
    fn test_accrue_fixed_expired_with_rollover() {
        // Fixed segment with end_time=1500, rollover=true. Current time=2000 → rollover
        let mut segs = vec![fixed_segment(1000, 1, 5, 1500, true)];
        let rates = default_rates();

        let (_accrued, deltas) = accrue_segments(&mut segs, Decimal::one(), 1000, 2000, &rates, Decimal::percent(5)).unwrap();
        // Should remain fixed (rolled over)
        assert!(segs[0].fixed_rate.is_some());
        let fr = segs[0].fixed_rate.as_ref().unwrap();
        // New end_time should be current_time + 1 month
        assert_eq!(fr.end.end_time, 2000 + 2_592_000);
        // Delta should reflect accrual only (stayed in 1mo category)
        assert!(deltas.variable == Int128::zero());
    }

    #[test]
    fn test_accrue_fixed_not_expired() {
        // Fixed segment with end_time=9999999. Current time = 1_000_000 (large elapsed for visible accrual)
        let mut segs = vec![fixed_segment(1_000_000_000, 6, 8, 99999999, false)];
        let mut rates = default_rates();
        rates.rates_last_accrued = 0;
        rates.credit_last_accrued = 0;

        // 1B * 8% * 1_000_000s / 31_536_000s ≈ 2536
        let (accrued, deltas) = accrue_segments(&mut segs, Decimal::one(), 0, 1_000_000, &rates, Decimal::percent(5)).unwrap();
        assert!(accrued > Uint128::zero());
        assert!(segs[0].fixed_rate.is_some()); // still fixed
        assert!(deltas.six_month > Int128::zero()); // accrual delta in 6mo
        assert_eq!(deltas.variable, Int128::zero());
    }

    // ========== Cross-pool interaction tests ==========

    #[test]
    fn test_repay_regular_then_peg_default_order() {
        // Simulate the positions.rs default repay flow: regular first, peg second
        let mut regular_segs = vec![var_segment(300), fixed_segment(200, 1, 5, 9999999, false)];
        let mut peg_segs = vec![var_segment(500)];

        let (regular_applied, reg_deltas) = repay_segments_default_order(&mut regular_segs, Uint128::new(800));
        let remaining = Uint128::new(800).checked_sub(regular_applied).unwrap_or(Uint128::zero());
        let (peg_applied, peg_deltas) = repay_segments_default_order(&mut peg_segs, remaining);

        assert_eq!(regular_applied, Uint128::new(500)); // all regular repaid
        assert_eq!(peg_applied, Uint128::new(300)); // 300 of 500 peg repaid
        assert_eq!(reg_deltas.variable, Int128::from(-300i128));
        assert_eq!(reg_deltas.one_month, Int128::from(-200i128));
        assert_eq!(peg_deltas.variable, Int128::from(-300i128));

        // Apply deltas to breakdown
        let mut bd = make_breakdown(300, 200, 0, 0, 500, 0, 0, 0);
        apply_delta_to_regular_debt(&mut bd, &reg_deltas);
        apply_delta_to_peg_debt(&mut bd, &peg_deltas);
        assert_eq!(bd.variable_amount, Uint128::zero());
        assert_eq!(bd.one_month_amount, Uint128::zero());
        assert_eq!(bd.peg_variable_amount, Uint128::new(200));
    }

    #[test]
    fn test_shared_caps_regular_plus_peg() {
        // Test that create_segments_from_split uses combined breakdown for cap checking
        let env = mock_env();
        let caps = default_caps(); // 20% cap on 1-month

        // Breakdown: regular has 150 in 1mo, peg has 50 in 1mo → combined = 200
        let bd = make_breakdown(500, 150, 0, 0, 200, 50, 0, 0);
        let view = combined_breakdown_for_caps(&bd);
        assert_eq!(view.one_month, Uint128::new(200));

        // Total debt after = 1000. Cap = 20% of 1000 = 200. Already at 200 → can't add more 1mo
        let split = SegmentSplit {
            one_month: Some(Decimal::one()),
            ..SegmentSplit::default()
        };
        let mut segs = vec![];
        let result = create_segments_from_split(
            Uint128::new(10), &split, &caps, Decimal::percent(5),
            &mut segs, &view, Uint128::new(1010), &env,
        );
        // 20% of 1010 = 202, new total = 210 > 202 → exceeds
        assert!(result.is_err());
    }

    #[test]
    fn test_targeted_repay_across_both_pools() {
        // Simulate targeted repay with splits for both pools
        let mut regular_segs = vec![var_segment(500), fixed_segment(500, 3, 7, 9999999, false)];
        let mut peg_segs = vec![var_segment(400), fixed_segment(100, 1, 5, 9999999, false)];

        let regular_split = SegmentSplit {
            variable: Some(Decimal::percent(30)),
            three_month: Some(Decimal::percent(20)),
            ..SegmentSplit::default()
        };
        let peg_split = SegmentSplit {
            variable: Some(Decimal::percent(40)),
            one_month: Some(Decimal::percent(10)),
            ..SegmentSplit::default()
        };

        let repay_amount = Uint128::new(1000);
        let (reg_applied, reg_d) = repay_segments_targeted(&mut regular_segs, repay_amount, &regular_split).unwrap();
        let (peg_applied, peg_d) = repay_segments_targeted(&mut peg_segs, repay_amount, &peg_split).unwrap();

        // Regular: 30% of 1000 = 300 from var, 20% of 1000 = 200 from 3mo
        assert_eq!(reg_d.variable, Int128::from(-300i128));
        assert_eq!(reg_d.three_month, Int128::from(-200i128));
        assert_eq!(reg_applied, Uint128::new(500));

        // Peg: 40% of 1000 = 400 from var, 10% of 1000 = 100 from 1mo
        assert_eq!(peg_d.variable, Int128::from(-400i128));
        assert_eq!(peg_d.one_month, Int128::from(-100i128));
        assert_eq!(peg_applied, Uint128::new(500));
    }

    // ========== apply_int128_delta edge cases ==========

    #[test]
    fn test_int128_delta_clamp_negative() {
        let mut val = Uint128::new(10);
        apply_int128_delta(&mut val, Int128::from(-20i128));
        assert_eq!(val, Uint128::zero()); // clamped
    }

    #[test]
    fn test_int128_delta_exact_zero() {
        let mut val = Uint128::new(100);
        apply_int128_delta(&mut val, Int128::from(-100i128));
        assert_eq!(val, Uint128::zero());
    }

    #[test]
    fn test_int128_delta_zero() {
        let mut val = Uint128::new(100);
        apply_int128_delta(&mut val, Int128::zero());
        assert_eq!(val, Uint128::new(100));
    }

    #[test]
    fn test_int128_delta_positive_add() {
        let mut val = Uint128::new(100);
        apply_int128_delta(&mut val, Int128::from(50i128));
        assert_eq!(val, Uint128::new(150));
    }
}
