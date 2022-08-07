use cosmwasm_std::{Deps, StdResult, Uint128, Addr, StdError, Order, QuerierWrapper, Decimal, to_binary, QueryRequest, WasmQuery, Storage, Env, MessageInfo};
use cw_storage_plus::Bound;
use membrane::{positions::{PropResponse, ConfigResponse, PositionResponse, BasketResponse, PositionsResponse, DebtCapResponse, BadDebtResponse, InsolvencyResponse}, types::{Position, Basket, AssetInfo, LiqAsset, cAsset, PriceInfo, PositionUserInfo, InsolventPosition}, stability_pool::PoolResponse};
use membrane::stability_pool::{ QueryMsg as SP_QueryMsg, LiquidatibleResponse as SP_LiquidatibleResponse };
use membrane::osmosis_proxy::{ QueryMsg as OsmoQueryMsg };
use osmo_bindings::SpotPriceResponse;

use crate::{state::{CONFIG, POSITIONS, REPAY, BASKETS, Config}, positions::{read_price, get_asset_liquidity, validate_position_owner}, math::{decimal_multiplication, decimal_division, decimal_subtraction}, ContractError};


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
    env: Env,
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

            let config = CONFIG.load( deps.storage )?;

            let ( borrow, max, value, prices) = get_avg_LTV_imut( deps.storage, env.clone(), deps.querier, position.clone().collateral_assets, config.clone() )?;

            Ok(PositionResponse {
                position_id: position.position_id.to_string(),
                collateral_assets: position.collateral_assets,
                credit_amount: position.credit_amount.to_string(),
                basket_id: position.basket_id.to_string(),
                avg_borrow_LTV: borrow,
                avg_max_LTV: max,
            })
        },

        None => return  Err(StdError::generic_err("NonExistent Position"))
    }
}

