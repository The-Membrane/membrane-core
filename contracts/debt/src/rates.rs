#![allow(non_snake_case)]

use std::cmp::{max, min, Ordering};

use cosmwasm_std::{
    attr, Addr, Decimal, DepsMut, Env, MessageInfo, QuerierWrapper, Response, StdError, StdResult,
    Storage, Uint128,
};

use membrane::debt::{AssetRateIndex, BasketDebt, Config, DebtPosition};
use membrane::helpers::get_asset_liquidity;
use membrane::math::{decimal_division, decimal_multiplication, decimal_subtraction};
use membrane::types::{
    Asset, AssetInfo, CreditAssetBreakdown, FixedRateCaps,
    IRMConfig, Rate, Rates, RateSegment,
};

use membrane::ltv_disco::{
    AssetQueueResponse as LTVDiscoQueueResponse, QueryMsg as LTVDiscoQueryMsg,
};
use membrane::system_discounts::{
    QueryMsg as DiscountQueryMsg, UserDiscountResponse,
};

use crate::state::{
    get_target_position, update_historical_interest_rates, update_position, BasketRateIndex,
    BASKET, BASKET_RATE_INDICES, CONFIG, RATES,
};
use crate::ContractError;

// ============================================================================
// Constants
// ============================================================================

pub const SECONDS_PER_YEAR: u64 = 31_536_000u64;
const MINIMUM_LIQUIDITY: Uint128 = Uint128::new(2_000_000_000_000u128);

// ============================================================================
// Collateral Types Cache (populated by Collateral contract messages)
// ============================================================================

use cosmwasm_schema::cw_serde;
use cw_storage_plus::Item;

/// Minimal collateral type info needed for rate calculations.
/// Populated via UpdateCollateralCache admin/cross-contract message.
#[cw_serde]
pub struct CollateralTypeInfo {
    /// Asset denomination string (matches AssetInfo::NativeToken denom)
    pub asset_denom: String,
    /// Maximum LTV (liquidation LTV) - used for fallback rate calculation
    pub max_ltv: Decimal,
}

pub const COLLATERAL_TYPES_CACHE: Item<Vec<CollateralTypeInfo>> =
    Item::new("collateral_types_cache");

// ============================================================================
// Taylor Series Approximation
// ============================================================================

/// Taylor series approximation of exp(x) for small x values.
/// exp(x) ~= 1 + x + x^2/2 + x^3/6
/// Input and output are in Decimal (not WAD).
///
/// CDP parity: maps to cdp/rates.rs::taylor_exp()
fn taylor_exp(x: Decimal) -> Decimal {
    let x2 = x.checked_mul(x).unwrap_or(Decimal::zero());
    let x3 = x2.checked_mul(x).unwrap_or(Decimal::zero());
    Decimal::one()
        + x
        + x2.checked_div(Decimal::from_ratio(2u128, 1u128))
            .unwrap_or(Decimal::zero())
        + x3.checked_div(Decimal::from_ratio(6u128, 1u128))
            .unwrap_or(Decimal::zero())
}

/// Signed taylor exp: handles negative exponents via 1/exp(|x|).
/// For positive x: returns taylor_exp(x)
/// For "negative" x (signaled by is_negative flag): returns 1/taylor_exp(|x|)
///
/// CDP parity: maps to cdp/rates.rs::signed_taylor_exp()
fn signed_taylor_exp(abs_x: Decimal, is_negative: bool) -> Decimal {
    let exp_val = taylor_exp(abs_x);
    if is_negative {
        Decimal::one()
            .checked_div(exp_val)
            .unwrap_or(Decimal::zero())
    } else {
        exp_val
    }
}

// ============================================================================
// Adaptive Smooth Rate
// ============================================================================

/// Adaptive smooth rate for regular debt.
/// Smoothly moves current_adaptive_rate toward the Disco destination rate using exponential decay.
///
/// Formula: new_rate = current_adaptive_rate * exp(speed * err * dt)
///   where err = (destination - current_adaptive_rate) / current_adaptive_rate  (normalized error)
///   and dt = time_elapsed / SECONDS_PER_YEAR
///
/// Clamped to [min_adaptive_rate, max_adaptive_rate].
///
/// CDP parity: maps to cdp/rates.rs::adaptive_smooth_rate()
pub fn adaptive_smooth_rate(
    current_adaptive_rate: Decimal,
    destination_rate: Decimal,
    time_elapsed: u64,
    irm_config: &IRMConfig,
) -> Decimal {
    if current_adaptive_rate.is_zero() {
        // Bootstrap: if current_adaptive_rate is zero, jump to destination
        return min(
            max(destination_rate, irm_config.min_adaptive_rate),
            irm_config.max_adaptive_rate,
        );
    }

    // Normalized error: (destination - current) / current
    let (err_abs, err_negative) = if destination_rate >= current_adaptive_rate {
        let diff = destination_rate - current_adaptive_rate;
        (
            decimal_division(diff, current_adaptive_rate).unwrap_or(Decimal::zero()),
            false,
        )
    } else {
        let diff = current_adaptive_rate - destination_rate;
        (
            decimal_division(diff, current_adaptive_rate).unwrap_or(Decimal::zero()),
            true,
        )
    };

    // dt = time_elapsed / SECONDS_PER_YEAR
    let dt = Decimal::from_ratio(time_elapsed as u128, SECONDS_PER_YEAR as u128);

    // exponent = speed * |err| * dt
    // Cap at 1.0 to keep the Taylor series exp() approximation accurate (<2% error).
    let raw_exponent = irm_config
        .adjustment_speed
        .checked_mul(err_abs)
        .unwrap_or(Decimal::zero())
        .checked_mul(dt)
        .unwrap_or(Decimal::zero());
    let exponent = min(raw_exponent, Decimal::one());

    // new_rate = current_adaptive_rate * exp(speed * err * dt)
    let multiplier = signed_taylor_exp(exponent, err_negative);
    let new_rate = decimal_multiplication(current_adaptive_rate, multiplier)
        .unwrap_or(current_adaptive_rate);

    // Don't overshoot the destination: clamp between old and destination
    let new_rate = if !err_negative {
        // Moving up toward destination: don't exceed it
        min(new_rate, destination_rate)
    } else {
        // Moving down toward destination: don't go below it
        max(new_rate, destination_rate)
    };

    // Clamp to global bounds
    min(
        max(new_rate, irm_config.min_adaptive_rate),
        irm_config.max_adaptive_rate,
    )
}

// ============================================================================
// Interest Accumulation Helpers
// ============================================================================

/// Apply interest rate over time: rate * (time_elapsed / SECONDS_PER_YEAR)
/// then decimal_multiplication(decimal, applied_rate).
///
/// CDP parity: maps to cdp/rates.rs::accumulate_interest_dec()
pub fn accumulate_interest_dec(
    decimal: Decimal,
    rate: Decimal,
    time_elapsed: u64,
) -> StdResult<Decimal> {
    let applied_rate = rate.checked_mul(Decimal::from_ratio(
        Uint128::from(time_elapsed),
        Uint128::from(SECONDS_PER_YEAR),
    ))?;

    decimal_multiplication(decimal, applied_rate)
}

/// Calculate fixed rate accrual for a specific time period.
///
/// CDP parity: maps to cdp/rates.rs::calculate_fixed_rate_accrual()
pub fn calculate_fixed_rate_accrual(
    amount: Uint128,
    rate: Decimal,
    time_start: u64,
    time_end: u64,
) -> StdResult<Uint128> {
    let time_elapsed = time_end.checked_sub(time_start).ok_or_else(|| {
        StdError::generic_err("Time end must be greater than time start")
    })?;

    let applied_rate = rate.checked_mul(Decimal::from_ratio(
        Uint128::from(time_elapsed),
        Uint128::from(SECONDS_PER_YEAR),
    ))?;

    let accrued = decimal_multiplication(
        Decimal::from_ratio(amount, Uint128::new(1)),
        applied_rate,
    )?
    .to_uint_floor();

    Ok(accrued)
}

