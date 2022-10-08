use cosmwasm_std::{
    to_binary, Addr, Decimal, Deps, Env, Order, QuerierWrapper, QueryRequest, StdError, StdResult,
    Storage, Uint128, WasmQuery,
};

use cw_storage_plus::Bound;

use membrane::oracle::{PriceResponse, QueryMsg as OracleQueryMsg};
use membrane::osmosis_proxy::{QueryMsg as OsmoQueryMsg, TokenInfoResponse};
use membrane::positions::{
    Config, BadDebtResponse, BasketResponse, CollateralInterestResponse, DebtCapResponse,
    InsolvencyResponse, InterestResponse, PositionResponse, PositionsResponse, PropResponse,
};

use membrane::types::{
    cAsset, Asset, AssetInfo, Basket, InsolventPosition, Position, PositionUserInfo,
    StoredPrice, SupplyCap, UserInfo,
};
use membrane::math::{decimal_division, decimal_multiplication, decimal_subtraction};

use crate::positions::{
    accumulate_interest, get_LP_pool_cAssets, get_asset_liquidity, get_stability_pool_liquidity,
    SECONDS_PER_YEAR,
};

use osmo_bindings::PoolStateResponse;

use crate::state::CREDIT_MULTI;
use crate::{
    positions::read_price,
    state::{BASKETS, CONFIG, POSITIONS, REPAY},
};

const MAX_LIMIT: u32 = 31;

