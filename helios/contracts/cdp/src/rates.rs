use cosmwasm_std::{Uint128, Decimal, Storage, QuerierWrapper, Env, StdResult, StdError, QueryRequest, WasmQuery, to_binary};
use membrane::osmosis_proxy::{QueryMsg as OsmoQueryMsg, TokenInfoResponse};
use membrane::types::{Basket, cAsset, Asset, SupplyCap, Position, };
use membrane::math::{decimal_multiplication, decimal_division, decimal_subtraction};

use crate::positions::{get_basket_debt_caps, get_LP_pool_cAssets, get_cAsset_ratios, get_asset_liquidity, get_asset_values, update_basket_debt};
use crate::state::CONFIG;

//Constants
pub const SECONDS_PER_YEAR: u64 = 31_536_000u64;


pub fn accumulate_interest(debt: Uint128, rate: Decimal, time_elapsed: u64) -> StdResult<Uint128> {

    let applied_rate = rate.checked_mul(Decimal::from_ratio(
        Uint128::from(time_elapsed),
        Uint128::from(SECONDS_PER_YEAR),
    ))?;

    let accrued_interest = debt * applied_rate;

    Ok(accrued_interest)
}

pub fn accumulate_interest_dec(decimal: Decimal, rate: Decimal, time_elapsed: u64) -> StdResult<Decimal> {

    let applied_rate = rate.checked_mul(Decimal::from_ratio(
        Uint128::from(time_elapsed),
        Uint128::from(SECONDS_PER_YEAR),
    ))?;

    let accrued_interest = decimal_multiplication(decimal, applied_rate);

    Ok(accrued_interest)
}

//Get Basket interests and then accumulate interest to all basket cAsset rate indices
pub fn update_rate_indices(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env, 
    basket: &mut Basket,
    negative_rate: bool,
    credit_price_rate: Decimal,
) -> StdResult<()>{

    //Get basket rates
    let mut interest_rates = get_interest_rates(storage, querier, env.clone(), basket)?;

    //Add/Subtract the repayment rate to the rates
    //These aren't saved so it won't compound
    interest_rates = interest_rates.clone().into_iter().map(|mut rate| {

        if negative_rate {
            if rate < credit_price_rate {
                rate = Decimal::zero();
            } else {
                rate = decimal_subtraction(rate, credit_price_rate);
            }            
            
        } else {
            rate += credit_price_rate;
        }

        rate
    })
    .collect::<Vec<Decimal>>();
    //This allows us to prioritize credit stability over profit/state of the basket
    //This means base rate is the range above (peg + margin of error) before rates go to 0
    

    //Calc time_elapsed
    let time_elapsed = env.block.time.seconds() - basket.clone().rates_last_accrued;

    //Accumulate rate on each rate_index
    for (i, basket_asset) in basket.clone().collateral_types.into_iter().enumerate(){


        if basket_asset.pool_info.is_none() {

        let accrued_rate = accumulate_interest_dec(
            basket_asset.rate_index,
            interest_rates[i],
            time_elapsed.clone(),
        )?;

        basket.collateral_types[i].rate_index += accrued_rate;
        }
    }

    //update rates_last_accrued
    basket.rates_last_accrued = env.block.time.seconds();

    Ok(())
}

