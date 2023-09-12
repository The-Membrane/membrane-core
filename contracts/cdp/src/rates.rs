use std::cmp::Ordering;

use cosmwasm_std::{Uint128, Decimal, Storage, QuerierWrapper, Env, StdResult, StdError, DepsMut, MessageInfo, Response, attr, Addr};

use membrane::system_discounts::QueryMsg as DiscountQueryMsg;
use membrane::types::{Basket, cAsset, Position, Rate};
use membrane::helpers::get_asset_liquidity;
use membrane::math::{decimal_multiplication, decimal_division, decimal_subtraction};

use crate::ContractError;
use crate::query::{get_asset_values, get_cAsset_ratios};
use crate::risk_engine::{get_basket_debt_caps, update_basket_debt};
use crate::state::{CONFIG, BASKET, get_target_position, update_position};

//Constants
pub const SECONDS_PER_YEAR: u64 = 31_536_000u64;
const MINIMUM_LIQUIDITY: Uint128 = Uint128::new(2_000_000_000_000u128);

/// Accrue interest for a list of Positions
pub fn external_accrue_call(
    deps: DepsMut, 
    info: MessageInfo,
    env: Env,
    position_owner: Option<String>,
    position_ids: Vec<Uint128>,
) -> Result<Response, ContractError>{
    let mut basket = BASKET.load(deps.storage)?;

    //Validate position owner
    let valid_position_owner: Addr;
    if let Some(position_owner) = position_owner {
        //Sent addr
        valid_position_owner = deps.api.addr_validate(&position_owner)?  
    } else { 
        //Msg sender
        valid_position_owner = info.clone().sender 
    }

    //Initialize accrued_interest
    let mut accrued_interest: Uint128 = Uint128::zero();

    //Accrue interest for each position
    for position_id in position_ids.clone() {
        let mut position = get_target_position(
            deps.storage,
            valid_position_owner.clone(),
            position_id,
        )?.1;
        
        let prev_loan = position.clone().credit_amount;
        
        accrue(
            deps.storage, 
            deps.querier, 
            env.clone(), 
            &mut position,
            &mut basket, 
            info.sender.to_string(),
            false,
            false,
        )?;

        accrued_interest += position.clone().credit_amount - prev_loan;

        update_position(deps.storage, valid_position_owner.clone(), position)?;
    }

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "accrue"),
            attr("position_ids", format!("{:?}", position_ids)),
            attr("accrued_interest", accrued_interest),
        ]))
}

pub fn accumulate_interest_dec(decimal: Decimal, rate: Decimal, time_elapsed: u64) -> StdResult<Decimal> {
    let applied_rate = rate.checked_mul(Decimal::from_ratio(
        Uint128::from(time_elapsed),
        Uint128::from(SECONDS_PER_YEAR),
    ))?;

    decimal_multiplication(decimal, applied_rate)
}

// Calculate Basket interests and then accumulate interest to all basket cAsset rate indices
pub fn update_rate_indices(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env, 
    basket: &mut Basket,
    negative_rate: bool,
    credit_price_rate: Decimal,
    for_query: bool,
) -> StdResult<()>{
    //Get basket rates
    let mut interest_rates = match get_interest_rates(storage, querier, env.clone(), basket, for_query){
        Ok(rates) => rates,
        Err(err) => {
            return Err(StdError::GenericErr {
                msg: format!("Error at line 98, error: {:?}", err)
            })
        }
    };
    
    let mut error: Option<StdError> = None;

    //Add/Subtract the repayment rate to the rates
    //These aren't saved so it won't compound
    interest_rates = interest_rates.clone().into_iter().map(|mut rate| {

        if negative_rate {
            //If the collateral interest rate is less than the redemption rate, set to 0. 
            //Avoids negative interest rates but not redemption rates.
            if rate < credit_price_rate {
                rate = Decimal::zero();
            } else {
                rate = match decimal_subtraction(rate, credit_price_rate){
                    Ok(rate) => rate,
                    Err(err) => {
                        error = Some(err);
                        Decimal::zero()
                    },
                };
            }            
            
        } else {
            rate += credit_price_rate;
        }

        rate
    })
    .collect::<Vec<Decimal>>();
    //This allows us to prioritize credit stability over profit/state of the basket
    //This means base_interest_rate + margin_of_error is the range above peg before rates go to 0
    
    // Assert that there are no errors
    if let Some(err) = error {
        return Err(err);
    }

    //Calc time_elapsed
    let time_elapsed = env.block.time.seconds() - basket.clone().rates_last_accrued;

    //Accumulate rate on each rate_index
    for (i, basket_asset) in basket.clone().collateral_types.into_iter().enumerate(){     
        let accrued_rate = accumulate_interest_dec(
            basket_asset.rate_index,
            interest_rates[i],
            time_elapsed,
        )?;

        basket.collateral_types[i].rate_index += accrued_rate;        
    }

    //Update rates_last_accrued
    basket.rates_last_accrued = env.block.time.seconds();
    
    Ok(())
}

