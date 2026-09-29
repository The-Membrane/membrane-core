#![allow(non_snake_case)]

use std::cmp::min;

use cosmwasm_std::{attr, Decimal, DepsMut, Env, QuerierWrapper, Response, StdResult, Storage};

use membrane::collateral::Config;
use membrane::ltv_disco::{QueryMsg as LTVDiscoQueryMsg, AverageLTVsResponse};
use membrane::math::{decimal_multiplication, decimal_subtraction};
use membrane::types::cAsset;

use crate::ContractError;
use crate::state::{
    LTVUpdateTracker, LTV_UPDATE_TRACKERS, MANUAL_MAX_LTVS, DISCO_BLOCKED,
    BASKET, CONFIG, record_ltv_snapshot,
};

/// Seconds per day for accrual calculations
pub const SECONDS_PER_DAY: u64 = 86_400u64;

// ============================================================================
// Exponential Math (from contracts/cdp/src/rates.rs)
// ============================================================================

/// Taylor series approximation of exp(x) for small x values.
/// exp(x) ≈ 1 + x + x²/2 + x³/6
/// Accurate to <2% error when x is capped at 1.0.
fn taylor_exp(x: Decimal) -> Decimal {
    let x2 = x.checked_mul(x).unwrap_or(Decimal::zero());
    let x3 = x2.checked_mul(x).unwrap_or(Decimal::zero());
    Decimal::one()
        + x
        + x2.checked_div(Decimal::from_ratio(2u128, 1u128)).unwrap_or(Decimal::zero())
        + x3.checked_div(Decimal::from_ratio(6u128, 1u128)).unwrap_or(Decimal::zero())
}

/// Signed taylor exp: handles negative exponents via 1/exp(|x|).
/// For positive x: returns taylor_exp(x)
/// For "negative" x (signaled by is_negative flag): returns 1/taylor_exp(|x|)
fn signed_taylor_exp(abs_x: Decimal, is_negative: bool) -> Decimal {
    let exp_val = taylor_exp(abs_x);
    if is_negative {
        Decimal::one().checked_div(exp_val).unwrap_or(Decimal::zero())
    } else {
        exp_val
    }
}

// ============================================================================
// Main Entry Point
// ============================================================================

/// Update basket LTVs based on Disco average LTVs.
/// Permissionless — iterates all collateral assets and updates their LTVs.
/// Asset update failures are logged but do not block other assets.
pub fn update_basket_ltvs(
    deps: DepsMut,
    env: Env,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut basket = BASKET.load(deps.storage)?;

    let mut attrs = vec![attr("action", "update_basket_ltvs")];
    let mut updated_count = 0u32;

    // Update LTVs for each collateral asset
    for asset in basket.collateral_types.iter_mut() {
        match update_asset_ltv(
            deps.storage,
            deps.querier,
            &env,
            &config,
            asset,
        ) {
            Ok(updated) => {
                if updated {
                    updated_count += 1;
                    attrs.push(attr("updated_asset", asset.asset.info.to_string()));
                }
            }
            Err(e) => {
                // Log but don't fail — continue with other assets
                attrs.push(attr(
                    "warning",
                    format!("Failed to update {}: {}", asset.asset.info.to_string(), e),
                ));
            }
        }
    }

    // Save the updated basket
    BASKET.save(deps.storage, &basket)?;

    attrs.push(attr("assets_updated", updated_count.to_string()));

    Ok(Response::new().add_attributes(attrs))
}

// ============================================================================
// Per-Asset Update
// ============================================================================

