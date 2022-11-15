use cosmwasm_std::{
    to_binary, Addr, Decimal, Deps, Env, Order, QuerierWrapper, QueryRequest, StdError, StdResult,
    Storage, Uint128, WasmQuery,
};

use cw_storage_plus::Bound;

use membrane::oracle::{PriceResponse, QueryMsg as OracleQueryMsg};
use membrane::osmosis_proxy::QueryMsg as OsmoQueryMsg;
use membrane::positions::{
    Config, BadDebtResponse, BasketResponse, CollateralInterestResponse, DebtCapResponse,
    InsolvencyResponse, InterestResponse, PositionResponse, PositionsResponse, PropResponse,
};

use membrane::types::{
    cAsset, AssetInfo, Basket, InsolventPosition, Position, PositionUserInfo,
    StoredPrice, UserInfo, 
};
use membrane::math::{decimal_division, decimal_multiplication, decimal_subtraction};


use crate::rates::{accrue_imut, get_interest_rates_imut};
use crate::risk_engine::get_basket_debt_caps_imut;

use osmo_bindings::PoolStateResponse;

use crate::{
    positions::read_price,
    state::{BASKETS, CONFIG, POSITIONS, LIQUIDATION},
};

const MAX_LIMIT: u32 = 31;

pub fn query_prop(deps: Deps) -> StdResult<PropResponse> {
    match LIQUIDATION.load(deps.storage) {
        Ok(prop) => Ok(PropResponse {
            liq_queue_leftovers: prop.clone().liq_queue_leftovers,
            stability_pool: prop.clone().stability_pool,
            sell_wall_distributions: prop.clone().sell_wall_distributions,
            positions_contract: prop.clone().positions_contract.to_string(),
            position_id: prop.clone().position_id,
            basket_id: prop.clone().basket_id,
            position_owner: prop.clone().position_owner.to_string(),
        }),
        Err(err) => return Err(err),
    }
}


pub fn query_position(
    deps: Deps,
    env: Env,
    position_id: Uint128,
    basket_id: Uint128,
    user: Addr,
) -> StdResult<PositionResponse> {
    let positions: Vec<Position> =
        match POSITIONS.load(deps.storage, (basket_id.clone().to_string(), user.clone())) {
            Err(_) => return Err(StdError::generic_err("No User Positions")),
            Ok(positions) => positions,
        };

    let mut basket = BASKETS.load(deps.storage, basket_id.to_string())?;

    let position = positions.into_iter().find(|x| x.position_id == position_id);

    match position {
        Some(mut position) => {
            let config = CONFIG.load(deps.storage)?;

            let (borrow, max, _value, _prices) = get_avg_LTV_imut(
                deps.storage,
                env.clone(),
                deps.querier,
                position.clone().collateral_assets,
                config.clone(),
            )?;

            accrue_imut(
                deps.storage,
                deps.querier,
                env.clone(),
                &mut position,
                &mut basket,
            )?;

            Ok(PositionResponse {
                position_id: position.position_id,
                collateral_assets: position.clone().collateral_assets,
                cAsset_ratios: get_cAsset_ratios_imut(
                    deps.storage,
                    env.clone(),
                    deps.querier,
                    position.clone().collateral_assets,
                    config.clone(),
                )?,
                credit_amount: position.credit_amount,
                basket_id: position.basket_id,
                avg_borrow_LTV: borrow,
                avg_max_LTV: max,
            })
        }

        None => return Err(StdError::generic_err("NonExistent Position")),
    }
}