/// Calculate interest rates for each asset in the basket
pub fn get_interest_rates(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    basket: &mut Basket,
    for_query: bool,
) -> StdResult<Vec<Decimal>> {
    //If for a query, take Basket saved latest rates
    if for_query {
        let rates = basket.clone().lastest_collateral_rates
            .into_iter().map(|rate| rate.rate)
            .collect::<Vec<Decimal>>();
        return Ok(rates);
    }

    let config = CONFIG.load(storage)?;

    let mut rates = vec![];

    for asset in basket.clone().collateral_types {
        //Base_Rate * max collateral_ratio
        //ex: 2% * 110% = 2.2%
        //Higher rates for riskier assets

        //base * (1/max_LTV)
        rates.push(decimal_multiplication(
            basket.clone().base_interest_rate,
            decimal_division(Decimal::one(), asset.max_LTV)?,
        )?);        
    }

    //Get proportion of debt && supply caps filled
    let mut debt_proportions = vec![];
    let mut supply_proportions = vec![];

    let debt_caps = match get_basket_debt_caps(storage, querier, env.clone(), basket) {
        Ok(caps) => caps,
        Err(err) => {
            return Err(StdError::GenericErr {
                msg: format!("Error at line 190, error: {:?}", err)
            })            
        }
    };
   
    //Get basket cAsset ratios
    let (basket_ratios, _) =
        get_cAsset_ratios(storage, env.clone(), querier, basket.clone().collateral_types, config.clone())?;
    

    for (i, cap) in basket.clone().collateral_supply_caps.iter().enumerate() {
        //Caps set to 0 can be used to push out unwanted assets by spiking rates
        if cap.supply_cap_ratio.is_zero() {
            debt_proportions.push(Decimal::percent(100));
            supply_proportions.push(Decimal::percent(100));
        } else if debt_caps[i].is_zero(){
            debt_proportions.push(Decimal::zero());
            supply_proportions.push(Decimal::zero());
        } else {
            //Push the debt_ratio and supply_ratio
            debt_proportions.push(Decimal::from_ratio(cap.debt_total, debt_caps[i]));
            supply_proportions.push(decimal_division(basket_ratios[i], cap.supply_cap_ratio)?)
        }
    }

    //Gets pro-rata rate and uses multiplier if above desired utilization
    let mut two_slope_pro_rata_rates = vec![];
    for (i, _rate) in rates.iter().enumerate() {
        //If proportions are above desired utilization, the rates start multiplying
        //For every % above the desired, it adds a multiple
        //Ex: Desired = 90%, proportion = 91%, interest = 2%. New rate = 4%.
        //Acts as two_slope rate
        
        //The debt_proportion is used unless the supply proportion is over 1 or the farthest into slope 2 
        //A proportion in Slope 2 is prioritized        
        if supply_proportions[i] <= Decimal::one() || ((supply_proportions[i] > Decimal::one() && debt_proportions[i] > Decimal::one()) && supply_proportions[i] < debt_proportions[i]) {
            //Slope 2
            if debt_proportions[i] > Decimal::one(){
                //Ex: 91% > 90%
                ////0.01 * 100 = 1
                //1% = 1
                let percent_over_desired = decimal_multiplication(
                    decimal_subtraction(debt_proportions[i], Decimal::one())?,
                    Decimal::percent(100_00),
                )?;
                let multiplier = percent_over_desired + Decimal::one();
                //Change rate of (rate) increase w/ the configuration multiplier
                let multiplier = multiplier * config.rate_slope_multiplier;

                //Ex cont: Multiplier = 2; Pro_rata rate = 1.8%.
                //// rate = 3.6%
                two_slope_pro_rata_rates.push(
                    decimal_multiplication(
                        decimal_multiplication(rates[i], debt_proportions[i])?,
                        multiplier,
                    )?,
                );
            } else {
                //Slope 1
                two_slope_pro_rata_rates.push(
                    decimal_multiplication(rates[i], debt_proportions[i])?,
                );
            }
        } else if supply_proportions[i] > Decimal::one() {
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
            two_slope_pro_rata_rates.push(
                decimal_multiplication(
                    decimal_multiplication(rates[i], supply_proportions[i])?,
                    multiplier,
                )?,
            );            
        }
    }

    //Calculate supply cap overages 
    if basket.multi_asset_supply_caps != vec![]{
        for multi_asset_cap in basket.clone().multi_asset_supply_caps {
            //Initialize total_ratio
            let mut total_ratio = Decimal::zero();

            //Find & add ratio for each asset
            for asset in multi_asset_cap.clone().assets{
                if let Some((i, _cap)) = basket.clone().collateral_supply_caps.into_iter().enumerate().find(|(_i, cap)| cap.asset_info.equal(&asset)){
                    total_ratio += basket_ratios[i];
                }
            }

            //Calc interest rate
            let multi_cap_proportion = decimal_division(total_ratio, multi_asset_cap.supply_cap_ratio)?;

            for asset in multi_asset_cap.clone().assets{
                if let Some((i, _cap)) = basket.clone().collateral_supply_caps.into_iter().enumerate().find(|(_i, cap)| cap.asset_info.equal(&asset)){
                    //Substitute if proportion of multi_asset_cap is greater than 1 and both debt/supply proportions
                    if multi_cap_proportion > Decimal::one() && multi_cap_proportion > supply_proportions[i] && multi_cap_proportion > debt_proportions[i]{
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
                        two_slope_pro_rata_rates[i] = decimal_multiplication(
                                decimal_multiplication(rates[i], multi_cap_proportion)?,
                                multiplier,
                            )?;                        
                    }
                }
            }
        }
    }
    //Update latest rates in the Basket
    let latest_rates = two_slope_pro_rata_rates.clone()
        .into_iter()
        .map(|rate| Rate {
            rate,
            last_time_updated: env.clone().block.time.seconds(),
        }).collect::<Vec<Rate>>();
    basket.lastest_collateral_rates = latest_rates;

        
    Ok(two_slope_pro_rata_rates)
}

