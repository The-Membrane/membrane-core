use std::cmp::min;
use std::str::FromStr;

use cosmwasm_std::{attr, to_json_binary, Addr, Api, Attribute, BankMsg, Coin, CosmosMsg, Decimal, Env, MessageInfo, QuerierWrapper, QueryRequest, ReplyOn, Response, StdError, StdResult, Storage, SubMsg, Uint128, WasmMsg, WasmQuery};
use osmosis_std::shim::Duration;
use osmosis_std::types::osmosis::downtimedetector::v1beta1::DowntimedetectorQuerier;

use membrane::helpers::{pool_query_and_exit, query_stability_pool_fee, asset_to_coin, validate_position_owner};
use membrane::math::{decimal_multiplication, decimal_division, decimal_subtraction, Uint256};
use membrane::cdp::{Config, ExecuteMsg, CallbackMsg};
use membrane::oracle::PriceResponse;
use membrane::range_bound_lp_vault::{ExecuteMsg as RBLP_ExecuteMsg, QueryMsg as RBLP_QueryMsg, UserIntentResponse};
use membrane::osmosis_proxy::QueryMsg as OsmoQueryMsg;
use membrane::stability_pool::{LiquidatibleResponse as SP_LiquidatibleResponse, ExecuteMsg as SP_ExecuteMsg, QueryMsg as SP_QueryMsg};
use membrane::liq_queue::{ExecuteMsg as LQ_ExecuteMsg, QueryMsg as LQ_QueryMsg, LiquidatibleResponse as LQ_LiquidatibleResponse};
use membrane::staking::ExecuteMsg as StakingExecuteMsg;
use membrane::deployable_venue::{ExecuteMsg as DeployableVenue_ExecuteMsg, QueryMsg as DeployableVenue_QueryMsg};
use membrane::chain_proxy::ExecuteMsg as ChainProxyExecuteMsg;
use membrane::types::{cAsset, Asset, AssetInfo, AssetPool, Basket, DeploymentVenue, PoolStateResponse, Position, StringEntry, UserInfo};

use crate::contract::set_active_deployment_venues;
use crate::error::ContractError; 
use crate::positions::{BAD_DEBT_REPLY_ID, DEPLOYABLE_VENUE_REPLY_ID, LIQ_QUEUE_REPLY_ID, SELL_COLLATERAL_REPLY_ID};
use crate::query::{insolvency_check, get_cAsset_ratios};
use crate::risk_engine::update_basket_tally;
use crate::state::{create_collateral_rate_assurance, get_target_position, update_position, DeployableVenuePropagation, LiquidationPropagation, LiquidationStat, SellCollateralPropagation, Timer, BASKET, CONFIG, DEPLOYABLE_VENUE, FREEZE_TIMER, LIQUIDATION, LIQUIDATION_STATS, SELL_COLLATERAL};

pub const SECONDS_PER_DAY: u64 = 86400;
pub const BAD_DEBT_CALLER_FEE: Decimal = Decimal::percent(1);

