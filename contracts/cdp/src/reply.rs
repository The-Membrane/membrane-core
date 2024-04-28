use std::str::FromStr;

use cosmwasm_std::{DepsMut, Env, Reply, StdResult, Response, SubMsg, Decimal, Uint128, StdError, attr, to_binary, WasmMsg, CosmosMsg};

use membrane::types::{AssetInfo, Asset, Basket, cAsset};
use membrane::stability_pool::ExecuteMsg as SP_ExecuteMsg;
use membrane::osmosis_proxy::ExecuteMsg as OP_ExecuteMsg;
use membrane::cdp::Config;
use membrane::math::decimal_subtraction;
use membrane::helpers::{withdrawal_msg, get_contract_balances};

use crate::risk_engine::update_basket_tally;
use crate::state::{LiquidationPropagation, LIQUIDATION, WITHDRAW, BASKET, get_target_position, update_position};
use crate::liquidations::build_sp_submsgs;

/// On error of a user's Stability Pool repayment, leave leftover to the SP within the LQ reply.
#[allow(unused_variables)]
pub fn handle_user_sp_repay_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(_result) => {
            //Its reply on error only
            Ok(Response::new())
        }        
        Err(string) => {
            //Readd the leftover to the repay amount tally that will go to the SP after the last successful LQ call.
            //This was removed in the inital user repay call
            let mut prop: LiquidationPropagation = LIQUIDATION.load(deps.storage)?;

            let leftover = prop.clone().user_repay_amount;
            prop.liq_queue_leftovers += leftover;
            prop.user_repay_amount = Decimal::zero();

            LIQUIDATION.save(deps.storage, &prop)?;

            Ok(Response::new()
                .add_attribute("error", string)
                .add_attribute("repay_amount_added_to_tally", leftover.to_string()))
        }
    }
}

/// On error of a Stability Pool liquidation, update the user's position to keep state intact.
/// Otherwise, the position is updated on a success in the liq_repay msg.
/// Successful LQ msgs don't update the position state unless the SP isn't being used to repay due to there being none leftover to handle.
#[allow(unused_variables)]
pub fn handle_sp_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(_result) => {
            //Its reply on error only
            Ok(Response::new())
        }        
        Err(string) => {
            //Load the state
            let mut prop: LiquidationPropagation = LIQUIDATION.load(deps.storage)?;
            let mut basket = prop.clone().basket;
            let config: Config = prop.clone().config;

            /////If nothing has been repaid, error so the position can't be farmed for fees/////
            /// SP fails when repaying 0
            /// If user_repay fails, it gets set to 0
            /// If LQ isn't used or fails, the total_repaid is stagnant.
            if prop.clone().total_repaid.is_zero() && prop.clone().user_repay_amount.is_zero(){
                return Err(StdError::GenericErr { msg: String::from("Stability pool failed, user didn't repay from Stability pool or it failed & LQ wasn't used, no repayments made") });
            }

            //Update supply caps
            if prop.clone().target_position.credit_amount.is_zero(){                
                //Remove position's assets from Supply caps 
                match update_basket_tally(
                    deps.storage, 
                    deps.querier, 
                    env.clone(), 
                    &mut basket, 
                    prop.clone().target_position.clone().collateral_assets,
                    prop.clone().target_position.clone().collateral_assets,
                    false, 
                    config.clone(),
                    true,
                ){
                    Ok(_) => {},
                    Err(err) => return Err(StdError::GenericErr { msg: err.to_string() }),
                };
            } else {
                //Remove liquidated assets from Supply caps
                match update_basket_tally(
                    deps.storage, 
                    deps.querier, 
                    env.clone(), 
                    &mut basket,
                    prop.clone().liquidated_assets,
                    prop.clone().target_position.clone().collateral_assets,
                    false,
                    config.clone(),
                    true,
                ){
                    Ok(_) => {},
                    Err(err) => return Err(StdError::GenericErr { msg: err.to_string() }),
                };
            }            
            //Update Basket
            BASKET.save(deps.storage, &basket)?;

            //Update the position w/ the new credit & collateral amount
            update_position(deps.storage, prop.clone().position_owner, prop.clone().target_position)?;

            Ok(Response::new()
                .add_attribute("error", string)
                .add_attribute("new_position", format!("{:?}", prop.target_position)))
        }
    }
}