/// Calculates the % change to accrue to the Position's debt
fn get_credit_rate_of_change(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    basket: &mut Basket,
    position: &mut Position,
    negative_rate: bool,
    credit_price_rate: Decimal,
    for_query: bool,
) -> StdResult<Decimal> {
    let config = CONFIG.load(storage)?;
    let (ratios, _) = match get_cAsset_ratios(storage, env.clone(), querier, position.clone().collateral_assets, config){
        Ok(ratios) => ratios,
        Err(err) => {
            return Err(StdError::GenericErr {
                msg: format!("Error at line 332, error: {:?}", err)
            })
        }
    };

    match update_rate_indices(storage, querier, env, basket, negative_rate, credit_price_rate, for_query){
        Ok(_ok) => {},
        Err(err) => {
            return Err(StdError::GenericErr {
                msg: format!("Error at line 341, error: {:?}", err)
            })
        }
    };

    let mut avg_change_in_index = Decimal::zero();
    //Calc average change in index btwn position & basket 
    //and update cAsset.rate_index
    for (i, cAsset) in position.clone().collateral_assets.iter().enumerate() {
        //Match asset and rate_index
        if let Some(basket_asset) = basket.clone().collateral_types
            .clone()
            .into_iter()
            .find(|basket_asset| basket_asset.asset.info.equal(&cAsset.asset.info)){
            ////Add proportionally the change in index
            // cAsset_ratio * change in index          
            avg_change_in_index += decimal_multiplication(ratios[i], decimal_division(basket_asset.rate_index, cAsset.rate_index)?)?;
            
            /////Update cAsset rate_index
            position.collateral_assets[i].rate_index = basket_asset.rate_index;        
        }
    }    
    //The change in index represents the rate accrued to the cAsset's index in the time since last accrual
    Ok(avg_change_in_index)
}

