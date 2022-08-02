use cosmwasm_std::{Deps, StdResult, Uint128, Addr, StdError, Order, QuerierWrapper, Decimal, to_binary, QueryRequest, WasmQuery, Storage, Env};
use cw_storage_plus::Bound;
use membrane::{positions::{PropResponse, ConfigResponse, PositionResponse, BasketResponse, PositionsResponse, DebtCapResponse}, types::{Position, Basket, AssetInfo, LiqAsset, cAsset, PriceInfo}, stability_pool::PoolResponse};
use membrane::stability_pool::{ QueryMsg as SP_QueryMsg, LiquidatibleResponse as SP_LiquidatibleResponse };
use membrane::osmosis_proxy::{ QueryMsg as OsmoQueryMsg };
use osmo_bindings::SpotPriceResponse;

use crate::{state::{CONFIG, POSITIONS, REPAY, BASKETS, Config}, positions::{read_price, get_asset_liquidity}, math::{decimal_multiplication, decimal_division, decimal_subtraction}, ContractError};


const MAX_LIMIT: u32 = 31;

pub fn query_prop(
    deps: Deps,
) -> StdResult<PropResponse>{
    match REPAY.load(deps.storage) {
        Ok( prop ) => {
            Ok( PropResponse {
                liq_queue_leftovers: prop.clone().liq_queue_leftovers,
                stability_pool: prop.clone().stability_pool,
                sell_wall_distributions: prop.clone().sell_wall_distributions,
                positions_contract: prop.clone().positions_contract.to_string(),
                position_id: prop.clone().position_id,
                basket_id: prop.clone().basket_id,
                position_owner: prop.clone().position_owner.to_string(),
            })
        },
        Err( err ) => return Err( err ),
    }
}

pub fn query_config(
    deps: Deps,
) -> StdResult<ConfigResponse>{
    match CONFIG.load(deps.storage) {
        Ok( config ) => {
            Ok( ConfigResponse {
                owner: config.clone().owner.to_string(),
                current_basket_id: config.clone().current_basket_id,
                stability_pool: config.clone().stability_pool.unwrap_or(Addr::unchecked("None")).into_string(),
                dex_router: config.clone().dex_router.unwrap_or(Addr::unchecked("None")).into_string(),
                fee_collector: config.clone().fee_collector.unwrap_or(Addr::unchecked("None")).into_string(),
                osmosis_proxy: config.clone().osmosis_proxy.unwrap_or( Addr::unchecked("None")).into_string(),
                debt_auction: config.clone().debt_auction.unwrap_or( Addr::unchecked("None")).into_string(),
                liq_fee: config.clone().liq_fee,
                oracle_time_limit: config.oracle_time_limit,
                debt_minimum: config.debt_minimum,
                
            })
        },
        Err( err ) => return Err( err ),
    }
}

pub fn query_position(
    deps: Deps,
    position_id: Uint128,
    basket_id: Uint128,
    user: Addr
) -> StdResult<PositionResponse>{
    let positions: Vec<Position> = match POSITIONS.load(deps.storage, (basket_id.to_string(), user.clone())){
        Err(_) => {  return Err(StdError::generic_err("No User Positions")) },
        Ok( positions ) => { positions },
    };

    let position = positions
    .into_iter()
    .find(|x| x.position_id == position_id);

    match position{
        Some (position) => {
            Ok(PositionResponse {
                position_id: position.position_id.to_string(),
                collateral_assets: position.collateral_assets,
                avg_borrow_LTV: position.avg_borrow_LTV.to_string(),
                avg_max_LTV: position.avg_max_LTV.to_string(),
                credit_amount: position.credit_amount.to_string(),
                basket_id: position.basket_id.to_string(),
            })
        },

        None => return  Err(StdError::generic_err("NonExistent Position"))
    }
}