/// Confirms insolvency and calculates repayment amount,
/// then sends liquidation messages to the modules if they have funds.
/// If not, sell wall.
#[allow(unused_variables)]
pub fn liquidate(
    storage: &mut dyn Storage,
    api: &dyn Api,
    querier: QuerierWrapper,
    env: Env,
    info: MessageInfo,
    position_id: Uint128,
    position_owner: String,
) -> Result<Response, ContractError> {
    //Check for Osmosis downtime 
    // match DowntimedetectorQuerier::new(&querier)
    //     .recovered_since_downtime_of_length(
    //         10 * 60 * 8, //8 hours from 6 second blocks
    //         Some(Duration {
    //             seconds: 60 * 60 * 1, //1 hour
    //             nanos: 0,
    //         })
    // ){
    //     Ok(resp) => {            
    //         if !resp.succesfully_recovered {
    //             return Err(ContractError::CustomError { val: String::from("Downtime recovery window hasn't elapsed yet ") })
    //         }
    //     },
    //     Err(_) => (),
    // };

    let mut basket: Basket = BASKET.load(storage)?;
    //Check if frozen
    if basket.frozen {
        return Err(ContractError::Frozen {});
    }

    //Check contract downtime
    let freeze_timer = match FREEZE_TIMER.load(storage){
        Ok(timer) => timer,
        Err(_) => Timer {
            start_time: 0,
            end_time: 0,
        },
    };
    if (env.block.time.seconds().checked_sub(freeze_timer.end_time).unwrap_or_else(|| SECONDS_PER_DAY/24)) < (SECONDS_PER_DAY/24){ //1 hour grace
        return Err(ContractError::Std(StdError::GenericErr { msg: format!("You can liquidate in {} seconds, there is a post-freeze grace period", (SECONDS_PER_DAY/24) - (env.block.time.seconds() - freeze_timer.end_time)) }));
    }

    //Load state
    let mut config: Config = CONFIG.load(storage)?;
    let valid_position_owner =
        validate_position_owner(api, info.clone(), Some(position_owner.clone()))?;

    let (_i, mut target_position) = get_target_position(
        storage,
        valid_position_owner.clone(),
        position_id,
    )?;

    //Check position health compared to max_LTV
    let (
        (insolvent, current_LTV, _available_fee), 
        (avg_borrow_LTV, avg_max_LTV, total_value, cAsset_prices_res, cAsset_ratios)
    ) = match insolvency_check(
        storage,
        env.clone(),
        querier,
        Some(basket.clone()),
        target_position.clone().collateral_assets,
        target_position.clone().credit_amount,
        basket.clone().credit_price,
        false,
        config.clone(),
    ){
        Ok(res) => res,
        Err(err) => return Err(ContractError::CustomError { val: String::from(format!("Insolvency check failed: {:?}", err)) }),
    };

    //REMOVE, FOR TESTING
    let insolvent = true;
    let current_LTV = Decimal::percent(90);
    
    if !insolvent {
        return Err(ContractError::PositionSolvent {});
    }

    //Convert from Response to price (Decimal)
    let cAsset_prices = cAsset_prices_res.clone().into_iter().map(|price| price.price).collect::<Vec<Decimal>>();
    
    //Get repay value and repay_amount
    let (pre_user_repay_repay_value, mut credit_repay_amount) = match get_repay_quantities(
        config.clone(),
        basket.clone(),
        target_position.clone(),
        current_LTV,
        avg_borrow_LTV,
        total_value,
    ){
        Ok(res) => res,
        Err(err) => return Err(ContractError::CustomError { val: String::from(format!("Repay quantities failed: {:?}", err)) }),
    };

    // Don't send any funds here, only send UserInfo and repayment amounts.
    // We want to act on the reply status but since SubMsg state won't revert if we catch the error,
    // assets we send prematurely won't come back.

    let res = Response::new();
    let mut submessages = vec![];
    let mut caller_fee_messages: Vec<CosmosMsg> = vec![];
    let mut attrs: Vec<Attribute> = vec![];

    //Set collateral_assets
    let mut collateral_assets = target_position.clone().collateral_assets;

    //Dynamic fee that goes to the caller (info.sender): current_LTV - max_LTV
    let mut caller_fee = match decimal_subtraction(current_LTV, avg_max_LTV){
        Ok(res) => res,
        Err(_) => return Err(ContractError::CustomError { val: "Caller fee calculation failed".to_string() }),
    };

    //Set pre-user repay amount 
    let pre_user_repay_repay_amount = credit_repay_amount;
    // println!("pre_user_repay_repay_amount: {:?}", pre_user_repay_repay_amount);

    // Track liquidation stat
    let mut liquidation_stats = match LIQUIDATION_STATS.load(storage) {
        Ok(maybe) => maybe,
        Err(_) => vec![],
    };
    liquidation_stats.push(LiquidationStat {
        block_time: env.block.time.seconds(),
        position_id,
        collateral_assets: target_position
            .collateral_assets
            .iter()
            .map(|c| c.asset.clone())
            .collect(),
        amount_liquidated: pre_user_repay_repay_amount.to_uint_floor(),
    });
    // Enforce stat limit
    let stat_limit = config.liquidation_stat_limit as usize;
    if stat_limit == 0 {
        liquidation_stats.clear();
    } else if liquidation_stats.len() > stat_limit {
        let excess = liquidation_stats.len() - stat_limit;
        liquidation_stats.drain(0..excess);
    }
    LIQUIDATION_STATS.save(storage, &liquidation_stats)?;

    //Get amount of repayment user can repay from the Stability Pool
    // let user_sp_repay_amount = get_user_repay_amount(querier, config.clone(), basket.clone(), position_id, position_owner.clone(), &mut credit_repay_amount, &mut submessages)?;
    // attrs.push(
    //     attr("user_sp_repay_amount", user_sp_repay_amount.to_string())
    // );

    //Get amount of repayment user can repay from its deployed Venues
    let user_repay_amount = get_deployable_venues_user_repay_amount(
        storage,
        querier, 
        config.clone(), 
        basket.clone(), 
        position_id, 
        position_owner.clone(), 
        &mut target_position,
        &mut credit_repay_amount, 
        &mut submessages,
        &mut attrs,
    )?;
    // println!("user_repay_amount: {:?}", user_repay_amount);
    
    //Account for rounding leaving leftovers
    if credit_repay_amount == Decimal::one(){
        credit_repay_amount = Decimal::zero();
    }
    
    //Track total leftover repayment after the liq_queue
    let leftover_repayment: Decimal = credit_repay_amount;
    // println!("leftover_repayment: {:?}", leftover_repayment);
    //Set repay value to the repay_value post user_repay
    let repay_value = basket.clone().credit_price.get_value(credit_repay_amount.to_uint_floor())?;

    //Track repay_amount_per_asset
    let mut per_asset_repayment: Vec<Decimal> = vec![];
    let mut liquidated_assets: Vec<cAsset> = vec![];
    //Track value paid within the mechanism
    let mut caller_fee_value_paid = Decimal::zero();

    let mut leftover_position_value = total_value;

    //If caller fee * repay_value is greater than the leftover_position_value, hardcode it to 1%
    //This is to prevent the caller fee from being greater than the value of the position
    let caller_fee_value = match decimal_multiplication(caller_fee, repay_value){
        Ok(value) => value,
        Err(_) => return Err(ContractError::CustomError { val: "Caller fee calculation failed".to_string() }),
    };
    let protocol_fee_value = match decimal_multiplication(config.liq_fee, repay_value){
        Ok(value) => value,
        Err(_) => return Err(ContractError::CustomError { val: "Protocol fee calculation failed".to_string() }),
    };
    if caller_fee_value + protocol_fee_value > leftover_position_value {
        caller_fee = min(
            BAD_DEBT_CALLER_FEE, 
            decimal_division(leftover_position_value, repay_value)?,
        );
        config.liq_fee = Decimal::zero();
    }

    // Create collateral rate assurance for liquidated assets
    let collateral_denoms: Vec<String> = liquidated_assets.iter()
        .map(|c_asset| c_asset.asset.info.to_string())
        .collect();
    let mut rate_assurance_msgs = vec![];
    if !collateral_denoms.is_empty() {
        rate_assurance_msgs = create_collateral_rate_assurance(
            storage,
            querier,
            env.clone(),
            collateral_denoms,
            &basket,
        )?;
    }

    //Calculate caller & protocol fees 
    //and amount to send to the Liquidation Queue.
    let (protocol_fee_msg, leftover_repayment) = match per_asset_fulfillments(
        storage,
        querier, 
        config.clone(), 
        basket.clone(), 
        info.sender.to_string(),
        caller_fee,
        &mut collateral_assets, 
        &mut leftover_position_value, 
        leftover_repayment.to_uint_floor(),
        repay_value,
        pre_user_repay_repay_value,
        cAsset_ratios.clone(), 
        cAsset_prices_res.clone(), 
        &mut submessages, 
        &mut caller_fee_messages, 
        &mut per_asset_repayment,
        &mut liquidated_assets,
        &mut caller_fee_value_paid,
        UserInfo {
            position_id,
            position_owner: position_owner.clone(),
        }
    ){
        Ok(res) => res,
        Err(err) => return Err(ContractError::CustomError { val: String::from(format!("Per asset fulfillments failed: {:?}", err)) }),
    };
        
    //Update collateral_assets to reflect the fees
    target_position.collateral_assets = collateral_assets;

    //If the user repaid the whole liquidation from user funds or the LQ isn't used, we need to update the position here
    if leftover_repayment.is_zero() && user_repay_amount >= pre_user_repay_repay_amount 
    || per_asset_repayment.is_empty() {
        //Update the credit
        target_position.credit_amount = match target_position.credit_amount.checked_sub(pre_user_repay_repay_amount.to_uint_floor()){
            Ok(diff) => diff,
            Err(_) => Uint128::zero(),
        };
        //Collateral from fees is updated above

        //Update supply caps
        if target_position.credit_amount.is_zero(){
            //Remove position's assets from Supply caps 
            match update_basket_tally(
                storage, 
                querier, 
                env.clone(), 
                &mut basket, 
                [target_position.clone().collateral_assets, liquidated_assets.clone()].concat(),
                target_position.clone().collateral_assets,
                false, 
                config.clone(),
                true,
            ){
                Ok(_) => {},
                Err(err) => return Err(err),
            };
        } else {
            //Remove liquidated assets from Supply caps
            match update_basket_tally(
                storage, 
                querier, 
                env.clone(), 
                &mut basket,
                liquidated_assets.clone(),
                target_position.clone().collateral_assets,
                false,
                config.clone(),
                true,
            ){
                Ok(_) => {},
                Err(err) => return Err(err),
            };
        }            
        //Update Basket
        BASKET.save(storage, &basket)?;

        //Update position w/ new credit amount
        update_position(storage, valid_position_owner.clone(), target_position.clone())?;      
    }

    //In case SP isn't used, we need to set LiquidationPropagation
    // Set repay values for reply msg
    let liquidation_propagation = LiquidationPropagation {
        per_asset_repayment,
        liq_queue_repayment: Decimal::from_ratio(leftover_repayment, Uint128::one()),
        stability_pool: Decimal::zero(),
        user_repay_amount,
        target_position,
        liquidated_assets,
        caller_fee_value_paid,
        total_repaid: user_repay_amount,
        position_owner: valid_position_owner.clone(),
        positions_contract: env.contract.address.clone(),
        sp_liq_fee: Decimal::zero(),
        cAsset_ratios,
        cAsset_prices: cAsset_prices_res,
        basket,
        config,
    };

    LIQUIDATION.save(storage, &liquidation_propagation)?;


    //Build SP msgs
    // let ( leftover_repayment ) = match build_sp_submsgs(
    //     storage, 
    //     querier,
    //     env.clone(), 
    //     config, 
    //     basket.clone(), 
    //     valid_position_owner.clone(), 
    //     Decimal::from_ratio(leftover_repayment, Uint128::one()), 
    //     credit_repay_amount, 
    //     leftover_position_value, 
    //     &mut submessages, 
    //     per_asset_repayment.clone(), 
    //     user_repay_amount,
    //     target_position.clone(),
    //     liquidated_assets,
    //     cAsset_ratios,
    //     cAsset_prices_res,
    //     caller_fee_value_paid,
    // ){
    //     Ok(res) => res,
    //     Err(err) => return Err(ContractError::CustomError { val: String::from(format!("SP submsgs failed: {:?}", err)) }),
    // };

    //Create the Bad debt callback message to be added as the last SubMsg
    let msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&ExecuteMsg::Callback(CallbackMsg::BadDebtCheck {
            position_id,
            position_owner: valid_position_owner.clone(),
        }))?,
        funds: vec![],
    });
    //The logic for this will be handled in the callback
    //Replying on Error is so any error doesn't cancel the full transaction
    //Don't care about the success case so didnt reply_always
    let call_back = SubMsg::reply_on_error(msg, BAD_DEBT_REPLY_ID);
    
    let mut liquidation_propagation: Option<String> = None;
    if let Ok(repay) = LIQUIDATION.load(storage) { liquidation_propagation = Some(format!("{:?}", repay)) }

    //Convert protocol_fee_msg to vec if it's Some, otherwise empty vec
    let protocol_fee_msgs = match protocol_fee_msg {
        Some(msg) => vec![msg],
        None => vec![],
    };

    Ok(res
        .add_submessages(submessages) //LQ & SP msgs
        .add_submessage(call_back)
        .add_messages(caller_fee_messages)
        .add_messages(protocol_fee_msgs)
        .add_messages(rate_assurance_msgs)
        .add_attributes(vec![
            attr("method", "liquidate"),
            attr(
                "propagation_info",
                format!("{:?}", liquidation_propagation.unwrap_or_else(|| String::from("None"))),
            ),
            attr("leftover_repayment", leftover_repayment.to_string()),
        ]))
    
}