/// Validate withdrawls by asserting that the amount withdrawn is less than or equal to the amount of the asset in the contract.
/// Assert new cAssets amount was saved correctly.
pub fn handle_withdraw_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    //Initialize Response Attributes
    let mut attrs = vec![];

    //Match on msg.result
    match msg.result.into_result() {
        Ok(_result) => {
            let withdraw_prop = WITHDRAW.load(deps.storage)?;

            //Assert valid withdrawal for each asset this reply is
            for (i, prev_collateral) in withdraw_prop.clone().positions_prev_collateral.into_iter().enumerate(){
                let asset_info: AssetInfo = prev_collateral.info.clone();
                let position_amount: Uint128 = prev_collateral.amount;
                let withdraw_amount: Uint128 = withdraw_prop.withdraw_amounts[i];

                let current_asset_balance = match get_contract_balances(
                    deps.querier,
                    env.clone(),
                    vec![asset_info.clone()],
                ) {
                    Ok(balances) => balances[0],
                    Err(err) => {
                        return Err(StdError::GenericErr {
                            msg: err.to_string(),
                        })
                    }
                };

                //If balance differnce is more than what they tried to withdraw, error
                if withdraw_prop.contracts_prev_collateral_amount[i] - current_asset_balance > withdraw_amount {
                    return Err(StdError::GenericErr {
                        msg: format!(
                            "Conditional 1: Invalid withdrawal, possible bug found by {}",
                            withdraw_prop.position_info.position_owner
                        ),
                    });
                }

                match get_target_position(
                    deps.storage,
                    deps.api.addr_validate(&withdraw_prop.position_info.position_owner)?,
                    withdraw_prop.position_info.position_id,
                ){
                    Ok((_i, user_position)) => {
                        //Assert the withdrawal was correctly saved to state
                        if let Some(cAsset) = user_position
                        .collateral_assets
                        .into_iter()
                        .find(|cAsset| cAsset.asset.info.equal(&asset_info))
                        {
                            if cAsset.asset.amount != (position_amount - withdraw_amount) {
                                return Err(StdError::GenericErr {
                                    msg: format!(
                                        "Conditional 2: Invalid withdrawal, possible bug found by {}",
                                        withdraw_prop.position_info.position_owner
                                    ),
                                });
                            }
                        }
                    },
                    Err(err) => {
                        //Error means the position was deleted from state, assert that collateral was supposed to be completely withdrawn
                        if !(position_amount - withdraw_amount).is_zero(){
                            return Err(StdError::GenericErr {
                                msg: err.to_string(),
                            })
                        }
                    }
                };                

                //Add Success attributes
                attrs.push(attr(
                    "successful_withdrawal",
                    Asset {
                        info: asset_info,
                        amount: withdraw_amount,
                    }
                    .to_string(),
                ));
            }
        //We can go by first entries for these fields bc the replies will come in FIFO in terms of assets sent
        
        } //We only reply on success
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }
    
    Ok(Response::new().add_attributes(attrs))
}

/// The reply used to output leftovers.
// #[allow(unused_variables)]
// pub fn handle_stability_pool_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
//     match msg.result.into_result() {
//         Ok(result) => {
//             //1) Parse potential leftover amount and send to sell_wall if there is any
//             //Don't need to change state bc the SP will be repaying thru the contract
//             //There should only be leftover here if the SP loses funds between the query and the repayment
//             //2) Send collateral to the SP in the repay function and call distribute

//             let liq_event = result
//                 .events
//                 .iter()
//                 .find(|e| {
//                     e.attributes
//                         .iter()
//                         .any(|attr| attr.key == "leftover_repayment")
//                 })
//                 .ok_or_else(|| {
//                     StdError::GenericErr { msg: String::from("unable to find stability pool event") }
//                 })?;