pub fn query_user_positions(
    deps: Deps,
    basket_id: Option<Uint128>,
    user: Addr,
    limit: Option<u32>
) -> StdResult<Vec<PositionResponse>>{

    let limit = limit.unwrap_or(MAX_LIMIT) as usize;
    
    //Basket_id means only position from said basket
    if basket_id.is_some(){

        let positions: Vec<Position> = match POSITIONS.load(deps.storage, (basket_id.unwrap().clone().to_string(), user.clone())){
            Err(_) => {  return Err(StdError::generic_err("No User Positions")) },
            Ok( positions ) => { positions },
        };

        let user_positions: Vec<PositionResponse> = positions
            .into_iter()
            .take(limit)
            .map(|position| 
                PositionResponse {
                        position_id: position.position_id.to_string(),
                        collateral_assets: position.collateral_assets,
                        avg_borrow_LTV: position.avg_borrow_LTV.to_string(),
                        avg_max_LTV: position.avg_max_LTV.to_string(),
                        credit_amount: position.credit_amount.to_string(),
                        basket_id: position.basket_id.to_string(),
                    }
            ).collect::<Vec<PositionResponse>>();

        Ok( user_positions )
        
    }else{ //If no basket_id, return all basket positions
        //Can use config.current basket_id-1 as the limiter to check all baskets

        let config = CONFIG.load(deps.storage)?;
        let mut response: Vec<PositionResponse> = Vec::new();

        //Uint128 to int
        let range: i32 = config.current_basket_id.to_string().parse().unwrap();

        for basket_id in 1..range{

                        
            match POSITIONS.load(deps.storage, (basket_id.to_string(), user.clone())) {
                Ok(positions) => {

                    for position in positions{
                        response.push(
                            PositionResponse {
                                position_id: position.position_id.to_string(),
                                collateral_assets: position.collateral_assets,
                                avg_borrow_LTV: position.avg_borrow_LTV.to_string(),
                                avg_max_LTV: position.avg_max_LTV.to_string(),
                                credit_amount: position.credit_amount.to_string(),
                                basket_id: position.basket_id.to_string(),
                            }
                        );
                    
                    }
                },
                Err(_) => {} //This is so errors don't stop the response builder, but we don't actually care about them here
            }
            
        }
        Ok( response )

    }

}

pub fn query_basket(
    deps: Deps,
    basket_id: Uint128,
) -> StdResult<BasketResponse>{

    let basket_res = match BASKETS.load(deps.storage, basket_id.to_string()){
        Ok( basket ) => {

            let credit_price = match basket.credit_price{
                Some(x) => { x.to_string()},
                None => { "None".to_string() },
            };
                        
            let credit_interest = match basket.credit_interest{
                Some(x) => { x.to_string()},
                None => { "None".to_string() },
            };

            BasketResponse {
                owner: basket.owner.to_string(),
                basket_id: basket.basket_id.to_string(),
                current_position_id: basket.current_position_id.to_string(),
                collateral_types: basket.collateral_types,
                credit_asset: basket.credit_asset,
                credit_price,
                credit_interest,
                debt_pool_ids: basket.debt_pool_ids,
                debt_liquidity_multiplier_for_caps: basket.debt_liquidity_multiplier_for_caps,
                liq_queue: basket.liq_queue.unwrap_or(Addr::unchecked("None")).to_string(),
            }
        },
        Err(_) => { return Err(StdError::generic_err("Invalid basket_id")) },
    };

    Ok( basket_res )


}