pub fn query_user_positions(
    deps: Deps,
    env: Env,
    basket_id: Option<Uint128>,
    user: Addr,
    limit: Option<u32>,
) -> StdResult<Vec<PositionResponse>> {
    let limit = limit.unwrap_or(MAX_LIMIT) as usize;

    let config = CONFIG.load(deps.storage)?;

    let mut error: Option<StdError> = None;

    //Basket_id means only position from said basket
    if basket_id.is_some() {
        let positions: Vec<Position> = match POSITIONS.load(
            deps.storage,
            (basket_id.clone().unwrap().clone().to_string(), user.clone()),
        ) {
            Err(_) => return Err(StdError::generic_err("No User Positions")),
            Ok(positions) => positions,
        };
        
        let mut basket =
            BASKETS.load(deps.storage, basket_id.clone().unwrap().clone().to_string())?;

        let mut user_positions: Vec<PositionResponse> = vec![];
        
        let _iter: () = positions.into_iter().take(limit).map(|mut position| {
            
            let (borrow, max, _value, _prices) = match get_avg_LTV_imut(
                deps.storage,
                env.clone(),
                deps.querier,
                position.clone().collateral_assets,
                config.clone(),
            ) {
                Ok((borrow, max, value, prices)) => (borrow, max, value, prices),
                Err(err) => {
                    error = Some(err);
                    (Decimal::zero(), Decimal::zero(), Decimal::zero(), vec![])
                }
            };

            match accrue_imut(
                deps.storage,
                deps.querier,
                env.clone(),
                &mut position,
                &mut basket,
            ) {
                Ok(()) => {}
                Err(err) => error = Some(err),
            };

            let cAsset_ratios = match get_cAsset_ratios_imut(
                deps.storage,
                env.clone(),
                deps.querier,
                position.clone().collateral_assets,
                config.clone(),
            ) {
                Ok(ratios) => ratios,
                Err(err) => {
                    error = Some(err);
                    vec![]
                }
            };

            if error.is_none() {
                user_positions.push(PositionResponse {
                    position_id: position.position_id,
                    collateral_assets: position.collateral_assets,
                    cAsset_ratios,
                    credit_amount: position.credit_amount,
                    basket_id: position.basket_id,
                    avg_borrow_LTV: borrow,
                    avg_max_LTV: max,
                })
            }
        }).collect();

        if error.is_some() {
            return Err(error.unwrap());
        }
        Ok(user_positions)
    } else {
        //If no basket_id, return all basket positions
        //Can use config.current basket_id-1 as the limiter to check all baskets

        let config = CONFIG.load(deps.storage)?;
        let mut response: Vec<PositionResponse> = Vec::new();
        let mut error: Option<StdError> = None;

        //Uint128 to int
        let range: i32 = config.current_basket_id.to_string().parse().unwrap();

        for basket_id in 1..range {
            let mut basket = BASKETS.load(deps.storage, basket_id.clone().to_string())?;

            if let Ok( positions ) = POSITIONS.load(deps.storage, (basket_id.to_string(), user.clone())) {
               
                    for mut position in positions {
                        let (borrow, max, _value, _prices) = get_avg_LTV_imut(
                            deps.storage,
                            env.clone(),
                            deps.querier,
                            position.clone().collateral_assets,
                            config.clone(),
                        )?;

                        match accrue_imut(
                            deps.storage,
                            deps.querier,
                            env.clone(),
                            &mut position,
                            &mut basket,
                        ) {
                            Ok(()) => {}
                            Err(err) => error = Some(err),
                        };

                        let cAsset_ratios = match get_cAsset_ratios_imut(
                            deps.storage,
                            env.clone(),
                            deps.querier,
                            position.clone().collateral_assets,
                            config.clone(),
                        ) {
                            Ok(ratios) => ratios,
                            Err(err) => {
                                error = Some(err);
                                vec![]
                            }
                        };

                        response.push(PositionResponse {
                            position_id: position.position_id,
                            collateral_assets: position.collateral_assets,
                            cAsset_ratios,
                            credit_amount: position.credit_amount,
                            basket_id: position.basket_id,
                            avg_borrow_LTV: borrow,
                            avg_max_LTV: max,
                        });
                    }               
            }
        }
        if error.is_some() {
            return Err(error.unwrap());
        }
        Ok(response)
    }
}

pub fn query_basket(deps: Deps, basket_id: Uint128) -> StdResult<BasketResponse> {
    let basket_res = match BASKETS.load(deps.storage, basket_id.to_string()) {
        Ok(basket) => BasketResponse {
            owner: basket.owner.to_string(),
            basket_id: basket.basket_id.to_string(),
            current_position_id: basket.current_position_id.to_string(),
            collateral_types: basket.collateral_types,
            credit_asset: basket.credit_asset,
            credit_price: basket.credit_price,
            liq_queue: basket
                .liq_queue
                .unwrap_or(Addr::unchecked("None"))
                .to_string(),
            collateral_supply_caps: basket.collateral_supply_caps,
            multi_asset_supply_caps: basket.multi_asset_supply_caps,
            base_interest_rate: basket.base_interest_rate,
            liquidity_multiplier: basket.liquidity_multiplier,
            desired_debt_cap_util: basket.desired_debt_cap_util,
            pending_revenue: basket.pending_revenue,
            negative_rates: basket.negative_rates,
            cpc_margin_of_error: basket.cpc_margin_of_error,
            frozen: basket.frozen,
            rev_to_stakers: basket.rev_to_stakers,
        },
        Err(_) => return Err(StdError::generic_err("Invalid basket_id")),
    };

    Ok(basket_res)
}

