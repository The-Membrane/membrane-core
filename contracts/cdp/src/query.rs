use std::str::FromStr;

use cosmwasm_std::{
    to_binary, Addr, Decimal, Deps, Env, Order, QuerierWrapper, QueryRequest, StdError, StdResult,
    Storage, Uint128, WasmQuery,
};

use cw_storage_plus::Bound;

use membrane::oracle::{PriceResponse, QueryMsg as OracleQueryMsg};
use membrane::osmosis_proxy::QueryMsg as OsmoQueryMsg;
use membrane::cdp::{
    Config, BadDebtResponse, CollateralInterestResponse,
    InsolvencyResponse, InterestResponse, PositionResponse, PositionsResponse,
};

use membrane::types::{
    cAsset, AssetInfo, Basket, InsolventPosition, Position, PositionUserInfo,
    StoredPrice, UserInfo, DebtCap, PoolInfo, PoolStateResponse
};
use membrane::math::{decimal_division, decimal_multiplication, decimal_subtraction};


use crate::positions::check_for_empty_position;
use crate::rates::{accrue, get_interest_rates};
use crate::risk_engine::get_basket_debt_caps;
use crate::positions::read_price;
use crate::state::{BASKET, CONFIG, POSITIONS, get_target_position};

const MAX_LIMIT: u32 = 31;

pub fn query_position(
    deps: Deps,
    env: Env,
    position_id: Uint128,
    user: Addr,
) -> StdResult<PositionResponse> {
    let mut basket = BASKET.load(deps.storage)?;

    let (_i, mut position) = match get_target_position(deps.storage, user.clone(), position_id.clone()){
        Ok(position) => position,
        Err(err) => return Err(StdError::GenericErr { msg: err.to_string() }),
    };
    
    let config = CONFIG.load(deps.storage)?;

    let (borrow, max, _value, _prices) = get_avg_LTV(
        deps.storage,
        env.clone(),
        deps.querier,
        config.clone(),
        position.clone().collateral_assets,
    )?;

    accrue(
        deps.storage,
        deps.querier,
        env.clone(),
        &mut position,
        &mut basket,
        user.to_string(),
    )?;
    
    Ok(PositionResponse {
        position_id: position.position_id,
        collateral_assets: position.clone().collateral_assets,
        cAsset_ratios: get_cAsset_ratios(
            deps.storage,
            env.clone(),
            deps.querier,
            position.clone().collateral_assets,
            config.clone(),
        )?.0,
        credit_amount: position.credit_amount,
        basket_id: basket.basket_id,
        avg_borrow_LTV: borrow,
        avg_max_LTV: max,
    })

}

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
    
    let mut basket = BASKET.load(deps.storage)?;
    let mut user_positions: Vec<PositionResponse> = vec![];
    
    let _iter: () = positions.into_iter().take(limit).map(|mut position| {
        
        let (borrow, max, _value, _prices) = match get_avg_LTV(
            deps.storage,
            env.clone(),
            deps.querier,
            config.clone(),
            position.clone().collateral_assets,
        ) {
            Ok((borrow, max, value, prices)) => (borrow, max, value, prices),
            Err(err) => {
                error = Some(err);
                (Decimal::zero(), Decimal::zero(), Decimal::zero(), vec![])
            }
        };

        match accrue(
            deps.storage,
            deps.querier,
            env.clone(),
            &mut position,
            &mut basket,
            user.to_string(),
        ) {
            Ok(()) => {}
            Err(err) => error = Some(err),
        };

        let (cAsset_ratios, _) = match get_cAsset_ratios(
            deps.storage,
            env.clone(),
            deps.querier,
            position.clone().collateral_assets,
            config.clone(),
        ) {
            Ok(ratios) => ratios,
            Err(err) => {
                error = Some(err);
                (vec![], vec![])
            }
        };

        if error.is_none() {
            user_positions.push(PositionResponse {
                position_id: position.position_id,
                collateral_assets: position.collateral_assets,
                cAsset_ratios,
                credit_amount: position.credit_amount,
                basket_id: basket.basket_id,
                avg_borrow_LTV: borrow,
                avg_max_LTV: max,
            })
        }
    }).collect();

    if error.is_some() {
        return Err(error.unwrap())
    }
    Ok(user_positions)
    
}