pub fn get_interest_rates(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    basket: &mut Basket,
) -> StdResult<Vec<Decimal>> {
    let config = CONFIG.load(storage)?;

    let mut rates = vec![];

    for asset in basket.clone().collateral_types {
        //We don't get individual rates for LPs
        if asset.pool_info.is_none() {
            //Base_Rate * max collateral_ratio
            //ex: 2% * 110% = 2.2%
            //Higher rates for riskier assets

            //base * (1/max_LTV)
            rates.push(decimal_multiplication(
                basket.clone().base_interest_rate,
                decimal_division(Decimal::one(), asset.max_LTV),
            ));
        }
    }

    //Get proportion of debt && supply caps filled
    let mut debt_proportions = vec![];
    let mut supply_proportions = vec![];

    let debt_caps = match get_basket_debt_caps(storage, querier, env.clone(), basket) {
        Ok(caps) => caps,
        Err(err) => {
            return Err(StdError::GenericErr {
                msg: err.to_string(),
            })
        }
    };

    //To include LP assets (but not share tokens) in the ratio calculation
    let caps_to_cAssets = basket
        .collateral_supply_caps
        .clone()
        .into_iter()
        .map(|cap| cAsset {
            asset: Asset {
                amount: cap.current_supply,
                info: cap.asset_info,
            },
            max_borrow_LTV: Decimal::zero(),
            max_LTV: Decimal::zero(),
            pool_info: None,
            rate_index: Decimal::one(),
        })
        .collect::<Vec<cAsset>>();

    let no_lp_basket: Vec<cAsset> =
        get_LP_pool_cAssets(querier, config.clone(), basket.clone(), caps_to_cAssets)?;

    //Get basket cAsset ratios
    let (basket_ratios, _) =
        get_cAsset_ratios(storage, env.clone(), querier, no_lp_basket, config.clone())?;

    let no_lp_caps = basket
        .collateral_supply_caps
        .clone()
        .into_iter()
        .filter(|cap| !cap.lp)
        .collect::<Vec<SupplyCap>>();

    for (i, cap) in no_lp_caps.clone().iter().enumerate() {
        //If there is 0 of an Asset then it's cap is 0 but its proportion is 100%
        if debt_caps[i].is_zero() || cap.supply_cap_ratio.is_zero() {
            debt_proportions.push(Decimal::percent(100));
            supply_proportions.push(Decimal::percent(100));
        } else {
            //Push the debt_ratio and supply_ratio
            debt_proportions.push(Decimal::from_ratio(cap.debt_total, debt_caps[i]));
            supply_proportions.push(decimal_division(basket_ratios[i], cap.supply_cap_ratio))
        }
    }

    //Gets pro-rata rate and uses multiplier if above desired utilization
    let mut two_slope_pro_rata_rates = vec![];
    for (i, _rate) in rates.iter().enumerate() {
        //If proportions are above desired utilization, the rates start multiplying
        //For every % above the desired, it adds a multiple
        //Ex: Desired = 90%, proportion = 91%, interest = 2%. New rate = 4%.
        //Acts as two_slope rate

        //The highest proportion is chosen between debt_cap and supply_cap of the asset
        if debt_proportions[i] > supply_proportions[i] {
            //Slope 2
            if debt_proportions[i] > basket.desired_debt_cap_util {
                //Ex: 91% > 90%
                ////0.01 * 100 = 1
                //1% = 1
                let percent_over_desired = decimal_multiplication(
                    decimal_subtraction(debt_proportions[i], basket.desired_debt_cap_util),
                    Decimal::percent(100_00),
                );
                let multiplier = percent_over_desired + Decimal::one();
                //Change rate of (rate) increase w/ the configuration multiplier
                let multiplier = multiplier * config.rate_slope_multiplier;

                //Ex cont: Multiplier = 2; Pro_rata rate = 1.8%.
                //// rate = 3.6%
                two_slope_pro_rata_rates.push(
                    decimal_multiplication(
                        decimal_multiplication(rates[i], debt_proportions[i]),
                        multiplier,
                    ),
                );
            } else {
                //Slope 1
                two_slope_pro_rata_rates.push(
                    decimal_multiplication(rates[i], debt_proportions[i]),
                );
            }
        } else {
            //Slope 2
            if supply_proportions[i] > Decimal::one() {
                //Ex: 91% > 90%
                ////0.01 * 100 = 1
                //1% = 1
                let percent_over_desired = decimal_multiplication(
                    decimal_subtraction(supply_proportions[i], Decimal::one()),
                    Decimal::percent(100_00),
                );
                let multiplier = percent_over_desired + Decimal::one();
                //Change rate of (rate) increase w/ the configuration multiplier
                let multiplier = multiplier * config.rate_slope_multiplier;

                //Ex cont: Multiplier = 2; Pro_rata rate = 1.8%.
                //// rate = 3.6%
                two_slope_pro_rata_rates.push(
                    decimal_multiplication(
                        decimal_multiplication(rates[i], supply_proportions[i]),
                        multiplier,
                    ),
                );
            } else {
                //Slope 1
                two_slope_pro_rata_rates.push(
                    decimal_multiplication(rates[i], supply_proportions[i]),
                );
            }
        }
    }

    Ok(two_slope_pro_rata_rates)
}