/// Update a single asset's LTVs based on Disco and control logic.
/// Only max_LTV is tracked dynamically; max_borrow_LTV is derived as
/// max_LTV - config.ltv_borrow_distance.
fn update_asset_ltv(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: &Env,
    config: &Config,
    asset: &mut cAsset,
) -> Result<bool, ContractError> {
    let asset_denom = asset.asset.info.to_string();
    let current_time = env.block.time.seconds();

    // Query Disco for average max LTV
    let disco_ltvs = query_disco_ltvs(
        querier,
        config.ltv_disco.clone(),
        asset_denom.clone(),
    )?;

    // PRE-MACHINE: Determine effective disco_ltv and handle blocking
    let disco_ltv = if disco_ltvs.average_max_ltv.is_zero() {
        // Disco returns 0 → drift to manually_set_max_ltv + block deposits
        let manual_ltv = MANUAL_MAX_LTVS
            .may_load(storage, asset_denom.clone())?
            .unwrap_or(asset.max_LTV);

        DISCO_BLOCKED.save(storage, asset_denom.clone(), &true)?;

        manual_ltv
    } else {
        // Disco has deposits → unblock
        DISCO_BLOCKED.save(storage, asset_denom.clone(), &false)?;

        disco_ltvs.average_max_ltv
    };

    // Load or initialize tracker
    let mut tracker = get_or_init_tracker(storage, &asset_denom, current_time, asset.max_LTV)?;

    // Process max_LTV update through the state machine
    let (new_max_ltv, updated) = process_ltv_update(
        asset.max_LTV,
        disco_ltv,
        &mut tracker.staged_max_ltv,
        &mut tracker.staged_timestamp,
        tracker.last_update_timestamp,
        current_time,
        config.ltv_upward_kp,
        config.ltv_downward_period,
        config.ltv_downward_decay_rate,
    )?;

    if updated {
        asset.max_LTV = new_max_ltv;

        // Derive max_borrow_LTV from max_LTV - ltv_borrow_distance
        asset.max_borrow_LTV = decimal_subtraction(asset.max_LTV, config.ltv_borrow_distance)
            .unwrap_or(Decimal::zero())
            .max(Decimal::zero());

        // Ensure validity
        cap_ltv_values(&mut asset.max_borrow_LTV, &mut asset.max_LTV)?;

        // Record historical snapshot
        record_ltv_snapshot(
            storage,
            env,
            &asset_denom,
            asset.max_LTV,
            asset.max_borrow_LTV,
            asset.max_LTV,
            tracker.staged_max_ltv,
            asset.max_borrow_LTV,
        )?;
    }

    // Update timestamp on every call (serves both directions)
    tracker.last_update_timestamp = current_time;

    // Save tracker
    LTV_UPDATE_TRACKERS.save(storage, asset_denom, &tracker)?;

    Ok(updated)
}

// ============================================================================
// State Machine: process_ltv_update
// ============================================================================

/// Process a single LTV value through the state machine.
/// Returns (new_value, was_updated).
///
/// **Priority 1:** If staged value exists → WAITING or APPLYING (blocks upward)
/// **Priority 2:** No staged value → evaluate direction (UPWARD, DOWNWARD, STABLE)
#[allow(clippy::too_many_arguments)]
pub fn process_ltv_update(
    current_ltv: Decimal,
    disco_ltv: Decimal,
    staged_ltv: &mut Option<Decimal>,
    staged_timestamp: &mut Option<u64>,
    last_update_timestamp: u64,
    current_time: u64,
    kp: Decimal,
    downward_period: u64,
    downward_decay_rate: Decimal,
) -> Result<(Decimal, bool), ContractError> {
    // === Priority 1: Staged value exists → WAITING or APPLYING ===
    if staged_ltv.is_some() {
        if let Some(staged_ts) = *staged_timestamp {
            if current_time >= staged_ts + downward_period {
                // === APPLYING_DOWNWARD: Timer elapsed → apply decay shift ===
                if let Some(staged_val) = *staged_ltv {
                    let new_ltv = apply_decay_downward_shift(
                        current_ltv,
                        staged_val,
                        downward_decay_rate,
                    )?;

                    // Clear staged values (next call evaluates fresh)
                    *staged_ltv = None;
                    *staged_timestamp = None;

                    return Ok((new_ltv, true));
                }
            } else {
                // === WAITING: Timer not elapsed → update staged bidirectionally ===
                if disco_ltv < current_ltv {
                    // Disco still below current → staged tracks Disco exactly
                    *staged_ltv = Some(disco_ltv);
                } else {
                    // Disco recovered above current → staged = current (zero shift)
                    // Timer still runs (mandatory cooldown)
                    *staged_ltv = Some(current_ltv);
                }
            }
        }

        // LTV frozen while staged exists
        return Ok((current_ltv, false));
    }

    // === Priority 2: No staged value → evaluate direction ===
    if disco_ltv > current_ltv {
        // === ACCRUING_UPWARD: Exponential convergence ===
        let new_ltv = calculate_exponential_accrual(
            current_ltv,
            disco_ltv,
            last_update_timestamp,
            current_time,
            kp,
        )?;

        let updated = new_ltv != current_ltv;
        Ok((new_ltv, updated))
    } else if disco_ltv < current_ltv {
        // === STAGED_DOWNWARD: Start timer ===
        *staged_ltv = Some(disco_ltv);
        *staged_timestamp = Some(current_time);

        Ok((current_ltv, false))
    } else {
        // === STABLE: No change ===
        Ok((current_ltv, false))
    }
}

// ============================================================================
// Upward: Exponential Convergence
// ============================================================================

