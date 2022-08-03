

use std::env;
use std::str::FromStr;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult, StdError, Addr, Uint128, QueryRequest, WasmQuery, Decimal, CosmosMsg, WasmMsg, BankMsg, Coin, from_binary, Order, Storage, Api, QuerierWrapper, Querier, SubMsg, Reply, attr, coin};
use cw2::set_contract_version;
use cw20::{Cw20ReceiveMsg, Cw20ExecuteMsg};
use cw_storage_plus::Bound;
use cosmwasm_bignumber::{ Uint256, Decimal256 };
use osmo_bindings::{ SpotPriceResponse, OsmosisMsg, FullDenomResponse, OsmosisQuery };

use membrane::stability_pool::{ExecuteMsg as SP_ExecuteMsg};
use membrane::positions::{ExecuteMsg, InstantiateMsg, QueryMsg, Cw20HookMsg, PositionResponse, PositionsResponse, BasketResponse, ConfigResponse, PropResponse, CallbackMsg};
use membrane::types::{ AssetInfo, Asset, cAsset, Basket, Position, LiqAsset, RepayPropagation, SellWallDistribution };
use membrane::osmosis_proxy::{ QueryMsg as OsmoQueryMsg, GetDenomResponse };
use membrane::debt_auction::{ ExecuteMsg as AuctionExecuteMsg };


//use crate::liq_queue::LiquidatibleResponse;
use crate::math::{decimal_multiplication, decimal_division, decimal_subtraction};
use crate::error::ContractError;
use crate::positions::{create_basket, assert_basket_assets, assert_sent_native_token_balance, deposit, withdraw, increase_debt, repay, liq_repay, edit_contract_owner, liquidate, edit_basket, sell_wall_using_ids, SELL_WALL_REPLY_ID, STABILITY_POOL_REPLY_ID, LIQ_QUEUE_REPLY_ID, withdrawal_msg, update_position_claims, CREATE_DENOM_REPLY_ID, BAD_DEBT_REPLY_ID};
use crate::query::{query_stability_pool_liquidatible, query_config, query_position, query_user_positions, query_basket_positions, query_basket, query_baskets, query_prop, query_stability_pool_fee, query_basket_debt_caps, query_bad_debt};
//use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg, AssetInfo, Cw20HookMsg, Asset, PositionResponse, PositionsResponse, BasketResponse, LiqModuleMsg};
//use crate::stability_pool::{Cw20HookMsg as SP_Cw20HookMsg, QueryMsg as SP_QueryMsg, LiquidatibleResponse as SP_LiquidatibleResponse, PoolResponse, ExecuteMsg as SP_ExecuteMsg};
//use crate::liq_queue::{ExecuteMsg as LQ_ExecuteMsg, QueryMsg as LQ_QueryMsg, LiquidatibleResponse as LQ_LiquidatibleResponse, Cw20HookMsg as LQ_Cw20HookMsg};
use crate::state::{Config, CONFIG, POSITIONS, BASKETS,  REPAY };

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:cdp";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");