/// Calculate the amount & value of debt to repay 
fn get_repay_quantities(
    config: Config,
    basket: Basket,
    target_position: Position,
    current_LTV: Decimal,
    borrow_LTV: Decimal,
    total_value: Decimal,
) -> Result<(Decimal, Decimal), ContractError>{
    
    // max_borrow_LTV/ current_LTV, * current_loan_value, current_loan_value - __ = value of loan amount
    let loan_value = basket.credit_price.get_value(target_position.credit_amount)?;

    //repay value = the % of the loan insolvent. Insolvent is anything between current and max borrow LTV.
    //IE, repay what to get the position down to borrow LTV
    //If the position LTV is above 100%, repay using all the collateral 
    let mut repay_value = if current_LTV >= Decimal::one() {
        total_value
    } else {
        decimal_multiplication( 
            decimal_division(
                 match decimal_subtraction(current_LTV, borrow_LTV){
                    Ok(res) => res,
                    Err(_) => return Err(ContractError::CustomError { val: "decimal_subtraction in repay_quantities failed".to_string() }),
                 }, 
                 current_LTV)?, 
                 loan_value)?
    };

    //Assert repay_value is above the minimum, if not repay at least the minimum
    //Repay the full loan if the resulting leftover credit amount is less than the minimum.
    let decimal_debt_minimum = Decimal::from_ratio(config.debt_minimum, Uint128::new(1u128));
    if repay_value < decimal_debt_minimum {
        //If setting the repay value to the minimum leaves at least the minimum in the position...
        //..then partially liquidate
        if loan_value < decimal_debt_minimum {
            repay_value = loan_value;
        } else if  loan_value - decimal_debt_minimum >= decimal_debt_minimum {            
            repay_value = decimal_debt_minimum;
        } else {
            //Else liquidate it all
            repay_value = loan_value;
        }
    }

    let credit_repay_amount = match basket.credit_price.get_amount(repay_value)?{
        //Repay amount has to be above 0, or there is nothing to liquidate and there was a mistake prior
        x if x <= Uint128::zero() => return Err(ContractError::PositionSolvent {}),
        //Can't repay more than the debt
        x if x > target_position.credit_amount =>
        {
            target_position.credit_amount
        }
        x => x,
    };

    Ok((repay_value, Decimal::from_ratio(credit_repay_amount, Uint128::one())))
}

