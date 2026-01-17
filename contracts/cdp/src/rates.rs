use std::cmp::{max, min, Ordering};
use std::str::FromStr;

use cosmwasm_std::{
    attr, Addr, Api, Decimal, DepsMut, Env, MessageInfo, Order, QuerierWrapper, Response, StdError,
    StdResult, Storage, Uint128,
};

use membrane::cdp::Config;
use membrane::helpers::get_asset_liquidity;
use membrane::math::{decimal_division, decimal_multiplication, decimal_subtraction};
use membrane::system_discounts::{QueryMsg as DiscountQueryMsg, UserDiscountResponse};
use membrane::types::{cAsset, Asset, Basket, IndividualCost, Position, Rate, SupplyCap};

use crate::query::{
    get_asset_values, get_cAsset_ratios, query_ltv_disco_for_asset_ltvs, VOLATILITY_LIST_LIMIT,
};
use crate::state::{get_target_position, update_position, BASKET, CONFIG, VOLATILITY};
use crate::ContractError;

//Constants
pub const SECONDS_PER_YEAR: u64 = 31_536_000u64;
const MINIMUM_LIQUIDITY: Uint128 = Uint128::new(2_000_000_000_000u128);

/// Get average raw volatility (price change %) for an asset.
/// Returns None if no volatility data exists (triggering fallback to LTV-based rate).
fn get_avg_volatility(storage: &dyn Storage, asset_info: &str) -> Option<Decimal> {
    VOLATILITY
        .load(storage, asset_info.to_string())
        .ok()
        .and_then(|vol_store| {
            // Use raw_volatility_list (pure price change %) for rate comparison
            if vol_store.raw_volatility_list.is_empty() {
                return None; // No data, use fallback
            }
            let sum: Decimal = vol_store.raw_volatility_list.iter().cloned().sum();
            decimal_division(
                sum,
                Decimal::from_str(&vol_store.raw_volatility_list.len().to_string()).unwrap(),
            )
            .ok()
        })
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

        let prev_loan = position.clone().credit_amount;

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

        accrued_interest += position.clone().credit_amount - prev_loan;

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

// Calculate Basket interests and then accumulate interest to all basket cAsset rate indices
pub fn update_rate_indices(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    basket: &mut Basket,
    supply_caps: &mut Vec<SupplyCap>,
    // negative_rate: bool,
    // credit_price_rate: Decimal,
    // rate_slope_multiplier: Decimal,
) -> StdResult<()> {
    //Get basket rates
    let interest_rates =
        match get_interest_rates(storage, querier, env.clone(), basket, supply_caps) {
            Ok(rates) => rates,
            Err(err) => {
                return Err(StdError::GenericErr {
                    msg: format!("Error at line 109: {}", err),
                })
            }
        };

    // let mut error: Option<StdError> = None;

    //Add/Subtract the repayment rate to the rates
    //These aren't saved so it won't compound
    // NOTE: REMOVED BC IT PUSHES LOW RISK USERS OUT DUE TO HIGH RATES CREATED BY HIGH RISK USERS
    // interest_rates = interest_rates.clone().into_iter().map(|mut rate| {

    //     if negative_rate {
    //         //If the collateral interest rate is less than the redemption rate, set to 0.
    //         //Avoids negative interest rates but not redemption rates.
    //         if rate < credit_price_rate {
    //             rate = Decimal::zero();
    //         } else {
    //             rate = match decimal_subtraction(rate, credit_price_rate){
    //                 Ok(rate) => rate,
    //                 Err(err) => {
    //                     error = Some(err);
    //                     Decimal::zero()
    //                 },
    //             };
    //         }
    //     } else {
    //         rate += decimal_multiplication(credit_price_rate, rate_slope_multiplier)?;
    //     }

    //     Ok(rate)
    // })
    // .collect::<StdResult<Vec<Decimal>>>()?;
    //This allows us to prioritize credit stability over profit/state of the basket
    //This means base_interest_rate + margin_of_error is the range above peg before rates go to 0

    // Assert that there are no errors
    // if let Some(err) = error {
    //     return Err(err);
    // }

    //Update latest rates in the Basket
    let latest_rates = interest_rates
        .clone()
        .into_iter()
        .map(|rate| Rate {
            rate,
            last_time_updated: env.clone().block.time.seconds(),
        })
        .collect::<Vec<Rate>>();
    basket.lastest_collateral_rates = latest_rates;

    //Calc time_elapsed
    let time_elapsed = env.block.time.seconds() - basket.clone().rates_last_accrued;

    //Accumulate rate on each rate_index
    for (i, basket_asset) in basket.clone().collateral_types.into_iter().enumerate() {
        let accrued_rate =
            accumulate_interest_dec(basket_asset.rate_index, interest_rates[i], time_elapsed)?;

        basket.collateral_types[i].rate_index += accrued_rate;
    }

    //Update rates_last_accrued
    basket.rates_last_accrued = env.block.time.seconds();

    Ok(())
}

/// Calculate interest rates for each asset in the basket
/// Maximum rate is 100% to avoid overflows due to supply cap/pricing errors
///
/// Goal: Comparative volatility-based rates
/// - Lowest volatility asset gets: base_interest_rate * (1 / max_LTV)
/// - Other assets get: base_interest_rate * (asset_avg_vol / lowest_avg_vol)
/// - Fallback (no volatility data): base_interest_rate * (1 / max_LTV)
/// - Assets with individual_cost set: use their rate directly
pub fn get_interest_rates(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    basket: &mut Basket,
    supply_caps: &mut Vec<SupplyCap>,
) -> StdResult<Vec<Decimal>> {
    let config = CONFIG.load(storage)?;

    // Query ltv_disco for LTVs
    let ltv_tuples = query_ltv_disco_for_asset_ltvs(
        querier,
        config.ltv_disco.clone(),
        basket.clone().collateral_types.clone(),
    )?;

    // First pass: collect average volatility for each asset
    let avg_volatilities: Vec<Option<Decimal>> = basket
        .collateral_types
        .iter()
        .map(|asset| get_avg_volatility(storage, &asset.asset.info.to_string()))
        .collect();

    // Find the lowest volatility among assets that have volatility data
    let lowest_vol: Option<Decimal> = avg_volatilities
        .iter()
        .filter_map(|v| *v)
        .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let mut rates = vec![];

    for (i, asset) in basket.clone().collateral_types.iter().enumerate() {
        if asset.individual_cost.is_some() {
            // Use individual cost rate if set (unchanged behavior)
            rates.push(asset.individual_cost.clone().unwrap().rate);
        } else {
            // Calculate LTV-based fallback rate: base * (1/max_LTV)
            let ltv_based_rate = decimal_multiplication(
                basket.clone().base_interest_rate,
                decimal_division(Decimal::one(), ltv_tuples[i].0)?,
            )?;

            match (avg_volatilities[i], lowest_vol) {
                // Both asset has volatility data and we have a lowest vol reference
                (Some(asset_vol), Some(low_vol)) => {
                    if asset_vol == low_vol {
                        // Lowest volatility asset: use LTV-based rate
                        rates.push(ltv_based_rate);
                    } else {
                        // Other assets: base_rate * (asset_vol / lowest_vol)
                        let vol_multiplier = decimal_division(asset_vol, low_vol)?;
                        rates.push(decimal_multiplication(
                            basket.clone().base_interest_rate,
                            vol_multiplier,
                        )?);
                    }
                }
                // No volatility data for this asset or no lowest vol reference: use fallback
                _ => {
                    rates.push(ltv_based_rate);
                }
            }
        }
    }

    //Get proportion of supply caps filled
    let mut supply_proportions = vec![];

    //Get basket cAsset ratios
    let (basket_ratios, _) = get_cAsset_ratios(
        storage,
        env.clone(),
        querier,
        basket.clone().collateral_types,
        config.clone(),
        Some(basket.clone()),
    )?;

    for (i, cap) in supply_caps.iter().enumerate() {
        //Caps set to 0 can be used to push out unwanted assets by spiking rates
        if cap.supply_cap_ratio.is_zero() {
            supply_proportions.push(Decimal::percent(100));
        } else {
            //Push the supply_ratio. Minimum is 100% to guarantee rates >= base.
            supply_proportions.push(max(
                decimal_division(basket_ratios[i], cap.supply_cap_ratio)?,
                Decimal::percent(100),
            ))
        }
    }

    //Gets pro-rata rate and uses multiplier if above desired utilization
    let mut two_slope_pro_rata_rates = vec![];
    for (i, _rate) in rates.iter().enumerate() {
        //If proportions are above desired utilization, the rates start multiplying
        //For every % above the desired, it adds a multiple
        //Ex: Desired = 90%, proportion = 91%, interest = 2%. New rate = 4%.
        //Acts as two_slope rate

        //Check if supply_proportions[i] is greater than 100% (i.e. in Slope 2)
        if supply_proportions[i] > Decimal::one() {
            //Slope 2
            //Ex: 91% > 90%
            ////0.01 * 100 = 1
            //1% = 1
            let percent_over_desired = decimal_multiplication(
                decimal_subtraction(supply_proportions[i], Decimal::one())?,
                Decimal::percent(100_00),
            )?;
            let multiplier = percent_over_desired + Decimal::one();
            //Change rate of (rate) increase w/ the configuration multiplier
            let multiplier = multiplier * config.rate_slope_multiplier;

            //Ex cont: Multiplier = 2; Pro_rata rate = 1.8%.
            //// rate = 3.6%
            two_slope_pro_rata_rates.push(min(
                decimal_multiplication(
                    decimal_multiplication(rates[i], supply_proportions[i])?,
                    multiplier,
                )?,
                Decimal::one(),
            ));
        } else {
            //Base Rate
            two_slope_pro_rata_rates.push(rates[i]);
        }
    }

    //Calculate multi-supply cap overages
    if basket.multi_asset_supply_caps != vec![] {
        for multi_asset_cap in basket.clone().multi_asset_supply_caps {
            //Initialize total_ratio
            let mut total_ratio = Decimal::zero();

            //Find & add ratio for each asset
            for asset in multi_asset_cap.clone().assets {
                if let Some((i, _cap)) = basket
                    .clone()
                    .collateral_supply_caps
                    .into_iter()
                    .enumerate()
                    .find(|(_i, cap)| cap.asset_info.equal(&asset))
                {
                    total_ratio += basket_ratios[i];
                }
            }

            //Calc interest rate
            let multi_cap_proportion =
                decimal_division(total_ratio, multi_asset_cap.supply_cap_ratio)?;

            for asset in multi_asset_cap.clone().assets {
                if let Some((i, _cap)) = basket
                    .clone()
                    .collateral_supply_caps
                    .clone()
                    .into_iter()
                    .enumerate()
                    .find(|(_i, cap)| cap.asset_info.equal(&asset))
                {
                    //Substitute if proportion of multi_asset_cap is greater than 1 and both debt/supply proportions
                    if multi_cap_proportion > Decimal::one()
                        && multi_cap_proportion > supply_proportions[i]
                    {
                        //Slope 2
                        //Ex: 91% > 90%
                        ////0.01 * 100 = 1
                        //1% = 1
                        let percent_over_desired = decimal_multiplication(
                            decimal_subtraction(multi_cap_proportion, Decimal::one())?,
                            Decimal::percent(100_00),
                        )?;
                        let multiplier = percent_over_desired + Decimal::one();
                        //Change rate of (rate) increase w/ the configuration multiplier
                        let multiplier = multiplier * config.rate_slope_multiplier;

                        //Ex cont: Multiplier = 2; Pro_rata rate = 1.8%.
                        //// rate = 3.6%
                        two_slope_pro_rata_rates[i] = min(
                            decimal_multiplication(
                                decimal_multiplication(rates[i], multi_cap_proportion)?,
                                multiplier,
                            )?,
                            Decimal::one(),
                        );
                    }
                }
            }
        }
    }

    Ok(two_slope_pro_rata_rates)
}

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
    negative_rate: bool,
    credit_price_rate: Decimal,
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
            return Err(StdError::GenericErr {
                msg: format!("Error at line 400: {}", err),
            })
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
            return Err(StdError::GenericErr {
                msg: format!("Error at line 416: {}", err),
            })
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
                    return Err(StdError::GenericErr {
                        msg: format!(
                            "Error at line 451 in rates: {}, {}, {}, {}",
                            err, ratios[i], basket_asset.rate_index, cAsset.rate_index
                        ),
                    })
                }
            };

            /////Update cAsset rate_index
            position.collateral_assets[i].rate_index = basket_asset.rate_index;
        }
    }
    //The change in index represents the rate accrued to the cAsset's index in the time since last accrual
    Ok((avg_change_in_index, ratios))
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
    // cAsset ratios
    /////Accrue Interest to the Repayment Price///
    //Calc Time-elapsed and update last_Accrued
    let time_elapsed = env.block.time.seconds() - basket.credit_last_accrued;

    let mut negative_rate: bool = false;
    let price_difference: Decimal;
    let mut credit_price_rate: Decimal = Decimal::zero();
    let mut skip_credit_price_accrual: bool = config.clone().skip_credit_price_accrual;

    ////If the credit oracle errors we only skip the repayment price accrual and not error the whole function
    let credit_asset = cAsset {
        asset: basket.clone().credit_asset,
        max_borrow_LTV: Decimal::zero(),
        max_LTV: Decimal::zero(),
        pool_info: None,
        rate_index: Decimal::one(),
        individual_cost: Some(IndividualCost {
            rate: Decimal::zero(),
            updater_address: None,
        }),
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

        //Now get % of supply
        let current_supply = basket.credit_asset.amount;
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
    basket.credit_last_accrued = env.block.time.seconds();

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
    if price_difference > basket.clone().cpc_margin_of_error {
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
            //Negative LTV interest needs to be enabled by the basket
            if !negative_rate || basket.negative_rates {
                new_price = decimal_multiplication(basket.credit_price.price, applied_rate)?;
            }

            basket.credit_price.price = new_price;
        }
    } else {
        credit_price_rate = Decimal::zero();
    }
    ///////////////////////////////////////////////////

    /////Accrue interest to the debt/////
    //Calc rate_of_change for the position's credit amount
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
            return Err(StdError::GenericErr {
                msg: format!("Error at line 605: {}", err),
            })
        }
    };

    //Calc new_credit_amount
    let new_credit_amount = decimal_multiplication(
        Decimal::from_ratio(position.credit_amount, Uint128::new(1)),
        rate_of_change,
    )? * Uint128::new(1u128);

    if new_credit_amount > position.credit_amount {
        //Calc accrued interest
        let mut accrued_interest = new_credit_amount - position.credit_amount;

        if let Some(contract) = config.clone().discounts_contract {
            //Get User's discounted interest
            accrued_interest = match get_discounted_interest(
                querier,
                contract.to_string(),
                user,
                accrued_interest,
            ) {
                Ok(discounted_interest) => discounted_interest,
                Err(_) => accrued_interest,
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
                Decimal::from_ratio(accrued_interest, Uint128::one()),
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

        //Set position's debt to the debt + accrued_interest
        position.credit_amount += new_interest;
        position.pending_interest += new_interest;
        position.total_interest_accrued += new_interest;

        //Add accrued interest to the Basket's debt tally
        basket.credit_asset.amount += new_interest;
    }

    Ok(ratios)
}

/// Calculate the discounted interest for a user
fn get_discounted_interest(
    querier: QuerierWrapper,
    discounts_contract: String,
    user: String,
    undiscounted_interest: Uint128,
) -> StdResult<Uint128> {
    //Get discount
    let discount: UserDiscountResponse =
        querier.query_wasm_smart(discounts_contract, &DiscountQueryMsg::UserDiscount { user })?;

    let discounted_interest = {
        let percent_of_interest = decimal_subtraction(Decimal::one(), discount.discount)?;
        decimal_multiplication(
            Decimal::from_ratio(undiscounted_interest, Uint128::one()),
            percent_of_interest,
        )?
    } * Uint128::one();

    Ok(discounted_interest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::CollateralVolatility;
    use cosmwasm_std::testing::mock_dependencies;

    #[test]
    fn test_get_avg_volatility_empty_list() {
        let deps = mock_dependencies();

        // No volatility data stored - should return None
        let result = get_avg_volatility(&deps.storage, "test_asset");
        assert!(result.is_none());
    }

    #[test]
    fn test_get_avg_volatility_with_data() {
        let mut deps = mock_dependencies();

        // Store volatility data with raw_volatility_list (price change %)
        let vol_store = CollateralVolatility {
            index: Decimal::one(),
            volatility_list: vec![], // Speed of volatility (not used for rate calc)
            raw_volatility_list: vec![
                Decimal::percent(10), // 0.10 = 10% price change
                Decimal::percent(20), // 0.20 = 20% price change
                Decimal::percent(30), // 0.30 = 30% price change
            ],
        };
        VOLATILITY
            .save(&mut deps.storage, "test_asset".to_string(), &vol_store)
            .unwrap();

        // Should return average: (0.10 + 0.20 + 0.30) / 3 = 0.20
        let result = get_avg_volatility(&deps.storage, "test_asset");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), Decimal::percent(20));
    }

    #[test]
    fn test_volatility_rate_calculation_logic() {
        // Test the comparative volatility logic
        let base_rate = Decimal::percent(2); // 2% base rate

        // Asset A: lowest volatility at 5%
        let asset_a_vol = Decimal::percent(5);
        // Asset B: higher volatility at 10%
        let asset_b_vol = Decimal::percent(10);
        // Asset C: highest volatility at 15%
        let asset_c_vol = Decimal::percent(15);

        let lowest_vol = asset_a_vol;

        // Asset A (lowest vol) should get the LTV-based rate (which we'll simulate as base_rate for this test)
        // Asset B: base_rate * (10% / 5%) = base_rate * 2 = 4%
        let asset_b_rate = decimal_multiplication(
            base_rate,
            decimal_division(asset_b_vol, lowest_vol).unwrap(),
        )
        .unwrap();
        assert_eq!(asset_b_rate, Decimal::percent(4));

        // Asset C: base_rate * (15% / 5%) = base_rate * 3 = 6%
        let asset_c_rate = decimal_multiplication(
            base_rate,
            decimal_division(asset_c_vol, lowest_vol).unwrap(),
        )
        .unwrap();
        assert_eq!(asset_c_rate, Decimal::percent(6));
    }

    /// Test using real historic price data from CoinMarketCap (Jan 15-16, 2026)
    /// Calculates daily volatility as |close - open| / open for each day
    #[test]
    fn test_real_crypto_volatility_btc_eth_comparison() {
        let mut deps = mock_dependencies();

        // BTC daily volatilities (from real CoinMarketCap data):
        // Jan 16, 2026: Open $95,554.10 -> Close $95,525.12 = ~0.03% volatility
        // Jan 15, 2026: Open $96,931.29 -> Close $95,551.19 = ~1.42% volatility
        // We'll use permille (1/1000) for more precision with small percentages
        let btc_vol_jan16 = Decimal::permille(3); // 0.03% = 0.0003
        let btc_vol_jan15 = Decimal::from_ratio(142u128, 10000u128); // 1.42% = 0.0142

        let btc_vol = CollateralVolatility {
            index: Decimal::one(),
            volatility_list: vec![],
            raw_volatility_list: vec![btc_vol_jan16, btc_vol_jan15],
        };
        VOLATILITY
            .save(&mut deps.storage, "btc".to_string(), &btc_vol)
            .unwrap();

        // ETH daily volatilities:
        // Jan 16, 2026: Open $3,317.34 -> Close $3,295.48 = ~0.66% volatility
        // Jan 15, 2026: Open $3,354.77 -> Close $3,317.10 = ~1.12% volatility
        let eth_vol_jan16 = Decimal::from_ratio(66u128, 10000u128); // 0.66%
        let eth_vol_jan15 = Decimal::from_ratio(112u128, 10000u128); // 1.12%

        let eth_vol = CollateralVolatility {
            index: Decimal::one(),
            volatility_list: vec![],
            raw_volatility_list: vec![eth_vol_jan16, eth_vol_jan15],
        };
        VOLATILITY
            .save(&mut deps.storage, "eth".to_string(), &eth_vol)
            .unwrap();

        // Calculate average volatilities
        let btc_avg = get_avg_volatility(&deps.storage, "btc").unwrap();
        let eth_avg = get_avg_volatility(&deps.storage, "eth").unwrap();

        // BTC avg: (0.03% + 1.42%) / 2 = 0.725%
        // ETH avg: (0.66% + 1.12%) / 2 = 0.89%
        // BTC should be the lowest volatility asset
        assert!(
            btc_avg < eth_avg,
            "BTC should have lower avg volatility than ETH"
        );

        // Test rate calculation: BTC gets base rate, ETH gets scaled rate
        let base_rate = Decimal::percent(2); // 2% APR base rate

        // ETH rate = base_rate * (eth_avg / btc_avg)
        // ~= 2% * (0.89% / 0.725%) = 2% * 1.227 = ~2.45%
        let eth_rate =
            decimal_multiplication(base_rate, decimal_division(eth_avg, btc_avg).unwrap()).unwrap();

        // ETH should pay a higher rate than base rate
        assert!(
            eth_rate > base_rate,
            "ETH should pay higher rate than BTC (base)"
        );
        // But not dramatically higher (since both are relatively stable)
        assert!(
            eth_rate < Decimal::percent(3),
            "ETH rate should be below 3%"
        );
    }

    /// Test with highly volatile meme coins vs stable large caps
    /// Using real CoinMarketCap data for DOGE, PEPE vs BTC
    #[test]
    fn test_real_crypto_volatility_meme_vs_btc() {
        let mut deps = mock_dependencies();

        // BTC (most stable)
        // Jan 16: 0.03%, Jan 15: 1.42%
        let btc_vol = CollateralVolatility {
            index: Decimal::one(),
            volatility_list: vec![],
            raw_volatility_list: vec![
                Decimal::permille(3),                    // 0.03%
                Decimal::from_ratio(142u128, 10000u128), // 1.42%
            ],
        };
        VOLATILITY
            .save(&mut deps.storage, "btc".to_string(), &btc_vol)
            .unwrap();

        // DOGE (meme coin, more volatile)
        // Jan 16, 2026: Open $0.14 -> Close $0.1381 = ~1.36% volatility
        // Jan 15, 2026: Open $0.1472 -> Close $0.14 = ~4.89% volatility
        let doge_vol = CollateralVolatility {
            index: Decimal::one(),
            volatility_list: vec![],
            raw_volatility_list: vec![
                Decimal::from_ratio(136u128, 10000u128), // 1.36%
                Decimal::from_ratio(489u128, 10000u128), // 4.89%
            ],
        };
        VOLATILITY
            .save(&mut deps.storage, "doge".to_string(), &doge_vol)
            .unwrap();

        // PEPE (highly volatile meme coin)
        // Jan 16, 2026: Open $0.000005911 -> Close $0.000005922 = ~0.19% volatility
        // Jan 15, 2026: Open $0.000006251 -> Close $0.000005911 = ~5.44% volatility
        let pepe_vol = CollateralVolatility {
            index: Decimal::one(),
            volatility_list: vec![],
            raw_volatility_list: vec![
                Decimal::from_ratio(19u128, 10000u128),  // 0.19%
                Decimal::from_ratio(544u128, 10000u128), // 5.44%
            ],
        };
        VOLATILITY
            .save(&mut deps.storage, "pepe".to_string(), &pepe_vol)
            .unwrap();

        let btc_avg = get_avg_volatility(&deps.storage, "btc").unwrap();
        let doge_avg = get_avg_volatility(&deps.storage, "doge").unwrap();
        let pepe_avg = get_avg_volatility(&deps.storage, "pepe").unwrap();

        // BTC avg: ~0.725%, DOGE avg: ~3.125%, PEPE avg: ~2.815%
        assert!(btc_avg < doge_avg, "BTC should be less volatile than DOGE");
        assert!(btc_avg < pepe_avg, "BTC should be less volatile than PEPE");

        let base_rate = Decimal::percent(2); // 2% base rate

        // DOGE rate = 2% * (3.125% / 0.725%) = 2% * 4.31 = ~8.62%
        let doge_rate =
            decimal_multiplication(base_rate, decimal_division(doge_avg, btc_avg).unwrap())
                .unwrap();

        // PEPE rate = 2% * (2.815% / 0.725%) = 2% * 3.88 = ~7.76%
        let pepe_rate =
            decimal_multiplication(base_rate, decimal_division(pepe_avg, btc_avg).unwrap())
                .unwrap();

        // Meme coins should have significantly higher rates
        assert!(
            doge_rate > Decimal::percent(5),
            "DOGE should pay at least 5% (got: {:?})",
            doge_rate
        );
        assert!(
            pepe_rate > Decimal::percent(5),
            "PEPE should pay at least 5% (got: {:?})",
            pepe_rate
        );

        // DOGE was more volatile on average, so should have higher rate than PEPE
        assert!(
            doge_rate > pepe_rate,
            "DOGE should pay higher rate than PEPE"
        );
    }

    /// Test with SOL and ATOM data
    #[test]
    fn test_real_crypto_volatility_sol_atom() {
        let mut deps = mock_dependencies();

        // SOL (Solana)
        // Jan 16, 2026: Open $142.33 -> Close $144.86 = ~1.78% volatility
        // Jan 15, 2026: Open $146.76 -> Close $142.33 = ~3.02% volatility
        let sol_vol = CollateralVolatility {
            index: Decimal::one(),
            volatility_list: vec![],
            raw_volatility_list: vec![
                Decimal::from_ratio(178u128, 10000u128), // 1.78%
                Decimal::from_ratio(302u128, 10000u128), // 3.02%
            ],
        };
        VOLATILITY
            .save(&mut deps.storage, "sol".to_string(), &sol_vol)
            .unwrap();

        // ATOM (Cosmos)
        // Jan 16, 2026: Open $2.4757 -> Close $2.4922 = ~0.67% volatility
        // Jan 15, 2026: Open $2.5836 -> Close $2.4757 = ~4.18% volatility
        let atom_vol = CollateralVolatility {
            index: Decimal::one(),
            volatility_list: vec![],
            raw_volatility_list: vec![
                Decimal::from_ratio(67u128, 10000u128),  // 0.67%
                Decimal::from_ratio(418u128, 10000u128), // 4.18%
            ],
        };
        VOLATILITY
            .save(&mut deps.storage, "atom".to_string(), &atom_vol)
            .unwrap();

        let sol_avg = get_avg_volatility(&deps.storage, "sol").unwrap();
        let atom_avg = get_avg_volatility(&deps.storage, "atom").unwrap();

        // SOL avg: (1.78% + 3.02%) / 2 = 2.4%
        // ATOM avg: (0.67% + 4.18%) / 2 = 2.425%
        // These are very close in volatility!

        let base_rate = Decimal::percent(2); // 2% base rate
        let lowest_vol = std::cmp::min(sol_avg, atom_avg);

        let sol_rate =
            decimal_multiplication(base_rate, decimal_division(sol_avg, lowest_vol).unwrap())
                .unwrap();

        let atom_rate =
            decimal_multiplication(base_rate, decimal_division(atom_avg, lowest_vol).unwrap())
                .unwrap();

        // Both should be close to base rate since volatilities are similar
        let rate_diff = if sol_rate > atom_rate {
            sol_rate - atom_rate
        } else {
            atom_rate - sol_rate
        };

        // Rate difference should be small (less than 0.5%) since volatilities are similar
        assert!(
            rate_diff < Decimal::from_ratio(5u128, 1000u128),
            "SOL and ATOM rates should be similar (diff: {:?})",
            rate_diff
        );
    }

    /// Test the full multi-asset scenario with 6 crypto assets
    #[test]
    fn test_real_crypto_full_basket() {
        let mut deps = mock_dependencies();

        // Store volatility data for all 6 assets from CoinMarketCap
        let assets = vec![
            (
                "btc",
                vec![
                    Decimal::permille(3),
                    Decimal::from_ratio(142u128, 10000u128),
                ],
            ), // 0.03%, 1.42%
            (
                "eth",
                vec![
                    Decimal::from_ratio(66u128, 10000u128),
                    Decimal::from_ratio(112u128, 10000u128),
                ],
            ), // 0.66%, 1.12%
            (
                "doge",
                vec![
                    Decimal::from_ratio(136u128, 10000u128),
                    Decimal::from_ratio(489u128, 10000u128),
                ],
            ), // 1.36%, 4.89%
            (
                "pepe",
                vec![
                    Decimal::from_ratio(19u128, 10000u128),
                    Decimal::from_ratio(544u128, 10000u128),
                ],
            ), // 0.19%, 5.44%
            (
                "sol",
                vec![
                    Decimal::from_ratio(178u128, 10000u128),
                    Decimal::from_ratio(302u128, 10000u128),
                ],
            ), // 1.78%, 3.02%
            (
                "atom",
                vec![
                    Decimal::from_ratio(67u128, 10000u128),
                    Decimal::from_ratio(418u128, 10000u128),
                ],
            ), // 0.67%, 4.18%
        ];

        for (asset_name, vol_list) in &assets {
            let vol = CollateralVolatility {
                index: Decimal::one(),
                volatility_list: vec![],
                raw_volatility_list: vol_list.clone(),
            };
            VOLATILITY
                .save(&mut deps.storage, asset_name.to_string(), &vol)
                .unwrap();
        }

        // Collect all avg volatilities
        let mut avg_vols: Vec<(&str, Decimal)> = assets
            .iter()
            .map(|(name, _)| (*name, get_avg_volatility(&deps.storage, name).unwrap()))
            .collect();

        // Sort by volatility to find the lowest
        avg_vols.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        let lowest_asset = avg_vols[0].0;
        let lowest_vol = avg_vols[0].1;

        // BTC should be the lowest volatility asset (avg ~0.725%)
        assert_eq!(
            lowest_asset, "btc",
            "BTC should have the lowest avg volatility"
        );

        let base_rate = Decimal::percent(2);

        // Calculate rates for all assets
        let mut rates: Vec<(&str, Decimal)> = avg_vols
            .iter()
            .map(|(name, avg_vol)| {
                let rate = decimal_multiplication(
                    base_rate,
                    decimal_division(*avg_vol, lowest_vol).unwrap(),
                )
                .unwrap();
                (*name, rate)
            })
            .collect();

        // Sort by rate
        rates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        // BTC should have the lowest rate (base rate)
        assert_eq!(rates[0].0, "btc", "BTC should have the lowest rate");

        // Print rate structure for verification
        println!("Rate structure based on real CoinMarketCap data:");
        for (name, rate) in &rates {
            println!(
                "  {}: {:.2}%",
                name,
                rate.to_string().parse::<f64>().unwrap_or(0.0) * 100.0
            );
        }

        // Verify rate ordering makes sense
        // Higher volatility assets should pay higher rates
        for i in 1..rates.len() {
            let prev_asset_vol = avg_vols
                .iter()
                .find(|(n, _)| n == &rates[i - 1].0)
                .unwrap()
                .1;
            let curr_asset_vol = avg_vols.iter().find(|(n, _)| n == &rates[i].0).unwrap().1;

            if curr_asset_vol > prev_asset_vol {
                assert!(
                    rates[i].1 >= rates[i - 1].1,
                    "Higher volatility assets should have higher rates"
                );
            }
        }
    }
}
