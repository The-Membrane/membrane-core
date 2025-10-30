use cosmwasm_std::{attr, Decimal, DepsMut, Env, QuerierWrapper, Response, StdResult, Storage};

use membrane::cdp::Config;
use membrane::ltv_disco::{QueryMsg as LTVDiscoQueryMsg, AverageLTVsResponse};
use membrane::math::{decimal_multiplication, decimal_subtraction};
use membrane::types::cAsset;

use crate::ContractError;
use crate::state::{LTVUpdateTracker, LTV_UPDATE_TRACKERS, BASKET, CONFIG};

/// Seconds per day for proportional controller calculations
pub const SECONDS_PER_DAY: u64 = 86_400u64;

/// Update basket LTVs based on Disco average LTVs
/// This is called either directly via ExecuteMsg or automatically during accrue
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
                // Log but don't fail - continue with other assets
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

/// Update a single asset's LTVs based on Disco and control logic
fn update_asset_ltv(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: &Env,
    config: &Config,
    asset: &mut cAsset,
) -> Result<bool, ContractError> {
    let asset_denom = asset.asset.info.to_string();
    let current_time = env.block.time.seconds();
    
    // Query Disco for average LTVs
    let disco_ltvs = query_disco_ltvs(
        querier,
        config.ltv_disco.clone(),
        asset_denom.clone(),
    )?;
    
    // If Disco returns zero (no deposits), don't update
    if disco_ltvs.average_max_ltv.is_zero() || disco_ltvs.average_max_borrow_ltv.is_zero() {
        return Ok(false);
    }
    
    // Load or initialize tracker
    let mut tracker = get_or_init_tracker(storage, asset_denom.clone(), current_time)?;
    
    let mut updated = false;
    
    // Process max_LTV
    let (new_max_ltv, max_ltv_updated) = process_ltv_update(
        asset.max_LTV,
        disco_ltvs.average_max_ltv,
        &mut tracker.staged_max_ltv,
        &mut tracker.staged_timestamp,
        tracker.last_upward_update,
        current_time,
        config.ltv_upward_kp,
        config.ltv_downward_period,
        config.ltv_max_downward_shift,
    )?;
    
    if max_ltv_updated {
        asset.max_LTV = new_max_ltv;
        updated = true;
    }
    
    // Process max_borrow_LTV
    let (new_max_borrow_ltv, max_borrow_ltv_updated) = process_ltv_update(
        asset.max_borrow_LTV,
        disco_ltvs.average_max_borrow_ltv,
        &mut tracker.staged_max_borrow_ltv,
        &mut tracker.staged_timestamp,
        tracker.last_upward_update,
        current_time,
        config.ltv_upward_kp,
        config.ltv_downward_period,
        config.ltv_max_downward_shift,
    )?;
    
    if max_borrow_ltv_updated {
        asset.max_borrow_LTV = new_max_borrow_ltv;
        updated = true;
    }
    
    // Update tracker's last upward update timestamp
    tracker.last_upward_update = current_time;
    
    // Ensure LTVs are valid: 0 < max_borrow_LTV < max_LTV < 1
    cap_ltv_values(&mut asset.max_borrow_LTV, &mut asset.max_LTV)?;
    
    // Save tracker
    LTV_UPDATE_TRACKERS.save(storage, asset_denom, &tracker)?;
    
    Ok(updated)
}