/// Calculate amount of debt the User can repay from the Stability Pool
// fn get_user_repay_amount(
//     querier: QuerierWrapper,
//     config: Config,
//     basket: Basket,
//     position_id: Uint128,
//     position_owner: String,
//     credit_repay_amount: &mut Decimal,
//     submessages: &mut Vec<SubMsg>,
// ) -> StdResult<Decimal>{

//     let mut user_repay_amount = Decimal::zero();
//     //Let the user repay their position if they are in the SP
//     if config.stability_pool.is_some() {
//         //Query Stability Pool to see if the user has funds
//         let user_deposits = match querier
//             .query::<AssetPool>(&QueryRequest::Wasm(WasmQuery::Smart {
//                 contract_addr: config.clone().stability_pool.unwrap_or_else(|| Addr::unchecked("")).to_string(),
//                 msg: to_json_binary(&SP_QueryMsg::AssetPool { 
//                     user: Some(position_owner.clone()),
//                     deposit_limit: None, 
//                     start_after: None,
//                 })?,
//             })){
//                 Ok(res) => res.deposits,
//                 Err(_) => vec![],
//             };

//         let total_user_deposit: Decimal = user_deposits
//             .iter()
//             .map(|user_deposit| user_deposit.amount)
//             .collect::<Vec<Decimal>>()
//             .into_iter()
//             .sum();
            
//         //If the user has funds, tell the SP to repay and subtract from credit_repay_amount
//         if !total_user_deposit.is_zero() {
//             //Set Repayment amount to what needs to get liquidated or total_deposits
//             user_repay_amount = {
//                 //Repay the full debt
//                 if total_user_deposit > *credit_repay_amount {
//                     *credit_repay_amount
//                 } else {
//                     total_user_deposit
//                 }
//             };

//             //Add Repay SubMsg
//             let repay_msg = SP_ExecuteMsg::Repay {
//                 user_info: UserInfo {
//                     position_id,
//                     position_owner,
//                 },
//                 repayment: Asset {
//                     amount: user_repay_amount.to_uint_floor(),
//                     info: basket.credit_asset.info,
//                 },
//             };

//             let msg = CosmosMsg::Wasm(WasmMsg::Execute {
//                 contract_addr: config.stability_pool.unwrap_or_else(|| Addr::unchecked("")).to_string(),
//                 msg: to_json_binary(&repay_msg)?,
//                 funds: vec![],
//             });

//             //Convert to submsg
//             let sub_msg: SubMsg = SubMsg::new(msg);
//             submessages.push(sub_msg);

//             //Subtract Repay amount from credit_repay_amount for the liquidation
//             *credit_repay_amount = match decimal_subtraction(*credit_repay_amount, user_repay_amount){
//                 Ok(res) => res,
//                 Err(_) => return Err(StdError::GenericErr { msg: "SP credit repay amount calculation failed".to_string() }),
//             };
//         }
//     }

//     Ok( user_repay_amount )
// }

