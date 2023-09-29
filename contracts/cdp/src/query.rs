use std::cmp::Ordering;

use cosmwasm_std::{
    to_binary, Addr, Decimal, Deps, Env, Order, QuerierWrapper, QueryRequest, StdError, StdResult,
    Storage, Uint128, WasmQuery,
};

use cw_storage_plus::Bound;

use membrane::oracle::{PriceResponse, QueryMsg as OracleQueryMsg};
use membrane::cdp::{
    Config, BadDebtResponse, CollateralInterestResponse,
    InsolvencyResponse, InterestResponse, PositionResponse, BasketPositionsResponse, RedeemabilityResponse,
};

use membrane::types::{
    cAsset, AssetInfo, Basket, InsolventPosition, Position, PositionUserInfo,
    UserInfo, DebtCap, RedemptionInfo, PremiumInfo
};
use membrane::math::{decimal_division, decimal_multiplication, decimal_subtraction};


use crate::positions::check_for_empty_position;
use crate::rates::{accrue, get_interest_rates};
use crate::risk_engine::get_basket_debt_caps;
use crate::positions::read_price;
use crate::state::{BASKET, CONFIG, POSITIONS, get_target_position, REDEMPTION_OPT_IN};

const MAX_LIMIT: u32 = 31;

/// Returns Position information
pub fn query_position(
    deps: Deps,
    env: Env,
    position_id: Uint128,
    user: Addr,
) -> StdResult<PositionResponse> {
    let basket = BASKET.load(deps.storage)?;

    let (_i, position) = match get_target_position(deps.storage, user.clone(), position_id){
        Ok(position) => position,
        Err(err) => return Err(StdError::GenericErr { msg: err.to_string() }),
    };
    
    let config = CONFIG.load(deps.storage)?;

    let (borrow, max, _value, _prices, ratios) = get_avg_LTV(
        deps.storage,
        env.clone(),
        deps.querier,
        config.clone(),
        Some(basket.clone()),
        position.clone().collateral_assets,
        false,
        false,
    )?;
    
    Ok(PositionResponse {
        position_id: position.position_id,
        collateral_assets: position.clone().collateral_assets,
        cAsset_ratios: ratios,
        credit_amount: position.credit_amount,
        basket_id: basket.basket_id,
        avg_borrow_LTV: borrow,
        avg_max_LTV: max,
    })

}

/// Returns Positions for a given user
pub fn query_user_positions(
    deps: Deps,
    env: Env,
    user: Addr,
    limit: Option<u32>,
) -> StdResult<Vec<PositionResponse>> {
    let limit = limit.unwrap_or(MAX_LIMIT) as usize;
    let config = CONFIG.load(deps.storage)?;
    let mut error: Option<StdError> = None;
    
    let positions: Vec<Position> = match POSITIONS.load(deps.storage,user.clone()){
        Err(_) => return Err(StdError::GenericErr{msg: String::from("No User Positions")}),
        Ok(positions) => positions,
    };
    
    let basket = BASKET.load(deps.storage)?;
    let mut user_positions: Vec<PositionResponse> = vec![];
    
    for position in positions.into_iter().take(limit) {
        
        let (borrow, max, _value, _prices, ratios) = match get_avg_LTV(
            deps.storage,
            env.clone(),
            deps.querier,
            config.clone(),
            Some(basket.clone()),
            position.clone().collateral_assets,
            false,
            false
        ) {
            Ok((borrow, max, value, prices, ratios)) => (borrow, max, value, prices, ratios),
            Err(err) => {
                error = Some(err);
                (Decimal::zero(), Decimal::zero(), Decimal::zero(), vec![], vec![])
            }
        };

        let cAsset_ratios = ratios;

        if error.is_none() {
            user_positions.push(PositionResponse {
                position_id: position.position_id,
                collateral_assets: position.collateral_assets,
                cAsset_ratios,
                credit_amount: position.credit_amount,
                basket_id: basket.clone().basket_id,
                avg_borrow_LTV: borrow,
                avg_max_LTV: max,
            })
        }
    };

    if let Some(error) = error{
        return Err(error)
    }

    Ok(user_positions)
    
}