pub fn query_baskets(
    deps: Deps,
    start_after: Option<Uint128>,
    limit: Option<u32>,
) -> StdResult<Vec<BasketResponse>>{

    let limit = limit.unwrap_or(MAX_LIMIT) as usize;

    let start: Option<Bound<String>> = match BASKETS.load(deps.storage, start_after.unwrap().to_string()){
        Ok(_x) => {
            Some(Bound::exclusive(start_after.unwrap().to_string()))
        },
        Err(_) => {
            None
        },
    };

    BASKETS
        .range(deps.storage, start, None, Order::Ascending)
        .take(limit)
        .map(|item| {
            let (k, basket) = item?;

            let credit_price = match basket.credit_price{
                Some(x) => { x.to_string()},
                None => { "None".to_string() },
            };
                        
            let credit_interest = match basket.credit_interest{
                Some(x) => { x.to_string()},
                None => { "None".to_string() },
            };

            Ok(BasketResponse {
                owner: basket.owner.to_string(),
                basket_id: k,
                current_position_id: basket.current_position_id.to_string(),
                collateral_types: basket.collateral_types,
                credit_asset: basket.credit_asset,
                credit_price,
                credit_interest,
                debt_pool_ids: basket.debt_pool_ids,
                debt_liquidity_multiplier_for_caps: basket.debt_liquidity_multiplier_for_caps,
                liq_queue: basket.liq_queue.unwrap_or(Addr::unchecked("None")).to_string(),
                
            })
            
        })
        .collect()
}

pub fn query_basket_positions(
    deps: Deps,
    basket_id: Uint128,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<Vec<PositionsResponse>>{
     
    let limit = limit.unwrap_or(MAX_LIMIT) as usize;

    let start_after_addr = deps.api.addr_validate(&start_after.unwrap())?;
    let start = Some(Bound::exclusive(start_after_addr));

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


pub fn query_stability_pool_fee(
    querier: QuerierWrapper,
    config: Config,
    basket: Basket,
) -> StdResult<Decimal> {

    let resp: PoolResponse = querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.stability_pool.unwrap().to_string(),
        msg: to_binary(&SP_QueryMsg::AssetPool {
            asset_info: basket.credit_asset.info,
        })?,
    }))?;
    
    Ok( resp.liq_premium )

}

pub fn query_stability_pool_liquidatible(
    querier: QuerierWrapper,
    config: Config,
    amount: Decimal,
    info: AssetInfo,
) -> StdResult<Decimal>{

    let query_res: SP_LiquidatibleResponse = querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.stability_pool.unwrap().to_string(),
        msg: to_binary(&SP_QueryMsg::CheckLiquidatible {
            asset: LiqAsset{
                amount: amount,
                info,
            },
        })?,
    }))?;

    Ok( query_res.leftover )
}

//Calculate debt caps
//They are 
pub fn query_basket_debt_caps(
    deps: Deps,
    env: Env,
    basket_id: Uint128,
) -> StdResult<DebtCapResponse>{
   
    let config: Config = CONFIG.load( deps.storage )?;

    let basket: Basket = BASKETS.load( deps.storage, basket_id.to_string() )?;

     //Get the Basket's asset ratios
     let cAsset_ratios = get_cAsset_ratios_imut( deps.storage, env, deps.querier, basket.clone().collateral_types, config.clone())?;

     //Get the debt cap 
     let debt_cap = get_asset_liquidity( 
        deps.querier, 
        config, 
        basket.debt_pool_ids, 
        basket.credit_asset.info 
        )? * basket.debt_liquidity_multiplier_for_caps;
 
     let mut asset_caps = vec![];
 
     for cAsset in cAsset_ratios{
         asset_caps.push( cAsset * debt_cap );
     }                       
 
     let mut res = vec![];
     //Append caps and asset_infos
     for ( index, asset ) in basket.collateral_types.iter().enumerate(){
        res.push( format!("{}: {}/{}, ", asset.asset.info, basket.collateral_types[index].debt_total, asset_caps[index]) );
     }
     
     Ok( DebtCapResponse { caps: res } )
}