/// Recalculate fixed rate for rollover.
/// Uses the position's collateral-weighted current_adaptive_rate * multiplier.
///
/// CDP parity: maps to cdp/rates.rs::recalculate_fixed_rate()
pub fn recalculate_fixed_rate(
    rates: &Rates,
    duration_months: u8,
    current_time: u64,
    collateral_weighted_rate: Decimal,
) -> StdResult<(Decimal, u64)> {
    let fixed_rate_cap = match duration_months {
        1 => &rates.fixed_rate_caps.one_month,
        3 => &rates.fixed_rate_caps.three_month,
        6 => &rates.fixed_rate_caps.six_month,
        _ => return Err(StdError::generic_err("Invalid duration_months")),
    };

    let new_rate = decimal_multiplication(collateral_weighted_rate, fixed_rate_cap.multiplier)?;

    // Calculate new end_time: add months from current_time
    let seconds_per_month = 2_592_000u64; // 30 days
    let new_end_time = current_time
        .checked_add(seconds_per_month * duration_months as u64)
        .ok_or_else(|| StdError::generic_err("End time calculation overflow"))?;

    Ok((new_rate, new_end_time))
}

/// Check and handle fixed rate expiration.
/// Returns (was_fixed, duration_months, converted_to_variable) to track basket totals changes.
///
/// CDP parity: maps to cdp/rates.rs::check_and_handle_fixed_rate_expiration()
pub fn check_and_handle_fixed_rate_expiration(
    segment: &mut RateSegment,
    rates: &Rates,
    current_time: u64,
    collateral_weighted_rate: Decimal,
) -> StdResult<(bool, u8, bool)> {
    let mut was_fixed = false;
    let mut duration_months = 0u8;
    let mut converted_to_variable = false;

    if let Some(ref mut fixed_rate) = segment.fixed_rate {
        was_fixed = true;
        duration_months = fixed_rate.end.duration_months;

        if current_time >= fixed_rate.end.end_time {
            if fixed_rate.end.rollover {
                // Recalculate fixed rate and refresh end_time
                let (new_rate, new_end_time) = recalculate_fixed_rate(
                    rates,
                    fixed_rate.end.duration_months,
                    current_time,
                    collateral_weighted_rate,
                )?;

                fixed_rate.rate = new_rate;
                fixed_rate.end.end_time = new_end_time;
            } else {
                // Convert to variable rate
                segment.fixed_rate = None;
                converted_to_variable = true;
            }
        }
    }

    Ok((was_fixed, duration_months, converted_to_variable))
}

// ============================================================================
// Debt Totals
// ============================================================================

/// Get total debt from all rate segments.
///
/// CDP parity: maps to cdp/rates.rs::get_total_debt_from_segments()
pub fn get_total_debt_from_segments(rate_segments: &[RateSegment]) -> Uint128 {
    rate_segments.iter().map(|segment| segment.amount).sum()
}

/// Get total position debt including both rate segments and peg_rate_segments.
///
/// CDP parity: maps to cdp/rates.rs::get_total_position_debt()
pub fn get_total_position_debt(position: &DebtPosition) -> Uint128 {
    get_total_debt_from_segments(&position.rate_segments)
        + get_total_debt_from_segments(&position.peg_rate_segments)
}

// ============================================================================
// LTV Disco Queries (for destination rate calculation)
// ============================================================================

/// Get total deposit tokens per asset from the LTV Disco contract.
/// Queries all cached collateral assets at once and returns a vec of Option<Uint128>.
/// Returns None for assets that have no LTV queue in the Disco.
///
/// CDP parity: maps to cdp/rates.rs::get_asset_total_deposits()
fn get_asset_total_deposits(
    querier: QuerierWrapper,
    ltv_disco_addr: &Addr,
    collateral_types: &[CollateralTypeInfo],
) -> Vec<Option<Uint128>> {
    let asset_strings: Vec<String> = collateral_types
        .iter()
        .map(|ct| ct.asset_denom.clone())
        .collect();

    // Query all queues at once
    let disco_response: Result<LTVDiscoQueueResponse, _> = querier.query_wasm_smart(
        ltv_disco_addr.to_string(),
        &LTVDiscoQueryMsg::GetAssetQueue {
            assets: asset_strings.clone(),
            limit: None,
            start_after: None,
        },
    );

    let queues = match disco_response {
        Ok(resp) => resp.queues,
        Err(_) => return collateral_types.iter().map(|_| None).collect(),
    };

    // Map each asset to its total deposits
    asset_strings
        .iter()
        .map(|asset_str| {
            queues
                .iter()
                .find(|(name, _)| name == asset_str)
                .map(|(_, queue)| {
                    let total: Uint128 = queue
                        .slots
                        .iter()
                        .map(|slot| slot.total_deposit_tokens)
                        .sum();
                    if total.is_zero() {
                        None
                    } else {
                        Some(total)
                    }
                })
                .flatten()
        })
        .collect()
}

// ============================================================================
// Interest Rate Calculation
// ============================================================================