/// Returns Positions in a Basket
pub fn query_basket_positions(
    deps: Deps,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<Vec<BasketPositionsResponse>> {
    let limit = limit.unwrap_or(MAX_LIMIT) as usize;

    let start = if let Some(start) = start_after {
        let start_after_addr = deps.api.addr_validate(&start)?;
        Some(Bound::exclusive(start_after_addr))
    } else {
        None
    };

    POSITIONS
        .range(deps.storage, start, None, Order::Ascending)
        .take(limit)
        .map(|item| {
            let (k, v) = item?;
            Ok(BasketPositionsResponse {
                user: k.to_string(),
                positions: v,
            })
        })
        .collect()
}

//Calculate debt caps
pub fn query_basket_debt_caps(deps: Deps, env: Env) -> StdResult<Vec<DebtCap>> {    
    let mut basket: Basket = BASKET.load(deps.storage)?;

    let asset_caps = get_basket_debt_caps(deps.storage, deps.querier, env, &mut basket)?;

    let mut res = vec![];
    //Append DebtCap
    for (index, cap) in basket.collateral_supply_caps.iter().enumerate() {        
        res.push(
                DebtCap {
                    collateral: cap.clone().asset_info,
                    debt_total: cap.debt_total,
                    cap: asset_caps[index],
                }
            );
    }

    Ok( res )
}

/// Returns Position info with bad debt in the Basket
pub fn query_bad_debt(deps: Deps) -> StdResult<BadDebtResponse> {
    let mut res = BadDebtResponse {
        has_bad_debt: vec![],
    };

    let _iter: (_) = POSITIONS
        .range(deps.storage, None, None, Order::Ascending)
        .map(|item| {
            let (addr, positions) = item.unwrap();

            for position in positions {
                //We do a lazy check for bad debt by checking if there is debt without any assets left in the position
                //This is allowed bc any calls here will be after a liquidation where the SP would've sold all it could to cover debts
                let empty = check_for_empty_position(position.collateral_assets);

                //If there are no assets and outstanding debt
                if empty && !position.credit_amount.is_zero() {
                    res.has_bad_debt.push((
                        PositionUserInfo {
                            position_id: Some(position.position_id),
                            position_owner: Some(addr.to_string()),
                        },
                        position.credit_amount,
                    ))
                }
            }
        });

    Ok(res)
}

/// Returns Position's insolvency status
/// The idea is that the response can be handled by acting on the fee. So if the fee is 0, then the position is solvent or doesn't exist.
pub fn query_position_insolvency(
    deps: Deps,
    env: Env,
    position_id: Uint128,
    position_owner: String,
) -> StdResult<InsolvencyResponse> {
    let config: Config = CONFIG.load(deps.storage)?;
    let valid_owner_addr = deps.api.addr_validate(&position_owner)?;
    let mut basket: Basket = BASKET.load(deps.storage)?;

    let (_i, mut target_position) = match get_target_position(deps.storage, valid_owner_addr, position_id){
        Ok(position) => position,
        //If position doesn't exist, return solvent response
        Err(_) => return Ok(InsolvencyResponse {
            insolvent_positions: vec![
                InsolventPosition {
                    insolvent: false,
                    position_info: UserInfo {
                        position_id,
                        position_owner,
                    },
                    current_LTV: Decimal::zero(),
                    available_fee: Uint128::zero(),
                }
            ],
        })
    };

    match accrue(
        deps.storage,
        deps.querier,
        env.clone(),
        config.clone(),
        &mut target_position,
        &mut basket,
        position_owner.clone(),
        false,
        true,
    ){
        Ok(_) => {}
        Err(_) => return Ok(InsolvencyResponse {
            insolvent_positions: vec![
                InsolventPosition {
                    insolvent: false,
                    position_info: UserInfo {
                        position_id,
                        position_owner,
                    },
                    current_LTV: Decimal::zero(),
                    available_fee: Uint128::zero(),
                }
            ],
        })
    
    };

    //Query insolvency
    let (insolvent, current_LTV, available_fee) = match insolvency_check(
        deps.storage,
        env,
        deps.querier,
        Some(basket.clone()),
        target_position.collateral_assets,
        target_position.credit_amount,
        basket.credit_price,
        false,
        config,
        true,
    ){
        Ok(((insolvent, current_LTV, available_fee), _)) => (insolvent, current_LTV, available_fee),
        Err(_) => {
            return Ok(InsolvencyResponse {
                insolvent_positions: vec![
                    InsolventPosition {
                        insolvent: false,
                        position_info: UserInfo {
                            position_id,
                            position_owner,
                        },
                        current_LTV: Decimal::zero(),
                        available_fee: Uint128::zero(),
                    }
                ],
            })
        }
    };

    Ok(InsolvencyResponse {
        insolvent_positions: vec![
            InsolventPosition {
                insolvent,
                position_info: UserInfo {
                    position_id,
                    position_owner,
                },
                current_LTV,
                available_fee,
            }
        ],
    })
}

/// Returns cAsset interest rates for the Basket
pub fn query_collateral_rates(
    deps: Deps,
    env: Env,
) -> StdResult<CollateralInterestResponse> {
    let mut basket = BASKET.load(deps.storage)?;

    let rates = get_interest_rates(deps.storage, deps.querier, env.clone(), &mut basket, true)?;

    let config = CONFIG.load(deps.storage)?;

    //Get repayment price - market price difference
    //Calc Time-elapsed and update last_Accrued
    let time_elapsed = env.block.time.seconds() - basket.credit_last_accrued;

    let mut negative_rate: bool = false;
    let mut price_difference: Decimal;

    if time_elapsed != 0u64 && basket.oracle_set {
        basket.credit_last_accrued = env.block.time.seconds();

        //Calculate new interest rate
        let credit_asset = cAsset {
            asset: basket.clone().credit_asset,
            max_borrow_LTV: Decimal::zero(),
            max_LTV: Decimal::zero(),
            pool_info: None,
            rate_index: Decimal::one(),
        };
        let credit_TWAP_price = match get_asset_values(
            deps.storage,
            env,
            deps.querier,
            vec![credit_asset],
            config,
            false
        ){
            Ok((_, prices)) => {
                if prices[0].price.is_zero() {
                    return Ok(CollateralInterestResponse { rates });
                }
                prices[0].price
            },
            //It'll error if the twap is longer than the pool lifespan
            Err(_) => return Ok(CollateralInterestResponse { rates }),
        };
        //We divide w/ the greater number first so the quotient is always 1.__
        price_difference = {
            //Compare market price & redemption price
            match credit_TWAP_price.cmp(&basket.credit_price.price) {
                Ordering::Greater => {
                    negative_rate = true;
                    decimal_subtraction(
                        decimal_division(credit_TWAP_price, basket.credit_price.price)?,
                        Decimal::one(),
                    )?
                }
                Ordering::Less => {
                    decimal_subtraction(
                        decimal_division(basket.credit_price.price, credit_TWAP_price)?,
                        Decimal::one(),
                    )?
                }
                Ordering::Equal => Decimal::zero(),
            }
        };

        //Don't accrue interest if price is within the margin of error
        if price_difference <= basket.clone().cpc_margin_of_error {
            price_difference = Decimal::zero();
        }

        let new_rates: Vec<Decimal> = rates
            .into_iter()
            .map(|rate| {
                //Accrue a year of repayment rate to interest rates
                if negative_rate {
                    decimal_multiplication(
                        rate,
                        decimal_subtraction(Decimal::one(), price_difference)?,
                    )
                } else {
                    decimal_multiplication(rate, (Decimal::one() + price_difference))
                }
            })
            .collect::<StdResult<Vec<Decimal>>>()?;

        Ok(CollateralInterestResponse { rates: new_rates })
    } else {
        Ok(CollateralInterestResponse { rates })
    }
}

/// Returns Basket credit redemption interest rate
pub fn query_basket_credit_interest(
    deps: Deps,
    env: Env,
) -> StdResult<InterestResponse> {
    let config = CONFIG.load(deps.storage)?;

    let basket = BASKET.load(deps.storage)?;

    let time_elapsed = env.block.time.seconds() - basket.credit_last_accrued;
    let mut price_difference = Decimal::zero();
    let mut negative_rate: bool = false;

    if !time_elapsed != 0u64 {
        //Calculate new interest rate
        let credit_asset = cAsset {
            asset: basket.clone().credit_asset,
            max_borrow_LTV: Decimal::zero(),
            max_LTV: Decimal::zero(),
            pool_info: None,
            rate_index: Decimal::one(),
        };

        let credit_TWAP_price = match  get_asset_values(
            deps.storage,
            env,
            deps.querier,
            vec![credit_asset],
            config,
            false
        ){
            Ok((_, prices)) => {
                if prices[0].price.is_zero() {
                    return Ok(InterestResponse {
                        credit_interest: Decimal::zero(),
                        negative_rate: false,
                    })
                }
                prices[0].price
            },
            //It'll error if the twap is longer than the pool lifespan
            Err(_) => return Ok(InterestResponse {
                credit_interest: Decimal::zero(),
                negative_rate: false,
            })
        };

        //We divide w/ the greater number first so the quotient is always 1.__
        price_difference = {
            //Compare market price & redemption price
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
                Ordering::Equal => Decimal::zero(),
            }
        };

        //Don't set interest if price is within the margin of error
        if price_difference <= basket.cpc_margin_of_error {
            price_difference = Decimal::zero();
        }
    }

    Ok(InterestResponse {
        credit_interest: price_difference,
        negative_rate,
    })
}

