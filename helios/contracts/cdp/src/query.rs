use cosmwasm_std::{Deps, StdResult, Uint128, Addr, StdError, Order, QuerierWrapper, Decimal, to_binary, QueryRequest, WasmQuery};
use cw_storage_plus::Bound;
use membrane::{positions::{PropResponse, ConfigResponse, PositionResponse, BasketResponse, PositionsResponse}, types::{Position, Basket, AssetInfo, LiqAsset}, stability_pool::PoolResponse};
use membrane::stability_pool::{ QueryMsg as SP_QueryMsg, LiquidatibleResponse as SP_LiquidatibleResponse };

use crate::state::{CONFIG, POSITIONS, REPAY, BASKETS, Config};


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
                stability_pool: config.clone().stability_pool.unwrap_or_else(|| Addr::unchecked("None")).into_string(),
                dex_router: config.clone().dex_router.unwrap_or_else(|| Addr::unchecked("None")).into_string(),
                fee_collector: config.clone().fee_collector.unwrap_or_else(|| Addr::unchecked("None")).into_string(),
                osmosis_proxy: config.clone().osmosis_proxy.unwrap_or_else(|| Addr::unchecked("None")).into_string(),
                liq_fee: config.clone().liq_fee,
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