pub fn query_basket_positions(
    deps: Deps,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<Vec<PositionsResponse>> {
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
            Ok(PositionsResponse {
                user: k.to_string(),
                positions: v,
            })
        })
        .collect()
}

//Calculate debt caps
pub fn query_basket_debt_caps(deps: Deps, env: Env) -> StdResult<Vec<DebtCap>> {    
    let mut basket: Basket = BASKET.load(deps.storage)?;

    let asset_caps = get_basket_debt_caps(deps.storage, deps.querier, env, &mut basket.clone())?;

    let mut res = vec![];
    //Append DebtCap
    for (index, cap) in basket.collateral_supply_caps.iter().enumerate() {        
        res.push(
                DebtCap{
                    collateral: cap.clone().asset_info,
                    debt_total: cap.debt_total,
                    cap: asset_caps[index],
                }
            );
    }

    Ok( res )
}

pub fn query_bad_debt(deps: Deps) -> StdResult<BadDebtResponse> {
    let mut res = BadDebtResponse {
        has_bad_debt: vec![],
    };

    let _iter: () = POSITIONS
        .range(deps.storage, None, None, Order::Ascending)
        .map(|item| {
            let (addr, positions) = item.unwrap();

            for position in positions {
                //We do a lazy check for bad debt by checking if there is debt without any assets left in the position
                //This is allowed bc any calls here will be after a liquidation where the sell wall would've sold all it could to cover debts
                let empty = check_for_empty_position(position.clone().collateral_assets);

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
        }).collect();

    Ok(res)
}

pub fn query_position_insolvency(
    deps: Deps,
    env: Env,
    position_id: Uint128,
    position_owner: String,
) -> StdResult<InsolvencyResponse> {
    let config: Config = CONFIG.load(deps.storage)?;
    let valid_owner_addr = deps.api.addr_validate(&position_owner)?;
    let mut basket: Basket = BASKET.load(deps.storage)?;

    let (_i, mut target_position) = match get_target_position(deps.storage, valid_owner_addr, position_id.clone()){
        Ok(position) => position,
        Err(err) => return Err(StdError::GenericErr { msg: err.to_string() }),
    };

    accrue(
        deps.storage,
        deps.querier,
        env.clone(),
        &mut target_position,
        &mut basket,
        position_owner.clone(),
    )?;

    ///
    let mut res = InsolvencyResponse {
        insolvent_positions: vec![],
    };

    let (insolvent, current_LTV, available_fee) = insolvency_check(
        deps.storage,
        env.clone(),
        deps.querier,
        target_position.collateral_assets,
        Decimal::from_ratio(target_position.credit_amount, Uint128::new(1u128)),
        basket.clone().credit_price,
        false,
        config.clone(),
    )?;

    //Since its a Singular position we'll output whether insolvent or not
    res.insolvent_positions.push(InsolventPosition {
        insolvent,
        position_info: UserInfo {
            position_id: target_position.position_id,
            position_owner: position_owner.to_string(),
        },
        current_LTV,
        available_fee,
    });

    Ok(res)
}

pub fn query_collateral_rates(
    deps: Deps,
    env: Env,
) -> StdResult<CollateralInterestResponse> {
    let mut basket = BASKET.load(deps.storage)?;

    let rates = get_interest_rates(deps.storage, deps.querier, env.clone(), &mut basket)?;

    let config = CONFIG.load(deps.storage)?;

    //Get repayment price - market price difference
    //Calc Time-elapsed and update last_Accrued
    let time_elapsed = env.block.time.seconds() - basket.credit_last_accrued;

    let negative_rate: bool;
    let mut price_difference: Decimal;

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
            deps.storage,
            env.clone(),
            deps.querier,
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
        if price_difference <= basket.clone().cpc_margin_of_error {
            price_difference = Decimal::zero();
        }

        let new_rates: Vec<Decimal> = rates
            .into_iter()
            .map(|mut rate| {
                //Accrue a year of repayment rate to interest rates
                if negative_rate {
                    rate = decimal_multiplication(
                        rate,
                        decimal_subtraction(Decimal::one(), price_difference),
                    );
                } else {
                    rate = decimal_multiplication(rate, (Decimal::one() + price_difference));
                }

                rate
            })
            .collect::<Vec<Decimal>>();

        Ok(CollateralInterestResponse { rates: new_rates })
    } else {
        Ok(CollateralInterestResponse { rates })
    }
}