////Helper/////
/// Returns cAsset ratios & prices for a Position
pub fn get_cAsset_ratios(
    storage: &dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    collateral_assets: Vec<cAsset>,
    config: Config,
) -> StdResult<(Vec<Decimal>, Vec<PriceResponse>)> {
    let (cAsset_values, cAsset_prices) = get_asset_values(
        storage,
        env,
        querier,
        collateral_assets,
        config,
        false
    )?;
    
    let total_value: Decimal = cAsset_values.iter().sum();

    //getting each cAsset's % of total value
    let mut cAsset_ratios: Vec<Decimal> = vec![];
    for cAsset in cAsset_values {
        if total_value.is_zero() {
            cAsset_ratios.push(Decimal::zero());
        } else {
            cAsset_ratios.push(decimal_division(cAsset, total_value)?);
        }
    }

    Ok((cAsset_ratios, cAsset_prices))
}

/// Function queries the price of an asset from the oracle.
/// If the query is within the oracle_time_limit, it will use the stored price.
pub fn query_price(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: Config,
    asset_info: AssetInfo,
    is_deposit_function: bool,
) -> StdResult<PriceResponse> {
    //Set timeframe
    let mut twap_timeframe: u64 = config.collateral_twap_timeframe;

    let basket = BASKET.load(storage)?;
    //if AssetInfo is the basket.credit_asset, change twap timeframe
    if asset_info.equal(&basket.credit_asset.info) {
        twap_timeframe = config.credit_twap_timeframe;
    }   

    //Try to use a stored price
    let stored_price_res = read_price(storage, &asset_info);
    
    //If depositing, always query a new price to ensure removed assets aren't deposited
    if !is_deposit_function {
        //Use the stored price if within the oracle_time_limit
        if let Ok(ref stored_price) = stored_price_res {
            let time_elapsed: u64 = env.block.time.seconds() - stored_price.last_time_updated;

            if time_elapsed <= config.oracle_time_limit {
                return Ok(stored_price.clone().price)
            }
        }
    }
    
    //Query Price
    let res = match querier.query::<PriceResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.oracle_contract.unwrap().to_string(),
        msg: to_binary(&OracleQueryMsg::Price {
            asset_info,
            twap_timeframe,
            oracle_time_limit: config.oracle_time_limit,
            basket_id: None,
        })?,
    })) {
        Ok(res) => {
            res
        }
        Err(err) => {
            //if the oracle is down, error
            return Err(err)
        }
    };

    Ok(res)
}