/// Calculate interest rates for each asset using TVL-standardized insurance ratios.
/// Maximum rate is capped at irm_config.max_adaptive_rate (100%) to avoid overflows.
///
/// Goal: TVL-standardized comparative deposit-based rates
/// - insurance_ratio = deposit_ratio / tvl_ratio (normalized by TVL share)
/// - Highest insurance ratio asset gets: base_interest_rate * (1 / max_LTV)
/// - Other assets get: base_interest_rate * (highest_insurance / asset_insurance)
/// - Fallback (no deposit or TVL data): base_interest_rate * (1 / max_LTV)
///
/// In the modular Debt contract, we use COLLATERAL_TYPES_CACHE for asset info
/// and the position's collateral_rate_indices ratios for TVL weighting.
///
/// CDP parity: maps to cdp/rates.rs::get_interest_rates()
pub fn get_interest_rates(
    _storage: &mut dyn Storage,
    querier: QuerierWrapper,
    _env: Env,
    collateral_types: &[CollateralTypeInfo],
    rates: &Rates,
    config: &Config,
    tvl_ratios: Option<Vec<Decimal>>,
) -> StdResult<Vec<Decimal>> {
    // Use provided TVL ratios or default to equal weighting
    let tvl_ratios: Vec<Decimal> = match tvl_ratios {
        Some(ratios) => ratios,
        None => {
            // Default: equal weight across all collateral types
            if collateral_types.is_empty() {
                return Ok(vec![]);
            }
            let equal_ratio = decimal_division(
                Decimal::one(),
                Decimal::from_ratio(collateral_types.len() as u128, 1u128),
            )?;
            vec![equal_ratio; collateral_types.len()]
        }
    };

    // Need an LTV Disco address from config's oracle_contract or similar.
    // In the modular design, we query via a stored Disco address.
    // For now, use oracle_contract as the LTV Disco proxy.
    // If no oracle_contract is configured, fall back to LTV-based rates.
    let ltv_disco_addr = match &config.oracle_contract {
        Some(addr) => addr.clone(),
        None => {
            // No oracle: return LTV-based fallback rates for each asset
            let mut fallback_rates = vec![];
            for ct in collateral_types.iter() {
                if ct.max_ltv.is_zero() {
                    fallback_rates.push(rates.base_interest_rate);
                } else {
                    let ltv_rate = decimal_multiplication(
                        rates.base_interest_rate,
                        decimal_division(Decimal::one(), ct.max_ltv)?,
                    )?;
                    fallback_rates.push(ltv_rate);
                }
            }
            return Ok(fallback_rates);
        }
    };

    // Collect total deposits from LTV Disco
    let asset_deposits: Vec<Option<Uint128>> =
        get_asset_total_deposits(querier, &ltv_disco_addr, collateral_types);

    // Calculate total deposits across all assets
    let total_deposits: Uint128 = asset_deposits.iter().filter_map(|d| *d).sum();

    // Calculate deposit ratios (each asset's share of total deposits)
    let deposit_ratios: Vec<Option<Decimal>> = asset_deposits
        .iter()
        .map(|dep| {
            dep.map(|d| {
                if total_deposits.is_zero() {
                    Decimal::zero()
                } else {
                    Decimal::from_ratio(d, total_deposits)
                }
            })
        })
        .collect();

    // Calculate insurance ratios: deposit_ratio / tvl_ratio
    // Higher ratio = more "over-insured" relative to TVL share
    let insurance_ratios: Vec<Option<Decimal>> = deposit_ratios
        .iter()
        .enumerate()
        .map(|(i, dep_ratio)| match (dep_ratio, tvl_ratios.get(i)) {
            (Some(dep), Some(&tvl)) if !tvl.is_zero() => {
                Some(decimal_division(*dep, tvl).unwrap_or(Decimal::zero()))
            }
            _ => None,
        })
        .collect();

    // Find highest insurance ratio (most over-insured asset relative to TVL)
    let highest_insurance: Option<Decimal> = insurance_ratios
        .iter()
        .filter_map(|r| *r)
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));

    let mut destination_rates = vec![];

    for (i, ct) in collateral_types.iter().enumerate() {
        // Calculate LTV-based fallback rate: base * (1/max_LTV)
        let ltv_based_rate = if ct.max_ltv.is_zero() {
            rates.base_interest_rate
        } else {
            decimal_multiplication(
                rates.base_interest_rate,
                decimal_division(Decimal::one(), ct.max_ltv)?,
            )?
        };

        match (insurance_ratios[i], highest_insurance) {
            // Both asset insurance ratio and highest reference available
            (Some(asset_insurance), Some(high_insurance)) => {
                if asset_insurance == high_insurance {
                    // Asset with HIGHEST insurance ratio = LOWEST rate (base LTV formula)
                    destination_rates.push(ltv_based_rate);
                } else {
                    // Other assets: base_rate * (highest_insurance / asset_insurance)
                    let insurance_multiplier =
                        decimal_division(high_insurance, asset_insurance)?;
                    let mut comparative_rate = decimal_multiplication(
                        rates.base_interest_rate,
                        insurance_multiplier,
                    )?;
                    // Cap at irm_config.max_adaptive_rate (100%)
                    if comparative_rate > config.irm_config.max_adaptive_rate {
                        comparative_rate = config.irm_config.max_adaptive_rate;
                    }
                    destination_rates.push(comparative_rate);
                }
            }
            // No insurance data: use LTV-based fallback
            _ => {
                destination_rates.push(ltv_based_rate);
            }
        }
    }

    Ok(destination_rates)
}

// ============================================================================
// Update Rate Indices
// ============================================================================

/// Calculate basket interest rates and accumulate to all per-asset rate indices.
/// Uses AdaptiveSmoothRate: rates are smoothed toward Disco destination (comparative deposits).
///
/// In the modular Debt contract, BASKET_RATE_INDICES stores per-asset rate indices
/// instead of basket.collateral_types[i].rate_index. COLLATERAL_TYPES_CACHE provides
/// the minimal collateral info needed (asset_denom, max_LTV).
///
/// CDP parity: maps to cdp/rates.rs::update_rate_indices()
pub fn update_rate_indices(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: &Config,
) -> StdResult<()> {
    let mut rates = RATES.load(storage)?;

    // Load collateral types cache
    let collateral_types = COLLATERAL_TYPES_CACHE
        .may_load(storage)?
        .unwrap_or_default();

    if collateral_types.is_empty() {
        // No collateral types cached yet - nothing to do
        rates.rates_last_accrued = env.block.time.seconds();
        RATES.save(storage, &rates)?;
        return Ok(());
    }

    // Get Disco destination rates
    let destination_rates = match get_interest_rates(
        storage,
        querier,
        env.clone(),
        &collateral_types,
        &rates,
        config,
        None,
    ) {
        Ok(rates) => rates,
        Err(err) => {
            return Err(StdError::generic_err(format!(
                "Error in get_interest_rates: {}",
                err
            )));
        }
    };

    // Calc time_elapsed since last rate update
    let time_elapsed = env.block.time.seconds() - rates.rates_last_accrued;

    // Ensure current_adaptive_rate vectors are the right length (bootstrap for new assets)
    while rates.current_adaptive_rate.len() < collateral_types.len() {
        rates.current_adaptive_rate.push(Decimal::zero());
    }
    while rates.peg_current_adaptive_rate.len() < collateral_types.len() {
        rates.peg_current_adaptive_rate.push(Decimal::zero());
    }

    // --- Regular debt: adaptive smooth toward Disco destination rate ---
    for (i, _ct) in collateral_types.iter().enumerate() {
        let new_rate = adaptive_smooth_rate(
            rates.current_adaptive_rate[i],
            destination_rates[i],
            time_elapsed,
            &config.irm_config,
        );
        rates.current_adaptive_rate[i] = new_rate;
    }

    // Update peg_current_adaptive_rate to match regular current_adaptive_rate (base)
    for (i, _ct) in collateral_types.iter().enumerate() {
        rates.peg_current_adaptive_rate[i] = rates.current_adaptive_rate[i];
    }

    // Base rates for regular (CDT) debt — NO acquisition bump.
    // The control system always sees the base rate on the next tick.
    let base_rates: Vec<Decimal> = collateral_types
        .iter()
        .enumerate()
        .map(|(i, _ct)| rates.current_adaptive_rate[i])
        .collect();

    // Peg effective rates for USDC debt — WITH acquisition bump.
    // The bump is scoped to peg debt only and NOT saved to current_adaptive_rate.
    let peg_effective_rates: Vec<Decimal> = collateral_types
        .iter()
        .enumerate()
        .map(|(i, ct)| {
            if !rates.acquisition_bump_rate.is_zero() {
                let bump_for_asset = decimal_multiplication(
                    rates.acquisition_bump_rate,
                    decimal_division(Decimal::one(), ct.max_ltv).unwrap_or(Decimal::one()),
                )
                .unwrap_or(Decimal::zero());
                rates.current_adaptive_rate[i] + bump_for_asset
            } else {
                rates.current_adaptive_rate[i]
            }
        })
        .collect();

    // Update latest rates with base rates (no acquisition bump)
    rates.lastest_collateral_rates = collateral_types
        .iter()
        .enumerate()
        .map(|(i, _ct)| Rate {
            rate: base_rates[i],
            last_time_updated: env.block.time.seconds(),
        })
        .collect();

    // Update rates_last_accrued
    rates.rates_last_accrued = env.block.time.seconds();

    // Save: current_adaptive_rate = base (no bump), lastest_collateral_rates = base (no bump)
    RATES.save(storage, &rates)?;

    // Update historical interest rates with base rates (no acquisition bump)
    for (i, ct) in collateral_types.iter().enumerate() {
        if let Err(err) = update_historical_interest_rates(
            storage,
            env.clone(),
            ct.asset_denom.clone(),
            base_rates[i],
        ) {
            return Err(StdError::generic_err(format!(
                "Error updating historical interest rates: {}",
                err
            )));
        }
    }

    // Accumulate rate indices per collateral asset
    for (i, ct) in collateral_types.iter().enumerate() {
        let mut basket_rate_idx = BASKET_RATE_INDICES
            .may_load(storage, ct.asset_denom.clone())?
            .unwrap_or(BasketRateIndex {
                rate_index: Decimal::one(),
                peg_rate_index: Decimal::one(),
            });

        // Regular (CDT) debt: base rate only (no acquisition bump)
        let accrued_rate =
            accumulate_interest_dec(basket_rate_idx.rate_index, base_rates[i], time_elapsed)?;
        basket_rate_idx.rate_index += accrued_rate;

        // Peg (USDC) debt: base rate + acquisition bump
        let peg_accrued =
            accumulate_interest_dec(basket_rate_idx.peg_rate_index, peg_effective_rates[i], time_elapsed)?;
        basket_rate_idx.peg_rate_index += peg_accrued;

        BASKET_RATE_INDICES.save(storage, ct.asset_denom.clone(), &basket_rate_idx)?;
    }

    Ok(())
}

