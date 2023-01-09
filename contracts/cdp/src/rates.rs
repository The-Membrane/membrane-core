use cosmwasm_std::{Uint128, Decimal, Storage, QuerierWrapper, Env, StdResult, StdError, DepsMut, MessageInfo, Response, attr};

use membrane::system_discounts::QueryMsg as DiscountQueryMsg;
use membrane::types::{Basket, cAsset, SupplyCap, Position, AssetInfo};
use membrane::helpers::get_asset_liquidity;
use membrane::math::{decimal_multiplication, decimal_division, decimal_subtraction};

use crate::ContractError;
use crate::positions::{get_cAsset_ratios, get_asset_values, get_target_position, update_position};
use crate::query::{get_asset_values_imut, get_cAsset_ratios_imut};
use crate::risk_engine::{get_basket_debt_caps_imut};
use crate::risk_engine::{get_basket_debt_caps, update_basket_debt};
use crate::state::{CONFIG, BASKET};

//Constants
pub const SECONDS_PER_YEAR: u64 = 31_536_000u64;

pub fn external_accrue_call(
    deps: DepsMut, 
    info: MessageInfo,
    env: Env,
    position_owner: Option<String>,
    position_id: Uint128,
) -> Result<Response, ContractError>{
    let mut basket = BASKET.load(deps.storage)?;
    let config = CONFIG.load(deps.storage)?;

    //Validate position owner
    let valid_position_owner: Addr;
    if let Some(position_owner) = position_owner {
        //If the SP is the sender
        if info.clone().sender == config.clone().stability_pool.unwrap().to_string(){
            valid_position_owner = deps.api.addr_validate(&position_owner)?;
        //Defaults to sender
        } else { valid_position_owner = info.clone().sender }
    } else { valid_position_owner = info.clone().sender }

    let mut position = get_target_position(
        deps.storage,
        valid_position_owner,
        position_id.clone()
    )?.1;
    
    let prev_loan = position.clone().credit_amount;
    
    accrue(
        deps.storage, 
        deps.querier, 
        env, 
        &mut position,
        &mut basket, 
        info.clone().sender.to_string()
    )?;

    let accrued_interest = position.clone().credit_amount - prev_loan;

    update_position(deps.storage, info.clone().sender, position)?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "accrue"),
            attr("position_id", position_id),
            attr("accrued_interest", accrued_interest),
        ]))
}