//TODO: //Add function to update existing cAssets and Baskets and Config

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
      
    let mut config = Config {
        liq_fee: msg.liq_fee,
        owner: info.sender.clone(),
        current_basket_id: Uint128::from(1u128),
        stability_pool: None, 
        dex_router: None,
        fee_collector: None,
        osmosis_proxy: None,    
        debt_auction: None,    
        oracle_time_limit: msg.oracle_time_limit,
        debt_minimum: msg.debt_minimum,
    };
    
    // //Set optional config parameters
    match msg.stability_pool {
        Some( address ) => {
            
            match deps.api.addr_validate( &address ){
                Ok( addr ) => config.stability_pool = Some( addr ),
                Err(_) => {},
            }
        },
        None => {},
    };

    match msg.dex_router {
        Some( address ) => {
            
            match deps.api.addr_validate( &address ){
                Ok( addr ) => config.dex_router = Some( addr ),
                Err(_) => {},
            }
        },
        None => {},
    };

    match msg.fee_collector {
        Some( address ) => {
            
            match deps.api.addr_validate( &address ){
                Ok( addr ) => config.fee_collector = Some( addr ),
                Err(_) => {},
            }
        },
        None => {},
    };

    match msg.osmosis_proxy {
        Some( address ) => {
            
            match deps.api.addr_validate( &address ){
                Ok( addr ) => config.osmosis_proxy = Some( addr ),
                Err(_) => {},
            }
        },
        None => {},
    };

    match msg.debt_auction {
        Some( address ) => {
            
            match deps.api.addr_validate( &address ){
                Ok( addr ) => config.debt_auction = Some( addr ),
                Err(_) => {},
            }
        },
        None => {},
    };

    let current_basket_id = &config.current_basket_id.clone().to_string();

    CONFIG.save(deps.storage, &config)?;

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let mut create_res = Response::new();
    let mut attrs = vec![];
    let sender = &info.sender.clone().to_string();

    attrs.push(("method", "instantiate"));
    attrs.push(("owner", sender));
    

    if msg.collateral_types.is_some() && msg.credit_asset.is_some(){

        let mut check = true;
        let collateral_types = msg.collateral_types.unwrap();

        //cAsset checks
        for cAsset in collateral_types.clone(){
            if cAsset.max_borrow_LTV >= cAsset.max_LTV && cAsset.max_borrow_LTV < Decimal::from_ratio( Uint128::new(100u128), Uint128::new(1u128)){
                check = false;
            }
        }
        if( check ) && msg.credit_asset.is_some(){
            create_res = create_basket(
                deps,
                info,
                msg.owner,
                collateral_types.clone(),
                msg.credit_asset.unwrap(),
                msg.credit_price,
                msg.credit_interest,
            )?;
            
            attrs.push(("basket_id", current_basket_id));
        }else{
            attrs.push(("basket_status", "Not created: cAsset.max_LTV can't be less than or equal to cAsset.max_borrow_LTV"));
        }
        
    }else{
        attrs.push(("basket_status", "Not created: Basket only created w/ collateral_types AND credit_asset filled"));
    }

    //response.add_attributes(attrs);
    Ok(create_res.add_attributes(attrs))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Receive(msg) => receive_cw20(deps, env, info, msg),
        ExecuteMsg::Deposit{ assets, position_owner, position_id, basket_id} => {
            let mut valid_assets = vec![];
            
            for asset in assets.clone(){
                valid_assets.push( assert_sent_native_token_balance( asset, &info )? );
            }
            let cAssets: Vec<cAsset> = assert_basket_assets(deps.storage, basket_id, valid_assets, true)?;
            deposit(deps, info, position_owner, position_id, basket_id, cAssets)
        }
    ,
        ExecuteMsg::Withdraw{ position_id, basket_id, assets } => {
            let cAssets: Vec<cAsset> = assert_basket_assets(deps.storage, basket_id, assets, false)?;
            withdraw(deps, env, info, position_id, basket_id, cAssets)
        },
        
        ExecuteMsg::IncreaseDebt { basket_id, position_id, amount } => increase_debt(deps, env, info, basket_id, position_id, amount),
        ExecuteMsg::Repay { basket_id, position_id, position_owner} => {
            let basket: Basket = match BASKETS.load(deps.storage, basket_id.to_string()) {
                Err(_) => { return Err(ContractError::NonExistentBasket {  })},
                Ok( basket ) => { basket },
            };

            let credit_asset = assert_sent_native_token_balance(basket.credit_asset.info, &info)?;
            repay(deps.storage, deps.querier, deps.api, env, info, basket_id, position_id, position_owner, credit_asset)
        },
        ExecuteMsg::LiqRepay { credit_asset} => {
            let credit_asset = assert_sent_native_token_balance(credit_asset.info, &info)?;
            liq_repay(deps, env, info, credit_asset)
        }
        ExecuteMsg::EditAdmin { owner } => edit_contract_owner(deps, info, owner),
        ExecuteMsg::EditBasket {basket_id,added_cAsset,owner,credit_interest, liq_queue, pool_ids, liquidity_multiplier } => edit_basket(deps, info, basket_id, added_cAsset, owner, credit_interest, liq_queue, pool_ids, liquidity_multiplier ),
        ExecuteMsg::CreateBasket { owner, collateral_types, credit_asset, credit_price, credit_interest } => create_basket(deps, info, owner, collateral_types, credit_asset, credit_price, credit_interest ),
        ExecuteMsg::Liquidate { basket_id, position_id, position_owner } => liquidate(deps.storage, deps.api, deps.querier, env, info, basket_id, position_id, position_owner),
        ExecuteMsg::Callback( msg ) => {
            if info.sender == env.contract.address{
                callback_handler( deps, msg )
            }else{
                return Err( ContractError::Unauthorized {  } )
            }
        },
        
     
    }
}