/// Calculate amount of debt the User can repay from its list of Deployable Venues
fn get_deployable_venues_user_repay_amount(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,    
    _config: Config,
    _basket: Basket,
    position_id: Uint128,
    position_owner: String,
    position: &mut Position,
    credit_repay_amount: &mut Decimal,
    submessages: &mut Vec<SubMsg>,
    attrs: &mut Vec<Attribute>,
) -> StdResult<Decimal>{

    let mut total_user_repay_amount = Decimal::zero();
    let mut used_venues: Vec<String> = vec![];
    let deployable_venues = position.deployed_to.clone();
    // println!("deployable_venues: {:?}", deployable_venues);
    for deployable_venue in deployable_venues {
        //If the venue has failed liquidation, skip it
        if deployable_venue.failed_liquidation {
            continue;
        }

        //Query Venue's Retrievable CDT
        let retrievable_cdt: Uint128 = match querier
            .query::<Uint128>(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: deployable_venue.address.to_string(),
                msg: to_json_binary(&DeployableVenue_QueryMsg::RetrievableCDT { user: position_owner.clone() })?,
            })){
                Ok(res) => res,
                Err(_) => Uint128::zero(),
            };
        let retrievable_cdt = Decimal::from_ratio(retrievable_cdt, Uint128::one());
        // println!("retrievable_cdt: {:?}", retrievable_cdt);
            
        //If the user has funds, tell the venue to repay and subtract from credit_repay_amount
        if !retrievable_cdt.is_zero() {
            //Add to used venues
            used_venues.push(deployable_venue.address.to_string());
            //Set Repayment amount to what needs to get liquidated or total_deposits
            let user_repay_amount = {
                //Repay the full debt
                if retrievable_cdt > *credit_repay_amount {
                    *credit_repay_amount
                } else {
                    retrievable_cdt
                }
            };
            //Add to total user repay amount
            total_user_repay_amount += user_repay_amount;
            // println!("user_repay_amount: {:?}", user_repay_amount);

            //Add Repay SubMsg
            let repay_msg = DeployableVenue_ExecuteMsg::RepayUserDebt {
                user_info: UserInfo {
                    position_id,
                    position_owner: position_owner.clone(),
                },
                repayment: user_repay_amount.to_uint_floor()
            };
            let msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: deployable_venue.address.to_string(),
                msg: to_json_binary(&repay_msg)?,
                funds: vec![],
            });
            //Convert to submsg
            let sub_msg: SubMsg = SubMsg::reply_always(msg, DEPLOYABLE_VENUE_REPLY_ID); 
            //This also means no error on errors in the venue's msg
            submessages.push(sub_msg);
            attrs.push(attr(    format!("repay_deployable_venue_from_{}", deployable_venue.address.to_string()), user_repay_amount.to_string()));

            //Subtract user repay amount from deployed debt amount for the venue
            if let Some((index, venue)) = position.deployed_to
                .iter_mut()
                .enumerate()
                .find(|(_, venue)| venue.address == deployable_venue.address)
                {
                    venue.deployed_debt_amount = match venue.deployed_debt_amount.checked_sub(user_repay_amount.to_uint_ceil()){
                        Ok(res) => res,
                        //if it errors during execution it'll skip anyway
                        Err(_) => Uint128::zero(),
                    };
                    //if the deployed debt amount is 0, remove the venue from the position
                    if venue.deployed_debt_amount.is_zero() {
                        //Remove venue from position (will be saved alongside liquidation logic)
                        position.deployed_to.remove(index);
                        //Update active deployment venues
                        set_active_deployment_venues(storage, vec![StringEntry {
                            entry: deployable_venue.address.to_string(),
                            remove: true,
                        }])?;
                    }
                }

            //Subtract Repay amount from credit_repay_amount for the liquidation
            *credit_repay_amount = match decimal_subtraction(*credit_repay_amount, user_repay_amount){
                Ok(res) => res,
                Err(_) => return Err(StdError::GenericErr { msg: format!("Deployable Venue {:?} credit repay amount calculation failed", deployable_venue.address.to_string()).to_string() }),
            };
        }
    }

    //Set Deployment Propagation
    DEPLOYABLE_VENUE.save(storage, &DeployableVenuePropagation {
        user: UserInfo {
            position_id,
            position_owner: position_owner.clone(),
        },
        venues: used_venues,
    })?;

    // println!("user_repay_amount: {:?}", user_repay_amount);

    Ok( total_user_repay_amount )
}