pub fn accumulate_interest_dec(decimal: Decimal, rate: Decimal, time_elapsed: u64) -> StdResult<Decimal> {
    let applied_rate = rate.checked_mul(Decimal::from_ratio(
        Uint128::from(time_elapsed),
        Uint128::from(SECONDS_PER_YEAR),
    ))?;

    Ok(decimal_multiplication(decimal, applied_rate))
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
        let accrued_rate = accumulate_interest_dec(
            basket_asset.rate_index,
            interest_rates[i],
            time_elapsed.clone(),
        )?;

        basket.collateral_types[i].rate_index += accrued_rate;        
    }

    //Update rates_last_accrued
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
        //Base_Rate * max collateral_ratio
        //ex: 2% * 110% = 2.2%
        //Higher rates for riskier assets

        //base * (1/max_LTV)
        rates.push(decimal_multiplication(
            basket.clone().base_interest_rate,
            decimal_division(Decimal::one(), asset.max_LTV),
        ));        
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
        
        //The debt_proportion is used unless the supply proportion is over 1 or the farthest into slope 2 
        //A proportion in Slope 2 is prioritized        
        if supply_proportions[i] <= Decimal::one() || ((supply_proportions[i] > Decimal::one() && debt_proportions[i] > Decimal::one()) && supply_proportions[i] < debt_proportions[i]) {
            //Slope 2
            if debt_proportions[i] > Decimal::one(){
                //Ex: 91% > 90%
                ////0.01 * 100 = 1
                //1% = 1
                let percent_over_desired = decimal_multiplication(
                    decimal_subtraction(debt_proportions[i], Decimal::one()),
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
        } else if supply_proportions[i] > Decimal::one() {
            //Slope 2            
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
                    total_ratio += new_basket_ratios[i];
                }
            }

            //Calc interest rate
            let multi_cap_proportion = decimal_division(total_ratio, multi_asset_cap.supply_cap_ratio);

            for asset in multi_asset_cap.clone().assets{
                if let Some((i, _cap)) = basket.clone().collateral_supply_caps.into_iter().enumerate().find(|(_i, cap)| cap.asset_info.equal(&asset)){
                    //Substitute if proportion of multi_asset_cap is greater than 1 and both debt/supply proportions
                    if multi_cap_proportion > Decimal::one() && multi_cap_proportion > supply_proportions[i] && multi_cap_proportion > debt_proportions[i]{
                        //Slope 2            
                        //Ex: 91% > 90%
                        ////0.01 * 100 = 1
                        //1% = 1
                        let percent_over_desired = decimal_multiplication(
                            decimal_subtraction(multi_cap_proportion, Decimal::one()),
                            Decimal::percent(100_00),
                        );
                        let multiplier = percent_over_desired + Decimal::one();
                        //Change rate of (rate) increase w/ the configuration multiplier
                        let multiplier = multiplier * config.rate_slope_multiplier;
            
                        //Ex cont: Multiplier = 2; Pro_rata rate = 1.8%.
                        //// rate = 3.6%
                        two_slope_pro_rata_rates[i] = decimal_multiplication(
                                decimal_multiplication(rates[i], multi_cap_proportion),
                                multiplier,
                            )  
                        
                    }
                }
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
            .find(|basket_asset| basket_asset.asset.info.equal(&cAsset.asset.info)){
            ////Add proportionally the change in index
            // cAsset_ratio * change in index          
            avg_change_in_index += decimal_multiplication(ratios[i], decimal_division(basket_asset.rate_index, cAsset.rate_index));
            
            /////Update cAsset rate_index
            position.collateral_assets[i].rate_index = basket_asset.rate_index;        
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
    user: String,
) -> StdResult<()> {
    let config = CONFIG.load(storage)?;

    /////Accrue Interest to the Repayment Price///
    //Calc Time-elapsed and update last_Accrued
    let mut time_elapsed = env.block.time.seconds() - basket.credit_last_accrued;

    let mut negative_rate: bool = false;
    let mut price_difference: Decimal = Decimal::zero();
    let mut credit_price_rate: Decimal = Decimal::zero();

    ////Controller barriers to reduce risk of manipulation///
    //Liquidity above 2M
    //At least 3% of total supply as liquidity
    let liquidity = get_asset_liquidity(querier, config.clone().liquidity_contract.unwrap().to_string(), basket.clone().credit_asset.info)?;
    
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
    if liquidity_ratio < Decimal::percent(3) || liquidity < Uint128::new(2_000_000_000_000u128){
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
            let mut applied_rate = credit_price_rate.checked_mul(Decimal::from_ratio(
                Uint128::from(time_elapsed),
                Uint128::from(SECONDS_PER_YEAR),
            ))?;

            //If a positive rate we add 1,
            //If a negative rate we subtract the applied_rate from 1
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
        let mut accrued_interest = new_credit_amount - position.credit_amount;

        if let Some(contract) = config.clone().discounts_contract {
            //Get User's discounted interest
            accrued_interest = get_discounted_interest(querier, contract.to_string(), user, accrued_interest.clone())?;
        }

        //Add accrued interest to the basket's pending revenue
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
        ){
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

fn get_discounted_interest(
    querier: QuerierWrapper,
    discounts_contract: String,
    user: String,
    nondiscounted_interest: Uint128,
) -> StdResult<Uint128>{
    //Get discount
    let discount = querier.query_wasm_smart::<Decimal>(discounts_contract, &DiscountQueryMsg::UserDiscount { user })?;

    let discounted_interest = {
        let percent_of_interest = decimal_subtraction(Decimal::one(), discount);
        decimal_multiplication(Decimal::from_ratio(nondiscounted_interest, Uint128::one()), percent_of_interest)
    } * Uint128::one();
    
    Ok(discounted_interest)
}

////////////Immutable fns for Queries/////
pub fn accrue_imut(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    position: &mut Position,
    basket: &mut Basket,
    user: String,
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
    let liquidity = get_asset_liquidity(querier, config.clone().liquidity_contract.unwrap().to_string(), basket.clone().credit_asset.info)?;
    
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

    if liquidity_ratio < Decimal::percent(3) || liquidity < Uint128::new(2_000_000_000_000u128){
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
        let credit_TWAP_price = get_asset_values_imut(
            storage,
            env.clone(),
            querier,
            vec![credit_asset],
            config.clone(),
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

        //Don't accrue interest if price is within the margin of error
        if price_difference > basket.clone().cpc_margin_of_error {
            
            //Multiply price_difference by the cpc_multiplier
            credit_price_rate = decimal_multiplication(price_difference, config.clone().cpc_multiplier);

            //Calculate rate of change
            let mut applied_rate = credit_price_rate.checked_mul(Decimal::from_ratio(
                Uint128::from(time_elapsed),
                Uint128::from(SECONDS_PER_YEAR),
            ))?;

            //If a positive rate we add 1,
            //If a negative rate we subtract the applied_rate from 1
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
    let rate_of_change = get_credit_rate_of_change_imut(
        storage,
        querier,
        env.clone(),
        basket,
        position,
        negative_rate,
        credit_price_rate,
    )?;
    
     //Calc new_credit_amount
     let mut new_credit_amount = decimal_multiplication(
        Decimal::from_ratio(position.credit_amount, Uint128::new(1)), 
        rate_of_change
    ) * Uint128::new(1u128);    
    
    if new_credit_amount > position.credit_amount {

        if let Some(contract) = config.clone().discounts_contract {
            let mut accrued_interest = new_credit_amount - position.credit_amount;
            accrued_interest = get_discounted_interest(querier, contract.to_string(), user, accrued_interest)?;

            new_credit_amount = position.credit_amount + accrued_interest;
        }        

        //Set position's debt to the debt + accrued_interest
        position.credit_amount = new_credit_amount;
    }    

    Ok(())
}

//Rate of change of a position's credit_amount due to interest rates
pub fn get_credit_rate_of_change_imut(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    basket: &mut Basket,
    position: &mut Position,
    negative_rate: bool,
    credit_price_rate: Decimal,
) -> StdResult<Decimal> {
    let config = CONFIG.load(storage)?;
    let ratios = get_cAsset_ratios_imut(storage, env.clone(), querier, position.clone().collateral_assets, config)?;

    get_rate_indices(storage, querier, env, basket, negative_rate, credit_price_rate)?;

    let mut avg_change_in_index = Decimal::zero();
    for (i, cAsset) in position.clone().collateral_assets.iter().enumerate() {
        //Match asset and rate_index
        if let Some(basket_asset) = basket.clone().collateral_types
            .clone()
            .into_iter()
            .find(|basket_asset| basket_asset.asset.info.equal(&cAsset.asset.info))
        {           
            ////Add proportionally the change in index
            // cAsset_ratio * change in index          
            avg_change_in_index += decimal_multiplication(ratios[i], decimal_division(basket_asset.rate_index, cAsset.rate_index) );

            /////Update cAsset rate_index
            //This isn't saved since its a query but should resemble the state progression
            position.collateral_assets[i].rate_index = basket_asset.rate_index;
        }
    }    
    
    //The change in index represents the rate accrued to the cAsset in the time since last accrual
    Ok(avg_change_in_index)
}

//Get Basket interests and then accumulate interest to all basket cAsset rate indices
pub fn get_rate_indices(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env, 
    basket: &mut Basket,
    negative_rate: bool,
    credit_price_rate: Decimal,
) -> StdResult<()>{
    //Get basket rates
    let mut interest_rates = get_interest_rates_imut(storage, querier, env.clone(), basket)?
        .into_iter()
        .map(|rate| rate.1)
        .collect::<Vec<Decimal>>();
    
    //Add/Sub repayment rate to the rates
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
        let accrued_rate = accumulate_interest_dec(
            basket_asset.rate_index,
            interest_rates[i],
            time_elapsed.clone(),
        )?;
        
        basket.collateral_types[i].rate_index += accrued_rate;        
    }
    
    Ok(())
}

pub fn get_interest_rates_imut(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    basket: &mut Basket,
) -> StdResult<Vec<(AssetInfo, Decimal)>> {
    let config = CONFIG.load(storage)?;
    let mut rates = vec![];

    for asset in basket.clone().collateral_types {
        //Base_Rate * max collateral_ratio
        //ex: 2% * 110% = 2.2%
        //Higher rates for riskier assets

        //base * (1/max_LTV)
        rates.push(decimal_multiplication(
            basket.clone().base_interest_rate,
            decimal_division(Decimal::one(), asset.max_LTV),
        ));        
    }

    //Get proportion of debt && supply caps filled
    let mut debt_proportions = vec![];
    let mut supply_proportions = vec![];

    let debt_caps = match get_basket_debt_caps_imut(storage, querier, env.clone(), basket.clone()) {
        Ok(caps) => caps,
        Err(err) => {
            return Err(StdError::GenericErr {
                msg: err.to_string(),
            })
        }
    };
    
    //Get basket cAsset ratios
    let basket_ratios: Vec<Decimal> =
        get_cAsset_ratios_imut(storage, env.clone(), querier, basket.clone().collateral_types, config.clone())?;

    for (i, cap) in basket.clone().collateral_supply_caps
        .into_iter()
        .collect::<Vec<SupplyCap>>()
        .iter()
        .enumerate()
    {
        //Caps set to 0 can be used to push out unwanted assets by spiking rates
        if cap.supply_cap_ratio.is_zero() {
            debt_proportions.push(Decimal::percent(100));
            supply_proportions.push(Decimal::percent(100));
        } else if debt_caps[i].is_zero(){
            debt_proportions.push(Decimal::zero());
            supply_proportions.push(Decimal::zero());
        } else {
            debt_proportions.push(Decimal::from_ratio(cap.debt_total, debt_caps[i]));
            supply_proportions.push(decimal_division(basket_ratios[i], cap.supply_cap_ratio));
        }
    }
    

    //Gets pro-rata rate and uses multiplier if above desired utilization
    let mut two_slope_pro_rata_rates = vec![];
    for (i, _rate) in rates.iter().enumerate() {
        //The debt_proportion is used unless the supply proportion is over 1 or the farthest into slope 2 
        //A proportion in Slope 2 is prioritized        
        if supply_proportions[i] <= Decimal::one() || ((supply_proportions[i] > Decimal::one() && debt_proportions[i] > Decimal::one()) && supply_proportions[i] - Decimal::one() < debt_proportions[i] - Decimal::one()) {
            //Slope 2
            if debt_proportions[i] > Decimal::one() {
                //Ex: 91% > 90%
                ////0.01 * 100 = 1
                //1% = 1
                let percent_over_desired = decimal_multiplication(
                    decimal_subtraction(debt_proportions[i], Decimal::one()),
                    Decimal::percent(100_00),
                );
                let multiplier = percent_over_desired + Decimal::one();
                //Change rate of (rate) increase w/ the configuration multiplier
                let multiplier = multiplier * config.rate_slope_multiplier;

                //Ex cont: Multiplier = 2; Proportional rate = 1.8%.
                //// rate = 3.6%
                two_slope_pro_rata_rates.push((
                    basket.clone().collateral_supply_caps[i].clone().asset_info,
                    decimal_multiplication(
                        decimal_multiplication(rates[i], debt_proportions[i]),
                        multiplier,
                    ),
                ));
            } else {
                //Slope 1
                two_slope_pro_rata_rates.push((
                    basket.clone().collateral_supply_caps[i].clone().asset_info,
                    decimal_multiplication(rates[i], debt_proportions[i]),
                ));
            }
        } else if supply_proportions[i] > Decimal::one() {
            //Slope 2            
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
            two_slope_pro_rata_rates.push((
                basket.clone().collateral_supply_caps[i].clone().asset_info,
                decimal_multiplication(
                    decimal_multiplication(rates[i], supply_proportions[i]),
                    multiplier,
                ),
            ));
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
                    total_ratio += new_basket_ratios[i];
                }
            }

            //Calc interest rate
            let multi_cap_proportion = decimal_division(total_ratio, multi_asset_cap.supply_cap_ratio);

            for asset in multi_asset_cap.clone().assets{
                if let Some((i, _cap)) = basket.clone().collateral_supply_caps.into_iter().enumerate().find(|(_i, cap)| cap.asset_info.equal(&asset)){
                    //Substitute if proportion of multi_asset_cap is greater than 1 and both debt/supply proportions
                    if multi_cap_proportion > Decimal::one() && multi_cap_proportion > supply_proportions[i] && multi_cap_proportion > debt_proportions[i]{
                        //Slope 2            
                        //Ex: 91% > 90%
                        ////0.01 * 100 = 1
                        //1% = 1
                        let percent_over_desired = decimal_multiplication(
                            decimal_subtraction(multi_cap_proportion, Decimal::one()),
                            Decimal::percent(100_00),
                        );
                        let multiplier = percent_over_desired + Decimal::one();
                        //Change rate of (rate) increase w/ the configuration multiplier
                        let multiplier = multiplier * config.rate_slope_multiplier;
            
                        //Ex cont: Multiplier = 2; Pro_rata rate = 1.8%.
                        //// rate = 3.6%
                        two_slope_pro_rata_rates[i] = decimal_multiplication(
                                decimal_multiplication(rates[i], multi_cap_proportion),
                                multiplier,
                            )  
                        
                    }
                }
            }
        }
    }

    Ok(two_slope_pro_rata_rates)
}