/// Calculate upward LTV accrual using exponential convergence.
/// Formula: new_ltv = disco - (disco - current) * e^(-kp * time_elapsed / 86400)
/// Path-independent: result is identical regardless of call frequency.
pub fn calculate_exponential_accrual(
    current_ltv: Decimal,
    disco_ltv: Decimal,
    last_update: u64,
    current_time: u64,
    kp: Decimal,
) -> Result<Decimal, ContractError> {
    // Guard against time going backwards
    if current_time < last_update {
        return Ok(current_ltv);
    }

    let time_elapsed = current_time - last_update;

    // If no time has passed, no accrual
    if time_elapsed == 0 {
        return Ok(current_ltv);
    }

    // Calculate exponent: kp * time_elapsed / SECONDS_PER_DAY
    let time_factor = Decimal::from_ratio(time_elapsed, SECONDS_PER_DAY);
    let raw_exponent = decimal_multiplication(kp, time_factor)?;

    // Cap exponent at 1.0 for Taylor series accuracy (<2% error)
    let exponent = min(raw_exponent, Decimal::one());

    // Calculate decay factor: e^(-exponent)
    let decay_factor = signed_taylor_exp(exponent, true);

    // Calculate gap: disco - current
    let gap = decimal_subtraction(disco_ltv, current_ltv)?;

    // new_ltv = disco - gap * decay_factor
    let remaining_gap = decimal_multiplication(gap, decay_factor)?;
    let new_ltv = decimal_subtraction(disco_ltv, remaining_gap)?;

    // Never overshoot the target
    Ok(new_ltv.min(disco_ltv))
}

// ============================================================================
// Downward: Decay Shift
// ============================================================================

/// Apply decay-based downward shift.
/// Formula: actual_shift = (current - staged) * decay_rate
/// Each application takes a fraction of the remaining gap.
pub fn apply_decay_downward_shift(
    current_ltv: Decimal,
    staged_ltv: Decimal,
    decay_rate: Decimal,
) -> Result<Decimal, ContractError> {
    // If staged >= current, no shift needed (zero gap)
    if staged_ltv >= current_ltv {
        return Ok(current_ltv);
    }

    // Calculate total gap
    let total_shift = decimal_subtraction(current_ltv, staged_ltv)?;

    // Apply decay rate: take decay_rate fraction of the gap
    let actual_shift = decimal_multiplication(total_shift, decay_rate)?;

    // Apply the shift
    let new_ltv = decimal_subtraction(current_ltv, actual_shift)?;

    Ok(new_ltv)
}

// ============================================================================
// Helpers
// ============================================================================

/// Query Disco for average max LTV for a specific asset.
fn query_disco_ltvs(
    querier: QuerierWrapper,
    ltv_disco_addr: cosmwasm_std::Addr,
    asset_denom: String,
) -> StdResult<AverageLTVsResponse> {
    querier.query_wasm_smart(
        ltv_disco_addr.to_string(),
        &LTVDiscoQueryMsg::GetAverageLTVs {
            assets: vec![asset_denom],
        },
    )
}

/// Get or initialize tracker for an asset.
/// On first initialization, saves the asset's current max_LTV to MANUAL_MAX_LTVS if not set.
fn get_or_init_tracker(
    storage: &mut dyn Storage,
    asset_denom: &str,
    current_time: u64,
    current_max_ltv: Decimal,
) -> StdResult<LTVUpdateTracker> {
    match LTV_UPDATE_TRACKERS.may_load(storage, asset_denom.to_string())? {
        Some(tracker) => Ok(tracker),
        None => {
            // Initialize MANUAL_MAX_LTVS if not already set
            if MANUAL_MAX_LTVS.may_load(storage, asset_denom.to_string())?.is_none() {
                MANUAL_MAX_LTVS.save(storage, asset_denom.to_string(), &current_max_ltv)?;
            }

            Ok(LTVUpdateTracker {
                last_update_timestamp: current_time,
                staged_max_ltv: None,
                staged_timestamp: None,
            })
        }
    }
}

/// Ensure LTVs stay within valid bounds (0-100%).
/// Enforces max_borrow_LTV < max_LTV by capping borrow at 95% of max if needed.
pub fn cap_ltv_values(
    max_borrow_ltv: &mut Decimal,
    max_ltv: &mut Decimal,
) -> Result<(), ContractError> {
    // Clamp to 0-100%
    *max_borrow_ltv = (*max_borrow_ltv).min(Decimal::percent(100)).max(Decimal::zero());
    *max_ltv = (*max_ltv).min(Decimal::percent(100)).max(Decimal::zero());

    // Ensure max_borrow_LTV < max_LTV
    if *max_borrow_ltv >= *max_ltv {
        *max_borrow_ltv = decimal_multiplication(*max_ltv, Decimal::percent(95))?;
    }

    Ok(())
}