// ============================================================================
// Segment Accrual (ported from CDP debt_manager::accrue_segments)
// ============================================================================

/// Per-segment changes for basket tracking.
/// Uses the same field semantics as CDP's debt_manager::SegmentDeltas.
#[derive(Default, Clone, Debug)]
pub struct AccrualDeltas {
    pub variable: i128,
    pub one_month: i128,
    pub three_month: i128,
    pub six_month: i128,
}

/// Accrue interest on a set of rate segments.
/// `rate_of_change` is the weighted avg index ratio for variable rate segments.
/// Fixed rate segments use their own fixed rate for time-based accrual.
/// Returns (total_accrued, deltas for basket tracking).
///
/// CDP parity: maps to cdp/debt_manager.rs::accrue_segments()
fn accrue_segments(
    segments: &mut Vec<RateSegment>,
    rate_of_change: Decimal,
    last_accrued_time: u64,
    current_time: u64,
    rates: &Rates,
    collateral_weighted_rate: Decimal,
) -> StdResult<(Uint128, AccrualDeltas)> {
    let mut total_accrued = Uint128::zero();
    let mut deltas = AccrualDeltas::default();

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
            check_and_handle_fixed_rate_expiration(
                segment,
                rates,
                current_time,
                collateral_weighted_rate,
            )?;

        let amount_after = segment.amount;
        let segment_delta = amount_after
            .checked_sub(amount_before)
            .unwrap_or(Uint128::zero());

        // Track deltas for basket
        if was_fixed && converted_to_variable {
            // Moved from fixed to variable
            match duration_before {
                1 => deltas.one_month -= amount_before.u128() as i128,
                3 => deltas.three_month -= amount_before.u128() as i128,
                6 => deltas.six_month -= amount_before.u128() as i128,
                _ => {}
            }
            deltas.variable += amount_after.u128() as i128;
        } else if was_fixed {
            match duration_before {
                1 => deltas.one_month += segment_delta.u128() as i128,
                3 => deltas.three_month += segment_delta.u128() as i128,
                6 => deltas.six_month += segment_delta.u128() as i128,
                _ => {}
            }
        } else {
            deltas.variable += segment_delta.u128() as i128;
        }
    }

    Ok((total_accrued, deltas))
}