/// Get Basket Redeemability
pub fn query_basket_redeemability(
    deps: Deps,
    position_owner: Option<String>,
    start_after: Option<u128>,
    limit: Option<u32>,
) -> StdResult<RedeemabilityResponse>{
    //Set premium start 
    let start = start_after.unwrap_or(0u128);

    let mut limit = limit.unwrap_or(MAX_LIMIT);

    //Set valid address
    let mut valid_address = None;
    if let Some(_user) = position_owner.clone(){
        valid_address = Some(deps.api.addr_validate(&_user)?);
    }

    //Initialize response
    let mut res: Vec<PremiumInfo> = vec![];

    //Query by premium
    for premium in start..100u128 {
        let users_of_premium: Vec<RedemptionInfo> = match REDEMPTION_OPT_IN.load(deps.storage, premium){
            Ok(list)=> list,
            Err(_err) => vec![], //If no users, return empty vec
        };

        //If there are users of this premium, add the state to the response
        if !users_of_premium.is_empty(){

            if let Some(_user) = position_owner.clone(){    
                //Add to the user's info to the response if in the premium
                let users_info_in_premium = users_of_premium
                    .into_iter()
                    .filter(|info: &RedemptionInfo| info.position_owner == valid_address.clone().unwrap())
                    .collect::<Vec<RedemptionInfo>>();

                if !users_info_in_premium.is_empty(){
                    res.push(PremiumInfo {
                        premium,
                        users_of_premium: users_info_in_premium,
                    });
                }

            } else {
                //Assert limit 
                if limit >= users_of_premium.len() as u32 {
                    //Add all users in this premium
                    res.push(PremiumInfo {
                        premium,
                        users_of_premium: users_of_premium.clone(),
                    });
                    //Update limit
                    limit = limit.checked_sub(users_of_premium.len() as u32).unwrap_or(0u32);
                } else {
                    //Add up to the remaining limit
                    let final_addition = users_of_premium.clone().into_iter().take(limit as usize).collect::<Vec<RedemptionInfo>>();

                    res.push(PremiumInfo {
                        premium,
                        users_of_premium: final_addition,
                    });
                }
            }            
        }
    }

    Ok(
        RedeemabilityResponse {
            premium_infos: res,
        }
    )
}