//             let leftover = &liq_event
//                 .attributes
//                 .iter()
//                 .find(|attr| attr.key == "leftover_repayment")
//                 .unwrap()
//                 .value;

//             let leftover_amount = Uint128::from_str(leftover)?;

//             //Success w/ leftovers: Leave leftovers for the next liquidation call
//             //Success w/o leftovers: Do nothing, LQ leftovers are what called this msg
//             //Error: Leave repayment for the next liquidation call

//             Ok(Response::new()
//                 .add_attributes([
//                     attr("leftover_repayment", leftover_amount),
//                 ]))
//         }
//         Err(error) => {            
//             Ok(Response::new()
//                 .add_attributes([
//                     attr("error", error),
//                 ]))
//         }
//     }
// }

/// Send the liquidation queue its collateral reward.
/// Send leftovers to the SP.
/// Note: We send collateral here bc the LQ queries have returned less debt than the executed msg before so we want to give the LQ exactly what its expecting.
#[allow(unused_variables)]
pub fn handle_liq_queue_reply(deps: DepsMut, msg: Reply, env: Env) -> StdResult<Response> {
    let mut attrs = vec![attr("method", "handle_liq_queue_reply")];

    match msg.result.into_result() {
        Ok(result) => {        
            //1) Parse potential repaid_amount and substract from running total
            //2) Send collateral to the Queue

            let liq_event = result
                .events
                .into_iter()
                .find(|e| e.attributes.iter().any(|attr| attr.key == "repay_amount"))
                .ok_or_else(|| StdError::GenericErr {  msg: String::from("unable to find liq-queue event")})?;

            let repay = &liq_event
                .attributes
                .iter()
                .find(|attr| attr.key == "repay_amount")
                .unwrap()
                .value;
            let repay_amount = Uint128::from_str(repay)?;

            let mut prop: LiquidationPropagation = LIQUIDATION.load(deps.storage)?;
            let mut basket = prop.clone().basket;
            let config = prop.clone().config;

            //Send successfully liquidated amount
            let amount = &liq_event
                .attributes
                .iter()
                .find(|attr| attr.key == "collateral_amount")
                .unwrap()
                .value;

            let send_amount = Uint128::from_str(amount)?;

            let token = &liq_event
                .attributes
                .iter()
                .find(|attr| attr.key == "collateral_token")
                .unwrap()
                .value;

            let asset_info = &liq_event
                .attributes
                .iter()
                .find(|attr| attr.key == "collateral_info")
                .unwrap()
                .value;

            let token_info: AssetInfo = if asset_info.eq(&String::from("token")) {
                AssetInfo::Token {
                    address: deps.api.addr_validate(token)?,
                }
            } else {
                AssetInfo::NativeToken {
                    denom: token.to_string(),
                }
            };
        
            let msg = withdrawal_msg(
                Asset {
                    info: token_info.clone(),
                    amount: send_amount,
                },
                basket.liq_queue.clone().unwrap(),
            )?;
            
            //Subtract repaid amount from total repay amount that is held in liq_queue_leftovers. The remaining is the leftover sent to the SP.
            if repay_amount != Uint128::zero() {
                if !prop.liq_queue_leftovers.is_zero() {
                         
                    prop.liq_queue_leftovers = match decimal_subtraction(
                        prop.liq_queue_leftovers,
                        Decimal::from_ratio(repay_amount, Uint128::new(1u128)),
                    ){
                        Ok(difference) => difference,
                        Err(_err) => return Err(StdError::GenericErr { msg: format!("leftovers: {} < repay: {}", prop.liq_queue_leftovers, repay_amount) }),                    
                    };
                    //SP reply handles LQ_leftovers

                    //Update credit amount based on liquidation's total repaid amount
                    prop.target_position.credit_amount -= repay_amount;
                } else {
                    return Err(StdError::GenericErr { msg: String::from("Repay amount is 0 before finishing LQ liquidations") })
                }
                
                //Update position claims in prop.target_position
                prop.target_position.collateral_assets
                    .iter_mut()
                    .find(|cAsset| cAsset.asset.info.equal(&token_info))
                    .unwrap()
                    .asset
                    .amount -= send_amount;
                //update liquidated assets
                prop.liquidated_assets.push(
                    cAsset {
                        asset: Asset {
                            amount: send_amount,
                            info: token_info.clone()
                        },
                        max_borrow_LTV: Decimal::zero(),
                        max_LTV: Decimal::zero(),
                        pool_info: None,
                        rate_index: Decimal::one(),                        
                    }
                );

                //Add to total repaid
                prop.total_repaid += Decimal::from_ratio(repay_amount, Uint128::new(1u128));
            }
        
            let mut sub_msgs = vec![];
            //If this is the last asset left to send and there is still more to liquidate, send the leftovers to the SP
            if prop.per_asset_repayment.len() == 1 {
                
                //If there are leftovers, send them to the SP
                if !prop.liq_queue_leftovers.is_zero(){
                    match build_sp_submsgs(
                        deps.storage, 
                        deps.querier, 
                        env, 
                        prop.clone().config, 
                        prop.clone().basket, 
                        prop.clone().position_owner, 
                        prop.clone().liq_queue_leftovers, 
                        prop.clone().liq_queue_leftovers,                     
                        prop.clone().stability_pool, 
                        &mut sub_msgs, 
                        prop.clone().per_asset_repayment, 
                        prop.clone().user_repay_amount, 
                        prop.clone().target_position, 
                        prop.clone().liquidated_assets, 
                        prop.clone().cAsset_ratios, 
                        prop.clone().cAsset_prices,
                        prop.clone().caller_fee_value_paid
                    ){
                        Ok(_) => {},
                        Err(err) => return Err(StdError::GenericErr { msg: err.to_string() }),
                    };
                } //If we aren't sending to the SP we need to update the positon && basket here
                else {
                    //Update supply caps
                    if prop.clone().target_position.credit_amount.is_zero(){                
                        //Remove position's assets from Supply caps 
                        match update_basket_tally(
                            deps.storage, 
                            deps.querier, 
                            env.clone(), 
                            &mut basket, 
                            prop.clone().target_position.clone().collateral_assets,
                            prop.clone().target_position.clone().collateral_assets,
                            false, 
                            config.clone(),
                            true,
                        ){
                            Ok(_) => {},
                            Err(err) => return Err(StdError::GenericErr { msg: err.to_string() }),
                        };
                    } else {
                        //Remove liquidated assets from Supply caps
                        match update_basket_tally(
                            deps.storage, 
                            deps.querier, 
                            env.clone(), 
                            &mut basket,
                            prop.clone().liquidated_assets,
                            prop.clone().target_position.clone().collateral_assets,
                            false,
                            config.clone(),
                            true,
                        ){
                            Ok(_) => {},
                            Err(err) => return Err(StdError::GenericErr { msg: err.to_string() }),
                        };
                    }            
                    //Update Basket
                    BASKET.save(deps.storage, &basket)?;

                    //Update position w/ new credit amount
                    update_position(deps.storage, prop.clone().position_owner, prop.clone().target_position)?;
                }
                attrs.extend(vec![
                    attr("sent_to_SP", prop.clone().liq_queue_leftovers.to_string()),
                ]);
            }

            //Remove Asset
            prop.per_asset_repayment.remove(0);
            LIQUIDATION.save(deps.storage, &prop)?;

            attrs.extend(vec![
                attr("repay_amount", repay_amount),
                attr("reward_amount", send_amount),
                attr("reward_info", token_info.to_string()),
            ]);

            Ok(Response::new()
                .add_submessages(sub_msgs)
                .add_message(msg)
                .add_attributes(attrs)
            )
        }
        Err(string) => {
            //Only reply on success
            Ok(Response::new()
                .add_attribute("error", string))
        }
    }
}