pub fn query_basket_credit_interest(
    deps: Deps,
    env: Env,
) -> StdResult<InterestResponse> {
    let config = CONFIG.load(deps.storage)?;

    let basket = BASKET.load(deps.storage)?;

    let time_elapsed = env.block.time.seconds() - basket.credit_last_accrued;
    let mut price_difference = Decimal::zero();
    let mut negative_rate: bool = false;

    if !(time_elapsed == 0u64) {
        //Calculate new interest rate
        let credit_asset = cAsset {
            asset: basket.clone().credit_asset,
            max_borrow_LTV: Decimal::zero(),
            max_LTV: Decimal::zero(),
            pool_info: None,
            rate_index: Decimal::one(),
        };

        let credit_TWAP_price = get_asset_values(
            deps.storage,
            env,
            deps.querier,
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
                Decimal::zero()
            }
        };

        //Don't set interest if price is within the margin of error
        if price_difference <= basket.clone().cpc_margin_of_error {
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
) -> StdResult<(Vec<Decimal>, Vec<Decimal>)> {
    let (cAsset_values, cAsset_prices) = get_asset_values(
        storage,
        env,
        querier,
        collateral_assets.clone(),
        config,
    )?;

    let total_value: Decimal = cAsset_values.iter().sum();

    //getting each cAsset's % of total value
    let mut cAsset_ratios: Vec<Decimal> = vec![];
    for cAsset in cAsset_values {
        if total_value.is_zero() {
            cAsset_ratios.push(Decimal::zero());
        } else {
            cAsset_ratios.push(decimal_division(cAsset, total_value));
        }
    }

    Ok((cAsset_ratios, cAsset_prices))
}

/// Function queries the price of an asset from the oracle
/// If the query is within the oracle_time_limit, it will use the stored price
pub fn query_price(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: Config,
    asset_info: AssetInfo,
) -> StdResult<Decimal> {
    //Set timeframe
    let mut twap_timeframe: u64 = config.collateral_twap_timeframe;

    let basket = BASKET.load(storage)?;
    //if AssetInfo is the basket.credit_asset, change twap timeframe
    if asset_info.equal(&basket.credit_asset.info) {
        twap_timeframe = config.credit_twap_timeframe;
    }   

    //Try to use a stored price
    let stored_price: StoredPrice = read_price(storage, &asset_info)?;

    let time_elapsed: u64 = env.block.time.seconds() - stored_price.last_time_updated;
    //Use the stored price if within the oracle_time_limit
    if time_elapsed <= config.oracle_time_limit {
        return Ok(stored_price.price)
    } 
    
    //Query Price
    let price = match querier.query::<PriceResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().oracle_contract.unwrap().to_string(),
        msg: to_binary(&OracleQueryMsg::Price {
            asset_info: asset_info.clone(),
            twap_timeframe,
            basket_id: None,
        })?,
    })) {
        Ok(res) => {
            res.price
        }
        Err(err) => { return Err(err) }
    };

    Ok(price)
}

/// Calc Asset values
pub fn get_asset_values(
    storage: &dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    assets: Vec<cAsset>,
    config: Config,
) -> StdResult<(Vec<Decimal>, Vec<Decimal>)> {
    //Getting proportions for position collateral to calculate avg LTV
    //Using the index in the for loop to parse through the assets Vec and collateral_assets Vec
    //, as they are now aligned due to the collateral check w/ the Config's data
    let mut cAsset_values: Vec<Decimal> = vec![];
    let mut cAsset_prices: Vec<Decimal> = vec![];

    if config.clone().oracle_contract.is_some() {
        for (_i, cAsset) in assets.iter().enumerate() {
            //If an Osmosis LP
            if cAsset.pool_info.is_some() {
                let pool_info = cAsset.clone().pool_info.unwrap();
                let mut asset_prices = vec![];

                for (pool_asset) in pool_info.clone().asset_infos {
                    let price = query_price(
                        storage,
                        querier,
                        env.clone(),
                        config.clone(),
                        pool_asset.info,
                    )?;
                    //Append price
                    asset_prices.push(price);
                }

                //Calculate & append LP price & value
                append_lp_price(
                    querier,
                    config.clone(),
                    cAsset.clone(),
                    asset_prices,
                    &mut cAsset_values,
                    &mut cAsset_prices,
                    pool_info.clone(),
                )?;
            } else {
                let price = query_price(
                    storage,
                    querier,
                    env.clone(),
                    config.clone(),
                    cAsset.clone().asset.info,
                )?;

                cAsset_prices.push(price);
                let collateral_value = decimal_multiplication(
                    Decimal::from_ratio(cAsset.asset.amount, Uint128::new(1u128)),
                    price,
                );
                cAsset_values.push(collateral_value);
            }
        }
    }

    Ok((cAsset_values, cAsset_prices))
}