pub fn callback_handler(
    deps: DepsMut,
    msg: CallbackMsg,
) -> Result<Response, ContractError>{
    
    match msg {
        CallbackMsg::BadDebtCheck { basket_id, position_owner, position_id } => {
            check_for_bad_debt( deps, basket_id, position_id, position_owner )
        },
    }
}

fn check_for_bad_debt(
    deps: DepsMut,
    basket_id: Uint128,
    position_id: Uint128,
    position_owner: Addr,
) -> Result<Response, ContractError>{

    let config: Config = CONFIG.load( deps.storage )?;

    let basket: Basket= match BASKETS.load(deps.storage, basket_id.to_string()) {
        Err(_) => { return Err(ContractError::NonExistentBasket {  })},
        Ok( basket ) => { basket },
    };
    let positions: Vec<Position> = match POSITIONS.load(deps.storage, (basket_id.to_string(), position_owner.clone())){
        Err(_) => {  return Err(ContractError::NoUserPositions {  }) },
        Ok( positions ) => { positions },
    };

    //Filter position by id
    let target_position = match positions.into_iter().find(|x| x.position_id == position_id) {
        Some(position) => position,
        None => return Err(ContractError::NonExistentPosition {  }) 
    };

    //We do a lazy check for bad debt by checking if there is debt without any assets left in the position
    //This is allowed bc any calls here will be after a liquidation where the sell wall would've sold all it could to cover debts
    let total_assets: Uint128 = 
        target_position.collateral_assets
            .iter()
            .map(|asset| asset.asset.amount)
            .collect::<Vec<Uint128>>()
            .iter()
            .sum();

    if total_assets > Uint128::zero(){
        return Err( ContractError::PositionSolvent {  } )
    }else{
        let mut message: CosmosMsg;

        //Send bad debt amount to the auction contract
        if config.debt_auction.is_some(){
            let auction_msg = AuctionExecuteMsg::StartAuction {
                    basket_id, 
                    position_id, 
                    position_owner: position_owner.to_string(), 
                    debt_amount: target_position.credit_amount * Uint128::new(1u128)
                };

            message = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.debt_auction.unwrap().to_string(), 
                msg: to_binary(&auction_msg)?, 
                funds: vec![ ],
            })
        }else{
            return Err( ContractError::CustomError { val: "Debt Auction contract not added to config".to_string() } )
        }
        

        return Ok( Response::new().add_message(message) )
    }
}