/// Calculate cAsset values & returns a tuple of (cAsset_values, cAsset_prices)
pub fn get_asset_values(
    storage: &dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    assets: Vec<cAsset>,
    config: Config,
    is_deposit_function: bool,
) -> StdResult<(Vec<Decimal>, Vec<PriceResponse>)> {
    //Enforce Vec max size
    if assets.len() > 50 {
        return Err(StdError::GenericErr {
            msg: String::from("Max asset_infos length is 50"),
        });
    }

    //Getting proportions for position collateral to calculate avg LTV
    //Using the index in the for loop to parse through the assets Vec and collateral_assets Vec
    //, as they are now aligned due to the collateral check w/ the Config's data
    let mut cAsset_values: Vec<Decimal> = vec![];
    let mut cAsset_prices: Vec<PriceResponse> = vec![];

    if config.oracle_contract.is_some() {
        for (_i, cAsset) in assets.iter().enumerate() {
            //Query prices
            //The oracle handles LP pricing
            let price_res = query_price(
                storage,
                querier,
                env.clone(),
                config.clone(),
                cAsset.clone().asset.info,
                is_deposit_function,
            )?;
            let cAsset_value = price_res.get_value(cAsset.asset.amount)?;
            
            cAsset_prices.push(price_res);
            cAsset_values.push(cAsset_value);
        
        }
    }

    Ok((cAsset_values, cAsset_prices))
}