pub fn query_user_positions(
    deps: Deps,
    env: Env,
    basket_id: Option<Uint128>,
    user: Addr,
    limit: Option<u32>
) -> StdResult<Vec<PositionResponse>>{

    let limit = limit.unwrap_or(MAX_LIMIT) as usize;

    let config = CONFIG.load( deps.storage )?;

    let mut error: Option<StdError> = None;
    
    //Basket_id means only position from said basket
    if basket_id.is_some(){

        let positions: Vec<Position> = match POSITIONS.load(deps.storage, (basket_id.unwrap().clone().to_string(), user.clone())){
            Err(_) => {  return Err(StdError::generic_err("No User Positions")) },
            Ok( positions ) => { positions },
        };

        let mut user_positions: Vec<PositionResponse> = vec![];
        
    let _iter = positions
            .into_iter()
            .take(limit)
            .map(|position| {
                
                let ( borrow, max, value, prices) = match get_avg_LTV_imut( deps.storage, env.clone(), deps.querier, position.clone().collateral_assets, config.clone() ){

                    Ok( ( borrow, max, value, prices) ) => {
                        ( borrow, max, value, prices)
                    },
                    Err( err ) => { 
                        error = Some( err );
                        ( Decimal::zero(), Decimal::zero(), Decimal::zero(), vec![] )
                    }
                };

                if error.is_none(){
                    user_positions.push( PositionResponse {
                            position_id: position.position_id.to_string(),
                            collateral_assets: position.collateral_assets,
                            credit_amount: position.credit_amount.to_string(),
                            basket_id: position.basket_id.to_string(),
                            avg_borrow_LTV: borrow,
                            avg_max_LTV: max,
                    } )
                } 
                }
            );

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

                        let ( borrow, max, value, prices) = get_avg_LTV_imut( deps.storage, env.clone(), deps.querier, position.clone().collateral_assets, config.clone() )?;

                        response.push(
                            PositionResponse {
                                position_id: position.position_id.to_string(),
                                collateral_assets: position.collateral_assets,
                                credit_amount: position.credit_amount.to_string(),
                                basket_id: position.basket_id.to_string(),
                                avg_borrow_LTV: borrow,
                                avg_max_LTV: max,
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

    let start: Option<Bound<String>> = if let Some(_start) = start_after { match BASKETS.load(deps.storage, start_after.unwrap().to_string()){
        Ok(_x) => {
            Some(Bound::exclusive(start_after.unwrap().to_string()))
        },
        Err(_) => {
            None
        },
    }}else {
        None
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

    let start = if let Some(start) = start_after {
        let start_after_addr = deps.api.addr_validate(&start)?;
        Some(Bound::exclusive(start_after_addr))
    }else{
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


pub fn query_bad_debt(
    deps: Deps,
    basket_id: Uint128,
) -> StdResult<BadDebtResponse>{
    
    let mut res = BadDebtResponse { has_bad_debt: vec![] } ;

    let _iter = POSITIONS
        .prefix(basket_id.to_string())
        .range(deps.storage, None, None, Order::Ascending)
        .map(|item| {
            let (addr, positions) = item.unwrap();
            
            for position in positions{
                //We do a lazy check for bad debt by checking if there is debt without any assets left in the position
                //This is allowed bc any calls here will be after a liquidation where the sell wall would've sold all it could to cover debts
                let total_assets: Uint128 = 
                position.collateral_assets
                    .iter()
                    .map(|asset| asset.asset.amount)
                    .collect::<Vec<Uint128>>()
                    .iter()
                    .sum();

                //If there are no assets and outstanding debt
                if total_assets.is_zero() && !position.credit_amount.is_zero(){
                    res.has_bad_debt.push( 
                        ( PositionUserInfo {
                            basket_id,
                            position_id: Some( position.position_id ),
                            position_owner: Some( addr.to_string() ),
                        }, position.credit_amount ) )
                }        
            }
        });
        
    Ok( res )
}


pub fn query_basket_insolvency(
    deps: Deps,
    env: Env,
    basket_id: Uint128,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<InsolvencyResponse>{

    let config: Config = CONFIG.load( deps.storage )?;

    let basket: Basket = BASKETS.load( deps.storage, basket_id.to_string() )?;
    
    let mut res = InsolvencyResponse { insolvent_positions: vec![] };
    let mut error: Option<StdError> = None;

    let limit = limit.unwrap_or(MAX_LIMIT) as usize;

    let start = if let Some(start) = start_after {
        let start_after_addr = deps.api.addr_validate(&start)?;
        Some(Bound::exclusive(start_after_addr))
    }else{
        None
    };

    let _iter = POSITIONS
        .prefix(basket_id.to_string())
        .range(deps.storage, start, None, Order::Ascending)
        .take(limit)
        .map(|item| {
            let (addr, positions) = item.unwrap();
            
            for position in positions{

                let ( insolvent, current_LTV, available_fee ) = match insolvency_check_imut(deps.storage, env.clone(), deps.querier, position.collateral_assets, position.credit_amount, basket.clone().credit_price.unwrap(), false, config.clone()){
                    Ok( ( insolvent, current_LTV, available_fee ) ) => ( insolvent, current_LTV, available_fee ),
                    Err( err ) => {
                                                error = Some( err );
                                                ( false, Decimal::zero(), Uint128::zero() )
                                            },
                };
                
                if insolvent {
                    res.insolvent_positions.push( InsolventPosition {
                        insolvent,
                        position_info: PositionUserInfo { 
                            basket_id: basket_id.clone(), 
                            position_id: Some( position.position_id ), 
                            position_owner: Some( addr.to_string() ), 
                        },
                        current_LTV,
                        available_fee,
                    } );
                }
            }
        });

    
    if error.is_some() {
        return Err( error.unwrap() );
    } else {           
    Ok( res )
    }
}

pub fn query_position_insolvency(
    deps: Deps,
    env: Env,
    basket_id: Uint128,
    position_id: Uint128,
    position_owner: String,
) -> StdResult<InsolvencyResponse>{

    let config: Config = CONFIG.load( deps.storage )?;

    let valid_owner_addr = deps.api.addr_validate( &position_owner)?;

    let basket: Basket = BASKETS.load( deps.storage, basket_id.to_string() )?;
    
    let positions: Vec<Position> = POSITIONS.load( deps.storage, (basket_id.to_string(), valid_owner_addr))?;

    let target_position = match positions.into_iter().find(|x| x.position_id == position_id){
        Some( position ) => position,
        None => return Err( StdError::NotFound { kind: "Position".to_string() } )
    };

    ///
    let mut res = InsolvencyResponse { insolvent_positions: vec![] };
      
    let ( insolvent, current_LTV, available_fee ) = insolvency_check_imut(deps.storage, env.clone(), deps.querier, target_position.collateral_assets, target_position.credit_amount, basket.clone().credit_price.unwrap(), false, config.clone())?;
                
    //Since its a Singular position we'll output whether insolvent or not
    res.insolvent_positions.push( InsolventPosition {
        insolvent,
        position_info: PositionUserInfo { 
            basket_id: basket_id.clone(), 
            position_id: Some( target_position.position_id ), 
            position_owner: Some( position_owner.to_string() ), 
        },
        current_LTV,
        available_fee,
    } );
    

                
    Ok( res )
    
}

////Helper/////

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

fn get_avg_LTV_imut(
    storage: &dyn Storage,
    env: Env,
    querier: QuerierWrapper, 
    collateral_assets: Vec<cAsset>,
    config: Config,
)-> StdResult<(Decimal, Decimal, Decimal, Vec<Decimal>)>{

    let (cAsset_values, cAsset_prices) = get_asset_values_imut(storage, env, querier, collateral_assets.clone(), config)?;

    //panic!("{}", cAsset_values.len());

    let total_value: Decimal = cAsset_values.iter().sum();
    
    //getting each cAsset's % of total value
    let mut cAsset_ratios: Vec<Decimal> = vec![];
    for cAsset in cAsset_values{
        cAsset_ratios.push( decimal_division( cAsset, total_value) );
    }

    //converting % of value to avg_LTV by multiplying collateral LTV by % of total value
    let mut avg_max_LTV: Decimal = Decimal::zero();
    let mut avg_borrow_LTV: Decimal = Decimal::zero();

    if cAsset_ratios.len() == 0{
        //TODO: Change back to no values. This is for testing without oracles
       //return Ok((Decimal::percent(0), Decimal::percent(0), Decimal::percent(0)))
       return Ok((Decimal::percent(50), Decimal::percent(50), Decimal::percent(100_000_000), vec![Decimal::one()]))
    }

    //Skip unecessary calculations if length is 1
    if cAsset_ratios.len() == 1 { return Ok(( collateral_assets[0].max_borrow_LTV, collateral_assets[0].max_LTV, total_value, cAsset_prices ))}
    
    for (i, _cAsset) in collateral_assets.clone().iter().enumerate(){
        avg_borrow_LTV += decimal_multiplication(cAsset_ratios[i], collateral_assets[i].max_borrow_LTV);
    }

    for (i, _cAsset) in collateral_assets.clone().iter().enumerate(){
        avg_max_LTV += decimal_multiplication(cAsset_ratios[i], collateral_assets[i].max_LTV);
    }
    

    Ok((avg_borrow_LTV, avg_max_LTV, total_value, cAsset_prices))
}

pub fn insolvency_check_imut( //Returns true if insolvent, current_LTV and available fee to the caller if insolvent
    storage: &dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    collateral_assets: Vec<cAsset>,
    credit_amount: Decimal,
    credit_price: Decimal,
    max_borrow: bool, //Toggle for either over max_borrow or over max_LTV (liquidatable), ie taking the minimum collateral ratio into account.
    config: Config,
) -> StdResult<(bool, Decimal, Uint128)>{

    //No assets but still has debt
    if collateral_assets.len() == 0 && !credit_amount.is_zero(){
        return Ok( (true, Decimal::percent(100), Uint128::zero()) )
    }
    
    let avg_LTVs: (Decimal, Decimal, Decimal, Vec<Decimal>) = get_avg_LTV_imut(storage, env, querier, collateral_assets, config)?;
    
    let asset_values: Decimal = avg_LTVs.2; //pulls total_asset_value
    
    let mut check: bool;
    let current_LTV = decimal_division( decimal_multiplication(credit_amount, credit_price) , asset_values);

    match max_borrow{
        true => { //Checks max_borrow
            check = current_LTV > avg_LTVs.0;
        },
        false => { //Checks max_LTV
            check = current_LTV > avg_LTVs.1;
        },
    }

    let available_fee = if check{
        match max_borrow{
            true => { //Checks max_borrow
                (current_LTV - avg_LTVs.0) * Uint128::new(1)
            },
            false => { //Checks max_LTV
                (current_LTV - avg_LTVs.1) * Uint128::new(1)
            },
        }
    } else {
        Uint128::zero()
    };

    Ok( (check, current_LTV, available_fee) )
}