//From a receive cw20 hook. Comes from the contract address so easy to validate sent funds. 
//Check if sent funds are equal to amount in msg so we don't have to recheck in the function
pub fn receive_cw20(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    cw20_msg: Cw20ReceiveMsg,
) -> Result<Response, ContractError> {

    let passed_asset: Asset = Asset {
        info: AssetInfo::Token {
            address: info.sender.clone(),
        },
        amount: cw20_msg.amount,
    };

    match from_binary(&cw20_msg.msg){
        //This only allows 1 cw20 token at a time when opening a position, whereas you can add multiple native assets
        Ok(Cw20HookMsg::Deposit { position_owner, basket_id, position_id}) => {      
            let valid_owner_addr: Addr = if let Some(position_owner) = position_owner {
                deps.api.addr_validate(&position_owner)?
            }else {
                deps.api.addr_validate(&cw20_msg.sender.clone())?
            };

            let cAssets: Vec<cAsset> = assert_basket_assets(deps.storage, basket_id, vec![ passed_asset ], true)?;

            deposit(deps, info, Some(valid_owner_addr.to_string()), position_id, basket_id, cAssets) 
        },

        Ok(Cw20HookMsg::Repay { basket_id, position_id, position_owner}) => {

            repay(deps.storage, deps.querier, deps.api, env, info, basket_id, position_id, position_owner, passed_asset )
        }
        Err(_) => Err(ContractError::Cw20MsgError {}),
    }

}



#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    //panic!("here".to_string());
    match msg.id {
        LIQ_QUEUE_REPLY_ID => handle_liq_queue_reply(deps, msg),
        STABILITY_POOL_REPLY_ID => handle_stability_pool_reply(deps, env, msg),
        SELL_WALL_REPLY_ID => handle_sell_wall_reply(deps, msg),
        CREATE_DENOM_REPLY_ID => handle_create_denom_reply(deps, msg),
        BAD_DEBT_REPLY_ID => Ok( Response::new()),
        id => Err(StdError::generic_err(format!("invalid reply id: {}", id))),
    }
}

fn handle_create_denom_reply(deps: DepsMut, msg: Reply) -> StdResult<Response>{
    match msg.result.into_result(){
        Ok( result ) => {

            let instantiate_event = result
                .events
                .into_iter()
                .find(|e| {
                    e.attributes
                        .iter()
                        .any(|attr| attr.key == "basket_id")
                })
                .ok_or_else(|| StdError::generic_err(format!("unable to find create_denom event")))?;

            let subdenom = &instantiate_event.attributes
                .iter()
                .find(|attr| attr.key == "subdenom")
                .unwrap()
                .value;

            let basket_id = &instantiate_event.attributes
                .iter()
                .find(|attr| attr.key == "basket_id")
                .unwrap()
                .value;

            let config: Config = CONFIG.load( deps.storage )?;

            //Query fulldenom to save to basket 
            let res: GetDenomResponse = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
                msg: to_binary(&OsmoQueryMsg::GetDenom {   
                    creator_address: config.osmosis_proxy.unwrap().to_string(),
                    subdenom: subdenom.to_string(),
                })?,
            }))?;

            BASKETS.update( deps.storage, basket_id.to_string(), |basket| -> StdResult<Basket>{
                match basket{
                    Some( mut basket ) => {
                        
                        basket.credit_asset = Asset {
                            info: AssetInfo::NativeToken { denom: res.denom },
                            ..basket.credit_asset
                        };

                        Ok( basket )
                    },
                    None => {return Err( StdError::GenericErr { msg: "Non-existent basket".to_string() } )},
                }
            })?;
                
        },//We only reply on success 
        Err( err ) => {return Err( StdError::GenericErr { msg: err } )}

    }


    Ok( Response::new() ) 
}