/// Accrue interest to the repayment price & Position debt amount
pub fn accrue(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    position: &mut Position,
    basket: &mut Basket,
    user: String,
    is_deposit_function: bool,
    for_query: bool, //necessary to reduce gas costs
) -> StdResult<()> {
    let config = CONFIG.load(storage)?;

    /////Accrue Interest to the Repayment Price///
    //Calc Time-elapsed and update last_Accrued
    let time_elapsed = env.block.time.seconds() - basket.credit_last_accrued;

    let mut negative_rate: bool = false;
    let price_difference: Decimal;
    let mut credit_price_rate: Decimal = Decimal::zero();

    ////Credit Price Controller barriers to reduce risk of manipulation
    //Liquidity above 2M
    //At least 3% of total supply as liquidity
    let liquidity: Uint128 = get_asset_liquidity(
        querier,
        config.clone().liquidity_contract.unwrap().to_string(),
        basket.clone().credit_asset.info
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
    let mut skip_accrual: bool = false;
    if liquidity_ratio < Decimal::percent(3) || liquidity < MINIMUM_LIQUIDITY || !basket.oracle_set{
        //Skip repayment accrual
        skip_accrual = true;
    }
    
    ////If the credit oracle errors we only skip the repayment price accrual and not error the whole function
    //Calculate new interest rate
    let credit_asset = cAsset {
        asset: basket.clone().credit_asset,
        max_borrow_LTV: Decimal::zero(),
        max_LTV: Decimal::zero(),
        pool_info: None,
        rate_index: Decimal::one(),
    };

    let credit_TWAP_price = match get_asset_values(
        storage,
        env.clone(),
        querier,
        vec![credit_asset],
        config.clone(),
        is_deposit_function,
    ){
        Ok(assets) => assets.1[0].price,
        Err(_) => {
            //Skip repayment accrual
            skip_accrual = true;
            
            Decimal::zero()
        }
    };

    if !skip_accrual {
        basket.credit_last_accrued = env.block.time.seconds();

        //We divide w/ the greater number first so the quotient is always 1.__
        price_difference = {
            //If market price > than repayment price
            match credit_TWAP_price.cmp(&basket.clone().credit_price){
                Ordering::Greater => {
                    negative_rate = true;
                    decimal_subtraction(
                        decimal_division(credit_TWAP_price, basket.clone().credit_price)?,
                        Decimal::one(),
                    )?
                },
                Ordering::Less => {
                    negative_rate = false;
                    decimal_subtraction(
                        decimal_division(basket.clone().credit_price, credit_TWAP_price)?,
                        Decimal::one(),
                    )?
                },
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

            let mut new_price = basket.credit_price;
            //Negative repayment interest needs to be enabled by the basket
            if !negative_rate || basket.negative_rates {
                new_price = decimal_multiplication(basket.credit_price, applied_rate)?;
            } 

            basket.credit_price = new_price;
        } else {
            credit_price_rate = Decimal::zero();
        }
    }

    /////Accrue interest to the debt/////      
    //Calc rate_of_change for the position's credit amount
    let rate_of_change = match get_credit_rate_of_change(
        storage,
        querier,
        env.clone(),
        basket,
        position,
        negative_rate,
        credit_price_rate,
        for_query,
    ){
        Ok(rate) => rate,
        Err(err) => {
            return Err(StdError::GenericErr {
                msg: format!("Error at line 498, liquidity: {}, error: {:?}", liquidity, err)
            })
        }
    };
    
    //Calc new_credit_amount
    let new_credit_amount = decimal_multiplication(
        Decimal::from_ratio(position.credit_amount, Uint128::new(1)), 
        rate_of_change
    )? * Uint128::new(1u128);
        
    if new_credit_amount > position.credit_amount {
        // return Err(StdError::GenericErr {
        //     msg: format!("Error at line 511, liquidity: {}, new_Credit: {}, pos_credit: {}, bool: {}", liquidity, new_credit_amount, position.credit_amount, new_credit_amount > position.credit_amount)
        // });
        //Calc accrued interest
        let mut accrued_interest = new_credit_amount - position.credit_amount;

        if let Some(contract) = config.clone().discounts_contract {
            //Get User's discounted interest
            accrued_interest = match get_discounted_interest(querier, contract.to_string(), user, accrued_interest){
                Ok(discounted_interest) => discounted_interest,
                Err(_) => accrued_interest,
            };
        }

        //Add accrued interest to the basket's pending revenue
        basket.pending_revenue += accrued_interest;

        //Set position's debt to the debt + accrued_interest
        position.credit_amount += accrued_interest;

        //Add accrued interest to the basket's debt cap
        match update_basket_debt(
            storage,
            env,
            querier,
            config,
            basket,
            position.clone().collateral_assets,
            accrued_interest,
            true,
        ){
            Ok(_ok) => {}
            Err(err) => {
                return Err(StdError::GenericErr {
                    msg: format!("Error at line 540, liquidity: {}, error: {:?}", liquidity, err),
                })
            }
        };
    }    

    Ok(())
}

/// Calculate the discounted interest for a user
fn get_discounted_interest(
    querier: QuerierWrapper,
    discounts_contract: String,
    user: String,
    undiscounted_interest: Uint128,
) -> StdResult<Uint128>{
    //Get discount
    let discount: Decimal = querier.query_wasm_smart(discounts_contract, &DiscountQueryMsg::UserDiscount { user })?;

    let discounted_interest = {
        let percent_of_interest = decimal_subtraction(Decimal::one(), discount)?;
        decimal_multiplication(Decimal::from_ratio(undiscounted_interest, Uint128::one()), percent_of_interest)?
    } * Uint128::one();
    
    Ok(discounted_interest)
}