/// Calculates the average LTV of a position.
/// Returns avg_borrow_LTV, avg_max_LTV, total_value and cAsset_prices.
pub fn get_avg_LTV(
    storage: &dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    config: Config,
    basket: Option<Basket>,
    collateral_assets: Vec<cAsset>,
    is_deposit_function: bool,
    is_liquidation_function: bool, //Skip softened borrow LTV
) -> StdResult<(Decimal, Decimal, Decimal, Vec<PriceResponse>, Vec<Decimal>)> {
    //Calc total value of collateral
    let (cAsset_values, cAsset_price_res) = get_asset_values(
        storage,
        env.clone(),
        querier,
        collateral_assets.clone(),
        config.clone(),
        is_deposit_function,
    )?;

    //Load basket
    let basket = if let Some(basket) = basket {
        basket
    } else {
        BASKET.load(storage)?
    };

    //Get basket cAsset ratios
    let (basket_cAsset_ratios, _) = get_cAsset_ratios(
        storage, 
        env, 
        querier, 
        basket.clone().collateral_types, 
        config
    )?;
    
    //Calculate avg LTV & return values
    calculate_avg_LTV(
        cAsset_values, 
        cAsset_price_res, 
        collateral_assets, 
        basket.clone().collateral_types, 
        basket_cAsset_ratios,
        is_liquidation_function,
    )
}

/// Calculations for avg_borrow_LTV, avg_max_LTV, total_value and cAsset_prices
pub fn calculate_avg_LTV(
    cAsset_values: Vec<Decimal>,
    cAsset_prices: Vec<PriceResponse>,    
    mut collateral_assets: Vec<cAsset>,
    basket_collateral_assets: Vec<cAsset>,
    basket_cAsset_ratios: Vec<Decimal>,
    is_liquidation_function: bool,
) -> StdResult<(Decimal, Decimal, Decimal, Vec<PriceResponse>, Vec<Decimal>)> {
    let total_value: Decimal = cAsset_values.iter().sum();

    //getting each cAsset's % of total value
    let mut cAsset_ratios: Vec<Decimal> = vec![];
    for cAsset in cAsset_values {
        if total_value == Decimal::zero() {
            cAsset_ratios.push(Decimal::zero());
        } else {
            cAsset_ratios.push(decimal_division(cAsset, total_value)?);
        }
    }

    //Converting % of value to avg_LTV by multiplying collateral LTV by % of total value
    let mut avg_max_LTV: Decimal = Decimal::zero();
    let mut avg_borrow_LTV: Decimal = Decimal::zero();

    if cAsset_ratios.is_empty(){
        return Ok((
            Decimal::percent(0),
            Decimal::percent(0),
            Decimal::percent(0),
            vec![],
            vec![],
        ));        
    }

    //Skip unecessary calculations if length is 1
    if cAsset_ratios.len() == 1 {
        return Ok((
            collateral_assets[0].max_borrow_LTV,
            collateral_assets[0].max_LTV,
            total_value,
            cAsset_prices,
            cAsset_ratios,
        ));
    }

    //Don't soften avg_borrow_LTV if we are liquidating, to keep liquidation price flat
    if !is_liquidation_function {
        //Alter borrow_LTV based on Basket supply ratio
        for (i, cAsset) in collateral_assets.clone().into_iter().enumerate() {
            //Find cAsset_ratio in basket
            if let Some((basket_index, _)) = basket_collateral_assets.iter().enumerate().find(|(_, x)| x.asset == cAsset.asset) {
                //Get the difference between max & borrow LTV
                let LTV_difference = cAsset.max_LTV - cAsset.max_borrow_LTV;

                //Multiply difference by basket ratio
                let added_LTV_difference = decimal_multiplication(LTV_difference, 
                    decimal_subtraction(Decimal::one(), basket_cAsset_ratios[basket_index])?
                )?;
                collateral_assets[i].max_borrow_LTV = cAsset.max_borrow_LTV + added_LTV_difference;
            }
        }
    }

    for (i, _cAsset) in collateral_assets.iter().enumerate() {
        avg_borrow_LTV +=
            decimal_multiplication(cAsset_ratios[i], collateral_assets[i].max_borrow_LTV)?;
    }

    for (i, _cAsset) in collateral_assets.iter().enumerate() {
        avg_max_LTV += decimal_multiplication(cAsset_ratios[i], collateral_assets[i].max_LTV)?;
    }

    Ok((avg_borrow_LTV, avg_max_LTV, total_value, cAsset_prices, cAsset_ratios))
}