pub fn query_prop(deps: Deps) -> StdResult<PropResponse> {
    match REPAY.load(deps.storage) {
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
            base_interest_rate: basket.base_interest_rate,
            liquidity_multiplier: basket.liquidity_multiplier,
            desired_debt_cap_util: basket.desired_debt_cap_util,
            pending_revenue: basket.pending_revenue,
            negative_rates: basket.negative_rates,
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
                base_interest_rate: basket.base_interest_rate,
                liquidity_multiplier: basket.liquidity_multiplier,
                desired_debt_cap_util: basket.desired_debt_cap_util,
                pending_revenue: basket.pending_revenue,
                negative_rates: basket.negative_rates,
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

fn get_credit_asset_multiplier_imut(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: Config,
    basket: Basket,
) -> StdResult<Decimal> {
    //Find Baskets with similar credit_asset
    let mut baskets: Vec<Basket> = vec![];

    //Has to be done ugly due to an immutable borrow
    //Uint128 to int
    let range: i32 = config.current_basket_id.to_string().parse().unwrap();

    for basket_id in 1..range {
        let stored_basket = BASKETS.load(storage, basket_id.to_string())?;

        if stored_basket
            .credit_asset
            .info
            .equal(&basket.credit_asset.info)
        {
            baskets.push(stored_basket);
        }
    }

    //Calc collateral_type totals
    let mut collateral_totals: Vec<Asset> = vec![];

    for basket in baskets {
        //Find collateral's corresponding total in list
        for collateral in basket.collateral_supply_caps {
            if !collateral.lp {
                if let Some((index, _total)) = collateral_totals
                    .clone()
                    .into_iter()
                    .enumerate()
                    .find(|(_i, asset)| asset.info.equal(&collateral.asset_info))
                {
                    //Add to collateral total
                    collateral_totals[index].amount += collateral.current_supply;
                } else {
                    //Add collateral type to list
                    collateral_totals.push(Asset {
                        info: collateral.asset_info,
                        amount: collateral.current_supply,
                    });
                }
            }
        }
    }

    //Get collateral_ratios
    let temp_cAssets: Vec<cAsset> = collateral_totals
        .clone()
        .into_iter()
        .map(|asset| cAsset {
            asset,
            max_borrow_LTV: Decimal::zero(),
            max_LTV: Decimal::zero(),
            pool_info: None,
        })
        .collect::<Vec<cAsset>>();
    let total_collateral_ratios =
        get_cAsset_ratios_imut(storage, env, querier, temp_cAssets, config)?;

    //Find Basket parameter's ratio of each collateral
    let mut basket_collateral_ratios: Vec<Decimal> = vec![];
    for (i, collateral) in basket
        .clone()
        .collateral_supply_caps
        .into_iter()
        .enumerate()
    {
        if !collateral.lp {
            //Push collateral_ratio
            if collateral_totals[i].amount.is_zero() {
                basket_collateral_ratios.push(Decimal::zero());
            } else {
                basket_collateral_ratios.push(decimal_division(
                    Decimal::from_ratio(collateral.current_supply, Uint128::new(1u128)),
                    Decimal::from_ratio(collateral_totals[i].amount, Uint128::new(1u128)),
                ));
            }
        }
    }

    //Find Basket parameter's ratio of total collateral
    let basket_tvl_ratio: Decimal = basket_collateral_ratios
        .clone()
        .into_iter()
        .enumerate()
        .map(|(i, basket_ratio)| {
            //Multiply the two lists of ratios
            decimal_multiplication(basket_ratio, total_collateral_ratios[i])
        })
        .collect::<Vec<Decimal>>()
        .into_iter()
        .sum();

    //Get credit_asset's liquidity multiplier
    let credit_asset_liquidity_multiplier =
        CREDIT_MULTI.load(storage, basket.clone().credit_asset.info.to_string())?;

    //Return ratio * credit_asset's multiplier
    Ok(decimal_multiplication(
        basket_tvl_ratio,
        credit_asset_liquidity_multiplier,
    ))
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
        if price_difference > config.clone().cpc_margin_of_error {
            price_difference =
                decimal_subtraction(price_difference, config.clone().cpc_margin_of_error);
        } else {
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

fn accrue_imut(
    storage: &dyn Storage,
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

    ////Controller barriers to reduce risk of manipulation
    //Liquidity above 2M
    //At least 3% of total supply as liquidity
    let liquidity = get_asset_liquidity(querier, config.clone(), basket.clone().credit_asset.info)?;
    //Now get % of supply
    if config.clone().osmosis_proxy.is_some() {
        let current_supply = querier
            .query::<TokenInfoResponse>(&QueryRequest::Wasm(
                (WasmQuery::Smart {
                    contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
                    msg: to_binary(&OsmoQueryMsg::GetTokenInfo {
                        denom: basket.clone().credit_asset.info.to_string(),
                    })?,
                }),
            ))?
            .current_supply;

        let liquidity_ratio = decimal_division(
            Decimal::from_ratio(liquidity, Uint128::new(1u128)),
            Decimal::from_ratio(current_supply, Uint128::new(1u128)),
        );
        if liquidity_ratio < Decimal::percent(3) {
            //Set time_elapsed to 0 to skip accrual
            time_elapsed = 0u64;
        }
    }
    if liquidity < Uint128::new(2_000_000_000_000u128) {
        //Set time_elapsed to 0 to skip accrual
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
        };
        let credit_TWAP_price = get_asset_values_imut(
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

        //Don't accrue interest if price is within the margin of error
        if price_difference > config.clone().cpc_margin_of_error {
            price_difference =
                decimal_subtraction(price_difference, config.clone().cpc_margin_of_error);

            //Calculate rate of change
            let mut applied_rate: Decimal;
            applied_rate = price_difference.checked_mul(Decimal::from_ratio(
                Uint128::from(time_elapsed),
                Uint128::from(SECONDS_PER_YEAR),
            ))?;

            //If a positive rate we add 1,
            //If a negative rate we add 1 and subtract 2xprice difference
            //---
            //Add 1 to make the value 1.__
            applied_rate += Decimal::one();
            if negative_rate {
                //Subtract price difference to make it .9___
                applied_rate = decimal_subtraction(Decimal::one(), price_difference);
            }

            let mut new_price = basket.credit_price;
            //Negative repayment interest needs to be enabled by the basket
            if negative_rate && basket.negative_rates || !negative_rate {
                new_price = decimal_multiplication(basket.credit_price, applied_rate);
            } 

            basket.credit_price = new_price;
        } else {
            price_difference = Decimal::zero();
        }
    }

    //Calc time-elapsed
    let time_elapsed = env.block.time.seconds() - position.last_accrued;
    //Update last accrued time
    position.last_accrued = env.block.time.seconds();

    //Calc avg_rate for the position
    let mut avg_rate = get_position_avg_rate_imut(
        storage,
        querier,
        env.clone(),
        basket,
        position.clone().collateral_assets,
    )?;

    //Accrue a year of repayment rate to interest rates
    if negative_rate {
        avg_rate = decimal_multiplication(
            avg_rate,
            decimal_subtraction(Decimal::one(), price_difference),
        );
    } else {
        avg_rate = decimal_multiplication(avg_rate, (Decimal::one() + price_difference));
    }

    //Calc accrued interested
    let accrued_interest = accumulate_interest(position.credit_amount, avg_rate, time_elapsed)?;

    //Add accrued interest to the position's debt
    position.credit_amount += accrued_interest * Uint128::new(1u128);

    Ok(())
}

fn get_position_avg_rate_imut(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    basket: &mut Basket,
    position_assets: Vec<cAsset>,
) -> StdResult<Decimal> {
    let config = CONFIG.load(storage)?;

    let new_assets = get_LP_pool_cAssets(querier, config.clone(), basket.clone(), position_assets)?;

    let ratios = get_cAsset_ratios_imut(storage, env.clone(), querier, new_assets.clone(), config)?;

    let interest_rates = get_interest_rates_imut(storage, querier, env, basket)?;

    let mut avg_rate = Decimal::zero();

    for (i, cAsset) in new_assets.clone().iter().enumerate() {
        //Match asset and rate
        if let Some(rate) = interest_rates
            .clone()
            .into_iter()
            .find(|rate| rate.0.equal(&cAsset.asset.info))
        {
            avg_rate += decimal_multiplication(ratios[i], rate.1);
        }
    }

    Ok(avg_rate)
}

fn get_interest_rates_imut(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    basket: &mut Basket,
) -> StdResult<Vec<(AssetInfo, Decimal)>> {
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

    let debt_caps = match get_basket_debt_caps_imut(storage, querier, env.clone(), basket.clone()) {
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
        })
        .collect::<Vec<cAsset>>();

    let no_lp_basket: Vec<cAsset> =
        get_LP_pool_cAssets(querier, config.clone(), basket.clone(), caps_to_cAssets)?;

    //Get basket cAsset ratios
    let basket_ratios: Vec<Decimal> =
        get_cAsset_ratios_imut(storage, env.clone(), querier, no_lp_basket, config.clone())?;

    let no_lp_caps = basket
        .collateral_supply_caps
        .clone()
        .into_iter()
        .filter(|cap| !cap.lp)
        .collect::<Vec<SupplyCap>>();

    for (i, cap) in no_lp_caps
        .clone()
        .into_iter()
        .filter(|cap| !cap.lp)
        .collect::<Vec<SupplyCap>>()
        .iter()
        .enumerate()
    {
        //If there is 0 of an Asset then it's cap is 0 but its proportion is 100%
        if debt_caps[i].is_zero() || cap.supply_cap_ratio.is_zero() {
            debt_proportions.push(Decimal::percent(100));
            supply_proportions.push(Decimal::percent(100));
        } else {
            debt_proportions.push(Decimal::from_ratio(cap.debt_total, debt_caps[i]));
            supply_proportions.push(decimal_division(basket_ratios[i], cap.supply_cap_ratio))
        }
    }

    //Gets pro-rata rate and uses multiplier if above desired utilization
    let mut two_slope_pro_rata_rates = vec![];
    for (i, _rate) in rates.iter().enumerate() {
        //If proportions are is above desired utilization, the rates start multiplying
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
                two_slope_pro_rata_rates.push((
                    no_lp_caps[i].clone().asset_info,
                    decimal_multiplication(
                        decimal_multiplication(rates[i], debt_proportions[i]),
                        multiplier,
                    ),
                ));
            } else {
                //Slope 1
                two_slope_pro_rata_rates.push((
                    no_lp_caps[i].clone().asset_info,
                    decimal_multiplication(rates[i], debt_proportions[i]),
                ));
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
                two_slope_pro_rata_rates.push((
                    no_lp_caps[i].clone().asset_info,
                    decimal_multiplication(
                        decimal_multiplication(rates[i], supply_proportions[i]),
                        multiplier,
                    ),
                ));
            } else {
                //Slope 1
                two_slope_pro_rata_rates.push((
                    no_lp_caps[i].clone().asset_info,
                    decimal_multiplication(rates[i], supply_proportions[i]),
                ));
            }
        }
    }

    Ok(two_slope_pro_rata_rates)
}

fn get_basket_debt_caps_imut(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    //These are Basket specific fields
    basket: Basket,
) -> StdResult<Vec<Uint128>> {
    let config: Config = CONFIG.load(storage)?;

    //Map supply caps to cAssets to get new ratios
    //The functions need Asset and Pool Info to calc value
    //Bc our LPs are aggregated w/ their paired assets, we don't need Pool Info
    let temp_cAssets: Vec<cAsset> = basket
        .clone()
        .collateral_supply_caps
        .into_iter()
        .map(|cap| {
            if cap.lp {
                //We skip LPs bc we don't want to double count their assets
                cAsset {
                    asset: Asset {
                        info: cap.asset_info,
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::zero(),
                    max_LTV: Decimal::zero(),
                    pool_info: None,
                }
            } else {
                cAsset {
                    asset: Asset {
                        info: cap.asset_info,
                        amount: cap.current_supply,
                    },
                    max_borrow_LTV: Decimal::zero(),
                    max_LTV: Decimal::zero(),
                    pool_info: None,
                }
            }
        })
        .collect::<Vec<cAsset>>();

    //Get the Basket's asset ratios
    let cAsset_ratios = get_cAsset_ratios_imut(
        storage,
        env.clone(),
        querier,
        temp_cAssets.clone(),
        config.clone(),
    )?;

    //Get credit_asset's liquidity_multiplier
    let credit_asset_multiplier = get_credit_asset_multiplier_imut(
        storage,
        querier,
        env.clone(),
        config.clone(),
        basket.clone(),
    )?;

    //Get the base debt cap
    let mut debt_cap =
        get_asset_liquidity(querier, config.clone(), basket.clone().credit_asset.info)?
            * credit_asset_multiplier;

    //Add SP liquidity to the cap
    debt_cap +=
        get_stability_pool_liquidity(querier, config.clone(), basket.clone().credit_asset.info)?;


    //If debt cap is less than the minimum, set it to the minimum
    if debt_cap < (config.base_debt_cap_multiplier * config.debt_minimum) {
        debt_cap = (config.base_debt_cap_multiplier * config.debt_minimum);
    }

    let mut per_asset_debt_caps = vec![];

    for (i, cAsset) in cAsset_ratios.clone().into_iter().enumerate() {
        if !basket.clone().collateral_supply_caps[i].lp {
            // If supply cap is 0, then debt cap is 0
            if basket.clone().collateral_supply_caps != vec![] {
                if basket.clone().collateral_supply_caps[i]
                    .supply_cap_ratio
                    .is_zero()
                {
                    per_asset_debt_caps.push(Uint128::zero());
                } else {
                    per_asset_debt_caps.push(cAsset * debt_cap);
                }
            } else {
                per_asset_debt_caps.push(cAsset * debt_cap);
            }
        }
    }

    Ok(per_asset_debt_caps)
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
        if price_difference > config.clone().cpc_margin_of_error {
            price_difference =
                decimal_subtraction(price_difference, config.clone().cpc_margin_of_error);
        } else {
            price_difference = Decimal::zero();
        }
    }

    Ok(InterestResponse {
        credit_interest: price_difference,
        negative_rate,
    })
}

////Helper/////

fn get_cAsset_ratios_imut(
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
            let stored_price: StoredPrice = match read_price(storage, &asset_info) {
                Ok(info) => info,
                Err(_) => {
                    //Set time to fail in the next check. We don't want the error to stop from querying though
                    StoredPrice {
                        price: Decimal::zero(),
                        last_time_updated: env
                            .block
                            .time
                            .plus_seconds(config.oracle_time_limit + 1u64)
                            .seconds(),
                    }
                }
            };

            let time_elapsed: Option<u64> = env
                .block
                .time
                .seconds()
                .checked_sub(stored_price.last_time_updated);
            //If its None then the subtraction was negative meaning the initial read_price() errored
            if time_elapsed.is_some() {
                if time_elapsed.unwrap() <= config.oracle_time_limit {
                    stored_price.price
                } else {
                    return Err(StdError::GenericErr {
                        msg: String::from("Oracle price invalid"),
                    });
                }
            }else{
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
        match max_borrow {
            true => {
                //Checks max_borrow
                (current_LTV - avg_LTVs.0) * Uint128::new(1)
            }
            false => {
                //Checks max_LTV
                (current_LTV - avg_LTVs.1) * Uint128::new(1)
            }
        }
    } else {
        Uint128::zero()
    };

    Ok((check, current_LTV, available_fee))
}
