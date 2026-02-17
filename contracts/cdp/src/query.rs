use std::cmp::Ordering;
use std::str::FromStr;

use cosmwasm_std::{
    to_binary, Coin, Decimal, Deps, Env, Order, QuerierWrapper, QueryRequest, StdError, StdResult,
    Storage, Uint128, WasmQuery, Addr,
};

use cw_storage_plus::Bound;

use membrane::oracle::{PriceResponse, QueryMsg as OracleQueryMsg};
use membrane::cdp::{
    Config, CollateralInterestResponse, UserIntentResponse,
    InterestResponse, PositionResponse, BasketPositionsResponse, LiquidationStatResponse, HistoricalOraclePricesResponse, HistoricalInterestRatesResponse
};
use membrane::ltv_disco::{QueryMsg as LTVDiscoQueryMsg, AverageLTVsResponse};

use membrane::types::{
    cAsset, Asset, AssetInfo, Basket, DebtCap, Position, StoredPrice, UserInfo
};
use membrane::math::{decimal_division, decimal_multiplication, decimal_subtraction};

use crate::positions::get_amount_from_LTV;
use crate::rates::get_total_debt_from_segments;
use crate::state::{get_target_position, CollateralVolatility, ACTIVE_DEPLOYMENT_VENUES, BASKET, CONFIG, HISTORICAL_ORACLE_PRICES, HISTORICAL_INTEREST_RATES, LIQUIDATION_STATS, POSITIONS, RATES, STORED_PRICES, USER_INTENTS, VOLATILITY, LTV_HISTORY, LTV_UPDATE_TRACKERS, update_historical_oracle};
use crate::ltv_updater::cap_ltv_values;

const MAX_LIMIT: u32 = 31;
pub const VOLATILITY_LIST_LIMIT: u32 = 48;

/// Returns liquidation stats with optional pagination
pub fn query_liquidation_stats(
    deps: Deps,
    start_after: Option<u64>,
    limit: Option<u32>,
) -> StdResult<Vec<LiquidationStatResponse>> {
    let mut stats = LIQUIDATION_STATS.load(deps.storage).unwrap_or_else(|_| vec![]);
    println!("stats: {:?}", stats);

    if let Some(sa) = start_after {
        stats = stats.into_iter().filter(|s| s.block_time > sa).collect();
    }

    let take = limit.unwrap_or(50) as usize;

    if stats.len() > take {
        stats.truncate(take);
    }
    // println!("stats: {:?}", stats);

    let resp: Vec<LiquidationStatResponse> = stats
        .into_iter()
        .map(|s| LiquidationStatResponse {
            block_time: s.block_time,
            position_id: s.position_id,
            collateral_assets: s.collateral_assets,
            amount_liquidated: s.amount_liquidated,
        })
        .collect();

    Ok(resp)
}