fn handle_stability_pool_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response>{

    match msg.result.into_result(){
         Ok(result)  => {
            //1) Parse potential leftover amount and send to sell_wall if there is any
            //Don't need to change state bc the SP will be repaying thru the contract
            //There should only be leftover here if the SP loses funds between the query and the repayment
            //2) Send collateral to the SP in the repay function and call distribute

            let mut res = Response::new();

            let liq_event = result
                .events
                .iter()
                .find(|e| {
                    e.attributes
                        .iter()
                        .any(|attr| attr.key == "leftover_repayment")
                })
                .ok_or_else(|| StdError::generic_err(format!("unable to find stability pool event")))?;

            let leftover = &liq_event.attributes
                .iter()
                .find(|attr| attr.key == "leftover_repayment")
                .unwrap()
                .value;

            let leftover_amount = Uint128::from_str(&leftover)?;


            let mut repay_propagation = REPAY.load(deps.storage)?;
            let mut submessages = vec![];

            //Success w/ leftovers: Sell Wall combined leftovers
            //Success w/o leftovers: Send LQ leftovers to the SP
            //Error: Sell Wall combined leftovers
            if leftover_amount != Uint128::zero(){


                //Sell Wall SP leftovers and LQ leftovers
                let ( sell_wall_msgs, collateral_distributions ) = sell_wall_using_ids( 
                    deps.storage,
                    env,
                    deps.querier,
                    repay_propagation.clone().basket_id,
                    repay_propagation.clone().position_id,
                    repay_propagation.clone().position_owner,
                    repay_propagation.clone().liq_queue_leftovers + Decimal::from_ratio(leftover_amount, Uint128::new(1u128)),
                    )?;
        
                submessages.extend( sell_wall_msgs.
                    into_iter()
                    .map(|msg| {
                        
                        SubMsg::reply_on_success(msg, SELL_WALL_REPLY_ID)
                    }).collect::<Vec<SubMsg>>() );
                    
                
                repay_propagation.sell_wall_distributions = add_distributions( repay_propagation.clone().sell_wall_distributions, SellWallDistribution {distributions: collateral_distributions} , );
                
                //Save to propagate
                REPAY.save(deps.storage, &repay_propagation)?;
                
            }else{
                //Send LQ leftovers to SP
                //This is an SP reply so we don't have to check if the SP is okay to call 
                let config: Config = CONFIG.load(deps.storage)?;

                let basket: Basket = BASKETS.load(deps.storage, repay_propagation.clone().basket_id.to_string() )?;
                
                //let sp_liq_fee = query_stability_pool_fee( deps.querier, config.clone(), basket.clone() )?;

                //Check for stability pool funds before any liquidation attempts
                //Sell wall any leftovers
                let leftover_repayment = 
                        query_stability_pool_liquidatible(
                            deps.querier, 
                            config.clone(), 
                            repay_propagation.liq_queue_leftovers,
                             basket.clone().credit_asset.info
                        )?;

                        
                if leftover_repayment > Decimal::zero(){

                    //Sell wall remaining
                    let ( sell_wall_msgs, collateral_distributions ) = sell_wall_using_ids( 
                        deps.storage,
                        env,
                        deps.querier, 
                        repay_propagation.clone().basket_id,
                        repay_propagation.clone().position_id,
                        repay_propagation.clone().position_owner,
                        leftover_repayment,
                        )?;
                    
                    //Save new distributions from this liquidations
                    repay_propagation.sell_wall_distributions = add_distributions(repay_propagation.sell_wall_distributions, SellWallDistribution {distributions: collateral_distributions} );
                    REPAY.save(deps.storage, &repay_propagation)?;

                    submessages.extend( sell_wall_msgs.
                        into_iter()
                        .map(|msg| {
                            //If this succeeds, we update the positions collateral claims
                            //If this fails, do nothing. Try again isn't a useful alternative.
                            SubMsg::reply_on_success(msg, SELL_WALL_REPLY_ID)
                        }).collect::<Vec<SubMsg>>() );

                }
                //Send whatever u can to the Stability Pool
                let sp_repay_amount = repay_propagation.liq_queue_leftovers - leftover_repayment;
                
                
                if !sp_repay_amount.is_zero(){
                    //Stability Pool message builder
                    let liq_msg = SP_ExecuteMsg::Liquidate {
                        credit_asset: LiqAsset{
                            amount: sp_repay_amount,
                            info: basket.clone().credit_asset.info,
                        },
                    };
                    
                    let msg: CosmosMsg =  CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: config.stability_pool.unwrap().to_string(),
                        msg: to_binary(&liq_msg)?,
                        funds: vec![],
                    });

                    let sub_msg: SubMsg = SubMsg::reply_always(msg, STABILITY_POOL_REPLY_ID);

                    submessages.push( sub_msg );

                    //Remove repayment from leftovers
                    repay_propagation.liq_queue_leftovers -= sp_repay_amount;
                    REPAY.save(deps.storage, &repay_propagation)?;
                    

                }
                
            }
            
            //TODO: Add detail
            Ok( res.add_submessages(submessages) )

             
            
        },
        Err( _ ) => {
            //If error, sell wall the SP repay amount and LQ leftovers
            let mut repay_propagation = REPAY.load(deps.storage)?;

            //Sell wall remaining
            let ( sell_wall_msgs, collateral_distributions ) = sell_wall_using_ids( 
                deps.storage,
                env,
                deps.querier,
                repay_propagation.clone().basket_id,
                repay_propagation.clone().position_id,
                repay_propagation.clone().position_owner,
                repay_propagation.liq_queue_leftovers + repay_propagation.stability_pool,
                )?;

            
            
            //Save new distributions from this liquidations
            repay_propagation.sell_wall_distributions = add_distributions(repay_propagation.sell_wall_distributions, SellWallDistribution {distributions: collateral_distributions} );
            REPAY.save(deps.storage, &repay_propagation)?;
            
            let res = Response::new().add_submessages( sell_wall_msgs.
                into_iter()
                .map(|msg| {
                    //If this succeeds, we update the positions collateral claims
                    //If this fails, do nothing. Try again isn't a useful alternative.
                    SubMsg::reply_on_success(msg, SELL_WALL_REPLY_ID)
                }).collect::<Vec<SubMsg>>() );

            //TODO: Add detail
            Ok( res )

        }        
    }        
}

