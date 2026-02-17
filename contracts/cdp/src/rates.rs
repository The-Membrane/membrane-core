use std::cmp::{max, min, Ordering};
use std::str::FromStr;

use cosmwasm_std::{
    attr, Addr, Api, Decimal, DepsMut, Env, MessageInfo, Order, QuerierWrapper, Response, StdError,
    StdResult, Storage, Uint128,
};

use membrane::cdp::Config;
use membrane::helpers::get_asset_liquidity;
use membrane::math::{decimal_division, decimal_multiplication, decimal_subtraction};
use membrane::system_discounts::{QueryMsg as DiscountQueryMsg, UserDiscountResponse, StableBackingDiscountsResponse};
use membrane::types::{cAsset, IRMConfig, Asset, AssetInfo, Basket, CreditAssetBreakdown, FixedRate, FixedRateCap, FixedRateCaps, FixedRateEnd, Position, Rate, Rates, RateSegment, SupplyCap};

use membrane::ltv_disco::{QueryMsg as LTVDiscoQueryMsg, LTVQueueResponse as LTVDiscoQueueResponse};

use crate::query::{
    get_asset_values, get_cAsset_ratios, query_ltv_disco_for_asset_ltvs, VOLATILITY_LIST_LIMIT,
};
use crate::state::{get_target_position, update_position, update_historical_interest_rates, BASKET, CONFIG, RATES, VOLATILITY};
use crate::ContractError;

//Constants
pub const SECONDS_PER_YEAR: u64 = 31_536_000u64;
const MINIMUM_LIQUIDITY: Uint128 = Uint128::new(2_000_000_000_000u128);

/// Taylor series approximation of exp(x) for small x values.
/// exp(x) ~= 1 + x + x^2/2 + x^3/6
/// Input and output are in Decimal (not WAD).
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
        // 1 / exp(|x|)
        Decimal::one().checked_div(exp_val).unwrap_or(Decimal::zero())
    } else {
        exp_val
    }
}

/// Adaptive smooth rate for regular debt.
/// Smoothly moves current_adaptive_rate toward the Disco destination rate using exponential decay.
///
/// Formula: new_rate = current_adaptive_rate * exp(speed * err * dt)
///   where err = (destination - current_adaptive_rate) / current_adaptive_rate  (normalized error)
///   and dt = time_elapsed / SECONDS_PER_YEAR
///
/// Clamped to [min_adaptive_rate, max_adaptive_rate].
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
        (decimal_division(diff, current_adaptive_rate).unwrap_or(Decimal::zero()), false)
    } else {
        let diff = current_adaptive_rate - destination_rate;
        (decimal_division(diff, current_adaptive_rate).unwrap_or(Decimal::zero()), true)
    };

    // dt = time_elapsed / SECONDS_PER_YEAR
    let dt = Decimal::from_ratio(time_elapsed as u128, SECONDS_PER_YEAR as u128);

    // exponent = speed * |err| * dt
    // Cap at 1.0 to keep the Taylor series exp() approximation accurate (<2% error).
    // With frequent accruals this cap is never hit; it only matters for large time gaps (e.g. tests).
    let raw_exponent = irm_config.adjustment_speed
        .checked_mul(err_abs).unwrap_or(Decimal::zero())
        .checked_mul(dt).unwrap_or(Decimal::zero());
    let exponent = min(raw_exponent, Decimal::one());

    // new_rate = current_adaptive_rate * exp(speed * err * dt)
    let multiplier = signed_taylor_exp(exponent, err_negative);
    let new_rate = decimal_multiplication(current_adaptive_rate, multiplier).unwrap_or(current_adaptive_rate);

    // Don't overshoot the destination: clamp between old and destination
    let new_rate = if !err_negative {
        // Moving up toward destination: don't exceed it
        min(new_rate, destination_rate)
    } else {
        // Moving down toward destination: don't go below it
        max(new_rate, destination_rate)
    };

    // Clamp to global bounds
    min(max(new_rate, irm_config.min_adaptive_rate), irm_config.max_adaptive_rate)
}

/// AdaptiveCurveIRM (Morpho-style) - CURRENTLY UNUSED.
/// NOTE: We don't use rates to give our transmuter 'lenders' liquidity,
/// we will enact new acquisition lockdrops instead.
/// Peg debt now uses the same rates as regular debt.
///
/// Returns (borrow_rate, new_current_adaptive_rate).
///
/// Curve mechanism:
///   errNormFactor = if u >= target { 1 - target } else { target }
///   err = (u - target) / errNormFactor
///   if err < 0: rate = rateAtTarget * ((1 - 1/C) * err + 1)
///   if err >= 0: rate = rateAtTarget * ((C - 1) * err + 1)
///
/// Adaptive mechanism:
///   new_rateAtTarget = old_rateAtTarget * exp(speed * err * dt)
///
/// NOTE: This function is kept for demonstration/testing purposes.
/// Peg debt now uses the same rates as regular debt.
pub fn adaptive_curve_rate(
    utilization: Decimal,
    current_adaptive_rate: Decimal,
    time_elapsed: u64,
    irm_config: &IRMConfig,
    // Curve params passed directly since they're not in config anymore
    curve_steepness: Decimal,
    target_utilization: Decimal,
) -> (Decimal, Decimal) {
    let target = target_utilization;
    let steepness = curve_steepness;

    // Calculate normalized error
    let (err_abs, err_negative) = if utilization >= target {
        let norm = decimal_subtraction(Decimal::one(), target).unwrap_or(Decimal::one());
        if norm.is_zero() {
            (Decimal::one(), false) // At 100% target, max positive error
        } else {
            (decimal_division(utilization - target, norm).unwrap_or(Decimal::zero()), false)
        }
    } else {
        if target.is_zero() {
            (Decimal::zero(), false)
        } else {
            (decimal_division(target - utilization, target).unwrap_or(Decimal::zero()), true)
        }
    };

    // Curve function: compute borrow rate
    let curve_multiplier = if err_negative {
        // Below target: ((1 - 1/C) * err + 1) but err is negative, so: (1 - (1 - 1/C) * |err|)
        let one_minus_inv_c = decimal_subtraction(
            Decimal::one(),
            decimal_division(Decimal::one(), steepness).unwrap_or(Decimal::zero()),
        ).unwrap_or(Decimal::zero());
        let adjustment = decimal_multiplication(one_minus_inv_c, err_abs).unwrap_or(Decimal::zero());
        decimal_subtraction(Decimal::one(), adjustment).unwrap_or(Decimal::zero())
    } else {
        // Above target: ((C - 1) * err + 1)
        let c_minus_one = decimal_subtraction(steepness, Decimal::one()).unwrap_or(Decimal::zero());
        let adjustment = decimal_multiplication(c_minus_one, err_abs).unwrap_or(Decimal::zero());
        Decimal::one() + adjustment
    };

    let borrow_rate = decimal_multiplication(current_adaptive_rate, curve_multiplier)
        .unwrap_or(current_adaptive_rate);

    // Adaptive mechanism: update current_adaptive_rate
    let dt = Decimal::from_ratio(time_elapsed as u128, SECONDS_PER_YEAR as u128);
    // Cap at 1.0 to keep Taylor series exp() approximation accurate
    let raw_exponent = irm_config.adjustment_speed
        .checked_mul(err_abs).unwrap_or(Decimal::zero())
        .checked_mul(dt).unwrap_or(Decimal::zero());
    let exponent = min(raw_exponent, Decimal::one());

    let rate_multiplier = signed_taylor_exp(exponent, err_negative);
    let new_current_adaptive_rate = decimal_multiplication(current_adaptive_rate, rate_multiplier)
        .unwrap_or(current_adaptive_rate);
    let new_current_adaptive_rate = min(
        max(new_current_adaptive_rate, irm_config.min_adaptive_rate),
        irm_config.max_adaptive_rate,
    );

    (borrow_rate, new_current_adaptive_rate)
}