pub fn query_baskets(
    deps: Deps,
    start_after: Option<Uint128>,
    limit: Option<u32>,
) -> StdResult<Vec<BasketResponse>> {
    let limit = limit.unwrap_or(MAX_LIMIT) as usize;

    let start: Option<Bound<String>> = if let Some(_start) = start_after {
        match BASKETS.load(deps.storage, start_after.unwrap().to_string()) {
            Ok(_x) => Some(Bound::exclusive(start_after.unwrap().to_string())),
            Err(_) => None,
        }
    } else {
        None
    };

    BASKETS
        .range(deps.storage, start, None, Order::Ascending)
        .take(limit)
        .map(|item| {
            let (k, basket) = item?;

            Ok(BasketResponse {
                owner: basket.owner.to_string(),
                basket_id: k,
                current_position_id: basket.current_position_id.to_string(),
                collateral_types: basket.collateral_types,
                credit_asset: basket.credit_asset,
                credit_price: basket.credit_price,
                liq_queue: basket
                    .liq_queue
                    .unwrap_or(Addr::unchecked("None"))
                    .to_string(),
                collateral_supply_caps: basket.collateral_supply_caps,
                multi_asset_supply_caps: basket.multi_asset_supply_caps,
                base_interest_rate: basket.base_interest_rate,
                liquidity_multiplier: basket.liquidity_multiplier,
                desired_debt_cap_util: basket.desired_debt_cap_util,
                pending_revenue: basket.pending_revenue,
                negative_rates: basket.negative_rates,
                cpc_margin_of_error: basket.cpc_margin_of_error,
                frozen: basket.frozen,
                rev_to_stakers: basket.rev_to_stakers,
            })
        })
        .collect()
}