/// Uses a Position's info to calculate if the user is insolvent.
/// Returns insolvent, current_LTV and available fee.
pub fn insolvency_check(
    storage: &dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    basket: Option<Basket>,
    collateral_assets: Vec<cAsset>,
    credit_amount: Uint128,
    credit_price: PriceResponse,
    max_borrow: bool, //Toggle for either over max_borrow or over max_LTV (liquidatable)
    config: Config,
    is_liquidation_function: bool, //Skip softened borrow LTV
) -> StdResult<((bool, Decimal, Uint128), (Decimal, Decimal, Decimal, Vec<PriceResponse>, Vec<Decimal>))> { //insolvent, current_LTV, available_fee, (avg_LTV return values)

    //Get avg LTVs
    let avg_LTVs: (Decimal, Decimal, Decimal, Vec<PriceResponse>, Vec<Decimal>) =
        get_avg_LTV(storage, env, querier, config, basket, collateral_assets.clone(), false, is_liquidation_function)?;

    //Insolvency check
    Ok((insolvency_check_calc(avg_LTVs.clone(), collateral_assets, credit_amount, credit_price, max_borrow)?, avg_LTVs))
}

/// Function handles calculations for the insolvency check
pub fn insolvency_check_calc(
    //BorrowLTV, MaxLTV, TotalAssetValue, cAssetPrices
    avg_LTVs: (Decimal, Decimal, Decimal, Vec<PriceResponse>, Vec<Decimal>),    
    collateral_assets: Vec<cAsset>, 
    credit_amount: Uint128,
    credit_price: PriceResponse,
    max_borrow: bool, //Toggle for either over max_borrow or over max_LTV (liquidatable), ie taking the minimum collateral ratio into account.
) -> StdResult<(bool, Decimal, Uint128)>{ 
    //No assets but still has debt, return insolvent and skip other checks
    let total_assets: Uint128 = collateral_assets
        .iter()
        .map(|asset| asset.asset.amount)
        .collect::<Vec<Uint128>>()
        .iter()
        .sum();
    
    // No assets with debt, return insolvent        
    if total_assets.is_zero() && !credit_amount.is_zero() {
        return Ok((true, Decimal::percent(100), Uint128::zero()));
    } // No assets and no debt, return not insolvent        
    else if credit_amount.is_zero() {
        return Ok((false, Decimal::percent(0), Uint128::zero()));
    }

    
    let total_asset_value: Decimal = avg_LTVs.2; //pulls total_asset_value
    let debt_value = credit_price.get_value(credit_amount)?;
    //current_LTV = debt_value / total_asset_value);
    let current_LTV = 
        debt_value.checked_div(total_asset_value).map_err(|_| StdError::GenericErr{msg: format!("Division by zero in insolvency_check_calc, line 907. debt_value: {}, total_asset_value: {}", debt_value, total_asset_value)})?; 
    
    //Return for testing
    // return Err(StdError::GenericErr{msg: format!("debt_value: {}, total_asset_value: {}, current_LTV: {}, max_borrow: {}", debt_value, total_asset_value, current_LTV, avg_LTVs.0)});

    let check: bool = match max_borrow {
        true => {
            //Checks max_borrow
            current_LTV > avg_LTVs.0
        }
        false => {
            //Checks max_LTV
            current_LTV > avg_LTVs.1
        }
    };

    let available_fee = if check && current_LTV > avg_LTVs.1{    
        //current_LTV - max_LTV
        let fee = current_LTV.checked_sub(avg_LTVs.1)?;
        //current_LTV - borrow_LTV
        let liq_range = current_LTV.checked_sub(avg_LTVs.0)?;
        //Fee value = repay_amount * fee
        liq_range.checked_div(current_LTV).map_err(|_| StdError::GenericErr{msg: format!("Division by zero in insolvency_check_calc, line 926. liq_range: {}, current_LTV: {}", liq_range, current_LTV)})?
                .checked_mul(debt_value)?
                .checked_mul(fee)?
        * Uint128::new(1)        
    } else {
        Uint128::zero()
    };

    Ok((check, current_LTV, available_fee))
}