/// Calculate LP share token value
/// Calculate LP price
/// Append price and value to lists
pub fn append_lp_price(
    querier: QuerierWrapper,
    config: Config,
    cAsset: cAsset,
    asset_prices: Vec<Decimal>,
    cAsset_values: &mut Vec<Decimal>,
    cAsset_prices: &mut Vec<Decimal>,
    pool_info: PoolInfo,
) -> StdResult<()>{
    //Calculate share value
    let cAsset_value = {
        //Query share asset amount
        let share_asset_amounts = querier
            .query::<PoolStateResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
                msg: to_binary(&OsmoQueryMsg::PoolState {
                    id: pool_info.pool_id,
                })?,
            }))?
            .shares_value(cAsset.asset.amount);

        //Calculate value of cAsset
        let mut value = Decimal::zero();
        for (i, price) in asset_prices.into_iter().enumerate() {
            //Assert we are pulling asset amount from the correct asset
            let asset_share =
                match share_asset_amounts.clone().into_iter().find(|coin| {
                    AssetInfo::NativeToken {
                        denom: coin.denom.clone(),
                    } == pool_info.clone().asset_infos[i].info
                }) {
                    Some(coin) => coin,
                    None => {
                        return Err(StdError::GenericErr {
                            msg: format!(
                                "Invalid asset denom: {}",
                                pool_info.clone().asset_infos[i].info
                            ),
                        })
                    }
                };
            //Normalize Asset amounts to native token decimal amounts (6 places: 1 = 1_000_000)
            let exponent_difference = pool_info.clone().asset_infos[i]
                .decimals
                .checked_sub(6u64)
                .unwrap();
            let asset_amount = Uint128::from_str(&asset_share.amount).map_err(|_| StdError::GenericErr { msg: String::from("Error parsing String into Uint128") })?
                / Uint128::new(10u64.pow(exponent_difference as u32) as u128);
            let decimal_asset_amount =
                Decimal::from_ratio(asset_amount, Uint128::new(1u128));

            //Price * # of assets in LP shares
            value += decimal_multiplication(price, decimal_asset_amount);
        }

        value
    };

    //Calculate LP price
    let cAsset_price = {
        let share_amount =
            Decimal::from_ratio(cAsset.asset.amount, Uint128::new(1u128));
        if !share_amount.is_zero() {
            decimal_division(cAsset_value, share_amount)
        } else {
            Decimal::zero()
        }
    };

    //Push to price and value list
    cAsset_prices.push(cAsset_price);
    cAsset_values.push(cAsset_value);

    Ok(())
}

/// Calculates the average LTV of a position
/// Returns avg_borrow_LTV, avg_max_LTV, total_value and cAsset_prices
pub fn get_avg_LTV(
    storage: &dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    config: Config,
    collateral_assets: Vec<cAsset>,
) -> StdResult<(Decimal, Decimal, Decimal, Vec<Decimal>)> {
    //Calc total value of collateral
    let (cAsset_values, cAsset_prices) = get_asset_values(
        storage,
        env,
        querier,
        collateral_assets.clone(),
        config,
    )?;
    
    //Calculate avg LTV & return values
    calculate_avg_LTV(cAsset_values, cAsset_prices, collateral_assets)
}