fn get_credit_rate_of_change(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    basket: &mut Basket,
    position: &mut Position,
    negative_rate: bool,
    credit_price_rate: Decimal,
) -> StdResult<Decimal> {

    let config = CONFIG.load(storage)?;

    let (ratios, _) = get_cAsset_ratios(storage, env.clone(), querier, position.clone().collateral_assets, config)?;

    update_rate_indices(storage, querier, env, basket, negative_rate, credit_price_rate)?;


    let mut avg_change_in_index = Decimal::zero();

    //Calc average change in index btwn position & basket 
    //and update cAsset.rate_index
    for (i, cAsset) in position.clone().collateral_assets.iter().enumerate() {

        //Match asset and rate_index
        if let Some(basket_asset) = basket.clone().collateral_types
            .clone()
            .into_iter()
            .find(|basket_asset| basket_asset.asset.info.equal(&cAsset.asset.info))
        {

            //If an LP, calc the new average index first
            if cAsset.clone().pool_info.is_some(){

                let pool_info = cAsset.clone().pool_info.unwrap();
                
                let mut avg_index = Decimal::zero();

                //Get avg_index
                for pool_asset in pool_info.asset_infos{

                    ///Find in collateral_types
                    if let Some(basket_pool_asset) = basket.clone().collateral_types
                        .clone()
                        .into_iter()
                        .find(|basket_pool_asset| basket_pool_asset.asset.info.equal(&pool_asset.info)){
                            
                            //Add proportion to avg_index
                            avg_index += decimal_multiplication(pool_asset.ratio, basket_pool_asset.rate_index);
                            
                        }
                    
                }

                //Set LP share rate_index
                position.collateral_assets[i].rate_index = avg_index.clone();
                

                ////Add proportionally the change in index
                // cAsset_ratio * change in index          
                avg_change_in_index += decimal_multiplication(ratios[i], decimal_division(avg_index, cAsset.rate_index));
            } else {

                ////Add proportionally the change in index
                // cAsset_ratio * change in index          
                avg_change_in_index += decimal_multiplication(ratios[i], decimal_division(basket_asset.rate_index, cAsset.rate_index));
                
                /////Update cAsset rate_index
                position.collateral_assets[i].rate_index = basket_asset.rate_index;
            }
        }
    }

    //The change in index represents the rate accrued to the cAsset in the time since last accrual
    Ok(avg_change_in_index)
}