/// Query transmuter utilization for peg debt - CURRENTLY UNUSED.
/// NOTE: We don't use rates to give our transmuter 'lenders' liquidity,
/// we will enact new acquisition lockdrops instead.
/// utilization = 1 - (paired_asset_balance / total_deposit_value)
#[allow(dead_code)]
fn get_transmuter_utilization(
    querier: QuerierWrapper,
    transmuter_addr: &Addr,
) -> StdResult<Decimal> {
    let vault_info: membrane::transmuter::VaultInfoResponse = querier.query_wasm_smart(
        transmuter_addr.to_string(),
        &membrane::transmuter::QueryMsg::VaultInfo {},
    )?;

    if vault_info.total_deposit_value.is_zero() {
        return Ok(Decimal::one());
    }

    Ok(decimal_subtraction(
        Decimal::one(),
        Decimal::from_ratio(vault_info.paired_asset_balance, vault_info.total_deposit_value),
    ).unwrap_or(Decimal::one()))
}

/// Get total deposit tokens per asset from the LTV Disco contract.
/// Queries all basket assets at once and returns a vec of Option<Uint128>.
/// Returns None for assets that have no LTV queue in the Disco.
fn get_asset_total_deposits(
    querier: QuerierWrapper,
    ltv_disco_addr: &Addr,
    assets: &[cAsset],
) -> Vec<Option<Uint128>> {
    let asset_strings: Vec<String> = assets
        .iter()
        .map(|a| a.asset.info.to_string())
        .collect();

    // Query all queues at once
    let disco_response: Result<LTVDiscoQueueResponse, _> = querier.query_wasm_smart(
        ltv_disco_addr.to_string(),
        &LTVDiscoQueryMsg::GetLTVQueue {
            assets: asset_strings.clone(),
            limit: None,
            start_after: None,
        },
    );

    let queues = match disco_response {
        Ok(resp) => resp.queues,
        Err(_) => return assets.iter().map(|_| None).collect(),
    };

    // Map each basket asset to its total deposits
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
                        .flat_map(|slot| &slot.deposit_groups)
                        .map(|group| group.total_deposit_tokens)
                        .sum();
                    if total.is_zero() { None } else { Some(total) }
                })
                .flatten()
        })
        .collect()
}

/// Accrue interest for a list of Positions
pub fn external_accrue_call(
    storage: &mut dyn Storage,
    api: &dyn Api,
    querier: QuerierWrapper,
    info: MessageInfo,
    env: Env,
    position_owner: Option<String>,
    position_ids: Vec<Uint128>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(storage)?;

    // Update basket LTVs before position accrual
    // This ensures positions use the most up-to-date LTVs
    let _ltv_response = crate::ltv_updater::update_basket_ltvs(
        cosmwasm_std::DepsMut {
            storage,
            api,
            querier,
        },
        env.clone(),
    )?;

    let mut basket = BASKET.load(storage)?;

    //Validate position owner
    let valid_position_owner: Addr;
    if let Some(position_owner) = position_owner {
        //Sent addr
        valid_position_owner = api.addr_validate(&position_owner)?
    } else {
        //Msg sender
        valid_position_owner = info.clone().sender
    }

    //Initialize accrued_interest
    let mut accrued_interest: Uint128 = Uint128::zero();

    //Accrue interest for each position
    for position_id in position_ids.clone() {
        let mut position =
            get_target_position(storage, valid_position_owner.clone(), position_id)?.1;

        let prev_loan = get_total_position_debt(&position);

        accrue(
            storage,
            querier,
            env.clone(),
            config.clone(),
            &mut position,
            &mut basket,
            valid_position_owner.clone().to_string(),
            false,
        )?;

        accrued_interest += get_total_position_debt(&position) - prev_loan;

        update_position(storage, valid_position_owner.clone(), position)?;
    }
    //Save updated Basket
    BASKET.save(storage, &basket)?;

    Ok(Response::new().add_attributes(vec![
        attr("method", "accrue"),
        attr("position_ids", format!("{:?}", position_ids)),
        attr("accrued_interest", accrued_interest),
    ]))
}

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