/// Calculations for avg_borrow_LTV, avg_max_LTV, total_value and cAsset_prices
pub fn calculate_avg_LTV(
    cAsset_values: Vec<Decimal>,
    cAsset_prices: Vec<Decimal>,    
    collateral_assets: Vec<cAsset>,
) -> StdResult<(Decimal, Decimal, Decimal, Vec<Decimal>)> {
    let total_value: Decimal = cAsset_values.iter().sum();

    //getting each cAsset's % of total value
    let mut cAsset_ratios: Vec<Decimal> = vec![];
    for cAsset in cAsset_values {
        if total_value == Decimal::zero() {
            cAsset_ratios.push(Decimal::zero());
        } else {
            cAsset_ratios.push(decimal_division(cAsset, total_value));
        }
    }

    //Converting % of value to avg_LTV by multiplying collateral LTV by % of total value
    let mut avg_max_LTV: Decimal = Decimal::zero();
    let mut avg_borrow_LTV: Decimal = Decimal::zero();

    if cAsset_ratios.len() == 0 {
        return Ok((
            Decimal::percent(0),
            Decimal::percent(0),
            Decimal::percent(0),
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
        ));
    }

    for (i, _cAsset) in collateral_assets.clone().iter().enumerate() {
        avg_borrow_LTV +=
            decimal_multiplication(cAsset_ratios[i], collateral_assets[i].max_borrow_LTV);
    }

    for (i, _cAsset) in collateral_assets.clone().iter().enumerate() {
        avg_max_LTV += decimal_multiplication(cAsset_ratios[i], collateral_assets[i].max_LTV);
    }

    Ok((avg_borrow_LTV, avg_max_LTV, total_value, cAsset_prices))
}



/// Uses a Position's info to calculate if the user is insolvent
/// Returns insolvent, current_LTV and available fee
pub fn insolvency_check(
    storage: &dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    collateral_assets: Vec<cAsset>,
    credit_amount: Decimal,
    credit_price: Decimal,
    max_borrow: bool, //Toggle for either over max_borrow or over max_LTV (liquidatable)
    config: Config,
) -> StdResult<(bool, Decimal, Uint128)> { //insolvent, current_LTV, available_fee

    //Get avg LTVs
    let avg_LTVs: (Decimal, Decimal, Decimal, Vec<Decimal>) =
        get_avg_LTV(storage, env, querier, config, collateral_assets.clone())?;

    //Insolvency check
    insolvency_check_calc(avg_LTVs, collateral_assets, credit_amount, credit_price, max_borrow)
}

/// Function handles calculations for the insolvency check
pub fn insolvency_check_calc(
    //BorrowLTV, MaxLTV, TotalAssetValue, cAssetPrices
    avg_LTVs: (Decimal, Decimal, Decimal, Vec<Decimal>),    
    collateral_assets: Vec<cAsset>, 
    credit_amount: Decimal,
    credit_price: Decimal,
    max_borrow: bool, //Toggle for either over max_borrow or over max_LTV (liquidatable), ie taking the minimum collateral ratio into account.
) -> StdResult<(bool, Decimal, Uint128)>{ 
    //No assets but still has debt, return insolvent and skip other checks
    let total_assets: Uint128 = collateral_assets
        .iter()
        .map(|asset| asset.asset.amount)
        .collect::<Vec<Uint128>>()
        .iter()
        .sum();
    if total_assets.is_zero() && !credit_amount.is_zero() {
        return Ok((true, Decimal::percent(100), Uint128::zero()));
    }

    let total_asset_value: Decimal = avg_LTVs.2; //pulls total_asset_value
    let check: bool;
    //current_LTV = credit_amount * credit_price / total_asset_value);
    let current_LTV = 
        credit_amount.checked_mul(credit_price)?
        .checked_div(total_asset_value).map_err(|_| StdError::generic_err("Division by zero"))?; 

    match max_borrow {
        true => {
            //Checks max_borrow
            check = current_LTV > avg_LTVs.0;
        }
        false => {
            //Checks max_LTV
            check = current_LTV > avg_LTVs.1;
        }
    }

    let available_fee = if check {    
        //current_LTV - max_LTV
        let fee = current_LTV.checked_sub(avg_LTVs.1)?;
        //current_LTV - borrow_LTV
        let liq_range = current_LTV.checked_sub(avg_LTVs.0)?;
        //Fee value = repay_amount * fee
        liq_range.checked_div(current_LTV).map_err(|_| StdError::generic_err("Division by zero"))?
                .checked_mul(
        credit_amount.checked_mul(credit_price)?)?
                .checked_mul(fee)?
        * Uint128::new(1)        
    } else {
        Uint128::zero()
    };

    Ok((check, current_LTV, available_fee))
}