/// Calculate & send fees.
/// Send liquidatible amount to Liquidation Queue.
fn per_asset_fulfillments(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    config: Config,
    basket: Basket,
    fee_recipient: String,
    caller_fee: Decimal,
    collateral_assets: &mut Vec<cAsset>,
    leftover_position_value: &mut Decimal,
    mut leftover_repayment: Uint128,
    repay_value: Decimal,
    pre_user_repay_repay_value: Decimal,
    cAsset_ratios: Vec<Decimal>,
    cAsset_prices: Vec<PriceResponse>,
    submessages: &mut Vec<SubMsg>,
    caller_fee_messages: &mut Vec<CosmosMsg>,
    per_asset_repayment: &mut Vec<Decimal>,
    liquidated_assets: &mut Vec<cAsset>,
    caller_fee_value_paid: &mut Decimal,
    position_info: UserInfo,
) -> StdResult<(Option<CosmosMsg>, Uint128)>{

    // println!("leftover_repayment: {:?}", leftover_repayment);

    let mut caller_coins: Vec<Coin> = vec![];
    let mut protocol_coins: Vec<Coin> = vec![];
    //the repayment value used for the LQ function
    //Other wise multiple collateral assets will save the wrong repay_amount_per_asset each time
    let fn_repayment = leftover_repayment;

    ////CALLER AND PROTOCOL FEE COLLECTIONS////
    for (num, cAsset) in collateral_assets.clone().iter().enumerate() {

        let repay_amount_per_asset = fn_repayment * cAsset_ratios[num];
        
        let collateral_price = cAsset_prices[num].clone();
        let collateral_repay_value = match decimal_multiplication(pre_user_repay_repay_value, cAsset_ratios[num]){
            Ok(res) => res,
            Err(_) => return Err(StdError::GenericErr { msg: "Collateral repay value (for fee) calculation failed".to_string() }),
        };
        let pre_user_repay_collateral_repay_amount: Uint128 = match collateral_price.get_amount(collateral_repay_value){
            Ok(res) => res,
            Err(_) => return Err(StdError::GenericErr { msg: "Collateral repay amount (for fee) calculation failed".to_string() }),
        };

        //Subtract Caller fee from Position's claims
        let mut caller_fee_in_collateral_amount = pre_user_repay_collateral_repay_amount * caller_fee;

        //If the caller fee is greater than the amount of collateral the Position has, set it to 0 
        //This is to prevent the caller fee from being greater than the value of the position
        if caller_fee_in_collateral_amount >= collateral_assets[num].asset.amount {
            caller_fee_in_collateral_amount = Uint128::zero();
        }
        
        //Add to caller_fee_value_paid
        let fee_value = match collateral_price.get_value(caller_fee_in_collateral_amount){
            Ok(res) => res,
            Err(_) => return Err(StdError::GenericErr { msg: "Caller fee value calculation failed".to_string() }),
        };
        *caller_fee_value_paid = *caller_fee_value_paid + fee_value;

        //Update collateral_assets to reflect the fee
        collateral_assets[num].asset.amount = match collateral_assets[num].asset.amount.checked_sub(caller_fee_in_collateral_amount){
            Ok(res) => res,
            Err(_) => return Err(StdError::GenericErr { msg: "Collateral fee amount calculation failed".to_string() }),
        };
        //Add to list of liquidated assets
        if caller_fee_in_collateral_amount > Uint128::zero() {
            liquidated_assets.push(
                cAsset {
                    asset: Asset {
                        amount: caller_fee_in_collateral_amount,
                        ..cAsset.clone().asset
                    },
                    ..cAsset.clone()
                }
            );
        }
        
        //Subtract Protocol fee from Position's claims
        let mut protocol_fee_in_collateral_amount = pre_user_repay_collateral_repay_amount * config.clone().liq_fee;

        //If the protocol fee is greater than the amount of collateral the Position has, set it to 0
        //This is to prevent the protocol fee from being greater than the value of the position
        if protocol_fee_in_collateral_amount >= collateral_assets[num].asset.amount {
            protocol_fee_in_collateral_amount = Uint128::zero();
        }
        
        //Update collateral_assets to reflect the fee
        collateral_assets[num].asset.amount = match collateral_assets[num].asset.amount.checked_sub(protocol_fee_in_collateral_amount) {
            Ok(res) => res,
            Err(_) => return Err(StdError::GenericErr { msg: "Protocol fee amount calculation failed".to_string() }),
        };
        //Add to list of liquidated assets
        if protocol_fee_in_collateral_amount > Uint128::zero() {
            liquidated_assets.push(
                cAsset {
                    asset: Asset {
                        amount: protocol_fee_in_collateral_amount,
                        ..cAsset.clone().asset
                    },
                    ..cAsset.clone()
                }
            );
        }
        
        //Subtract fees from leftover_position value
        //fee_value = total_fee_collateral_amount * collateral_price
        let fee_value = collateral_price.get_value((caller_fee_in_collateral_amount + protocol_fee_in_collateral_amount))?;

        //Remove fee_value from leftover_position_value
        *leftover_position_value = match decimal_subtraction(*leftover_position_value, fee_value){
            Ok(res) => res,
            Err(_) => return Err(StdError::GenericErr { msg: "Leftover position value calculation (for fee) failed".to_string() }),
        };
        
        //Create msgs to caller as well as to liq_queue if.is_some()
        match cAsset.clone().asset.info {
            AssetInfo::Token { address: _ } => { return Err(StdError::GenericErr { msg: String::from("Cw20 assets aren't allowed") }) },
            AssetInfo::NativeToken { denom: _ } => {
                let asset = Asset {
                    amount: caller_fee_in_collateral_amount,
                    ..cAsset.clone().asset
                };
                if caller_fee_in_collateral_amount > Uint128::zero() {
                    caller_coins.push(asset_to_coin(asset)?);
                }

                let asset = Asset {
                    amount: protocol_fee_in_collateral_amount,
                    ..cAsset.clone().asset
                };
                if protocol_fee_in_collateral_amount > Uint128::zero() {
                    protocol_coins.push(asset_to_coin(asset)?);
                }
            }
        } 

        /////////////LiqQueue calls///////////
        if basket.clone().liq_queue.is_some() && leftover_repayment > Uint128::zero(){
            //Repay amount using repay_value after the user's SP repayment            
            let collateral_price = cAsset_prices[num].clone();
            let collateral_repay_value = match decimal_multiplication(repay_value, cAsset_ratios[num]){
                Ok(res) => res,
                Err(_) => return Err(StdError::GenericErr { msg: "Collateral repay value calculation (for liq) failed".to_string() }),
            };
            let mut collateral_repay_amount: Uint128 = match collateral_price.get_amount(collateral_repay_value){
                Ok(res) => res,
                Err(_) => return Err(StdError::GenericErr { msg: "Collateral repay amount calculation (for liq) failed".to_string() }),
            };
                        
            //if collateral repay amount is more than the Position has in assets, 
            //Set collateral_repay_amount to the amount the Position has in assets
            if collateral_repay_amount > collateral_assets[num].asset.amount {
                collateral_repay_amount = collateral_assets[num].asset.amount;
            }
            
            //We're asking: "For this collateral amount, how can debt can you (the LQ) liquidate?"
            // This is ineffiecient bc the LQ will never pay 100% of the debt required, leaving the premium as leftover.
            let res: LQ_LiquidatibleResponse =
                match querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
                    contract_addr: basket.clone().liq_queue.unwrap_or_else(|| Addr::unchecked("")).to_string(),
                    msg: to_json_binary(&LQ_QueryMsg::CheckLiquidatible {
                        bid_for: cAsset.clone().asset.info,
                        collateral_price: collateral_price.clone(),
                        collateral_amount: Uint256::from(
                            (collateral_repay_amount).u128(),
                        ),
                        credit_info: basket.clone().credit_asset.info,
                        credit_price: basket.clone().credit_price,
                    })?,
                })){
                    Ok(res) => res,
                    //If this errors we go to the next asset.
                    //If they all error, the SP will get an initial call instead of waiting for the reply.
                    Err(_) => {
                        // println!("Error in liq_queue query");
                        continue;
                    }, 
                
                };
            //Calculate how much collateral we are sending to the liq_queue to liquidate
            let leftover: Uint128 = Uint128::from_str(&res.leftover_collateral)?;
            let queue_asset_amount_paid: Uint128 = match 
                collateral_repay_amount.checked_sub(leftover){
                    Ok(res) => res,
                    Err(_) => return Err(StdError::GenericErr { msg: "Queue asset amount paid calculation (for liq) failed".to_string() }),
                };

            //Don't send a message if the amount is 0
            if queue_asset_amount_paid.is_zero() || Uint128::from_str(&res.total_debt_repaid)?.is_zero(){
                continue;
            }
            //Call Liq Queue::Liquidate for the asset
            let liq_msg = LQ_ExecuteMsg::Liquidate {
                credit_price: basket.clone().credit_price,
                collateral_price: collateral_price.clone(),
                collateral_amount: Uint256::from(queue_asset_amount_paid.u128()),
                bid_for: cAsset.clone().asset.info,
            };

            //Keep track of remaining position value
            //value_paid_to_queue = queue_asset_amount_paid * collateral_price
            let value_paid_to_queue: Decimal = collateral_price.get_value(queue_asset_amount_paid)?;

            *leftover_position_value = match decimal_subtraction(*leftover_position_value, value_paid_to_queue){
                Ok(res) => res,
                Err(_) => return Err(StdError::GenericErr { msg: "Leftover position value calculation (for liq) failed".to_string() }),
            };
            
            //Calculate how much the queue repaid in credit
            let queue_credit_repaid = Uint128::from_str(&res.total_debt_repaid)?;
            //Subtract that from the running total for potential leftovers
            //i.e. after this function is over, this value will be the amount of credit that was not repaid
            leftover_repayment = match leftover_repayment.checked_sub(queue_credit_repaid){
                Ok(res) => res,
                Err(_) => return Err(StdError::GenericErr { msg: "Leftover repayment calculation (for liq) failed".to_string() }),
            };
            //The LQ has repaid more than the query returned in the past so we'll handle any possible excess from the SP in the liq_repay
            
            //Create CosmosMsg
            let msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: basket.clone().liq_queue.unwrap_or_else(|| Addr::unchecked("")).to_string(),
                msg: to_json_binary(&liq_msg)?,
                funds: vec![],
            });

            
            /////Store credit repay amount////
            //Only store if we make it this far in the block.
            //If we 'continue' beforehand we don't want to save these as it breaks the reply logic.
            per_asset_repayment.push(Decimal::from_ratio(repay_amount_per_asset, Uint128::one()));

            //Convert to submsg
            let sub_msg: SubMsg = SubMsg::reply_on_success(msg, LIQ_QUEUE_REPLY_ID);
            submessages.push(sub_msg);
        }
    }

    ///Whatever is left over, sell using the chain proxy's execute swaps function
    let mut collateral_to_sell: Vec<Coin> = vec![];
    if leftover_repayment > Uint128::zero() {
        // println!("leftover_repayment: {:?}", leftover_repayment);
        for (num, cAsset) in collateral_assets.clone().iter().enumerate() {
            //Calculate how much of each collateral asset we need to sell

            //Sell amount using leftover_repayment after LQ repayments         
            let collateral_price = cAsset_prices[num].clone();
            let collateral_sell_value = match decimal_multiplication(Decimal::from_ratio(leftover_repayment, Uint128::one()), cAsset_ratios[num]){
                Ok(res) => res,
                Err(_) => return Err(StdError::GenericErr { msg: "Collateral sell value calculation (for liq) failed in sell block".to_string() }),
            };
            let mut collateral_sell_amount: Uint128 = match collateral_price.get_amount(collateral_sell_value){
                Ok(res) => res,
                Err(_) => return Err(StdError::GenericErr { msg: "Collateral sell amount calculation (for liq) failed in sell block".to_string() }),
            };

            // println!("leftover_position_value: {:?}, collateral_sell_value: {:?}", leftover_position_value, collateral_sell_value);
            //Update leftover position value 
            *leftover_position_value = match decimal_subtraction(*leftover_position_value, collateral_sell_value){
                Ok(res) => res,
                Err(_) => {
                    collateral_sell_amount = collateral_assets[num].asset.amount;
                    Decimal::zero()
                },
            };
            // println!("leftover_position_value: {:?}", leftover_position_value);
            // println!("collateral_sell_amount: {:?}", collateral_sell_amount);

            collateral_to_sell.push(Coin {
                denom: cAsset.clone().asset.info.to_string(),
                amount: collateral_sell_amount,
            });
        }

        //Create Msg to sell collateral
        let sell_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.clone().chain_proxy.unwrap().to_string(),
            msg: to_json_binary(&ChainProxyExecuteMsg::ExecuteSwaps { 
                token_out: basket.clone().credit_asset.info.to_string(),
                max_slippage: Decimal::percent(90), //We are prioritizing offloading the collateral
            })?,
            funds: collateral_to_sell.clone(),
        });
        // Add to submessages 
        //TODO, MUST UPDATE POSITION CLAIMS IN THE REPLY
        submessages.push(SubMsg::reply_on_success(sell_msg, SELL_COLLATERAL_REPLY_ID));

        //Save SellCollateralPropagation
        SELL_COLLATERAL.save(storage, &SellCollateralPropagation {
            collateral_sold: collateral_to_sell,
            position_info: position_info.clone(),
        })?;
    }

    //Create Msg to send all native token liq fees for fn caller
    let msg = CosmosMsg::Bank(BankMsg::Send {
        to_address: fee_recipient.clone(),
        amount: caller_coins.clone(),
    });
    if !caller_coins.is_empty(){
        caller_fee_messages.push(msg);
    }

    // println!("caller_coins: {:?}", caller_coins);
    // println!("protocol_coins: {:?}", protocol_coins);

    if !protocol_coins.is_empty(){
        //Create Msg to send all native token liq fees for MBRN to the staking contract
        let protocol_fee_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.clone().staking_contract.unwrap_or_else(|| Addr::unchecked("")).to_string(),
            msg: to_json_binary(&StakingExecuteMsg::DepositFee {})?,
            funds: protocol_coins,
        }); 

        Ok((Some(protocol_fee_msg), leftover_repayment))
    } else {
        Ok((None, leftover_repayment))
    }

}