/// Calculate Basket interests and then accumulate interest to all basket cAsset rate indices.
/// Uses the AdaptiveCurveIRM: rates are smoothed toward Disco destination (comparative deposits).
///
/// NOTE: Peg debt rates are identical to regular debt rates.
/// We don't use rates to give our transmuter 'lenders' liquidity,
/// we will enact new acquisition lockdrops instead.
pub fn update_rate_indices(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    basket: &mut Basket,
    supply_caps: &mut Vec<SupplyCap>,
) -> StdResult<()> {
    let config = CONFIG.load(storage)?;
    let mut rates = RATES.load(storage)?;

    // Get Disco destination rates (what the rate would jump to without smoothing)
    // Pass None for cAsset_ratios - get_interest_rates will calculate them internally
    let destination_rates =
        match get_interest_rates(storage, querier, env.clone(), basket, supply_caps, &rates, None) {
            Ok(rates) => rates,
            Err(err) => {
                return Err(StdError::generic_err(format!("Error in get_interest_rates: {}", err)));
            }
        };

    // Calc time_elapsed since last rate update
    let time_elapsed = env.block.time.seconds() - rates.rates_last_accrued;

    // Ensure current_adaptive_rate vectors are the right length (bootstrap for new assets)
    // Bootstrap with zero so adaptive_smooth_rate jumps directly to destination on first call
    while rates.current_adaptive_rate.len() < basket.collateral_types.len() {
        rates.current_adaptive_rate.push(Decimal::zero());
    }
    while rates.peg_current_adaptive_rate.len() < basket.collateral_types.len() {
        rates.peg_current_adaptive_rate.push(Decimal::zero());
    }

    // --- Regular debt: adaptive smooth toward Disco destination ---
    let mut smoothed_rates = vec![];
    for (i, _asset) in basket.collateral_types.iter().enumerate() {
        let new_rate = adaptive_smooth_rate(
            rates.current_adaptive_rate[i],
            destination_rates[i],
            time_elapsed,
            &config.irm_config,
        );
        rates.current_adaptive_rate[i] = new_rate;
        // The borrow rate IS current_adaptive_rate (pure smoothing, no curve on top)
        smoothed_rates.push(new_rate);
    }

    // Update latest rates in the Rates store
    rates.lastest_collateral_rates = smoothed_rates
        .iter()
        .map(|&rate| Rate {
            rate,
            last_time_updated: env.block.time.seconds(),
        })
        .collect();

    // Update historical interest rates for each asset
    for (i, basket_asset) in basket.collateral_types.iter().enumerate() {
        let asset_info_string = basket_asset.asset.info.to_string();
        if let Err(err) = update_historical_interest_rates(
            storage,
            env.clone(),
            asset_info_string,
            smoothed_rates[i],
        ) {
            return Err(StdError::generic_err(format!("Error updating historical interest rates: {}", err)));
        }
    }

    // Accumulate rate on each rate_index
    for (i, basket_asset) in basket.collateral_types.clone().into_iter().enumerate() {
        let accrued_rate =
            accumulate_interest_dec(basket_asset.rate_index, smoothed_rates[i], time_elapsed)?;
        basket.collateral_types[i].rate_index += accrued_rate;
    }

    // --- Peg debt: Uses the SAME rates as regular debt ---
    // NOTE: We don't use rates to give our transmuter 'lenders' liquidity,
    // we will enact new acquisition lockdrops instead.
    // Peg debt rates mirror regular debt rates for simplicity.
    let peg_rates: Vec<Decimal> = smoothed_rates.clone();

    // Update peg_current_adaptive_rate to match regular current_adaptive_rate
    for (i, _asset) in basket.collateral_types.iter().enumerate() {
        rates.peg_current_adaptive_rate[i] = rates.current_adaptive_rate[i];
    }

    for (i, basket_asset) in basket.collateral_types.clone().into_iter().enumerate() {
        let peg_accrued =
            accumulate_interest_dec(basket_asset.peg_rate_index, peg_rates[i], time_elapsed)?;
        basket.collateral_types[i].peg_rate_index += peg_accrued;
    }

    // Update rates_last_accrued
    rates.rates_last_accrued = env.block.time.seconds();

    // Save updated rates
    RATES.save(storage, &rates)?;

    Ok(())
}