/// Returns active deployment venues with optional pagination
pub fn query_active_deployment_venues(
    deps: Deps,
    venue: Option<String>,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<Vec<String>> {
    let mut venues = ACTIVE_DEPLOYMENT_VENUES.load(deps.storage)?;
    let limit = limit.unwrap_or(MAX_LIMIT) as usize;

    // Sort for deterministic ordering
    venues.sort();

    if let Some(target) = venue {
        // Optional single-venue output: return the venue if it's active, else empty
        if venues.iter().any(|v| v == &target) {
            return Ok(vec![target]);
        } else {
            return Ok(vec![]);
        }
    }

    // Apply start_after filtering if provided, after sorting for deterministic order
    let mut filtered: Vec<String> = if let Some(sa) = start_after {
        venues.into_iter().filter(|v| v > &sa).collect()
    } else {
        venues
    };

    // Apply limit in all cases
    if filtered.len() > limit {
        filtered.truncate(limit);
    }

    Ok(filtered)
}
pub fn query_user_intent_state(
    deps: Deps,
    _env: Env,
    start_after: Option<String>,
    limit: Option<u32>,
    // Users
    user: Vec<String>,
)-> StdResult<Vec<UserIntentResponse>> {
    let limit = limit.unwrap_or(MAX_LIMIT) as usize;

    let start = if let Some(start) = start_after {
        Some(Bound::exclusive(start))
    } else {
        None
    };

    if user.len() > 0 {
        return user
            .into_iter()
            .map(|user| {
                let intent = USER_INTENTS.load(deps.storage, user.clone())?;
                Ok(UserIntentResponse {
                    user,
                    intent,
                })
            })
            .collect();
    } else {
        return USER_INTENTS
            .range(deps.storage, start, None, Order::Ascending)
            .take(limit)
            .map(|item| {
                let (k, v) = item?;
                Ok(UserIntentResponse {
                    user: k,
                    intent: v,
                })
            }).collect();
    }
}

/// Returns Positions in a Basket
pub fn query_basket_positions(
    deps: Deps,
    env: Env,
    start_after: Option<String>,
    limit: Option<u32>,
    // Single position
    user_info: Option<UserInfo>,
    // Single user
    user: Option<String>,
) -> StdResult<Vec<BasketPositionsResponse>> {
    /////Check single user and single position first/////
    /// User, default limit is 10 anyway
    if let Some(user) = user {
        
        let user = deps.api.addr_validate(&user)?;

        let positions: Vec<Position> = match POSITIONS.load(deps.storage,user.clone()){
            Err(_) => return Err(StdError::generic_err("No User Positions")),
            Ok(positions) => positions,
        };
        
        let mut user_positions: Vec<PositionResponse> = vec![];
        
        for position in positions.into_iter() {
            let credit_amount = crate::rates::get_total_position_debt(&position);
            user_positions.push(PositionResponse {
                position_id: position.position_id,
                collateral_assets: position.collateral_assets,
                cAsset_ratios: vec![],
                credit_amount,
                rate_segments: position.rate_segments,
                avg_borrow_LTV: Decimal::zero(),
                avg_max_LTV: Decimal::zero(),
                deployed_to: position.deployed_to,
                pending_interest: position.pending_interest,
                total_interest_accrued: position.total_interest_accrued,
                peg_rate_segments: position.peg_rate_segments,
            });
        };

        return Ok(vec![BasketPositionsResponse {
            user: user.to_string(),
            positions: user_positions,
        }])
    } else if let Some(user_info) = user_info {
        let user = deps.api.addr_validate(&user_info.position_owner)?;

        let (_i, position) = match get_target_position(deps.storage, user.clone(), user_info.position_id){
            Ok(position) => position,
            Err(err) => return Err(StdError::generic_err(err.to_string())),
        };

        return Ok(vec![BasketPositionsResponse {
            user: user.to_string(),
            positions: vec![PositionResponse {
                position_id: position.position_id,
                collateral_assets: position.clone().collateral_assets,
                cAsset_ratios: vec![],
                credit_amount: crate::rates::get_total_position_debt(&position),
                rate_segments: position.rate_segments,
                avg_borrow_LTV: Decimal::zero(),
                avg_max_LTV: Decimal::zero(),
                deployed_to: position.deployed_to,
                pending_interest: position.pending_interest,
                total_interest_accrued: position.total_interest_accrued,
                peg_rate_segments: position.peg_rate_segments,
            }],
        }])
    }

    //Basket Positions
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
                positions: v
                    .into_iter()
                    .map(|pos| {
                        let credit_amount = crate::rates::get_total_position_debt(&pos);
                        PositionResponse {
                            position_id: pos.position_id,
                            collateral_assets: pos.collateral_assets,
                            cAsset_ratios: vec![],
                            credit_amount,
                            rate_segments: pos.rate_segments,
                            avg_borrow_LTV: Decimal::zero(),
                            avg_max_LTV: Decimal::zero(),
                            deployed_to: pos.deployed_to,
                            pending_interest: pos.pending_interest,
                            total_interest_accrued: pos.total_interest_accrued,
                            peg_rate_segments: pos.peg_rate_segments,
                        }
                    })
                    .collect(),
            })
        })
        .collect()
}


/// Returns cAsset interest rates for the Basket
pub fn query_collateral_rates(
    deps: Deps,
) -> StdResult<CollateralInterestResponse> {
    let rates_store = RATES.load(deps.storage)?;

    let rates = rates_store.lastest_collateral_rates.into_iter().map(|rate| rate.rate).collect::<Vec<Decimal>>();

    Ok(CollateralInterestResponse { rates })
}