/// Apply discount proportionally across segments.
///
/// CDP parity: maps to cdp/debt_manager.rs::apply_discount_to_segments()
fn apply_discount_to_segments(
    segments: &mut Vec<RateSegment>,
    discount_amount: Uint128,
    total_debt: Uint128,
) -> AccrualDeltas {
    let mut deltas = AccrualDeltas::default();
    if discount_amount.is_zero() || total_debt.is_zero() {
        return deltas;
    }

    for segment in segments.iter_mut() {
        let proportion = if total_debt > Uint128::zero() {
            decimal_division(
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
            deltas.variable -= segment_discount.u128() as i128;
        } else {
            match segment
                .fixed_rate
                .as_ref()
                .unwrap()
                .end
                .duration_months
            {
                1 => deltas.one_month -= segment_discount.u128() as i128,
                3 => deltas.three_month -= segment_discount.u128() as i128,
                6 => deltas.six_month -= segment_discount.u128() as i128,
                _ => {}
            }
        }
    }

    deltas
}

// ============================================================================
// Delta Application Helpers
// ============================================================================

/// Apply an i128 delta to a Uint128 value (clamp to zero on negative).
fn apply_i128_delta(value: &mut Uint128, delta: i128) {
    if delta < 0 {
        let abs = (-delta) as u128;
        *value = value.checked_sub(Uint128::new(abs)).unwrap_or(Uint128::zero());
    } else {
        *value = value
            .checked_add(Uint128::new(delta as u128))
            .unwrap_or(*value);
    }
}

/// Apply AccrualDeltas to CreditAssetBreakdown (regular debt pool).
///
/// CDP parity: maps to cdp/debt_manager.rs::apply_delta_to_regular_debt()
fn apply_delta_to_regular_debt(breakdown: &mut CreditAssetBreakdown, deltas: &AccrualDeltas) {
    apply_i128_delta(&mut breakdown.variable_amount, deltas.variable);
    apply_i128_delta(&mut breakdown.one_month_amount, deltas.one_month);
    apply_i128_delta(&mut breakdown.three_month_amount, deltas.three_month);
    apply_i128_delta(&mut breakdown.six_month_amount, deltas.six_month);
}

/// Apply AccrualDeltas to CreditAssetBreakdown (peg debt pool).
///
/// CDP parity: maps to cdp/debt_manager.rs::apply_delta_to_peg_debt()
fn apply_delta_to_peg_debt(breakdown: &mut CreditAssetBreakdown, deltas: &AccrualDeltas) {
    apply_i128_delta(&mut breakdown.peg_variable_amount, deltas.variable);
    apply_i128_delta(&mut breakdown.peg_one_month_amount, deltas.one_month);
    apply_i128_delta(&mut breakdown.peg_three_month_amount, deltas.three_month);
    apply_i128_delta(&mut breakdown.peg_six_month_amount, deltas.six_month);
}

/// Apply AccrualDeltas to FixedRateCaps.
///
/// CDP parity: maps to cdp/debt_manager.rs::apply_delta_to_caps()
fn apply_delta_to_caps(caps: &mut FixedRateCaps, deltas: &AccrualDeltas) {
    apply_i128_delta(&mut caps.one_month.amount, deltas.one_month);
    apply_i128_delta(&mut caps.three_month.amount, deltas.three_month);
    apply_i128_delta(&mut caps.six_month.amount, deltas.six_month);
}

/// Merge two AccrualDeltas structs (adds b into a).
fn merge_deltas(a: &mut AccrualDeltas, b: &AccrualDeltas) {
    a.variable += b.variable;
    a.one_month += b.one_month;
    a.three_month += b.three_month;
    a.six_month += b.six_month;
}

// ============================================================================
// Discount Helpers
// ============================================================================

/// Calculate the discounted interest for a user.
///
/// CDP parity: maps to cdp/rates.rs::get_discounted_interest()
fn get_discounted_interest(
    querier: QuerierWrapper,
    discounts_contract: String,
    user: String,
    undiscounted_interest: Uint128,
) -> StdResult<Uint128> {
    // Use regular UserDiscount query (modular debt doesn't have direct collateral access
    // for checking force_redemption assets, so we always use regular discount)
    let user_discount: UserDiscountResponse = querier.query_wasm_smart(
        discounts_contract,
        &DiscountQueryMsg::UserDiscount { user },
    )?;

    let discounted_interest = {
        let percent_of_interest = decimal_subtraction(Decimal::one(), user_discount.discount)?;
        decimal_multiplication(
            Decimal::from_ratio(undiscounted_interest, Uint128::one()),
            percent_of_interest,
        )?
        .to_uint_floor()
    };

    Ok(discounted_interest)
}

// ============================================================================
// External Accrual Entry Point
// ============================================================================

/// Accrue interest for a list of positions.
/// This is the external entry point called by users or other contracts.
///
/// CDP parity: maps to cdp/rates.rs::external_accrue_call()
pub fn accrue(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_owner: Option<String>,
    position_ids: Vec<Uint128>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut basket = BASKET.load(deps.storage)?;

    // Validate position owner
    let valid_position_owner: Addr = if let Some(position_owner) = position_owner {
        deps.api.addr_validate(&position_owner)?
    } else {
        info.sender.clone()
    };

    // Initialize accrued_interest
    let mut accrued_interest: Uint128 = Uint128::zero();

    // Accrue interest for each position
    for position_id in position_ids.clone() {
        let mut position =
            get_target_position(deps.storage, valid_position_owner.clone(), position_id)?.1;

        let prev_loan = get_total_position_debt(&position);

        accrue_position(
            deps.storage,
            deps.querier,
            env.clone(),
            &config,
            &mut position,
            &mut basket,
            valid_position_owner.to_string(),
        )?;

        accrued_interest += get_total_position_debt(&position) - prev_loan;

        update_position(deps.storage, valid_position_owner.clone(), position)?;
    }

    // Save updated Basket
    BASKET.save(deps.storage, &basket)?;

    Ok(Response::new().add_attributes(vec![
        attr("method", "accrue"),
        attr("position_ids", format!("{:?}", position_ids)),
        attr("accrued_interest", accrued_interest),
    ]))
}

// ============================================================================
// Internal Per-Position Accrual
// ============================================================================

/// Internal per-position accrual logic.
/// Computes credit price rate change, updates rate indices, computes weighted
/// rate_of_change from basket_rate_index / position_rate_index, accrues segments,
/// applies discounts, distributes interest to pending_revenue, and syncs indices.
///
/// CDP parity: maps to cdp/rates.rs::accrue() (the internal function)
pub fn accrue_position(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: &Config,
    position: &mut DebtPosition,
    basket: &mut BasketDebt,
    user: String,
) -> StdResult<()> {
    let mut rates = RATES.load(storage)?;

    // Load collateral types cache for index lookups
    let collateral_types = COLLATERAL_TYPES_CACHE
        .may_load(storage)?
        .unwrap_or_default();

    // ========================================================================
    // 1. Credit Price Controller (Repayment Price Accrual)
    // ========================================================================

    let time_elapsed = env.block.time.seconds() - rates.credit_last_accrued;
    let mut skip_credit_price_accrual: bool = config.skip_credit_price_accrual;
    #[allow(unused_assignments)]
    let mut negative_rate: bool = false;
    #[allow(unused_assignments)]
    let mut credit_price_rate: Decimal = Decimal::zero();

    // Try to get the credit TWAP price from the oracle
    let credit_TWAP_price: Decimal = if let Some(ref oracle_addr) = config.oracle_contract {
        // Query oracle for credit price
        let _total_credit_amount = basket.credit_asset.total_all_debt();
        let credit_asset_info = basket.credit_asset.info.clone();

        // Query using membrane oracle
        match querier.query_wasm_smart::<membrane::oracle::PriceResponse>(
            oracle_addr.to_string(),
            &membrane::oracle::QueryMsg::Price {
                asset_info: credit_asset_info.clone(),
                twap_timeframe: config.credit_twap_timeframe,
                oracle_time_limit: 600,
                basket_id: None,
            },
        ) {
            Ok(price_resp) => {
                if price_resp.price.is_zero() {
                    skip_credit_price_accrual = true;
                    Decimal::zero()
                } else {
                    price_resp.price
                }
            }
            Err(_) => {
                skip_credit_price_accrual = true;
                Decimal::zero()
            }
        }
    } else {
        skip_credit_price_accrual = true;
        Decimal::zero()
    };

    // Credit Price Controller barriers
    if !skip_credit_price_accrual {
        let liquidity: Uint128 = get_asset_liquidity(
            querier,
            config
                .liquidity_contract
                .clone()
                .unwrap_or_else(|| Addr::unchecked(""))
                .to_string(),
            basket.credit_asset.info.clone(),
        )?;

        let current_supply = basket.credit_asset.total_all_debt();
        let liquidity_ratio = {
            if !current_supply.is_zero() {
                decimal_division(
                    Decimal::from_ratio(liquidity, Uint128::new(1u128)),
                    Decimal::from_ratio(current_supply, Uint128::new(1u128)),
                )?
            } else {
                Decimal::one()
            }
        };

        if liquidity_ratio < Decimal::percent(3)
            || liquidity < MINIMUM_LIQUIDITY
            || !basket.oracle_set
        {
            skip_credit_price_accrual = true;
        }
    }

    // Calculate credit price rate and accrue if not skipped
    rates.credit_last_accrued = env.block.time.seconds();

    // Guard: if either price is zero, skip CPC (division by zero)
    let price_difference = if credit_TWAP_price.is_zero() || basket.credit_price.price.is_zero() {
        negative_rate = false;
        Decimal::zero()
    } else {
        match credit_TWAP_price.cmp(&basket.credit_price.price) {
            Ordering::Greater => {
                negative_rate = true;
                decimal_subtraction(
                    decimal_division(credit_TWAP_price, basket.credit_price.price)?,
                    Decimal::one(),
                )?
            }
            Ordering::Less => {
                negative_rate = false;
                decimal_subtraction(
                    decimal_division(basket.credit_price.price, credit_TWAP_price)?,
                    Decimal::one(),
                )?
            }
            Ordering::Equal => {
                negative_rate = false;
                Decimal::zero()
            }
        }
    };

    if price_difference > rates.cpc_margin_of_error {
        credit_price_rate = decimal_multiplication(price_difference, config.cpc_multiplier)?;

        if !skip_credit_price_accrual {
            let mut applied_rate = credit_price_rate.checked_mul(Decimal::from_ratio(
                Uint128::from(time_elapsed),
                Uint128::from(SECONDS_PER_YEAR),
            ))?;

            if negative_rate {
                applied_rate = decimal_subtraction(Decimal::one(), applied_rate)?;
            } else {
                applied_rate += Decimal::one();
            }

            let mut new_price = basket.credit_price.price;
            if !negative_rate || rates.negative_rates {
                new_price = decimal_multiplication(basket.credit_price.price, applied_rate)?;
            }

            basket.credit_price.price = new_price;
        }
    } else {
        let _ = Decimal::zero(); // credit_price_rate already zero from init
    }

    // ========================================================================
    // 2. Update Rate Indices
    // ========================================================================

    // Save last_accrued_time BEFORE update_rate_indices (which updates rates_last_accrued).
    // Fixed rate accrual needs the original last_accrued_time.
    let last_accrued_time = rates.rates_last_accrued;
    let current_time = env.block.time.seconds();

    // Update basket rate indices (smooth toward destination, accumulate)
    match update_rate_indices(storage, querier, env.clone(), config) {
        Ok(_) => {}
        Err(err) => {
            return Err(StdError::generic_err(format!(
                "Error in update_rate_indices: {}",
                err
            )));
        }
    };

    // ========================================================================
    // 3. Calculate Rate of Change (weighted avg basket/position index ratio)
    // ========================================================================

    // Ensure position has collateral_rate_indices populated
    // If empty, bootstrap from current basket indices
    if position.collateral_rate_indices.is_empty() && !collateral_types.is_empty() {
        for ct in collateral_types.iter() {
            let basket_idx = BASKET_RATE_INDICES
                .may_load(storage, ct.asset_denom.clone())?
                .unwrap_or(BasketRateIndex {
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                });
            position.collateral_rate_indices.push(AssetRateIndex {
                asset_denom: ct.asset_denom.clone(),
                rate_index: basket_idx.rate_index,
                peg_rate_index: basket_idx.peg_rate_index,
                ratio: Decimal::zero(),
            });
        }
    }

    // Calculate collateral value ratios from position's stored ratios.
    // If ratios are zero (not yet set), use equal weighting.
    let total_ratio: Decimal = position
        .collateral_rate_indices
        .iter()
        .map(|idx| idx.ratio)
        .fold(Decimal::zero(), |acc, r| acc + r);

    let ratios: Vec<Decimal> = if total_ratio.is_zero() && !position.collateral_rate_indices.is_empty()
    {
        // Equal weight fallback
        let equal = decimal_division(
            Decimal::one(),
            Decimal::from_ratio(position.collateral_rate_indices.len() as u128, 1u128),
        )
        .unwrap_or(Decimal::zero());
        vec![equal; position.collateral_rate_indices.len()]
    } else {
        position
            .collateral_rate_indices
            .iter()
            .map(|idx| idx.ratio)
            .collect()
    };

    // Compute weighted avg rate_of_change for regular debt
    let mut rate_of_change = Decimal::zero();
    for (i, idx) in position.collateral_rate_indices.iter().enumerate() {
        let basket_idx = BASKET_RATE_INDICES
            .may_load(storage, idx.asset_denom.clone())?
            .unwrap_or(BasketRateIndex {
                rate_index: Decimal::one(),
                peg_rate_index: Decimal::one(),
            });

        if !idx.rate_index.is_zero() {
            rate_of_change += decimal_multiplication(
                ratios[i],
                decimal_division(basket_idx.rate_index, idx.rate_index)?,
            )?;
        }
    }

    // Compute weighted avg rate_of_change for peg debt
    let mut peg_rate_of_change = Decimal::zero();
    if !position.peg_rate_segments.is_empty() {
        for (i, idx) in position.collateral_rate_indices.iter().enumerate() {
            let basket_idx = BASKET_RATE_INDICES
                .may_load(storage, idx.asset_denom.clone())?
                .unwrap_or(BasketRateIndex {
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                });

            if !idx.peg_rate_index.is_zero() {
                peg_rate_of_change += decimal_multiplication(
                    ratios[i],
                    decimal_division(basket_idx.peg_rate_index, idx.peg_rate_index)?,
                )?;
            }
        }
    }

    // ========================================================================
    // 4. Compute collateral-weighted current_adaptive_rate for fixed rollovers
    // ========================================================================

    // Reload RATES since update_rate_indices saved updated values
    let credit_last_accrued = rates.credit_last_accrued;
    let mut rates = RATES.load(storage)?;
    rates.credit_last_accrued = credit_last_accrued;

    let mut regular_weighted_rate = Decimal::zero();
    let mut peg_weighted_rate = Decimal::zero();

    for (i, idx) in position.collateral_rate_indices.iter().enumerate() {
        // Find the collateral_type index in COLLATERAL_TYPES_CACHE
        if let Some(ct_idx) = collateral_types
            .iter()
            .position(|ct| ct.asset_denom == idx.asset_denom)
        {
            if ct_idx < rates.current_adaptive_rate.len() {
                regular_weighted_rate += decimal_multiplication(
                    ratios[i],
                    rates.current_adaptive_rate[ct_idx],
                )?;
            }
            if ct_idx < rates.peg_current_adaptive_rate.len() {
                peg_weighted_rate += decimal_multiplication(
                    ratios[i],
                    rates.peg_current_adaptive_rate[ct_idx],
                )?;
            }
        }
    }

    // ========================================================================
    // 5. Accrue Rate Segments
    // ========================================================================

    let mut total_accrued_interest = Uint128::zero();

    // Accrue regular rate segments
    let (regular_accrued, regular_deltas) = accrue_segments(
        &mut position.rate_segments,
        rate_of_change,
        last_accrued_time,
        current_time,
        &rates,
        regular_weighted_rate,
    )?;
    total_accrued_interest += regular_accrued;

    // Accrue peg rate segments
    let (peg_accrued, peg_deltas) = accrue_segments(
        &mut position.peg_rate_segments,
        peg_rate_of_change,
        last_accrued_time,
        current_time,
        &rates,
        peg_weighted_rate,
    )?;
    total_accrued_interest += peg_accrued;

    // ========================================================================
    // 6. Sync position rate indices to basket
    // ========================================================================

    for idx in position.collateral_rate_indices.iter_mut() {
        let basket_idx = BASKET_RATE_INDICES
            .may_load(storage, idx.asset_denom.clone())?
            .unwrap_or(BasketRateIndex {
                rate_index: Decimal::one(),
                peg_rate_index: Decimal::one(),
            });
        idx.rate_index = basket_idx.rate_index;
        idx.peg_rate_index = basket_idx.peg_rate_index;
    }

    // ========================================================================
    // 7. Apply Discounts and Distribute Interest
    // ========================================================================

    if total_accrued_interest > Uint128::zero() {
        // Apply discounts if configured
        let mut discounted_interest = total_accrued_interest;
        if let Some(ref contract) = config.discounts_contract {
            discounted_interest = match get_discounted_interest(
                querier,
                contract.to_string(),
                user,
                total_accrued_interest,
            ) {
                Ok(discounted) => discounted,
                Err(_) => total_accrued_interest,
            };
        }

        // Track new_total_pending
        let mut new_interest = Uint128::zero();

        // Split accrued interest into per-asset distribution
        for (i, idx) in position.collateral_rate_indices.iter().enumerate() {
            let ratio = ratios[i];
            let amount = decimal_multiplication(
                ratio,
                Decimal::from_ratio(discounted_interest, Uint128::one()),
            )?;
            let amount_floor = amount.to_uint_floor();

            // Add amount to per-asset distribution in pending_revenue
            let asset_info = AssetInfo::NativeToken {
                denom: idx.asset_denom.clone(),
            };
            if let Some(asset) = basket
                .pending_revenue
                .per_asset_rev
                .iter_mut()
                .find(|a| a.info.equal(&asset_info))
            {
                asset.amount += amount_floor;
            } else {
                basket.pending_revenue.per_asset_rev.push(Asset {
                    info: asset_info,
                    amount: amount_floor,
                });
            }

            new_interest += amount_floor;
        }

        // Add accrued interest to basket's pending revenue
        basket.pending_revenue.total_pending += new_interest;

        position.pending_interest += new_interest;
        position.total_interest_accrued += new_interest;

        // Apply discount to segment amounts if discount was applied
        if discounted_interest < total_accrued_interest {
            let discount_amount = total_accrued_interest
                .checked_sub(discounted_interest)
                .unwrap_or(Uint128::zero());

            let total_debt_before_discount = get_total_position_debt(position);

            // Apply discount proportionally to both pools
            let regular_discount_deltas = apply_discount_to_segments(
                &mut position.rate_segments,
                discount_amount,
                total_debt_before_discount,
            );
            let peg_discount_deltas = apply_discount_to_segments(
                &mut position.peg_rate_segments,
                discount_amount,
                total_debt_before_discount,
            );

            // Merge discount deltas with accrual deltas
            let mut final_regular_deltas = regular_deltas.clone();
            merge_deltas(&mut final_regular_deltas, &regular_discount_deltas);
            let mut final_peg_deltas = peg_deltas.clone();
            merge_deltas(&mut final_peg_deltas, &peg_discount_deltas);

            // Apply merged deltas to basket
            apply_delta_to_regular_debt(&mut basket.credit_asset, &final_regular_deltas);
            apply_delta_to_caps(&mut rates.fixed_rate_caps, &final_regular_deltas);
            apply_delta_to_peg_debt(&mut basket.credit_asset, &final_peg_deltas);
            apply_delta_to_caps(&mut rates.fixed_rate_caps, &final_peg_deltas);
        } else {
            // No discount - apply accrual deltas directly
            apply_delta_to_regular_debt(&mut basket.credit_asset, &regular_deltas);
            apply_delta_to_caps(&mut rates.fixed_rate_caps, &regular_deltas);
            apply_delta_to_peg_debt(&mut basket.credit_asset, &peg_deltas);
            apply_delta_to_caps(&mut rates.fixed_rate_caps, &peg_deltas);
        }
    } else {
        // No accrued interest, but still apply deltas (e.g., fixed->variable conversions)
        apply_delta_to_regular_debt(&mut basket.credit_asset, &regular_deltas);
        apply_delta_to_caps(&mut rates.fixed_rate_caps, &regular_deltas);
        apply_delta_to_peg_debt(&mut basket.credit_asset, &peg_deltas);
        apply_delta_to_caps(&mut rates.fixed_rate_caps, &peg_deltas);
    }

    // Save updated rates (credit_last_accrued, fixed_rate_caps)
    RATES.save(storage, &rates)?;

    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use membrane::types::{FixedRate, FixedRateCap, FixedRateEnd};

    #[test]
    fn test_taylor_exp_zero() {
        let result = taylor_exp(Decimal::zero());
        assert_eq!(result, Decimal::one());
    }

    #[test]
    fn test_taylor_exp_small() {
        // exp(0.01) ~= 1.01005
        let result = taylor_exp(Decimal::percent(1));
        assert!(result > Decimal::one());
        assert!(result < Decimal::from_str("1.011").unwrap());
    }

    #[test]
    fn test_signed_taylor_exp_positive() {
        let result = signed_taylor_exp(Decimal::percent(5), false);
        assert!(result > Decimal::one());
    }

    #[test]
    fn test_signed_taylor_exp_negative() {
        let result = signed_taylor_exp(Decimal::percent(5), true);
        assert!(result < Decimal::one());
        assert!(result > Decimal::zero());
    }

    #[test]
    fn test_adaptive_smooth_rate_bootstrap() {
        let irm = IRMConfig {
            adjustment_speed: Decimal::from_str("50").unwrap(),
            min_adaptive_rate: Decimal::percent(1),
            max_adaptive_rate: Decimal::one(),
        };

        // Zero current rate should jump to destination (clamped)
        let result = adaptive_smooth_rate(Decimal::zero(), Decimal::percent(5), 100, &irm);
        assert_eq!(result, Decimal::percent(5));
    }

    #[test]
    fn test_adaptive_smooth_rate_moves_toward_destination() {
        let irm = IRMConfig {
            adjustment_speed: Decimal::from_str("50").unwrap(),
            min_adaptive_rate: Decimal::percent(1),
            max_adaptive_rate: Decimal::one(),
        };

        // Current 2%, destination 10% - should increase
        let result =
            adaptive_smooth_rate(Decimal::percent(2), Decimal::percent(10), 86400, &irm);
        assert!(result > Decimal::percent(2));
        assert!(result <= Decimal::percent(10));
    }

    #[test]
    fn test_adaptive_smooth_rate_moves_down() {
        let irm = IRMConfig {
            adjustment_speed: Decimal::from_str("50").unwrap(),
            min_adaptive_rate: Decimal::percent(1),
            max_adaptive_rate: Decimal::one(),
        };

        // Current 20%, destination 5% - should decrease
        let result =
            adaptive_smooth_rate(Decimal::percent(20), Decimal::percent(5), 86400, &irm);
        assert!(result < Decimal::percent(20));
        assert!(result >= Decimal::percent(5));
    }

    #[test]
    fn test_adaptive_smooth_rate_clamps_to_bounds() {
        let irm = IRMConfig {
            adjustment_speed: Decimal::from_str("50").unwrap(),
            min_adaptive_rate: Decimal::percent(2),
            max_adaptive_rate: Decimal::percent(50),
        };

        // Destination below min -> should clamp to min
        let result = adaptive_smooth_rate(
            Decimal::percent(3),
            Decimal::percent(1),
            SECONDS_PER_YEAR,
            &irm,
        );
        assert!(result >= Decimal::percent(2));

        // Bootstrap with destination above max -> clamp to max
        let result = adaptive_smooth_rate(
            Decimal::zero(),
            Decimal::percent(80),
            SECONDS_PER_YEAR,
            &irm,
        );
        assert_eq!(result, Decimal::percent(50));
    }

    #[test]
    fn test_accumulate_interest_dec() {
        // 10% rate over 1 year should yield ~10% of the principal
        let result =
            accumulate_interest_dec(Decimal::one(), Decimal::percent(10), SECONDS_PER_YEAR)
                .unwrap();
        // 1.0 * 0.10 * (31536000/31536000) = 0.10
        assert_eq!(result, Decimal::percent(10));
    }

    #[test]
    fn test_accumulate_interest_dec_half_year() {
        // 10% rate over half a year
        let result =
            accumulate_interest_dec(Decimal::one(), Decimal::percent(10), SECONDS_PER_YEAR / 2)
                .unwrap();
        // 1.0 * 0.10 * 0.5 = 0.05
        assert_eq!(result, Decimal::percent(5));
    }

    #[test]
    fn test_calculate_fixed_rate_accrual() {
        // 1,000,000 at 10% over 1 year
        let result = calculate_fixed_rate_accrual(
            Uint128::new(1_000_000),
            Decimal::percent(10),
            0,
            SECONDS_PER_YEAR,
        )
        .unwrap();
        // 1_000_000 * 0.10 * 1.0 = 100_000
        assert_eq!(result, Uint128::new(100_000));
    }

    #[test]
    fn test_calculate_fixed_rate_accrual_partial() {
        // 1,000,000 at 10% over 1/4 year
        let result = calculate_fixed_rate_accrual(
            Uint128::new(1_000_000),
            Decimal::percent(10),
            0,
            SECONDS_PER_YEAR / 4,
        )
        .unwrap();
        // 1_000_000 * 0.10 * 0.25 = 25_000
        assert_eq!(result, Uint128::new(25_000));
    }

    #[test]
    fn test_recalculate_fixed_rate() {
        let rates = Rates {
            current_adaptive_rate: vec![],
            peg_current_adaptive_rate: vec![],
            lastest_collateral_rates: vec![],
            base_interest_rate: Decimal::percent(2),
            fixed_rate_caps: FixedRateCaps {
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
            },
            rates_last_accrued: 1000,
            credit_last_accrued: 1000,
            cpc_margin_of_error: Decimal::one(),
            negative_rates: false,
            acquisition_bump_rate: Decimal::zero(),
        };

        // 5% weighted rate * 1.1 multiplier = 5.5%
        let (rate, end_time) =
            recalculate_fixed_rate(&rates, 1, 1000, Decimal::percent(5)).unwrap();
        assert_eq!(rate, Decimal::from_str("0.055").unwrap());
        assert_eq!(end_time, 1000 + 2_592_000);

        // 3-month: 5% * 1.3 = 6.5%
        let (rate, end_time) =
            recalculate_fixed_rate(&rates, 3, 1000, Decimal::percent(5)).unwrap();
        assert_eq!(rate, Decimal::from_str("0.065").unwrap());
        assert_eq!(end_time, 1000 + 3 * 2_592_000);
    }

    #[test]
    fn test_check_and_handle_fixed_rate_expiration_not_expired() {
        let rates = Rates {
            current_adaptive_rate: vec![],
            peg_current_adaptive_rate: vec![],
            lastest_collateral_rates: vec![],
            base_interest_rate: Decimal::percent(2),
            fixed_rate_caps: FixedRateCaps {
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
            },
            rates_last_accrued: 0,
            credit_last_accrued: 0,
            cpc_margin_of_error: Decimal::one(),
            negative_rates: false,
            acquisition_bump_rate: Decimal::zero(),
        };

        let mut segment = RateSegment {
            amount: Uint128::new(1000),
            fixed_rate: Some(FixedRate {
                rate: Decimal::percent(5),
                end: FixedRateEnd {
                    end_time: 9999999,
                    rollover: false,
                    duration_months: 3,
                },
            }),
        };

        let (was_fixed, duration, converted) =
            check_and_handle_fixed_rate_expiration(&mut segment, &rates, 1000, Decimal::percent(5))
                .unwrap();
        assert!(was_fixed);
        assert_eq!(duration, 3);
        assert!(!converted);
        assert!(segment.fixed_rate.is_some());
    }

    #[test]
    fn test_check_and_handle_fixed_rate_expiration_expired_no_rollover() {
        let rates = Rates {
            current_adaptive_rate: vec![],
            peg_current_adaptive_rate: vec![],
            lastest_collateral_rates: vec![],
            base_interest_rate: Decimal::percent(2),
            fixed_rate_caps: FixedRateCaps {
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
            },
            rates_last_accrued: 0,
            credit_last_accrued: 0,
            cpc_margin_of_error: Decimal::one(),
            negative_rates: false,
            acquisition_bump_rate: Decimal::zero(),
        };

        let mut segment = RateSegment {
            amount: Uint128::new(1000),
            fixed_rate: Some(FixedRate {
                rate: Decimal::percent(5),
                end: FixedRateEnd {
                    end_time: 500,
                    rollover: false,
                    duration_months: 1,
                },
            }),
        };

        let (was_fixed, duration, converted) =
            check_and_handle_fixed_rate_expiration(&mut segment, &rates, 1000, Decimal::percent(5))
                .unwrap();
        assert!(was_fixed);
        assert_eq!(duration, 1);
        assert!(converted);
        assert!(segment.fixed_rate.is_none());
    }

    #[test]
    fn test_check_and_handle_fixed_rate_expiration_rollover() {
        let rates = Rates {
            current_adaptive_rate: vec![],
            peg_current_adaptive_rate: vec![],
            lastest_collateral_rates: vec![],
            base_interest_rate: Decimal::percent(2),
            fixed_rate_caps: FixedRateCaps {
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
            },
            rates_last_accrued: 0,
            credit_last_accrued: 0,
            cpc_margin_of_error: Decimal::one(),
            negative_rates: false,
            acquisition_bump_rate: Decimal::zero(),
        };

        let mut segment = RateSegment {
            amount: Uint128::new(1000),
            fixed_rate: Some(FixedRate {
                rate: Decimal::percent(5),
                end: FixedRateEnd {
                    end_time: 500,
                    rollover: true,
                    duration_months: 3,
                },
            }),
        };

        let (was_fixed, duration, converted) =
            check_and_handle_fixed_rate_expiration(&mut segment, &rates, 1000, Decimal::percent(5))
                .unwrap();
        assert!(was_fixed);
        assert_eq!(duration, 3);
        assert!(!converted); // rolled over, not converted
        assert!(segment.fixed_rate.is_some());
        let fr = segment.fixed_rate.unwrap();
        // 5% * 1.3 = 6.5%
        assert_eq!(fr.rate, Decimal::from_str("0.065").unwrap());
        assert_eq!(fr.end.end_time, 1000 + 3 * 2_592_000);
    }

    #[test]
    fn test_get_total_debt_from_segments() {
        let segments = vec![
            RateSegment {
                amount: Uint128::new(500),
                fixed_rate: None,
            },
            RateSegment {
                amount: Uint128::new(300),
                fixed_rate: Some(FixedRate {
                    rate: Decimal::percent(5),
                    end: FixedRateEnd {
                        end_time: 9999,
                        rollover: false,
                        duration_months: 1,
                    },
                }),
            },
        ];
        assert_eq!(
            get_total_debt_from_segments(&segments),
            Uint128::new(800)
        );
    }

    #[test]
    fn test_get_total_position_debt() {
        let position = DebtPosition {
            position_id: Uint128::one(),
            rate_segments: vec![RateSegment {
                amount: Uint128::new(500),
                fixed_rate: None,
            }],
            peg_rate_segments: vec![RateSegment {
                amount: Uint128::new(200),
                fixed_rate: None,
            }],
            deployed_to: vec![],
            pending_interest: Uint128::zero(),
            total_interest_accrued: Uint128::zero(),
            vol_window_initial_debt: None,
            collateral_rate_indices: vec![],
        };
        assert_eq!(get_total_position_debt(&position), Uint128::new(700));
    }

    #[test]
    fn test_accrue_segments_variable() {
        let rates = Rates {
            current_adaptive_rate: vec![],
            peg_current_adaptive_rate: vec![],
            lastest_collateral_rates: vec![],
            base_interest_rate: Decimal::percent(2),
            fixed_rate_caps: FixedRateCaps {
                one_month: FixedRateCap {
                    cap: Decimal::zero(),
                    amount: Uint128::zero(),
                    multiplier: Decimal::one(),
                },
                three_month: FixedRateCap {
                    cap: Decimal::zero(),
                    amount: Uint128::zero(),
                    multiplier: Decimal::one(),
                },
                six_month: FixedRateCap {
                    cap: Decimal::zero(),
                    amount: Uint128::zero(),
                    multiplier: Decimal::one(),
                },
            },
            rates_last_accrued: 1000,
            credit_last_accrued: 1000,
            cpc_margin_of_error: Decimal::one(),
            negative_rates: false,
            acquisition_bump_rate: Decimal::zero(),
        };

        let mut segs = vec![RateSegment {
            amount: Uint128::new(1000),
            fixed_rate: None,
        }];

        // rate_of_change = 1.05 (5% increase)
        let (accrued, deltas) = accrue_segments(
            &mut segs,
            Decimal::from_str("1.05").unwrap(),
            1000,
            2000,
            &rates,
            Decimal::percent(5),
        )
        .unwrap();

        assert_eq!(accrued, Uint128::new(50));
        assert_eq!(deltas.variable, 50i128);
        assert_eq!(segs[0].amount, Uint128::new(1050));
    }

    #[test]
    fn test_deposit_rate_calculation_logic() {
        // Test the comparative deposit logic (same as CDP test)
        let base_rate = Decimal::percent(2);

        let asset_a_dep = Uint128::new(1_000_000);
        let asset_b_dep = Uint128::new(500_000);

        let highest_dep = asset_a_dep;

        let asset_b_rate = decimal_multiplication(
            base_rate,
            Decimal::from_ratio(highest_dep, asset_b_dep),
        )
        .unwrap();
        assert_eq!(asset_b_rate, Decimal::percent(4));
    }
}