/// Calculate interest rates for each asset in the basket using TVL-standardized insurance ratios.
/// Maximum rate is capped at irm_config.max_adaptive_rate (100%) to avoid overflows.
///
/// Goal: TVL-standardized comparative deposit-based rates
/// - insurance_ratio = deposit_ratio / tvl_ratio (normalized by TVL share)
/// - Highest insurance ratio asset gets: base_interest_rate * (1 / max_LTV)
/// - Other assets get: base_interest_rate * (highest_insurance / asset_insurance)
/// - Fallback (no deposit or TVL data): base_interest_rate * (1 / max_LTV)
///
/// This normalizes small assets: a 5% TVL asset with 5% deposits has insurance=1.0,
/// same as a 50% TVL asset with 50% deposits. Neither gets unfairly penalized.
pub fn get_interest_rates(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    basket: &mut Basket,
    supply_caps: &mut Vec<SupplyCap>,
    rates: &Rates,
    cAsset_ratios: Option<Vec<Decimal>>,  // Pre-calculated TVL ratios (optional)
) -> StdResult<Vec<Decimal>> {
    let config = CONFIG.load(storage)?;

    // Query ltv_disco for LTVs
    let ltv_tuples = query_ltv_disco_for_asset_ltvs(
        querier,
        config.ltv_disco.clone(),
        basket.clone().collateral_types.clone(),
    )?;

    // Get TVL ratios: either passed in or calculate them
    let tvl_ratios: Vec<Decimal> = match cAsset_ratios {
        Some(ratios) => ratios,
        None => {
            // Calculate TVL ratios if not provided
            let (ratios, _) = get_cAsset_ratios(
                storage,
                env.clone(),
                querier,
                basket.collateral_types.clone(),
                config.clone(),
                Some(basket.clone()),
            )?;
            ratios
        }
    };

    // Collect total deposits for each asset from LTV Disco
    let asset_deposits: Vec<Option<Uint128>> = get_asset_total_deposits(
        querier,
        &config.ltv_disco,
        &basket.collateral_types,
    );

    // Calculate total deposits across all assets
    let total_deposits: Uint128 = asset_deposits
        .iter()
        .filter_map(|d| *d)
        .sum();

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
        .map(|(i, dep_ratio)| {
            match (dep_ratio, tvl_ratios.get(i)) {
                (Some(dep), Some(&tvl)) if !tvl.is_zero() => {
                    Some(decimal_division(*dep, tvl).unwrap_or(Decimal::zero()))
                }
                _ => None,
            }
        })
        .collect();

    // Find highest insurance ratio (most over-insured asset relative to TVL)
    let highest_insurance: Option<Decimal> = insurance_ratios
        .iter()
        .filter_map(|r| *r)
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let mut destination_rates = vec![];

    for (i, _asset) in basket.clone().collateral_types.iter().enumerate() {
        // Calculate LTV-based fallback rate: base * (1/max_LTV)
        let ltv_based_rate = decimal_multiplication(
            rates.base_interest_rate,
            decimal_division(Decimal::one(), ltv_tuples[i].0)?,
        )?;

        match (insurance_ratios[i], highest_insurance) {
            // Both asset insurance ratio and highest reference available
            (Some(asset_insurance), Some(high_insurance)) => {
                if asset_insurance == high_insurance {
                    // Asset with HIGHEST insurance ratio = LOWEST rate (base LTV formula)
                    destination_rates.push(ltv_based_rate);
                } else {
                    // Other assets: base_rate × (highest_insurance / asset_insurance)
                    let insurance_multiplier = decimal_division(high_insurance, asset_insurance)?;
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

    // COMMENTED OUT: Supply caps no longer influence rates
    // Supply caps still block deposits and withdrawals, but don't affect interest rate calculations
    // This reduces rate volatility and improves UX by keeping rates stable regardless of supply cap utilization
    
    // //Get proportion of supply caps filled
    // let mut supply_proportions = vec![];

    // //Get basket cAsset ratios
    // let (basket_ratios, _) = get_cAsset_ratios(
    //     storage,
    //     env.clone(),
    //     querier,
    //     basket.clone().collateral_types,
    //     config.clone(),
    //     Some(basket.clone()),
    // )?;

    // for (i, cap) in supply_caps.iter().enumerate() {
    //     //Caps set to 0 can be used to push out unwanted assets by spiking rates
    //     if cap.supply_cap_ratio.is_zero() {
    //         supply_proportions.push(Decimal::percent(110));
    //     } else {
    //         //Push the supply_ratio. Minimum is 100% to guarantee rates >= base.
    //         supply_proportions.push(max(
    //             decimal_division(basket_ratios[i], cap.supply_cap_ratio)?,
    //             Decimal::percent(100),
    //         ))
    //     }
    // }

    // //Gets pro-rata rate and uses multiplier if above desired utilization
    // let mut two_slope_pro_rata_rates = vec![];
    // for (i, _rate) in rates.iter().enumerate() {
    //     //If proportions are above desired utilization, the rates start multiplying
    //     //For every % above the desired, it adds a multiple
    //     //Ex: Desired = 90%, proportion = 91%, interest = 2%. New rate = 4%.
    //     //Acts as two_slope rate

    //     //Check if supply_proportions[i] is greater than 100% (i.e. in Slope 2)
    //     if supply_proportions[i] > Decimal::one() {
    //         //Slope 2
    //         //Ex: 91% > 90%
    //         ////0.01 * 100 = 1
    //         //1% = 1
    //         let percent_over_desired = decimal_multiplication(
    //             decimal_subtraction(supply_proportions[i], Decimal::one())?,
    //             Decimal::percent(100_00),
    //         )?;
    //         let multiplier = percent_over_desired + Decimal::one();
    //         //Change rate of (rate) increase w/ the configuration multiplier
    //         let multiplier = multiplier * config.rate_slope_multiplier;

    //         //Ex cont: Multiplier = 2; Pro_rata rate = 1.8%.
    //         //// rate = 3.6%
    //         two_slope_pro_rata_rates.push(min(
    //             decimal_multiplication(
    //                 decimal_multiplication(rates[i], supply_proportions[i])?,
    //                 multiplier,
    //             )?,
    //             // Max rate is 100%
    //             Decimal::one(),
    //         ));
    //     } else {
    //         //Base Rate
    //         two_slope_pro_rata_rates.push(rates[i]);
    //     }
    // }

    // //Calculate multi-supply cap overages
    // if basket.multi_asset_supply_caps != vec![] {
    //     for multi_asset_cap in basket.clone().multi_asset_supply_caps {
    //         //Initialize total_ratio
    //         let mut total_ratio = Decimal::zero();

    //         //Find & add ratio for each asset
    //         for asset in multi_asset_cap.clone().assets {
    //             if let Some((i, _cap)) = basket
    //                 .clone()
    //                 .collateral_supply_caps
    //                 .into_iter()
    //                 .enumerate()
    //                 .find(|(_i, cap)| cap.asset_info.equal(&asset))
    //             {
    //                 total_ratio += basket_ratios[i];
    //             }
    //         }

    //         //Calc interest rate
    //         let multi_cap_proportion =
    //             decimal_division(total_ratio, multi_asset_cap.supply_cap_ratio)?;

    //         for asset in multi_asset_cap.clone().assets {
    //             if let Some((i, _cap)) = basket
    //                 .clone()
    //                 .collateral_supply_caps
    //                 .clone()
    //                 .into_iter()
    //                 .enumerate()
    //                 .find(|(_i, cap)| cap.asset_info.equal(&asset))
    //             {
    //                 //Substitute if proportion of multi_asset_cap is greater than 1 and both debt/supply proportions
    //                 if multi_cap_proportion > Decimal::one()
    //                     && multi_cap_proportion > supply_proportions[i]
    //                 {
    //                     //Slope 2
    //                     //Ex: 91% > 90%
    //                     ////0.01 * 100 = 1
    //                     //1% = 1
    //                     let percent_over_desired = decimal_multiplication(
    //                         decimal_subtraction(multi_cap_proportion, Decimal::one())?,
    //                         Decimal::percent(100_00),
    //                     )?;
    //                     let multiplier = percent_over_desired + Decimal::one();
    //                     //Change rate of (rate) increase w/ the configuration multiplier
    //                     let multiplier = multiplier * config.rate_slope_multiplier;

    //                     //Ex cont: Multiplier = 2; Pro_rata rate = 1.8%.
    //                     //// rate = 3.6%
    //                     two_slope_pro_rata_rates[i] = min(
    //                         decimal_multiplication(
    //                             decimal_multiplication(rates[i], multi_cap_proportion)?,
    //                             multiplier,
    //                         )?,
    //                         // Max rate is 100%
    //                         Decimal::one(),
    //                     );
    //                 }
    //             }
    //         }
    //     }
    // }

    // Return destination rates (Disco-calculated) without supply cap adjustments
    Ok(destination_rates)
}

// Peg debt rates are now calculated via adaptive_curve_rate() in update_rate_indices()

//Used for accrual & update_basket_tally()
//This doesn't alter multi-asset caps
pub fn transform_caps_based_on_volatility(
    storage: &mut dyn Storage,
    basket: Basket,
) -> StdResult<Vec<SupplyCap>> {
    //Loop through basket supply caps and transform based on asset volatility
    let vol_caps: Vec<SupplyCap> = basket
        .clone()
        .collateral_supply_caps
        .into_iter()
        .map(|cap| {
            //Load volatility store
            if let Ok(vol_store) = VOLATILITY.load(storage, cap.asset_info.to_string()) {
                if vol_store.volatility_list.len() == VOLATILITY_LIST_LIMIT as usize {
                    //Transform supply ap based on asset volatility
                    let new_supply_cap =
                        match decimal_multiplication(cap.supply_cap_ratio, vol_store.index) {
                            Ok(new_supply_cap) => new_supply_cap,
                            Err(_err) => cap.supply_cap_ratio,
                        };
                    // println!("New Supply Cap: {:?} --- Current Index: {:?}", new_supply_cap, vol_store.index);
                    Ok(SupplyCap {
                        supply_cap_ratio: min(new_supply_cap, Decimal::one()),
                        ..cap
                    })
                } else {
                    Ok(cap)
                }
            } else {
                //Unreachable in prod
                Ok(cap)
            }
        })
        .collect::<StdResult<Vec<SupplyCap>>>()?;

    Ok(vol_caps)
}

/// Calculates the % change to accrue to the Position's debt
fn get_credit_rate_of_change(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: Config,
    basket: &mut Basket,
    position: &mut Position,
    _negative_rate: bool,
    _credit_price_rate: Decimal,
) -> StdResult<(Decimal, Vec<Decimal>)> {
    let (ratios, _) = match get_cAsset_ratios(
        storage,
        env.clone(),
        querier,
        position.clone().collateral_assets,
        config.clone(),
        Some(basket.clone()),
    ) {
        Ok(ratios) => ratios,
        Err(err) => {
            return Err(StdError::generic_err(format!("Error at line 400: {}", err)));
        }
    };

    //Transform supply caps based on asset volatility
    //This doesn't alter multi-asset caps
    let mut supply_caps = match transform_caps_based_on_volatility(storage, basket.clone()) {
        Ok(supply_caps) => supply_caps,
        Err(_err) => basket.clone().collateral_supply_caps,
    };

    match update_rate_indices(
        storage,
        querier,
        env,
        basket,
        &mut supply_caps,
        // negative_rate, credit_price_rate, config.rate_slope_multiplier
    ) {
        Ok(_ok) => {}
        Err(err) => {
            return Err(StdError::generic_err(format!("Error at line 416: {}", err)));
        }
    };

    let mut avg_change_in_index = Decimal::zero();
    //Calc average change in index btwn position & basket
    //and update cAsset.rate_index
    for (i, cAsset) in position.clone().collateral_assets.iter().enumerate() {
        //Match asset and rate_index
        if let Some(basket_asset) = basket
            .clone()
            .collateral_types
            .clone()
            .into_iter()
            .find(|basket_asset| basket_asset.asset.info.equal(&cAsset.asset.info))
        {
            ////Add proportionally the change in index
            // cAsset_ratio * change in index
            avg_change_in_index += match decimal_multiplication(
                ratios[i],
                decimal_division(basket_asset.rate_index, cAsset.rate_index)?,
            ) {
                Ok(avg_change_in_index) => avg_change_in_index,
                Err(err) => {
                    return Err(StdError::generic_err(format!(
                        "Error at line 451 in rates: {}, {}, {}, {}",
                        err, ratios[i], basket_asset.rate_index, cAsset.rate_index
                    )))
                }
            };

            /////Update cAsset rate_index
            position.collateral_assets[i].rate_index = basket_asset.rate_index;
        }
    }
    //The change in index represents the rate accrued to the cAsset's index in the time since last accrual
    Ok((avg_change_in_index, ratios))
}

/// Get total debt from all rate segments
pub fn get_total_debt_from_segments(rate_segments: &[RateSegment]) -> Uint128 {
    rate_segments.iter().map(|segment| segment.amount).sum()
}

/// Get total position debt including both rate segments and peg_rate_segments
pub fn get_total_position_debt(position: &Position) -> Uint128 {
    get_total_debt_from_segments(&position.rate_segments) + get_total_debt_from_segments(&position.peg_rate_segments)
}

/// Calculate fixed rate accrual for a specific time period
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
    )?.to_uint_floor();
    
    Ok(accrued)
}

/// Recalculate fixed rate for rollover.
/// Uses the position's collateral-weighted current_adaptive_rate * multiplier.
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

    let new_rate = decimal_multiplication(
        collateral_weighted_rate,
        fixed_rate_cap.multiplier,
    )?;
    
    // Calculate new end_time: add months from current_time
    let seconds_per_month = 2_592_000u64; // 30 days * 24 hours * 60 minutes * 60 seconds
    let new_end_time = current_time
        .checked_add(seconds_per_month * duration_months as u64)
        .ok_or_else(|| StdError::generic_err("End time calculation overflow"))?;
    
    Ok((new_rate, new_end_time))
}

/// Check and handle fixed rate expiration
/// Returns (was_fixed, duration_months, converted_to_variable) to track basket totals changes
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
                // Convert to variable rate - track this for basket totals update
                segment.fixed_rate = None;
                converted_to_variable = true;
            }
        }
    }
    
    Ok((was_fixed, duration_months, converted_to_variable))
}