/// Process a single LTV value (either max_LTV or max_borrow_LTV)
/// Returns (new_value, was_updated)
#[allow(clippy::too_many_arguments)]
pub fn process_ltv_update(
    current_ltv: Decimal,
    disco_ltv: Decimal,
    staged_ltv: &mut Option<Decimal>,
    staged_timestamp: &mut Option<u64>,
    last_upward_update: u64,
    current_time: u64,
    kp: Decimal,
    downward_period: u64,
    max_downward_shift: Decimal,
) -> Result<(Decimal, bool), ContractError> {
    let mut new_ltv = current_ltv;
    let mut updated = false;
    
    // Determine direction: upward or downward
    if disco_ltv > current_ltv {
        // Upward movement: use proportional controller
        new_ltv = calculate_upward_accrual(
            current_ltv,
            disco_ltv,
            last_upward_update,
            current_time,
            kp,
        )?;
        
        // Clear any staged downward values since we're going up
        *staged_ltv = None;
        *staged_timestamp = None;
        
        updated = new_ltv != current_ltv;
        
    } else if disco_ltv < current_ltv {
        // Downward movement: stage and apply with delay
        
        // Check if we should apply a previously staged downward shift
        if let Some(staged_ts) = staged_timestamp {
            if current_time >= *staged_ts + downward_period {
                // Period elapsed, apply the staged shift
                if let Some(staged_val) = staged_ltv {
                    new_ltv = apply_capped_downward_shift(
                        current_ltv,
                        *staged_val,
                        max_downward_shift,
                    )?;
                    
                    // Clear staged values (next downward needs new full period)
                    *staged_ltv = None;
                    *staged_timestamp = None;
                    
                    updated = true;
                }
            } else {
                // Still waiting - update staged value if Disco is even lower
                if disco_ltv < staged_ltv.unwrap_or(current_ltv) {
                    *staged_ltv = Some(disco_ltv);
                }
            }
        } else {
            // No staged value yet - start the timer
            *staged_ltv = Some(disco_ltv);
            *staged_timestamp = Some(current_time);
        }
    }
    // If disco_ltv == current_ltv, no change needed
    
    Ok((new_ltv, updated))
}

/// Calculate upward LTV accrual using proportional controller
pub fn calculate_upward_accrual(
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
    
    // Calculate error (how far we are from target)
    let error = decimal_subtraction(disco_ltv, current_ltv)?;
    
    // Calculate accrual: kp * error * (time_elapsed / SECONDS_PER_DAY)
    // This gives us the proportional response per day
    let time_factor = Decimal::from_ratio(time_elapsed, SECONDS_PER_DAY);
    let accrual = decimal_multiplication(
        decimal_multiplication(kp, error)?,
        time_factor,
    )?;
    
    // Apply accrual but don't overshoot
    let new_ltv = current_ltv + accrual;
    Ok(new_ltv.min(disco_ltv))
}

/// Apply capped downward shift
pub fn apply_capped_downward_shift(
    current_ltv: Decimal,
    staged_ltv: Decimal,
    max_shift_percent: Decimal,
) -> Result<Decimal, ContractError> {
    // Calculate the desired shift
    let desired_shift = decimal_subtraction(current_ltv, staged_ltv)?;
    
    // Calculate the maximum allowed shift
    let max_allowed_shift = decimal_multiplication(current_ltv, max_shift_percent)?;
    
    // Take the minimum of desired and allowed
    let actual_shift = desired_shift.min(max_allowed_shift);
    
    // Apply the shift
    let new_ltv = decimal_subtraction(current_ltv, actual_shift)?;
    
    Ok(new_ltv)
}

/// Query Disco for average LTVs for a specific asset
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

/// Get or initialize tracker for an asset
fn get_or_init_tracker(
    storage: &mut dyn Storage,
    asset_denom: String,
    current_time: u64,
) -> StdResult<LTVUpdateTracker> {
    match LTV_UPDATE_TRACKERS.may_load(storage, asset_denom.clone())? {
        Some(tracker) => Ok(tracker),
        None => {
            // Initialize new tracker
            let tracker = LTVUpdateTracker {
                last_upward_update: current_time,
                staged_max_ltv: None,
                staged_max_borrow_ltv: None,
                staged_timestamp: None,
            };
            Ok(tracker)
        }
    }
}

/// Ensure LTVs stay within valid bounds
pub fn cap_ltv_values(
    max_borrow_ltv: &mut Decimal,
    max_ltv: &mut Decimal,
) -> Result<(), ContractError> {
    // Ensure values are within 0-100%
    *max_borrow_ltv = (*max_borrow_ltv).min(Decimal::percent(100)).max(Decimal::zero());
    *max_ltv = (*max_ltv).min(Decimal::percent(100)).max(Decimal::zero());
    
    // Ensure max_borrow_LTV < max_LTV
    if *max_borrow_ltv >= *max_ltv {
        // Adjust max_borrow_LTV to be slightly less than max_LTV
        *max_borrow_ltv = decimal_multiplication(*max_ltv, Decimal::percent(95))?;
    }
    
    Ok(())
}