pub fn query_basket_positions(
    deps: Deps,
    basket_id: Uint128,
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
        .prefix(basket_id.to_string())
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
pub fn query_basket_debt_caps(
    deps: Deps,
    env: Env,
    basket_id: Uint128,
) -> StdResult<DebtCapResponse> {
    
    let basket: Basket = BASKETS.load(deps.storage, basket_id.to_string())?;

    let asset_caps = get_basket_debt_caps_imut(deps.storage, deps.querier, env, basket.clone())?;

    let mut res = String::from("");
    //Append caps and asset_infos
    for (index, cap) in basket.collateral_supply_caps.iter().enumerate() {
        
        res += &format!(
            "{}: {}/{}, ",
            cap.asset_info, cap.debt_total, asset_caps[index]
        );
        
    }

    Ok(DebtCapResponse { caps: res })
}

pub fn query_bad_debt(deps: Deps, basket_id: Uint128) -> StdResult<BadDebtResponse> {
    let mut res = BadDebtResponse {
        has_bad_debt: vec![],
    };

    let _iter: () = POSITIONS
        .prefix(basket_id.to_string())
        .range(deps.storage, None, None, Order::Ascending)
        .map(|item| {
            let (addr, positions) = item.unwrap();

            for position in positions {
                //We do a lazy check for bad debt by checking if there is debt without any assets left in the position
                //This is allowed bc any calls here will be after a liquidation where the sell wall would've sold all it could to cover debts
                let total_assets: Uint128 = position
                    .collateral_assets
                    .iter()
                    .map(|asset| asset.asset.amount)
                    .collect::<Vec<Uint128>>()
                    .iter()
                    .sum();

                //If there are no assets and outstanding debt
                if total_assets.is_zero() && !position.credit_amount.is_zero() {
                    res.has_bad_debt.push((
                        PositionUserInfo {
                            basket_id,
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

pub fn query_basket_insolvency(
    deps: Deps,
    env: Env,
    basket_id: Uint128,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<InsolvencyResponse> {
    let config: Config = CONFIG.load(deps.storage)?;

    let mut basket: Basket = BASKETS.load(deps.storage, basket_id.to_string())?;

    let mut res = InsolvencyResponse {
        insolvent_positions: vec![],
    };
    let mut error: Option<StdError> = None;

    let limit = limit.unwrap_or(MAX_LIMIT) as usize;

    let start = if let Some(start) = start_after {
        let start_after_addr = deps.api.addr_validate(&start)?;
        Some(Bound::exclusive(start_after_addr))
    } else {
        None
    };

    let _iter: () = POSITIONS
        .prefix(basket_id.to_string())
        .range(deps.storage, start, None, Order::Ascending)
        .take(limit)
        .map(|item| {
            let (addr, positions) = item.unwrap();

            for mut position in positions {
                match accrue_imut(
                    deps.storage,
                    deps.querier,
                    env.clone(),
                    &mut position,
                    &mut basket,
                ) {
                    Ok(()) => {}
                    Err(err) => error = Some(err),
                };

                let (insolvent, current_LTV, available_fee) = match insolvency_check_imut(
                    deps.storage,
                    env.clone(),
                    deps.querier,
                    position.collateral_assets,
                    Decimal::from_ratio(position.credit_amount, Uint128::new(1u128)),
                    basket.clone().credit_price,
                    false,
                    config.clone(),
                ) {
                    Ok((insolvent, current_LTV, available_fee)) => {
                        (insolvent, current_LTV, available_fee)
                    }
                    Err(err) => {
                        error = Some(err);
                        (false, Decimal::zero(), Uint128::zero())
                    }
                };

                if insolvent {
                    res.insolvent_positions.push(InsolventPosition {
                        insolvent,
                        position_info: UserInfo {
                            basket_id: basket_id.clone(),
                            position_id: position.position_id,
                            position_owner: addr.to_string(),
                        },
                        current_LTV,
                        available_fee,
                    });
                }
            }
        }).collect();

    if error.is_some() {
        return Err(error.unwrap());
    } else {
        Ok(res)
    }
}

pub fn query_position_insolvency(
    deps: Deps,
    env: Env,
    basket_id: Uint128,
    position_id: Uint128,
    position_owner: String,
) -> StdResult<InsolvencyResponse> {
    let config: Config = CONFIG.load(deps.storage)?;

    let valid_owner_addr = deps.api.addr_validate(&position_owner)?;

    let mut basket: Basket = BASKETS.load(deps.storage, basket_id.to_string())?;

    let positions: Vec<Position> =
        POSITIONS.load(deps.storage, (basket_id.to_string(), valid_owner_addr))?;

    let mut target_position = match positions.into_iter().find(|x| x.position_id == position_id) {
        Some(position) => position,
        None => {
            return Err(StdError::NotFound {
                kind: "Position".to_string(),
            })
        }
    };

    accrue_imut(
        deps.storage,
        deps.querier,
        env.clone(),
        &mut target_position,
        &mut basket,
    )?;

    ///
    let mut res = InsolvencyResponse {
        insolvent_positions: vec![],
    };

    let (insolvent, current_LTV, available_fee) = insolvency_check_imut(
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
            basket_id: basket_id.clone(),
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
    basket_id: Uint128,
) -> StdResult<CollateralInterestResponse> {
    let mut basket = BASKETS.load(deps.storage, basket_id.to_string())?;

    let rates = get_interest_rates_imut(deps.storage, deps.querier, env.clone(), &mut basket)?;

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
        let credit_TWAP_price = get_asset_values_imut(
            deps.storage,
            env.clone(),
            deps.querier,
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

        //Don't accrue interest if price is within the margin of error
        if price_difference <= basket.clone().cpc_margin_of_error {
            price_difference = Decimal::zero();
        }

        let new_rates: Vec<(AssetInfo, Decimal)> = rates
            .into_iter()
            .map(|mut rate| {
                //Accrue a year of repayment rate to interest rates
                if negative_rate {
                    rate.1 = decimal_multiplication(
                        rate.1,
                        decimal_subtraction(Decimal::one(), price_difference),
                    );
                } else {
                    rate.1 = decimal_multiplication(rate.1, (Decimal::one() + price_difference));
                }

                rate
            })
            .collect::<Vec<(AssetInfo, Decimal)>>();

        Ok(CollateralInterestResponse { rates: new_rates })
    } else {
        Ok(CollateralInterestResponse { rates })
    }
}


pub fn query_basket_credit_interest(
    deps: Deps,
    env: Env,
    basket_id: Uint128,
) -> StdResult<InterestResponse> {
    let config = CONFIG.load(deps.storage)?;

    let basket = BASKETS.load(deps.storage, basket_id.to_string())?;

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

        let credit_TWAP_price = get_asset_values_imut(
            deps.storage,
            env,
            deps.querier,
            vec![credit_asset],
            config.clone(),
            Some(basket_id),
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

pub fn get_cAsset_ratios_imut(
    storage: &dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    collateral_assets: Vec<cAsset>,
    config: Config,
) -> StdResult<Vec<Decimal>> {
    let (cAsset_values, _cAsset_prices) = get_asset_values_imut(
        storage,
        env,
        querier,
        collateral_assets.clone(),
        config,
        None,
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

    Ok(cAsset_ratios)
}

fn query_price_imut(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: Config,
    asset_info: AssetInfo,
    basket_id: Option<Uint128>,
) -> StdResult<Decimal> {
    //Set timeframe
    let mut twap_timeframe: u64 = config.collateral_twap_timeframe;

    if let Some(basket_id) = basket_id {
        let basket = BASKETS.load(storage, basket_id.to_string())?;
        //if AssetInfo is the basket.credit_asset
        if asset_info.equal(&basket.credit_asset.info) {
            twap_timeframe = config.credit_twap_timeframe;
        }
    }

    //Query Price
    let price = match querier.query::<PriceResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().oracle_contract.unwrap().to_string(),
        msg: to_binary(&OracleQueryMsg::Price {
            asset_info: asset_info.clone(),
            twap_timeframe,
            basket_id,
        })?,
    })) {
        Ok(res) => {
            //
            res.avg_price
        }
        Err(_err) => {
            //If the query errors, try and use a stored price
            let stored_price: StoredPrice = read_price(storage, &asset_info)?;

            let time_elapsed: u64 = env.block.time.seconds() - stored_price.last_time_updated;
            //Use the stored price if within the oracle_time_limit
            if time_elapsed <= config.oracle_time_limit {
                stored_price.price
            } else {
                return Err(StdError::GenericErr {
                    msg: String::from("Oracle price invalid"),
                });
            }
        }
    };

    Ok(price)
}

//Get Asset values / query oracle
pub fn get_asset_values_imut(
    storage: &dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    assets: Vec<cAsset>,
    config: Config,
    basket_id: Option<Uint128>,
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
                    let price = query_price_imut(
                        storage,
                        querier,
                        env.clone(),
                        config.clone(),
                        pool_asset.info,
                        basket_id,
                    )?;
                    //Append price
                    asset_prices.push(price);
                }

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
                        let exponent_difference =
                            pool_info.clone().asset_infos[i].decimals - (6u64);
                        let asset_amount = asset_share.amount
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

                    decimal_division(cAsset_value, share_amount)
                };

                //Push to price and value list
                cAsset_prices.push(cAsset_price);
                cAsset_values.push(cAsset_value);
            } else {
                let price = query_price_imut(
                    storage,
                    querier,
                    env.clone(),
                    config.clone(),
                    cAsset.clone().asset.info,
                    None,
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

fn get_avg_LTV_imut(
    storage: &dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    collateral_assets: Vec<cAsset>,
    config: Config,
) -> StdResult<(Decimal, Decimal, Decimal, Vec<Decimal>)> {
    let (cAsset_values, cAsset_prices) = get_asset_values_imut(
        storage,
        env,
        querier,
        collateral_assets.clone(),
        config,
        None,
    )?;

    let total_value: Decimal = cAsset_values.iter().sum();

    //getting each cAsset's % of total value
    let mut cAsset_ratios: Vec<Decimal> = vec![];
    for cAsset in cAsset_values {
        cAsset_ratios.push(decimal_division(cAsset, total_value));
    }

    //converting % of value to avg_LTV by multiplying collateral LTV by % of total value
    let mut avg_max_LTV: Decimal = Decimal::zero();
    let mut avg_borrow_LTV: Decimal = Decimal::zero();

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

pub fn insolvency_check_imut(
    //Returns true if insolvent, current_LTV and available fee to the caller if insolvent
    storage: &dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    collateral_assets: Vec<cAsset>,
    credit_amount: Decimal,
    credit_price: Decimal,
    max_borrow: bool, //Toggle for either over max_borrow or over max_LTV (liquidatable)
    config: Config,
) -> StdResult<(bool, Decimal, Uint128)> { //insolvent, current_LTV, available_fee

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

    let avg_LTVs: (Decimal, Decimal, Decimal, Vec<Decimal>) =
        get_avg_LTV_imut(storage, env, querier, collateral_assets, config)?;

    let asset_values: Decimal = avg_LTVs.2; //pulls total_asset_value

    let check: bool;
    let current_LTV = decimal_division(
        decimal_multiplication(credit_amount, credit_price),
        asset_values,
    );

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
        let fee = decimal_subtraction(current_LTV, avg_LTVs.1);
        //current_LTV - borrow_LTV
        let liq_range = decimal_subtraction(current_LTV, avg_LTVs.0);
        //Fee value = repay_amount * fee
        decimal_multiplication(
            decimal_multiplication( 
                decimal_division( liq_range, current_LTV), 
                decimal_multiplication(credit_amount, credit_price)), 
            fee) 
        * Uint128::new(1)
        
    } else {
        Uint128::zero()
    };

    Ok((check, current_LTV, available_fee))
}