fn get_cAsset_ratios_imut(
    storage: &dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    collateral_assets: Vec<cAsset>,
    config: Config,
) -> StdResult<Vec<Decimal>>{
    let (cAsset_values, cAsset_prices) = get_asset_values_imut(storage, env, querier, collateral_assets.clone(), config)?;

    let total_value: Decimal = cAsset_values.iter().sum();

    //getting each cAsset's % of total value
    let mut cAsset_ratios: Vec<Decimal> = vec![];
    for cAsset in cAsset_values{
        cAsset_ratios.push(cAsset/total_value) ;
    }

    //Error correction for ratios so we end up w/ least amount undistributed funds
    let ratio_total: Option<Decimal> = Some(cAsset_ratios.iter().sum());

    if ratio_total.unwrap() != Decimal::percent(100){
        let mut new_ratios: Vec<Decimal> = vec![];
        
        match ratio_total{
            Some( total ) if total > Decimal::percent(100) => {

                    let margin_of_error = total - Decimal::percent(100);

                    let num_users = Decimal::new(Uint128::from( cAsset_ratios.len() as u128 ));

                    let error_correction = decimal_division( margin_of_error, num_users );

                    new_ratios = cAsset_ratios.into_iter()
                    .map(|ratio| 
                        decimal_subtraction( ratio, error_correction )
                    ).collect::<Vec<Decimal>>();
                    
            },
            Some( total ) if total < Decimal::percent(100) => {

                let margin_of_error = Decimal::percent(100) - total;

                let num_users = Decimal::new(Uint128::from( cAsset_ratios.len() as u128 ));

                let error_correction = decimal_division( margin_of_error, num_users );

                new_ratios = cAsset_ratios.into_iter()
                        .map(|ratio| 
                            ratio + error_correction
                        ).collect::<Vec<Decimal>>();
            },
            None => { return Err(StdError::GenericErr { msg: "Input amounts were null".to_string() }) },
            Some(_) => { /*Unreachable due to if statement*/ },
        }
        return Ok( new_ratios )
    }

    Ok( cAsset_ratios )
}


//Get Asset values / query oracle
pub fn get_asset_values_imut(
    storage: &dyn Storage, 
    env: Env, 
    querier: QuerierWrapper, 
    assets: Vec<cAsset>, 
    config: Config
) -> StdResult<(Vec<Decimal>, Vec<Decimal>)>
{

   //Getting proportions for position collateral to calculate avg LTV
    //Using the index in the for loop to parse through the assets Vec and collateral_assets Vec
    //, as they are now aligned due to the collateral check w/ the Config's data
    let mut cAsset_values: Vec<Decimal> = vec![];
    let mut cAsset_prices: Vec<Decimal> = vec![];

    if config.clone().osmosis_proxy.is_some(){

        for (i, casset) in assets.iter().enumerate() {

        let price_info: PriceInfo = match read_price( storage, &casset.asset.info ){
            Ok( info ) => { info },
            Err(_) => { 
                //Set time to fail in the next check. We don't want the error to stop from querying though
                PriceInfo {
                    price: Decimal::zero(),
                    last_time_updated: env.block.time.plus_seconds( config.oracle_time_limit + 1u64 ).seconds(),
                } 
            },
        }; 
        let mut valid_price: bool = false;
        let mut price: Decimal;

        //If last_time_updated hasn't hit the limit set by the config...
        //..don't query and use the saved price.
        //Else try to query new price.
        let time_elapsed: Option<u64> = env.block.time.seconds().checked_sub(price_info.last_time_updated);

        //If its None then the subtraction was negative meaning the initial read_price() errored
        if time_elapsed.is_some() && time_elapsed.unwrap() <= config.oracle_time_limit{
            price = price_info.price;
            valid_price = true;
        }else{

            //TODO: REPLACE WITH TWAP WHEN RELEASED PLEEEEASSSE, DONT LET THIS BE OUR DOWNFALL
            price = match querier.query::<SpotPriceResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
                msg: to_binary(&OsmoQueryMsg::SpotPrice {
                    asset: casset.asset.info.to_string(),
                })?,
            })){
                Ok( res ) => { res.price },
                Err( err ) => { 
                    
                    if valid_price{
                        price_info.price
                    }else{
                        return Err( err )
                    }
                }
            };
        }
        
        cAsset_prices.push(price);
        let collateral_value = decimal_multiplication(Decimal::from_ratio(casset.asset.amount, Uint128::new(1u128)), price);
        cAsset_values.push(collateral_value); 
        }
    }
    

    
    Ok((cAsset_values, cAsset_prices))
}