/// Returns Basket credit redemption interest rate
pub fn query_basket_credit_interest(
    deps: Deps,
    env: Env,
) -> StdResult<InterestResponse> {
    let config = CONFIG.load(deps.storage)?;

    let basket = BASKET.load(deps.storage)?;

    let rates_store = RATES.load(deps.storage)?;
    let time_elapsed = env.block.time.seconds() - rates_store.credit_last_accrued;
    let mut price_difference = Decimal::zero();
    let mut negative_rate: bool = false;

    if !time_elapsed != 0u64 {
        //Calculate new interest rate
        // Convert CreditAssetBreakdown to Asset for price calculation
        let total_credit_amount = basket.credit_asset.variable_amount
            + basket.credit_asset.one_month_amount
            + basket.credit_asset.three_month_amount
            + basket.credit_asset.six_month_amount;
        
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

        let credit_TWAP_price = match  get_asset_values(
            deps.storage,
            env,
            deps.querier,
            vec![credit_asset],
            config,
            Some(basket.clone()),
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
        if price_difference <= rates_store.cpc_margin_of_error {
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
/// Handles Volatility tracking & saving
pub fn get_cAsset_ratios(
    storage: &mut dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    collateral_assets: Vec<cAsset>,
    config: Config,
    basket: Option<Basket>,
) -> StdResult<(Vec<Decimal>, Vec<PriceResponse>)> {
    let (cAsset_values, cAsset_prices) = get_asset_values(
        storage,
        env.clone(),
        querier,
        collateral_assets.clone(),
        config,
        basket.clone(),
        false
    )?;

    //Loop through collateral assets to save prices & volatility
    for (i, cAsset) in collateral_assets.iter().enumerate() {

        // Update historical oracle for this fresh price
        let price_str = cAsset_prices[i].price.to_string();
        let _ = update_historical_oracle(
            storage,
            env.clone(),
            cAsset.asset.info.to_string(),
            price_str,
        );
        //Check if the querier used the stored price by asserting equality
        //This also skips any equal prices which should be fairly rare anyway
        let stored_price_res = STORED_PRICES.load(storage, cAsset.asset.info.to_string()); 
        if let Ok(ref stored_price) = stored_price_res {
            if stored_price.price.price != cAsset_prices[i].price.clone() {
                
                //Save new Stored price
                STORED_PRICES.save(storage, cAsset.asset.info.to_string(),
                &StoredPrice {
                    price: cAsset_prices[i].clone(),
                    last_time_updated: env.block.time.seconds(),
                })?;

                //Bc the prices aren't equal we need to update the volatility list
                let mut volatility_store = match VOLATILITY.load(storage, cAsset.asset.info.to_string()){
                    Ok(volatility) => volatility,
                    Err(_) => CollateralVolatility {
                        index: Decimal::one(),
                        volatility_list: vec![],
                        raw_volatility_list: vec![],
                    },
                };
                //Get new volatility %
                let new_volatility = decimal_division(cAsset_prices[i].price.abs_diff(stored_price.price.price), stored_price.price.price)?;
                //Get speed of price change by dividing by the time elapsed
                let time_elapsed = env.block.time.seconds() - stored_price.last_time_updated;
                let speed_of_volatility = match decimal_division(new_volatility, Decimal::from_str(&time_elapsed.to_string())?){
                    Ok(speed) => speed,
                    //In case the time elapsed is so large it errors
                    Err(_) => Decimal::zero(),
                };
                //Add speed of volatility to the list (for index calculation)
                volatility_store.volatility_list.push(speed_of_volatility);
                //If the list is at the limit, remove the first element
                if volatility_store.volatility_list.len() > VOLATILITY_LIST_LIMIT as usize {
                    volatility_store.volatility_list.remove(0);
                }
                //Add raw volatility % to the list (for comparative rate calculation)
                volatility_store.raw_volatility_list.push(new_volatility);
                //If the list is at the limit, remove the first element
                if volatility_store.raw_volatility_list.len() > VOLATILITY_LIST_LIMIT as usize {
                    volatility_store.raw_volatility_list.remove(0);
                }
                //Find the current average volatility
                let mut avg_volatility: Decimal = volatility_store.volatility_list.iter().sum();
                avg_volatility = decimal_division(avg_volatility, Decimal::from_str(&volatility_store.volatility_list.len().to_string())?)?;

                //With volatility btwn any time points standardized to the same units (vol/time)
                // we can now calculate the change in index based on the % difference btwn the avg volatility & the newest speed of volatility
                let mut change_in_index = decimal_division(avg_volatility, speed_of_volatility)?;
                //If change is < 1, meaning new speed is less than avg
                //The new change is 1 - new_vol
                if change_in_index < Decimal::one() {
                    change_in_index = match Decimal::one().checked_sub(new_volatility){
                        Ok(diff) => diff,
                        Err(_) => Decimal::percent(1) //Vol is over 100% we set change to 0.01
                    };
                }

                //Index can't hit 0
                volatility_store.index = decimal_multiplication(volatility_store.index, change_in_index)?;
                //Index can't go above 1
                // volatility_store.index = Decimal::one().min(volatility_store.index);
                //Save the new volatility store
                VOLATILITY.save(storage, cAsset.asset.info.to_string(), &volatility_store)?;
                
                
                //This index will be used to lower the Basket's supply caps on rate calculations & supply tallies
            }
        } 
        //Save new Stored price & skip volatility calcs
        else {
            STORED_PRICES.save(storage, cAsset.asset.info.to_string(),
            &StoredPrice {
                price: cAsset_prices[i].clone(),
                last_time_updated: env.block.time.seconds(),
            })?;

        }
    }
    
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

//For debt_cap_queries
pub fn get_cAsset_ratios_imut(
    storage: &dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    collateral_assets: Vec<cAsset>,
    config: Config,
    basket: Option<Basket>,
) -> StdResult<(Vec<Decimal>, Vec<PriceResponse>)> {
    let (cAsset_values, cAsset_prices) = get_asset_values(
        storage,
        env,
        querier,
        collateral_assets,
        config,
        basket.clone(),
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

/// Function queries the price of assets from the oracle.
/// If the query is within the oracle_time_limit, it will use the stored price.
pub fn query_prices(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: Config,
    asset_infos: Vec<AssetInfo>, //Pass a single asset_info for Credit market price queries
    basket: Option<Basket>,
    is_deposit_function: bool,
) -> StdResult<Vec<PriceResponse>> {
    //Set timeframe
    let mut twap_timeframe: u64 = config.collateral_twap_timeframe;
    
    //Load basket
    let basket = if let Some(basket) = basket {
        basket
    } else {
        BASKET.load(storage)?
    };

    //if AssetInfo is the basket.credit_asset, change twap timeframe
    if asset_infos[0].equal(&basket.credit_asset.info) {
        twap_timeframe = config.credit_twap_timeframe;
    }   

    //Price list
    let mut prices: Vec<(String, PriceResponse)> = vec![];
    let mut bulk_asset_query = asset_infos.clone();
    for asset_info in asset_infos.clone() {
        //Try to use a stored price
        let stored_price_res = STORED_PRICES.load(storage, asset_info.to_string()); 
        //Set the old_price if the stored price is within the oracle_time_limit
        let mut old_price: Option<PriceResponse> = None;
        if let Ok(ref stored_price) = stored_price_res {
            let time_elapsed: u64 = env.block.time.seconds() - stored_price.last_time_updated;

            if time_elapsed <= config.oracle_time_limit {
                old_price = Some(stored_price.clone().price)
            }
        }
        
        //If depositing, always query a new price to ensure removed assets aren't deposited
        if !is_deposit_function {
            //Use the stored price if it was within the oracle_time_limit
            if let Some(old_price) = old_price {
                prices.push((asset_info.to_string(), old_price));

                //Remove the asset from the bulk_asset_query list
                bulk_asset_query.retain(|asset| !asset.equal(&asset_info));
            }
            
        }

    }
    
    //Query the remaining Prices
    if bulk_asset_query.len() != 0 {
        match querier.query::<Vec<PriceResponse>>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: config.clone().oracle_contract.unwrap_or_else(|| Addr::unchecked("")).to_string(),
            msg: to_binary(&OracleQueryMsg::Prices {
                asset_infos: bulk_asset_query.clone(),
                twap_timeframe,
                oracle_time_limit: config.oracle_time_limit,
            })?,
        })) {
            Ok(res) => {
                //Add new prices
                for (i, price) in res.iter().enumerate() {
                    prices.push((bulk_asset_query[i].to_string(), price.clone()));
                }
            }
            Err(err) => {
                //if the oracle is down, error
                return Err(err)
            }
        };
    }
    
    //Sort prices based on the asset_info order
    let mut sorted_prices: Vec<PriceResponse> = vec![];
    for asset_info in asset_infos {
        for (i, (asset, price)) in prices.clone().into_iter().enumerate() {
            if asset == asset_info.to_string() {
                sorted_prices.push(price);
                prices.remove(i);
            }
        }
    }
    Ok(sorted_prices)
}

/* REDEMPTION LOGIC COMMENTED OUT
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
                    .filter(|info: &RedemptionInfo| info.position_owner == valid_address.clone().unwrap_or_else(|| Addr::unchecked("")))
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
END REDEMPTION LOGIC COMMENTED OUT */

pub fn simulate_LTV_mint(
    deps: Deps,
    env: Env,
    user_info: UserInfo,
    LTV: Decimal,
) -> StdResult<Uint128> {
    let (_, target_position) = match get_target_position(
        deps.storage,
        deps.api.addr_validate(&user_info.position_owner)?, 
        user_info.position_id){
            Ok(position) => position,
            Err(err) => return Err(StdError::generic_err(err.to_string())),
        };

    let amount = match  get_amount_from_LTV(
        deps.storage,
        deps.querier, 
        env.clone(), 
        CONFIG.load(deps.storage)?,
        target_position,
        BASKET.load(deps.storage)?,
        LTV
    ){
        Ok(amount) => amount,
        Err(err) => return Err(StdError::generic_err(err.to_string())),
    };

    Ok( amount )
}

/// Query ltv_disco contract for average LTVs per asset
/// Returns Vec of (max_ltv, max_borrow_ltv) tuples corresponding to each asset
pub fn query_ltv_disco_for_asset_ltvs(
    querier: QuerierWrapper,
    ltv_disco_addr: Addr,
    assets: Vec<cAsset>,
) -> StdResult<Vec<(Decimal, Decimal)>> {
    let mut ltv_tuples = Vec::new();

    println!("[INSOLVENCY_DEBUG] === query_ltv_disco_for_asset_ltvs ===");
    println!("[INSOLVENCY_DEBUG] ltv_disco_addr: {}", ltv_disco_addr);
    println!("[INSOLVENCY_DEBUG] assets count: {}", assets.len());

    for (i, asset) in assets.iter().enumerate() {
        println!("[INSOLVENCY_DEBUG] Querying LTV disco for asset {}: {}", i, asset.asset.info);
        println!("[INSOLVENCY_DEBUG] Asset {} stored LTVs: max_LTV={}, max_borrow_LTV={}", 
            i, asset.max_LTV, asset.max_borrow_LTV);
        
        // Query ltv_disco for this specific asset's average LTVs
        let response: AverageLTVsResponse = querier.query_wasm_smart(
            ltv_disco_addr.to_string(),
            &LTVDiscoQueryMsg::GetAverageLTVs {
                assets: vec![asset.asset.info.to_string()],
            },
        )?;

        println!("[INSOLVENCY_DEBUG] LTV disco response for asset {}: average_max_ltv={}, average_max_borrow_ltv={}", 
            i, response.average_max_ltv, response.average_max_borrow_ltv);

        // If ltv_disco returns zero (no deposits for this asset), fall back to cAsset's stored LTVs
        let mut max_ltv = if response.average_max_ltv.is_zero() {
            println!("[INSOLVENCY_DEBUG] Using fallback max_LTV from asset: {}", asset.max_LTV);
            asset.max_LTV
        } else {
            println!("[INSOLVENCY_DEBUG] Using LTV disco max_ltv: {}", response.average_max_ltv);
            response.average_max_ltv
        };

        let mut max_borrow_ltv = if response.average_max_borrow_ltv.is_zero() {
            println!("[INSOLVENCY_DEBUG] Using fallback max_borrow_LTV from asset: {}", asset.max_borrow_LTV);
            asset.max_borrow_LTV
        } else {
            println!("[INSOLVENCY_DEBUG] Using LTV disco max_borrow_ltv: {}", response.average_max_borrow_ltv);
            response.average_max_borrow_ltv
        };

        // Ensure LTVs are valid: max_borrow_ltv < max_ltv
        cap_ltv_values(&mut max_borrow_ltv, &mut max_ltv)
            .map_err(|e| StdError::generic_err(format!("Failed to cap LTV values: {}", e)))?;

        println!("[INSOLVENCY_DEBUG] Final LTVs for asset {} (after capping): max_ltv={}, max_borrow_ltv={}", 
            i, max_ltv, max_borrow_ltv);

        ltv_tuples.push((max_ltv, max_borrow_ltv));
    }

    Ok(ltv_tuples)
}

/// Calculate cAsset values & returns a tuple of (cAsset_values, cAsset_prices)
pub fn get_asset_values(
    storage: &dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    assets: Vec<cAsset>,
    config: Config,
    basket: Option<Basket>,
    is_deposit_function: bool,
) -> StdResult<(Vec<Decimal>, Vec<PriceResponse>)> {
    //Enforce Vec max size
    if assets.len() > 50 {
        return Err(StdError::generic_err("Max asset_infos length is 50"));
    }

    //Getting proportions for position collateral to calculate avg LTV
    //Using the index in the for loop to parse through the assets Vec and collateral_assets Vec
    //, as they are now aligned due to the collateral check w/ the Config's data
    let mut cAsset_values: Vec<Decimal> = vec![];
    let mut cAsset_prices: Vec<PriceResponse> = vec![];

    if config.oracle_contract.is_some() && assets.len() > 0 {
        //Set asset_infos
        let asset_infos: Vec<AssetInfo> = assets.iter().map(|asset| asset.asset.info.clone()).collect();

        //Query prices
        cAsset_prices = query_prices(
            storage,
            querier.clone(),
            env.clone(),
            config.clone(),
            asset_infos,
            basket.clone(),
            is_deposit_function,
        )?;
        
        //Calculate cAsset values
        for (i, cAsset) in assets.iter().enumerate() {
            let cAsset_value = cAsset_prices[i].get_value(cAsset.asset.amount)?;
            println!("[INSOLVENCY_DEBUG] Asset {} value calc: amount={}, price={}, decimals={}, value={}", 
                i, cAsset.asset.amount, cAsset_prices[i].price, cAsset_prices[i].decimals, cAsset_value);
            cAsset_values.push(cAsset_value);
        
        }
    }
    
    Ok((cAsset_values, cAsset_prices))
}

/// Calculates the average LTV of a position.
/// Returns avg_borrow_LTV, avg_max_LTV, total_value, cAsset_prices & cAsset_ratios
pub fn get_avg_LTV(
    storage: &dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    config: Config,
    basket: Option<Basket>,
    collateral_assets: Vec<cAsset>,
    is_deposit_function: bool,
) -> StdResult<(Decimal, Decimal, Decimal, Vec<PriceResponse>, Vec<Decimal>)> {
    //Load basket
    let basket = if let Some(basket) = basket {
        basket
    } else {
        BASKET.load(storage)?
    };

    //Calc total value of collateral
    let (cAsset_values, cAsset_price_res) = get_asset_values(
        storage,
        env.clone(),
        querier,
        collateral_assets.clone(),
        config.clone(),
        Some(basket.clone()),
        is_deposit_function,
    )?;
    
    //Query ltv_disco for asset LTVs
    let ltv_tuples = query_ltv_disco_for_asset_ltvs(
        querier,
        config.ltv_disco.clone(),
        collateral_assets.clone(),
    )?;
    
    //Calculate avg LTV & return values
    calculate_avg_LTV(
        cAsset_values, 
        cAsset_price_res, 
        collateral_assets,
        ltv_tuples,
    )
}

/// Calculations for avg_borrow_LTV, avg_max_LTV, total_value, cAsset_prices & cAsset_ratios
pub fn calculate_avg_LTV(
    cAsset_values: Vec<Decimal>,
    cAsset_prices: Vec<PriceResponse>,    
    collateral_assets: Vec<cAsset>,
    ltv_tuples: Vec<(Decimal, Decimal)>, // (max_ltv, max_borrow_ltv) for each asset
) -> StdResult<(Decimal, Decimal, Decimal, Vec<PriceResponse>, Vec<Decimal>)> {
    let total_value: Decimal = cAsset_values.iter().sum();
    println!("[INSOLVENCY_DEBUG] calculate_avg_LTV: total_value (sum of cAsset_values) = {}", total_value);

    //getting each cAsset's % of total value
    let mut cAsset_ratios: Vec<Decimal> = vec![];
    for (i, cAsset) in cAsset_values.iter().enumerate() {
        let ratio = if total_value == Decimal::zero() {
            Decimal::zero()
        } else {
            decimal_division(*cAsset, total_value)?
        };
        println!("[INSOLVENCY_DEBUG] Asset {} ratio: value={}, ratio={}", i, cAsset, ratio);
        cAsset_ratios.push(ratio);
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
            ltv_tuples[0].1, // max_borrow_ltv from ltv_disco
            ltv_tuples[0].0, // max_ltv from ltv_disco
            total_value,
            cAsset_prices,
            cAsset_ratios,
        ));
    }

    for (i, _cAsset) in collateral_assets.iter().enumerate() {
        let contribution = decimal_multiplication(cAsset_ratios[i], ltv_tuples[i].1)?; // Use queried max_borrow_ltv
        println!("[INSOLVENCY_DEBUG] Asset {} avg_borrow_LTV contribution: ratio={} * max_borrow_ltv={} = {}", 
            i, cAsset_ratios[i], ltv_tuples[i].1, contribution);
        avg_borrow_LTV += contribution;
    }

    for (i, _cAsset) in collateral_assets.iter().enumerate() {
        let contribution = decimal_multiplication(cAsset_ratios[i], ltv_tuples[i].0)?; // Use queried max_ltv
        println!("[INSOLVENCY_DEBUG] Asset {} avg_max_LTV contribution: ratio={} * max_ltv={} = {}", 
            i, cAsset_ratios[i], ltv_tuples[i].0, contribution);
        avg_max_LTV += contribution;
    }

    println!("[INSOLVENCY_DEBUG] Final avg_borrow_LTV: {}", avg_borrow_LTV);
    println!("[INSOLVENCY_DEBUG] Final avg_max_LTV: {}", avg_max_LTV);
    println!("[INSOLVENCY_DEBUG] Final total_value: {}", total_value);

    Ok((avg_borrow_LTV, avg_max_LTV, total_value, cAsset_prices, cAsset_ratios))
}


/// Uses a Position's info to calculate if the user is insolvent.
/// Returns insolvent, current_LTV and available fee.
pub fn insolvency_check(
    storage: &mut dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    basket: Option<Basket>,
    collateral_assets: Vec<cAsset>,
    credit_amount: Uint128,
    credit_price: PriceResponse,
    max_borrow: bool, //Toggle for either over max_borrow or over max_LTV (liquidatable)
    config: Config,
) -> StdResult<((bool, Decimal, Uint128), (Decimal, Decimal, Decimal, Vec<PriceResponse>, Vec<Decimal>))> { //insolvent, current_LTV, available_fee, (avg_LTV return values)

    println!("[INSOLVENCY_DEBUG] === ENTERING INSOLVENCY_CHECK ===");
    println!("[INSOLVENCY_DEBUG] credit_amount: {}", credit_amount);
    println!("[INSOLVENCY_DEBUG] credit_price: price={}, decimals={}", credit_price.price, credit_price.decimals);
    println!("[INSOLVENCY_DEBUG] max_borrow flag: {}", max_borrow);
    println!("[INSOLVENCY_DEBUG] collateral_assets count: {}", collateral_assets.len());

    //Get avg LTVs
    let avg_LTVs: (Decimal, Decimal, Decimal, Vec<PriceResponse>, Vec<Decimal>) =
        get_avg_LTV(storage, env, querier, config, basket, collateral_assets.clone(), false)?;

    println!("[INSOLVENCY_DEBUG] get_avg_LTV returned:");
    println!("  avg_borrow_LTV: {}", avg_LTVs.0);
    println!("  avg_max_LTV: {}", avg_LTVs.1);
    println!("  total_asset_value: {}", avg_LTVs.2);
    println!("  price_responses count: {}", avg_LTVs.3.len());
    println!("  asset_ratios count: {}", avg_LTVs.4.len());

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
    
    // Log collateral asset amounts
    println!("[INSOLVENCY_DEBUG] Collateral assets:");
    for (i, asset) in collateral_assets.iter().enumerate() {
        println!("  Asset {}: denom={}, amount={}", i, asset.asset.info, asset.asset.amount);
    }
    println!("[INSOLVENCY_DEBUG] total_assets (raw): {}", total_assets);
    
    // No assets with debt, return insolvent        
    if total_assets.is_zero() && !credit_amount.is_zero() {
        println!("[INSOLVENCY_DEBUG] No assets but has debt - returning insolvent");
        return Ok((true, Decimal::percent(100), Uint128::zero()));
    } // No assets and no debt, return not insolvent        
    else if credit_amount.is_zero() {
        println!("[INSOLVENCY_DEBUG] No debt - returning not insolvent");
        return Ok((false, Decimal::percent(0), Uint128::zero()));
    }

    
    let total_asset_value: Decimal = avg_LTVs.2; //pulls total_asset_value
    let debt_value = credit_price.get_value(credit_amount)?;
    
    // Log all key values
    println!("[INSOLVENCY_DEBUG] === INSOLVENCY CALCULATION ===");
    println!("[INSOLVENCY_DEBUG] credit_amount (raw tokens): {}", credit_amount);
    println!("[INSOLVENCY_DEBUG] credit_price: price={}, decimals={}", credit_price.price, credit_price.decimals);
    println!("[INSOLVENCY_DEBUG] debt_value (USD): {}", debt_value);
    println!("[INSOLVENCY_DEBUG] total_asset_value (USD): {}", total_asset_value);
    println!("[INSOLVENCY_DEBUG] avg_borrow_LTV (max_borrow_LTV): {}", avg_LTVs.0);
    println!("[INSOLVENCY_DEBUG] avg_max_LTV (liquidation_LTV): {}", avg_LTVs.1);
    println!("[INSOLVENCY_DEBUG] max_borrow flag: {}", max_borrow);
    
    // Log asset values and prices
    println!("[INSOLVENCY_DEBUG] Asset values and prices:");
    for (i, price_resp) in avg_LTVs.3.iter().enumerate() {
        if i < collateral_assets.len() {
            let asset_value = if i < avg_LTVs.4.len() {
                let ratio = avg_LTVs.4[i];
                total_asset_value * ratio
            } else {
                Decimal::zero()
            };
            println!("  Asset {}: price={}, decimals={}, value={}", 
                i, price_resp.price, price_resp.decimals, asset_value);
        }
    }
    
    //current_LTV = debt_value / total_asset_value);
    let current_LTV = 
        debt_value.checked_div(total_asset_value).map_err(|_| StdError::generic_err( format!("Division by zero in insolvency_check_calc, line 907. debt_value: {}, total_asset_value: {}", debt_value, total_asset_value)))?;
    
    println!("[INSOLVENCY_DEBUG] current_LTV: {}", current_LTV);
    println!("[INSOLVENCY_DEBUG] current_LTV as percent: {}%", current_LTV * Decimal::percent(100));

    let check: bool = match max_borrow {
        true => {
            //Checks max_borrow
            let result = current_LTV > avg_LTVs.0;
            println!("[INSOLVENCY_DEBUG] Checking max_borrow: current_LTV ({}) > avg_borrow_LTV ({}) = {}", current_LTV, avg_LTVs.0, result);
            result
        }
        false => {
            //Checks max_LTV
            let result = current_LTV > avg_LTVs.1;
            println!("[INSOLVENCY_DEBUG] Checking max_LTV: current_LTV ({}) > avg_max_LTV ({}) = {}", current_LTV, avg_LTVs.1, result);
            result
        }
    };

    let available_fee = if check && current_LTV > avg_LTVs.1{    
        //current_LTV - max_LTV
        let fee = current_LTV.checked_sub(avg_LTVs.1)?;
        //current_LTV - borrow_LTV
        let liq_range = current_LTV.checked_sub(avg_LTVs.0)?;
        //Fee value = repay_amount * fee
        let fee_value = liq_range.checked_div(current_LTV).map_err(|_| StdError::generic_err( format!("Division by zero in insolvency_check_calc, line 926. liq_range: {}, current_LTV: {}", liq_range, current_LTV)))?
                .checked_mul(debt_value)?
                .checked_mul(fee)?
                .to_uint_floor();
        println!("[INSOLVENCY_DEBUG] available_fee calculated: {}", fee_value);
        fee_value
    } else {
        println!("[INSOLVENCY_DEBUG] available_fee: 0 (not liquidatable)");
        Uint128::zero()
    };

    println!("[INSOLVENCY_DEBUG] === RESULT: insolvent={}, current_LTV={}, available_fee={} ===", check, current_LTV, available_fee);
    Ok((check, current_LTV, available_fee))
}

/// Returns historical oracle prices for an asset
pub fn query_historical_oracle_prices(
    deps: Deps,
    asset: String,
) -> StdResult<HistoricalOraclePricesResponse> {
    let prices = HISTORICAL_ORACLE_PRICES.may_load(deps.storage, asset)?
        .unwrap_or_else(|| vec![]);
    
    // Convert state::PriceTimestamp to membrane::cdp::PriceTimestamp
    let converted_prices: Vec<membrane::cdp::PriceTimestamp> = prices
        .into_iter()
        .map(|pt| membrane::cdp::PriceTimestamp {
            price: pt.price,
            timestamp: pt.timestamp,
        })
        .collect();
    
    Ok(HistoricalOraclePricesResponse { prices: converted_prices })
}

/// Returns historical interest rates for an asset
pub fn query_historical_interest_rates(
    deps: Deps,
    asset: String,
) -> StdResult<HistoricalInterestRatesResponse> {
    let rates = HISTORICAL_INTEREST_RATES.may_load(deps.storage, asset)?
        .unwrap_or_else(|| vec![]);
    
    // Convert state::RateTimestamp to membrane::cdp::RateTimestamp
    let converted_rates: Vec<membrane::cdp::RateTimestamp> = rates
        .into_iter()
        .map(|rt| membrane::cdp::RateTimestamp {
            rate: rt.rate,
            timestamp: rt.timestamp,
        })
        .collect();
    
    Ok(HistoricalInterestRatesResponse { rates: converted_rates })
}

/// Check if assets are in volatile windows (current volatility > average volatility)
pub fn query_volatility_window(
    deps: Deps,
    assets: Vec<String>,
) -> StdResult<membrane::cdp::VolatilityWindowResponse> {
    let mut results = Vec::new();

    for asset in assets {
        let in_window = if let Ok(vol_store) = VOLATILITY.load(deps.storage, asset) {
            if vol_store.raw_volatility_list.is_empty() {
                false // No volatility data, not in volatile window
            } else {
                // Get the most recent volatility
                let current_vol = vol_store.raw_volatility_list.last().copied().unwrap_or(Decimal::zero());

                // Calculate average volatility
                let sum: Decimal = vol_store.raw_volatility_list.iter().copied().sum();
                let avg_volatility = sum / Decimal::from_ratio(vol_store.raw_volatility_list.len() as u128, 1u128);

                // In volatile window if current > average
                current_vol > avg_volatility
            }
        } else {
            false // No volatility data for this asset
        };
        results.push(in_window);
    }

    Ok(membrane::cdp::VolatilityWindowResponse { in_volatile_window: results })
}

/// Query to simulate liquidation market sales and estimate slippage cost
/// **Note**: Uses Astroport simulation only. Returns error if Duality routes configured.
pub fn query_simulate_liquidation(
    deps: Deps,
    collateral_to_sell: Vec<Coin>,
    target_denom: String,
) -> StdResult<membrane::cdp::SimulateLiquidationResponse> {
    let config = CONFIG.load(deps.storage)?;
    let basket = BASKET.load(deps.storage)?;

    // Get chain proxy address (neutron-proxy contract)
    let chain_proxy = config.chain_proxy.ok_or_else(|| {
        StdError::generic_err("Chain proxy not configured")
    })?;

    // Call the simulation helper from liquidations module
    let (total_input_value, total_output_value, slippage_cost) =
        crate::liquidations::simulate_liquidation_sales(
            &deps.querier,
            chain_proxy,
            collateral_to_sell,
            target_denom,
            basket.credit_price,
        )?;

    Ok(membrane::cdp::SimulateLiquidationResponse {
        total_input_value,
        total_output_value,
        slippage_cost,
    })
}

/// Query historical LTV snapshots for an asset
pub fn query_historical_ltv(
    deps: Deps,
    asset_denom: String,
    start_time: Option<u64>,
    end_time: Option<u64>,
    limit: Option<u32>,
) -> StdResult<membrane::cdp::HistoricalLTVResponse> {
    let limit = limit.unwrap_or(100).min(500) as usize; // Max 500 snapshots
    let start = start_time.unwrap_or(0);
    let end = end_time.unwrap_or(u64::MAX);

    // Load all snapshots for this asset
    let all_snapshots = LTV_HISTORY.may_load(deps.storage, asset_denom.clone())?.unwrap_or_else(|| vec![]);

    // Filter by time range and limit
    let snapshots: Vec<membrane::cdp::LTVSnapshot> = all_snapshots
        .into_iter()
        .filter(|snapshot| snapshot.timestamp >= start && snapshot.timestamp <= end)
        .take(limit)
        .collect();

    Ok(membrane::cdp::HistoricalLTVResponse {
        asset_denom,
        snapshots,
    })
}

/// Query LTV shift schedule information for an asset
pub fn query_ltv_shift_info(
    deps: Deps,
    env: Env,
    asset_denom: String,
) -> StdResult<membrane::cdp::LTVShiftInfoResponse> {
    // Load the LTV update tracker for this asset
    let tracker = LTV_UPDATE_TRACKERS.may_load(deps.storage, asset_denom.clone())?
        .ok_or_else(|| StdError::generic_err(format!("No LTV tracker found for asset {}", asset_denom)))?;

    let current_time = env.block.time.seconds();

    // Calculate time until the next shift (if there is a staged downward shift)
    let (next_shift_time, time_until_shift) = if let Some(staged_ts) = tracker.staged_timestamp {
        let config = CONFIG.load(deps.storage)?;
        let shift_time = staged_ts + config.ltv_downward_period;
        let time_until = if shift_time > current_time {
            shift_time - current_time
        } else {
            0
        };
        (shift_time, time_until)
    } else {
        // No pending shift
        (0, 0)
    };

    Ok(membrane::cdp::LTVShiftInfoResponse {
        current_shift_number: 0, // This could be enhanced to track shift count
        next_shift_time,
        time_until_shift,
    })
}