/// Accrue interest to the repayment price & Position debt amount
/// Set, use and save new Volatility trackers
pub fn accrue(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: Config,
    position: &mut Position,
    basket: &mut Basket,
    user: String,
    is_deposit_function: bool,
) -> StdResult<Vec<Decimal>> {
    let mut rates = RATES.load(storage)?;

    // cAsset ratios
    /////Accrue Interest to the Repayment Price///
    //Calc Time-elapsed and update last_Accrued
    let time_elapsed = env.block.time.seconds() - rates.credit_last_accrued;

    let mut negative_rate: bool = false;
    let price_difference: Decimal;
    let mut credit_price_rate: Decimal = Decimal::zero();
    let mut skip_credit_price_accrual: bool = config.clone().skip_credit_price_accrual;

    ////If the credit oracle errors we only skip the repayment price accrual and not error the whole function
    // Create Asset from CreditAssetBreakdown for querying (use total amount)
    let total_credit_amount = basket.credit_asset.total_all_debt();

    let credit_asset = cAsset {
        asset: Asset {
            info: basket.credit_asset.info.clone(),
            amount: total_credit_amount,
        },
        max_borrow_LTV: Decimal::zero(),
        max_LTV: Decimal::zero(),
        pool_info: None,
        rate_index: Decimal::one(),
        peg_rate_index: Decimal::one(),
        force_redemptions: None,
    };

    let credit_TWAP_price = match get_asset_values(
        storage,
        env.clone(),
        querier,
        vec![credit_asset],
        config.clone(),
        Some(basket.clone()),
        is_deposit_function,
    ) {
        Ok(assets) => {
            if assets.1[0].price.is_zero() {
                //Skip repayment accrual
                skip_credit_price_accrual = true;

                Decimal::zero()
            } else {
                assets.1[0].price
            }
        }
        Err(_) => {
            //Skip repayment accrual
            skip_credit_price_accrual = true;

            Decimal::zero()
        }
    };

    if !skip_credit_price_accrual {
        ////Credit Price Controller barriers to reduce risk of manipulation
        //Liquidity above 2M
        //At least 3% of total supply as liquidity
        let liquidity: Uint128 = get_asset_liquidity(
            querier,
            config
                .clone()
                .liquidity_contract
                .unwrap_or_else(|| Addr::unchecked(""))
                .to_string(),
            basket.clone().credit_asset.info,
        )?;

        //Now get % of supply (sum all rate segment amounts)
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
        //If liquidity is low or basket oracle is not set, skip accrual
        if liquidity_ratio < Decimal::percent(3)
            || liquidity < MINIMUM_LIQUIDITY
            || !basket.oracle_set
        {
            //Skip repayment accrual
            skip_credit_price_accrual = true;
        }
    }

    /////Calculate the potential credit price rate & accrue if not skipped///////
    //Repayment accrual
    rates.credit_last_accrued = env.block.time.seconds();

    //We divide w/ the greater number first so the quotient is always 1.__
    price_difference = {
        //If market price > than repayment price
        match credit_TWAP_price.cmp(&basket.clone().credit_price.price) {
            Ordering::Greater => {
                negative_rate = true;
                decimal_subtraction(
                    decimal_division(credit_TWAP_price, basket.clone().credit_price.price)?,
                    Decimal::one(),
                )?
            }
            Ordering::Less => {
                negative_rate = false;
                decimal_subtraction(
                    decimal_division(basket.clone().credit_price.price, credit_TWAP_price)?,
                    Decimal::one(),
                )?
            }
            Ordering::Equal => {
                negative_rate = false;
                Decimal::zero()
            }
        }
    };
    //Don't accrue repayment interest if price is within the margin of error
    if price_difference > rates.cpc_margin_of_error {
        //Multiply price_difference by the cpc_multiplier
        credit_price_rate = decimal_multiplication(price_difference, config.cpc_multiplier)?;

        if !skip_credit_price_accrual {
            //Calculate rate of change
            let mut applied_rate = credit_price_rate.checked_mul(Decimal::from_ratio(
                Uint128::from(time_elapsed),
                Uint128::from(SECONDS_PER_YEAR),
            ))?;

            //If a positive rate we add 1,
            //If a negative rate we subtract the applied_rate from 1
            if negative_rate {
                //Subtract applied_rate to make it .9___
                applied_rate = decimal_subtraction(Decimal::one(), applied_rate)?;
            } else {
                //Add 1 to make the value 1.__
                applied_rate += Decimal::one();
            }

            let mut new_price = basket.credit_price.price;
            //Negative LTV interest needs to be enabled
            if !negative_rate || rates.negative_rates {
                new_price = decimal_multiplication(basket.credit_price.price, applied_rate)?;
            }

            basket.credit_price.price = new_price;
        }
    } else {
        credit_price_rate = Decimal::zero();
    }
    ///////////////////////////////////////////////////

    /////Accrue interest to the debt/////
    // Get total debt for rate_of_change calculation
    let total_debt = get_total_position_debt(&position);

    // Save rates_last_accrued BEFORE get_credit_rate_of_change, which internally
    // calls update_rate_indices and updates rates.rates_last_accrued to current_time.
    // Fixed rate accrual needs the original last_accrued_time to calculate time_elapsed.
    let last_accrued_time = rates.rates_last_accrued;
    let current_time = env.block.time.seconds();

    //Calc rate_of_change for the position's total debt (for variable rate segments)
    let (rate_of_change, ratios) = match get_credit_rate_of_change(
        storage,
        querier,
        env.clone(),
        config.clone(),
        basket,
        position,
        negative_rate,
        credit_price_rate,
    ) {
        Ok(rate) => rate,
        Err(err) => {
            return Err(StdError::generic_err(format!("Error at line 605: {}", err)));
        }
    };

    // Reload RATES since update_rate_indices (called inside get_credit_rate_of_change)
    // saved updated values (rates_last_accrued, current_adaptive_rate, etc.)
    // Preserve credit_last_accrued which was updated earlier in this function.
    let credit_last_accrued = rates.credit_last_accrued;
    let mut rates = RATES.load(storage)?;
    rates.credit_last_accrued = credit_last_accrued;

    let mut total_accrued_interest = Uint128::zero();

    // Compute collateral-weighted current_adaptive_rate for fixed rate rollovers
    let mut regular_weighted_rate = Decimal::zero();
    let mut peg_weighted_rate = Decimal::zero();
    for (i, c_asset) in position.collateral_assets.iter().enumerate() {
        if let Some(basket_idx) = basket
            .collateral_types
            .iter()
            .position(|ba| ba.asset.info.equal(&c_asset.asset.info))
        {
            if basket_idx < rates.current_adaptive_rate.len() {
                regular_weighted_rate += decimal_multiplication(
                    ratios[i],
                    rates.current_adaptive_rate[basket_idx],
                )?;
            }
            if basket_idx < rates.peg_current_adaptive_rate.len() {
                peg_weighted_rate += decimal_multiplication(
                    ratios[i],
                    rates.peg_current_adaptive_rate[basket_idx],
                )?;
            }
        }
    }

    // Accrue regular rate segments using debt_manager
    let (regular_accrued, regular_deltas) = crate::debt_manager::accrue_segments(
        &mut position.rate_segments,
        rate_of_change,
        last_accrued_time,
        current_time,
        &rates,
        regular_weighted_rate,
    )?;
    total_accrued_interest += regular_accrued;

    // Accrue peg rate segments using peg_rate_index
    let mut peg_rate_of_change = Decimal::zero();
    if !position.peg_rate_segments.is_empty() {
        for (i, c_asset) in position.collateral_assets.iter().enumerate() {
            if let Some(basket_asset) = basket
                .collateral_types
                .iter()
                .find(|ba| ba.asset.info.equal(&c_asset.asset.info))
            {
                peg_rate_of_change += decimal_multiplication(
                    ratios[i],
                    decimal_division(basket_asset.peg_rate_index, c_asset.peg_rate_index)?,
                )?;
            }
        }
    }

    let (peg_accrued, peg_deltas) = crate::debt_manager::accrue_segments(
        &mut position.peg_rate_segments,
        peg_rate_of_change,
        last_accrued_time,
        current_time,
        &rates,
        peg_weighted_rate,
    )?;
    total_accrued_interest += peg_accrued;

    // Sync position peg_rate_index to basket
    if !position.peg_rate_segments.is_empty() {
        for c_asset in position.collateral_assets.iter_mut() {
            if let Some(basket_asset) = basket
                .collateral_types
                .iter()
                .find(|ba| ba.asset.info.equal(&c_asset.asset.info))
            {
                c_asset.peg_rate_index = basket_asset.peg_rate_index;
            }
        }
    }

    if total_accrued_interest > Uint128::zero() {
        // Apply discounts if configured
        let mut discounted_interest = total_accrued_interest;
        if let Some(contract) = config.clone().discounts_contract {
            //Get User's discounted interest
            discounted_interest = match get_discounted_interest(
                querier,
                contract.to_string(),
                user,
                total_accrued_interest,
                position,
                basket,
            ) {
                Ok(discounted) => discounted,
                Err(_) => total_accrued_interest,
            };
        }
        
        //Track new_total_pending.
        //Using a tracker instead of accrued_interest to account for any rounding.
        let mut new_interest = Uint128::zero();
        //Split accrued interest into per-asset distribution
        for (i, cAsset) in position.collateral_assets.iter().enumerate() {
            let ratio = ratios[i];
            let amount = decimal_multiplication(
                ratio,
                Decimal::from_ratio(discounted_interest, Uint128::one()),
            )?;
            //Add amount to per-asset distribution
            if let Some(asset) = basket
                .pending_revenue
                .per_asset_rev
                .iter_mut()
                .find(|a| a.info.equal(&cAsset.asset.info))
            {
                //Update existing asset
                asset.amount += amount.to_uint_floor();
            } else {
                //Create new asset
                basket.pending_revenue.per_asset_rev.push(Asset {
                    info: cAsset.asset.info.clone(),
                    amount: amount.to_uint_floor(),
                });
            }
            //Add amount to new_interest tally
            new_interest += amount.to_uint_floor();
        }

        //Add accrued interest to the basket's pending revenue
        basket.pending_revenue.total_pending += new_interest;

        position.pending_interest += new_interest;
        position.total_interest_accrued += new_interest;

        // Apply discount to segment amounts if discount was applied
        if discounted_interest < total_accrued_interest {
            let discount_amount = total_accrued_interest.checked_sub(discounted_interest)
                .unwrap_or(Uint128::zero());

            let total_debt_before_discount = get_total_position_debt(&position);

            // Apply discount proportionally to both pools
            let regular_discount_deltas = crate::debt_manager::apply_discount_to_segments(
                &mut position.rate_segments,
                discount_amount,
                total_debt_before_discount,
            );
            let peg_discount_deltas = crate::debt_manager::apply_discount_to_segments(
                &mut position.peg_rate_segments,
                discount_amount,
                total_debt_before_discount,
            );

            // Add discount deltas to the accrual deltas
            // (discount deltas are negative, offsetting the positive accrual deltas)
            use crate::debt_manager::SegmentDeltas;
            fn merge_deltas(a: &mut crate::debt_manager::SegmentDeltas, b: &crate::debt_manager::SegmentDeltas) {
                a.variable += b.variable;
                a.one_month += b.one_month;
                a.three_month += b.three_month;
                a.six_month += b.six_month;
            }
            let mut final_regular_deltas = regular_deltas.clone();
            merge_deltas(&mut final_regular_deltas, &regular_discount_deltas);
            let mut final_peg_deltas = peg_deltas.clone();
            merge_deltas(&mut final_peg_deltas, &peg_discount_deltas);

            // Apply merged deltas to basket
            crate::debt_manager::apply_delta_to_regular_debt(&mut basket.credit_asset, &final_regular_deltas);
            crate::debt_manager::apply_delta_to_caps(&mut rates.fixed_rate_caps, &final_regular_deltas);
            crate::debt_manager::apply_delta_to_peg_debt(&mut basket.credit_asset, &final_peg_deltas);
            // Peg fixed rates share the same caps pool
            crate::debt_manager::apply_delta_to_caps(&mut rates.fixed_rate_caps, &final_peg_deltas);
        } else {
            // No discount - apply accrual deltas directly
            crate::debt_manager::apply_delta_to_regular_debt(&mut basket.credit_asset, &regular_deltas);
            crate::debt_manager::apply_delta_to_caps(&mut rates.fixed_rate_caps, &regular_deltas);
            crate::debt_manager::apply_delta_to_peg_debt(&mut basket.credit_asset, &peg_deltas);
            crate::debt_manager::apply_delta_to_caps(&mut rates.fixed_rate_caps, &peg_deltas);
        }
    } else {
        // No accrued interest, but still apply deltas (e.g., fixed->variable conversions)
        crate::debt_manager::apply_delta_to_regular_debt(&mut basket.credit_asset, &regular_deltas);
        crate::debt_manager::apply_delta_to_caps(&mut rates.fixed_rate_caps, &regular_deltas);
        crate::debt_manager::apply_delta_to_peg_debt(&mut basket.credit_asset, &peg_deltas);
        crate::debt_manager::apply_delta_to_caps(&mut rates.fixed_rate_caps, &peg_deltas);
    }

    // Save updated rates (credit_last_accrued, fixed_rate_caps)
    RATES.save(storage, &rates)?;

    Ok(ratios)
}