pub fn accrue(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    position: &mut Position,
    basket: &mut Basket,
) -> StdResult<()> {
    let config = CONFIG.load(storage)?;

    //Accrue Interest to the Repayment Price
    //--
    //Calc Time-elapsed and update last_Accrued
    let mut time_elapsed = env.block.time.seconds() - basket.credit_last_accrued;

    let mut negative_rate: bool = false;
    let mut price_difference: Decimal = Decimal::zero();
    let mut credit_price_rate: Decimal = Decimal::zero();

    ////Controller barriers to reduce risk of manipulation
    //Liquidity above 2M
    //At least 3% of total supply as liquidity
    let liquidity = get_asset_liquidity(querier, config.clone(), basket.clone().credit_asset.info)?;
    
    //Now get % of supply
    let current_supply = basket.credit_asset.amount;

    let liquidity_ratio = { 
        if !current_supply.is_zero() {
            decimal_division(
                Decimal::from_ratio(liquidity, Uint128::new(1u128)),
                Decimal::from_ratio(current_supply, Uint128::new(1u128)),
            )
        } else {
            Decimal::one()        
        }
    };
    if liquidity_ratio < Decimal::percent(3) {
        //Set time_elapsed to 0 to skip repayment accrual
        time_elapsed = 0u64;
    }
    
    if liquidity < Uint128::new(2_000_000_000_000u128) {
        //Set time_elapsed to 0 to skip repayment accrual
        time_elapsed = 0u64;
    }

    if !(time_elapsed == 0u64) && basket.oracle_set {
        basket.credit_last_accrued = env.block.time.seconds();

        //Calculate new interest rate

        let credit_asset = cAsset {
            asset: basket.clone().credit_asset,
            max_borrow_LTV: Decimal::zero(),
            max_LTV: Decimal::zero(),
            pool_info: None,
            rate_index: Decimal::one(),
        };

        let credit_TWAP_price = get_asset_values(
            storage,
            env.clone(),
            querier,
            vec![credit_asset],
            config.clone(),
            Some(basket.clone().basket_id),
        )?
        .1[0];

        //We divide w/ the greater number first so the quotient is always 1.__
        price_difference = {
            //If market price > than repayment price
            if credit_TWAP_price > basket.clone().credit_price {
                negative_rate = true;
                decimal_subtraction(
                    decimal_division(credit_TWAP_price, basket.clone().credit_price),
                    Decimal::one(),
                )
            } else if basket.clone().credit_price > credit_TWAP_price {
                negative_rate = false;
                decimal_subtraction(
                    decimal_division(basket.clone().credit_price, credit_TWAP_price),
                    Decimal::one(),
                )
            } else {
                negative_rate = false;
                Decimal::zero()
            }
        };
       

        //Don't accrue repayment interest if price is within the margin of error
        if price_difference > basket.clone().cpc_margin_of_error {

            //Multiply price_difference by the cpc_multiplier
            credit_price_rate = decimal_multiplication(price_difference, config.clone().cpc_multiplier);

            //Calculate rate of change
            let mut applied_rate: Decimal;
            applied_rate = credit_price_rate.checked_mul(Decimal::from_ratio(
                Uint128::from(time_elapsed),
                Uint128::from(SECONDS_PER_YEAR),
            ))?;

            //If a positive rate we add 1,
            //If a negative rate we subtract the applied_rate from 1
            //---
            if negative_rate {
                //Subtract applied_rate to make it .9___
                applied_rate = decimal_subtraction(Decimal::one(), applied_rate);
            } else {
                //Add 1 to make the value 1.__
                applied_rate += Decimal::one();
            }

            let mut new_price = basket.credit_price;
            //Negative repayment interest needs to be enabled by the basket
            if negative_rate && basket.negative_rates || !negative_rate {
                new_price = decimal_multiplication(basket.credit_price, applied_rate);
            } 

            basket.credit_price = new_price;
        } else {
            credit_price_rate = Decimal::zero();
        }
    }

    /////Accrue interest to the debt/////  
    
    //Calc rate_of_change for the position's credit amount
    let rate_of_change = get_credit_rate_of_change(
        storage,
        querier,
        env.clone(),
        basket,
        position,
        negative_rate,
        credit_price_rate,
    )?;
    

    //Calc new_credit_amount
    let new_credit_amount = decimal_multiplication(
        Decimal::from_ratio(position.credit_amount, Uint128::new(1)), 
        rate_of_change
    ) * Uint128::new(1u128);
    

    if new_credit_amount > position.credit_amount {
        //Calc accrued interest
        let accrued_interest = (new_credit_amount * Uint128::new(1u128)) - position.credit_amount;

        //Add accrued interest to the basket's pending revenue
        //Okay with rounding down here since the position's credit will round down as well
        basket.pending_revenue += accrued_interest;

        //Set position's debt to the debt + accrued_interest
        position.credit_amount = new_credit_amount;

        //Add accrued interest to the basket's debt cap
        match update_basket_debt(
            storage,
            env.clone(),
            querier,
            config.clone(),
            basket,
            position.clone().collateral_assets,
            accrued_interest,
            true,
            true,
        ) {
            Ok(_ok) => {}
            Err(err) => {
                return Err(StdError::GenericErr {
                    msg: err.to_string(),
                })
            }
        };
    }
    

    Ok(())
}