//Add to the front of the "queue" bc message semantics are depth first
//LIFO
fn add_distributions(
    mut old_distributions: Vec<SellWallDistribution>,
    new_distrbiutions: SellWallDistribution,
)-> Vec<SellWallDistribution>{
    
    old_distributions.push( new_distrbiutions );

    old_distributions
}

fn handle_liq_queue_reply(deps: DepsMut, msg: Reply) -> StdResult<Response>{

    match msg.result.into_result(){
         Ok(result)  => {
            //1) Parse potential repaid_amount and substract from running total
            //2) Send collateral to the Queue
            

            let liq_event = result
                .events
                .into_iter()
                .find(|e| {
                    e.attributes
                        .iter()
                        .any(|attr| attr.key == "repay_amount")
                })
                .ok_or_else(|| StdError::generic_err(format!("unable to find liq-queue event")))?;

            let repay = &liq_event.attributes
                .iter()
                .find(|attr| attr.key == "repay_amount")
                .unwrap()
                .value;

            
            let repay_amount = Uint128::from_str(&repay)?;

            let mut prop: RepayPropagation = REPAY.load(deps.storage)?;

            let basket = BASKETS.load(deps.storage, prop.basket_id.to_string())?;
            
            let config = CONFIG.load(deps.storage)?;

            //Send successfully liquidated amount
            let amount = &liq_event.attributes
                .iter()
                .find(|attr| attr.key == "collateral_amount")
                .unwrap()
                .value;

            let send_amount = Uint128::from_str(&amount)?;

            let token = &liq_event.attributes
                .iter()
                .find(|attr| attr.key == "collateral_token")
                .unwrap()
                .value;

            let asset_info = &liq_event.attributes
                .iter()
                .find(|attr| attr.key == "collateral_info")
                .unwrap()
                .value;
            
            let token_info: AssetInfo = if asset_info.eq(&"token".to_string()){
                    AssetInfo::Token { address: deps.api.addr_validate(&token)? }
                } else {
                    AssetInfo::NativeToken { denom: token.to_string() }
                };
            

            let msg = withdrawal_msg( 
                Asset {
                    info: token_info.clone(),
                    amount: send_amount,
                },
                basket.liq_queue.unwrap()
             )?;

                          
             //Subtract repaid amount from LQs repay responsibility. If it hits 0 then there were no LQ errors.
             if repay_amount != Uint128::zero(){

                prop.liq_queue_leftovers = decimal_subtraction( prop.liq_queue_leftovers, Decimal::from_ratio(repay_amount, Uint128::new(1u128)));              

                REPAY.save(deps.storage, &prop)?;
                //SP reply handles LQ_leftovers 

                update_position_claims(deps.storage, prop.basket_id, prop.position_id, prop.position_owner, token_info, send_amount)?;
            }

            
            //TODO: Add detail
            Ok(Response::new().add_message(msg))

             
            
        },
        Err( string ) => {
            //If error, do nothing
            //The SP reply will handle the sell wall
            Ok( Response::new().add_attribute( "error", string) )
        }        
    }        
}