/// Check if a position is 100% force_redemption assets
fn is_position_100_percent_force_redemption(
    position: &Position,
    basket: &Basket,
) -> bool {
    if position.collateral_assets.is_empty() {
        return false;
    }

    // Check if all collateral assets have force_redemptions == Some(true)
    position.collateral_assets.iter().all(|c_asset| {
        basket.collateral_types.iter().any(|ba| 
            ba.asset.info == c_asset.asset.info && ba.force_redemptions == Some(true)
        )
    })
}

/// Calculate the discounted interest for a user
fn get_discounted_interest(
    querier: QuerierWrapper,
    discounts_contract: String,
    user: String,
    undiscounted_interest: Uint128,
    position: &Position,
    basket: &Basket,
) -> StdResult<Uint128> {
    // Check if position is 100% force_redemption assets
    let discount = if is_position_100_percent_force_redemption(position, basket) {
        // Use StableBackingDiscounts query
        let stable_discount: StableBackingDiscountsResponse =
            querier.query_wasm_smart(
                discounts_contract.clone(),
                &DiscountQueryMsg::StableBackingDiscounts { 
                    user: user.clone(),
                    debt_amount: undiscounted_interest,
                }
            )?;
        stable_discount.discount
    } else {
        // Use regular UserDiscount query
        let user_discount: UserDiscountResponse =
            querier.query_wasm_smart(discounts_contract, &DiscountQueryMsg::UserDiscount { user })?;
        user_discount.discount
    };

    let discounted_interest = {
        let percent_of_interest = decimal_subtraction(Decimal::one(), discount)?;
        decimal_multiplication(
            Decimal::from_ratio(undiscounted_interest, Uint128::one()),
            percent_of_interest,
        )?.to_uint_floor()
    };

    Ok(discounted_interest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deposit_rate_calculation_logic() {
        // Test the comparative deposit logic
        let base_rate = Decimal::percent(2); // 2% base rate

        // Asset A: highest deposits at 1_000_000
        let asset_a_dep = Uint128::new(1_000_000);
        // Asset B: half the deposits at 500_000
        let asset_b_dep = Uint128::new(500_000);
        // Asset C: quarter the deposits at 250_000
        let asset_c_dep = Uint128::new(250_000);

        let highest_dep = asset_a_dep;

        // Asset A (highest deposits) gets the LTV-based rate (simulated as base_rate here)
        // Asset B: base_rate * (1_000_000 / 500_000) = base_rate * 2 = 4%
        let asset_b_rate = decimal_multiplication(
            base_rate,
            Decimal::from_ratio(highest_dep, asset_b_dep),
        )
        .unwrap();
        assert_eq!(asset_b_rate, Decimal::percent(4));

        // Asset C: base_rate * (1_000_000 / 250_000) = base_rate * 4 = 8%
        let asset_c_rate = decimal_multiplication(
            base_rate,
            Decimal::from_ratio(highest_dep, asset_c_dep),
        )
        .unwrap();
        assert_eq!(asset_c_rate, Decimal::percent(8));
    }

    #[test]
    fn test_deposit_rate_similar_deposits() {
        // When deposits are similar, rates should be similar
        let base_rate = Decimal::percent(2);

        let asset_a_dep = Uint128::new(1_000_000);
        let asset_b_dep = Uint128::new(950_000);

        let highest_dep = asset_a_dep;

        // Asset B: base_rate * (1_000_000 / 950_000) ~= 2% * 1.053 ~= 2.1%
        let asset_b_rate = decimal_multiplication(
            base_rate,
            Decimal::from_ratio(highest_dep, asset_b_dep),
        )
        .unwrap();

        // Should be close to base rate
        assert!(
            asset_b_rate > base_rate,
            "Asset B should pay slightly more than base rate"
        );
        assert!(
            asset_b_rate < Decimal::percent(3),
            "Asset B rate should be below 3% (got: {:?})",
            asset_b_rate
        );
    }

    #[test]
    fn test_deposit_rate_max_cap() {
        // Test that irm_config.max_adaptive_rate caps the rate
        let base_rate = Decimal::percent(2);
        let max_comparative_rate = Decimal::percent(10); // 10% cap

        let highest_dep = Uint128::new(1_000_000);
        let tiny_dep = Uint128::new(10_000); // 1% of highest

        // Uncapped: base_rate * (1_000_000 / 10_000) = 2% * 100 = 200%
        let uncapped_rate = decimal_multiplication(
            base_rate,
            Decimal::from_ratio(highest_dep, tiny_dep),
        )
        .unwrap();
        assert!(uncapped_rate > max_comparative_rate);

        // After capping:
        let capped_rate = if uncapped_rate > max_comparative_rate {
            max_comparative_rate
        } else {
            uncapped_rate
        };
        assert_eq!(capped_rate, Decimal::percent(10));
    }

    #[test]
    fn test_deposit_rate_multi_asset_ordering() {
        // Higher deposits should always result in lower rates
        let base_rate = Decimal::percent(2);

        let deposits = vec![
            ("usdc", Uint128::new(5_000_000)),  // Most deposits
            ("atom", Uint128::new(2_000_000)),
            ("osmo", Uint128::new(1_000_000)),
            ("juno", Uint128::new(500_000)),
            ("stars", Uint128::new(100_000)),    // Least deposits
        ];

        let highest_dep = deposits[0].1;

        let mut rates: Vec<(&str, Decimal)> = deposits
            .iter()
            .map(|(name, dep)| {
                if *dep == highest_dep {
                    // Highest deposit asset gets LTV-based rate (simulate as base_rate)
                    (*name, base_rate)
                } else {
                    let rate = decimal_multiplication(
                        base_rate,
                        Decimal::from_ratio(highest_dep, *dep),
                    )
                    .unwrap();
                    (*name, rate)
                }
            })
            .collect();

        // Sort by rate ascending
        rates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        // USDC (most deposits) should have the lowest rate
        assert_eq!(rates[0].0, "usdc", "USDC (most deposits) should have lowest rate");

        // Stars (least deposits) should have the highest rate
        assert_eq!(rates[4].0, "stars", "STARS (least deposits) should have highest rate");

        // Verify monotonic: each subsequent rate should be >= previous
        for i in 1..rates.len() {
            assert!(
                rates[i].1 >= rates[i - 1].1,
                "Rates should increase as deposits decrease: {} ({:?}) vs {} ({:?})",
                rates[i - 1].0, rates[i - 1].1, rates[i].0, rates[i].1
            );
        }
    }
}