// This function is used to build (sub)messages for the Stability Pool.
// Also returns leftover debt repayment amount.
// pub fn build_sp_submsgs(
//     storage: &mut dyn Storage,
//     querier: QuerierWrapper,
//     env: Env,
//     config: Config,
//     basket: Basket,
//     valid_position_owner: Addr,
//     mut leftover_repayment: Decimal,
//     credit_repay_amount: Decimal,
//     mut leftover_position_value: Decimal,
//     submessages: &mut Vec<SubMsg>,
//     per_asset_repayment: Vec<Decimal>,
//     user_repay_amount: Decimal,
//     target_position: Position,
//     liquidated_assets: Vec<cAsset>,
//     cAsset_ratios: Vec<Decimal>,
//     cAsset_prices: Vec<PriceResponse>,
//     caller_fee_value_paid: Decimal,
// ) -> Result<(Decimal), ContractError>{

//     //Starts at what LQ is supposed to pay
//     let liq_queue_repayment = match credit_repay_amount.checked_sub(leftover_repayment){
//         Ok(res) => res,
//         Err(_) => return Err(ContractError::CustomError { val: "Leftover repayment calculation (for build_sp_submsgs) failed".to_string() }),
//     };
    
//     if config.stability_pool.is_some() && !leftover_repayment.is_zero() {        
//         let sp_pool: AssetPool = querier.query_wasm_smart::<AssetPool>(
//             config.clone().stability_pool.unwrap_or_else(|| Addr::unchecked("")).to_string(), 
//             &SP_QueryMsg::AssetPool {
//                 user: None,
//                 deposit_limit: Some(1),
//                 start_after: None,
//             }
//         )?;