fn handle_sell_wall_reply(deps: DepsMut, msg: Reply) -> StdResult<Response>{

    
    match msg.result.into_result(){ 

        Ok( result ) => {
            //On success we update the position owner's claims bc it means the protocol sent assets on their behalf
            let mut repay_propagation = REPAY.load( deps.storage )?;
            
            let mut res = Response::new();
            let mut attrs = vec![];

            //We use the distribution at the end of the list bc new ones were appended, and msgs are fulfilled depth first.
            match repay_propagation.sell_wall_distributions.pop(){
                Some( distribution ) => {

                    //Update position claims for each distributed asset
                    for (asset, amount) in distribution.distributions{
                        update_position_claims(
                            deps.storage, 
                            repay_propagation.clone().basket_id, 
                            repay_propagation.clone().position_id, 
                            repay_propagation.clone().position_owner, 
                            asset.clone(), 
                            (amount * Uint128::new(1u128)),
                        )?;

                        let res_asset = LiqAsset {
                            info: asset,
                            amount,
                        };
                        attrs.push( ("distribution", res_asset.to_string()) );
                    }
                },
                None => { 
                    //If None it means the distribution wasn't added when the sell wall msg was added which should be impossible 
                    //Either way, Error
                    return Err( StdError::GenericErr { msg: "Distributions were added to the state propagation incorrectly".to_string() } )
                    }       
            }

            //Save propagation w/ removed tail
            REPAY.save(deps.storage, &repay_propagation)?;

            Ok( res.add_attributes(attrs) )
            
        }
        Err( string ) => {
            //This is only reply_on_success so this shouldn't be reached
            Ok( Response::new().add_attribute( "error", string) )
        }        
    }
}



#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => { to_binary(&query_config(deps)?) }
        QueryMsg::GetPosition { position_id, basket_id, user} => {
            let valid_addr: Addr = deps.api.addr_validate(&user)?;
            to_binary(&query_position(deps, position_id, basket_id, valid_addr)?)
        },
        QueryMsg::GetUserPositions { basket_id, user, limit } => {
            let valid_addr: Addr = deps.api.addr_validate(&user)?;
            to_binary(&query_user_positions(deps, basket_id, valid_addr, limit)?)
        },
        QueryMsg::GetBasketPositions { basket_id, start_after, limit } => {
            to_binary(&query_basket_positions(deps, basket_id, start_after, limit)?)
        },
        QueryMsg::GetBasket { basket_id } => {
            to_binary(&query_basket(deps, basket_id)?)
        },
        QueryMsg::GetAllBaskets { start_after, limit } => {
            to_binary(&query_baskets(deps, start_after, limit)?)
        },
        QueryMsg::Propagation {  } => {
            to_binary(&query_prop( deps )?)
        },
        QueryMsg::GetBasketDebtCaps { basket_id } => {
            to_binary( &query_basket_debt_caps(deps, env, basket_id)?)
        },
        QueryMsg::GetBasketBadDebt { basket_id } => {
            to_binary( &query_bad_debt( deps, basket_id )? ) 
        },
    }
}