//         let sp_liq_fee = sp_pool.liq_premium;
            
        
//         //If LTV is 90% and the fees are 10%, the position would pay everything to pay the liquidators.
//         //So above that, the liquidators are losing the premium guarantee.
//         // !( leftover_position_value >= leftover_repay_value * sp_fee)

//         //Working on the LQ's leftovers
//         let leftover_repayment_value = basket.credit_price.get_value(leftover_repayment.to_uint_floor())?;

//         //SP liq_fee Guarantee check
//         //if leftover_position_value is less than leftover_repay value + the SP fee, we liquidate what we can
//         if leftover_position_value < decimal_multiplication(leftover_repayment_value, (Decimal::one() + sp_liq_fee))?{
//             //Set Position value to the discounted value the SP will be distributed
//             leftover_position_value = match decimal_division(leftover_position_value, (Decimal::one() + sp_liq_fee)){
//                 Ok(res) => res,
//                 Err(_) => return Err(ContractError::CustomError { val: "Leftover position value calculation (for SP) failed".to_string() }),
//             };
//             //Set leftover_repayment to the amount of credit the Position value can pay
//             leftover_repayment = Decimal::from_ratio(basket.credit_price.get_amount(leftover_position_value)?, Uint128::one());            
//         }        

//         //If SP AssetPool is 0, repay nothing
//         if sp_pool.credit_asset.amount.is_zero(){
//             leftover_repayment = Decimal::zero();
//         }
        
//         // Set repay values for reply msg
//         let liquidation_propagation = LiquidationPropagation {
//             per_asset_repayment,
//             liq_queue_repayment,
//             stability_pool: leftover_repayment, 
//             user_repay_amount,
//             target_position,
//             liquidated_assets,
//             caller_fee_value_paid,
//             total_repaid: user_repay_amount,
//             position_owner: valid_position_owner,
//             positions_contract: env.contract.address,
//             sp_liq_fee,
//             cAsset_ratios, 
//             cAsset_prices,
//             basket,
//             config: config.clone(),
//         };

//         LIQUIDATION.save(storage, &liquidation_propagation)?;

//         //We use 1 as our 0 to account for LQ rounding errors
//         if leftover_repayment > Decimal::one() {

//             //Stability Pool message builder
//             let liq_msg = SP_ExecuteMsg::Liquidate {
//                 liq_amount: leftover_repayment
//             };

//             let msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
//                 contract_addr: config.stability_pool.unwrap_or_else(|| Addr::unchecked("")).to_string(),
//                 msg: to_json_binary(&liq_msg)?,
//                 funds: vec![],
//             });

//             //Can't use SubMsg replies bc there are too many msgs in the SP liquidation flow which means loss of funds if we error too deep
//             let sub_msg: SubMsg = SubMsg::new(msg);

//             submessages.push(sub_msg);

//             //Replying on errors means we can NOT make state changes that we wouldn't allow no matter the tx result, as our altereed state will NOT revert.
//             //Errors also won't revert the whole transaction
//             //( https://github.com/CosmWasm/cosmwasm/blob/main/SEMANTICS.md#submessages )
//         }

//         //Collateral distributions get handled in the reply        
//     } else {
//         //In case SP isn't used, we need to set LiquidationPropagation
//         // Set repay values for reply msg
//         let liquidation_propagation = LiquidationPropagation {
//             per_asset_repayment,
//             liq_queue_repayment,
//             stability_pool: Decimal::zero(),
//             user_repay_amount,
//             target_position,
//             liquidated_assets,
//             caller_fee_value_paid,
//             total_repaid: user_repay_amount,
//             position_owner: valid_position_owner,
//             positions_contract: env.contract.address,
//             sp_liq_fee: Decimal::zero(),
//             cAsset_ratios,
//             cAsset_prices,
//             basket,
//             config,
//         };

//         LIQUIDATION.save(storage, &liquidation_propagation)?;
//     }

//     Ok((leftover_repayment))
// }

// Returns leftover liquidatible amount from the stability pool
// pub fn query_stability_pool_liquidatible(
//     querier: QuerierWrapper,
//     config: Config,
//     amount: Decimal,
// ) -> StdResult<Decimal> {
//     let query_res: SP_LiquidatibleResponse =
//         querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
//             contract_addr: config.stability_pool.unwrap_or_else(|| Addr::unchecked("")).to_string(),
//             msg: to_json_binary(&SP_QueryMsg::CheckLiquidatible {
//                 amount
//             })?,
//         }))?;

//     Ok(query_res.leftover)
// }