use std::str::FromStr;

use cosmwasm_std::{DepsMut, Env, Reply, StdResult, Response, SubMsg, Decimal, Uint128, StdError, attr, to_binary, WasmMsg, CosmosMsg};

use membrane::types::{AssetInfo, Asset, Basket, cAsset};
use membrane::stability_pool::ExecuteMsg as SP_ExecuteMsg;
use membrane::osmosis_proxy::ExecuteMsg as OP_ExecuteMsg;
use membrane::cdp::{Config, ExecuteMsg};
use membrane::math::decimal_subtraction;
use membrane::helpers::{withdrawal_msg, get_contract_balances, asset_to_coin};

use crate::positions::STABILITY_POOL_REPLY_ID;
use crate::risk_engine::update_basket_tally;
use crate::state::{LiquidationPropagation, LIQUIDATION, WITHDRAW, CONFIG, BASKET, CLOSE_POSITION, ClosePositionPropagation, get_target_position, update_position_claims, update_position};
use crate::liquidations::sell_wall;

/// Only necessary after the last of successful router swaps, uses the returned asset to repay the position's debt
pub fn handle_router_repayment_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(_result) => {
            //Load state
            let mut prop = LIQUIDATION.load(deps.storage)?;
            let mut basket: Basket = prop.clone().basket;
            
            //Query contract balance of the basket credit_asset
            let credit_asset_balance = get_contract_balances(
                deps.querier, 
                env.clone(), 
                vec![basket.credit_asset.info.clone()]
            )?[0];

            //Skip if balance is 0
            if credit_asset_balance.is_zero() {
                return Err(StdError::GenericErr { msg: format!("Router sale success returned 0 {}", basket.credit_asset.info) });
            }

            //Create burn_msg with queried funds
            //This works because the contract doesn't hold excess credit_asset, all repayments are burned & revenue isn't minted
            let burn_msg = CosmosMsg::Wasm(WasmMsg::Execute { 
                contract_addr: prop.clone().config.osmosis_proxy.unwrap().to_string(), 
                msg: to_binary(
                    &OP_ExecuteMsg::BurnTokens { 
                        denom: basket.credit_asset.info.clone().to_string(), 
                        amount: credit_asset_balance.clone(), 
                        burn_from_address: env.contract.address.to_string(),
                    }
                )?, 
                funds: vec![]
            });
            
            //Update position w/ new credit amount
            prop.target_position.credit_amount -= credit_asset_balance.clone();
            update_position(deps.storage, prop.clone().position_owner, prop.clone().target_position)?;

            
            //////Update Basket and save
            if prop.clone().target_position.credit_amount.is_zero(){                
                //Remove position's assets from Supply caps 
                update_basket_tally(
                    deps.storage, 
                    deps.querier, 
                    env.clone(), 
                    &mut basket, 
                    prop.clone().target_position.collateral_assets,
                    false, 
                    prop.clone().config,
                    true
                ).map_err(|err| StdError::GenericErr { msg: err.to_string() })?;
            } else {                    
                //Remove liquidated assets from Supply caps 
                update_basket_tally(
                    deps.storage, 
                    deps.querier, 
                    env.clone(), 
                    &mut basket, 
                    prop.clone().liquidated_assets, 
                    false, 
                    prop.clone().config,
                    true
                ).map_err(|err| StdError::GenericErr { msg: err.to_string() })?;
            }

            //Subtract repaid debt from Basket
            basket.credit_asset.amount = match basket.credit_asset.amount.checked_sub(credit_asset_balance){
                Ok(difference) => difference,
                Err(_err) => return Err(StdError::GenericErr { msg: String::from("Repay amount is greater than Basket credit amount from the router") }),
            };
            BASKET.save(deps.storage, &basket)?;
            ////

            Ok(Response::new()
            .add_message(burn_msg)
            .add_attribute("amount_repaid", credit_asset_balance))
        },
        
        Err(err) => {
            //Its reply on success only
            Ok(Response::new().add_attribute("error", err))
        }
    }    
}

/// On success, update position claims & attempt to withdraw leftover using a WithdrawMsg
pub fn handle_close_position_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(_result) => {
            //Load Close Position Prop
            let state_propagation: ClosePositionPropagation = CLOSE_POSITION.load(deps.storage)?;
            
            //Create user info variables
            let valid_position_owner = deps.api.addr_validate(&state_propagation.position_info.position_owner)?;
            let position_id = state_propagation.position_info.position_id;             

            //Load State
            let basket: Basket = BASKET.load(deps.storage)?;
            let config: Config = CONFIG.load(deps.storage)?;
            
            //Query contract balance of the basket credit_asset
            let credit_asset_balance = get_contract_balances(
                deps.querier, 
                env.clone(), 
                vec![basket.credit_asset.info.clone()]
            )?[0];

            //Create repay_msg
            let repay_msg = ExecuteMsg::Repay { 
                position_id, 
                position_owner: Some(valid_position_owner.clone().to_string()),
                send_excess_to: Some(valid_position_owner.clone().to_string()),
            };

            //Create repay_msg with queried funds
            //This works because the contract doesn't hold excess credit_asset, all repayments are burned & revenue isn't minted
            let repay_msg = CosmosMsg::Wasm(WasmMsg::Execute { 
                contract_addr: env.contract.address.to_string(), 
                msg: to_binary(&repay_msg)?, 
                funds: vec![asset_to_coin(
                    Asset { 
                        info: basket.credit_asset.info.clone(),
                        amount: credit_asset_balance.clone(),
                    })?]
            });
            
            //Update position claims for each asset withdrawn + sold
            for withdrawn_collateral in state_propagation.clone().withdrawn_assets{

                update_position_claims(
                    deps.storage, 
                    deps.querier, 
                    env.clone(), 
                    config.clone(),
                    position_id,
                    valid_position_owner.clone(), 
                    withdrawn_collateral.info, 
                    withdrawn_collateral.amount
                )?;
            }

            //Load position
            let (_i, target_position) = match get_target_position(
                deps.storage, 
                valid_position_owner.clone(), 
                position_id, 
            ){
                Ok(position) => position,
                Err(err) => return Err(StdError::GenericErr { msg: err.to_string() })
            };

            //Withdrawing everything thats left
            let assets_to_withdraw: Vec<Asset> = target_position.collateral_assets
                .into_iter()
                .filter(|cAsset| cAsset.asset.amount > Uint128::zero())
                .map(|cAsset| cAsset.asset)
                .collect::<Vec<Asset>>();
            
            if assets_to_withdraw.len() > 0 {                
                //Create WithdrawMsg
                let withdraw_msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute { 
                    contract_addr: env.contract.address.to_string(), 
                    msg: to_binary(& ExecuteMsg::Withdraw { 
                        position_id, 
                        assets: assets_to_withdraw, 
                        send_to: state_propagation.send_to, 
                    })?, 
                    funds: vec![],
                });

                //Response 
                Ok(Response::new()
                    .add_message(repay_msg)
                    .add_attribute("amount_repaid", credit_asset_balance)
                    .add_message(withdraw_msg)
                    .add_attribute("sold_assets", format!("{:?}", state_propagation.withdrawn_assets))            
                )
            } else {
                //Response 
                Ok(Response::new()
                    .add_message(repay_msg)
                    .add_attribute("amount_repaid", credit_asset_balance)
                    .add_attribute("sold_assets", format!("{:?}", state_propagation.withdrawn_assets))            
                )
            }
        },
        
        Err(err) => {
            //Its reply on success only
            Ok(Response::new().add_attribute("error", err))
        }
    }
}

/// On error of an user's Stability Pool repayment, leave leftover handling to the SP reply unless SP wasn't called.
/// If so, sell wall the leftover.
#[allow(unused_variables)]
pub fn handle_user_sp_repay_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(_result) => {
            //Its reply on error only
            Ok(Response::new())
        }        
        Err(string) => {
            //If error, do nothing if the SP was used
            //The SP reply will handle the sell wall
            let mut submessages: Vec<SubMsg> = vec![];
            let mut repay_amount = Decimal::zero();
            let mut prop: LiquidationPropagation = LIQUIDATION.load(deps.storage)?;

            //If SP wasn't called, meaning User's SP funds can't be handled there, sell wall the leftovers
            if prop.stability_pool == Decimal::zero() {                
                repay_amount = prop.clone().user_repay_amount;

                //Sell wall asset's repayment amount
                let (sell_wall_msgs, lp_withdraw_msgs) = sell_wall(
                    deps.storage, 
                    deps.querier,
                    env, 
                    &mut prop, 
                    repay_amount
                ).map_err(|err| StdError::GenericErr { msg: err.to_string() } )?;
                //Turn lp withdraw msgs into submessages so they run before the sell_wall_msgs
                let lp_withdraw_msgs = lp_withdraw_msgs.into_iter().map(|msg| SubMsg::new(msg)).collect::<Vec<SubMsg>>();
                submessages.extend(lp_withdraw_msgs);
                submessages.extend(sell_wall_msgs);

            } else {                    
                //Since Error && SP was used (ie there will be a reply later in the execution)...
                //we add the leftovers to the liq_queue_leftovers so the stability pool reply handles it
                prop.liq_queue_leftovers += prop.user_repay_amount;
            }

            LIQUIDATION.save(deps.storage, &prop)?;

            Ok(Response::new()
                .add_submessages(submessages)
                .add_attribute("error", string)
                .add_attribute("sent_to_sell_wall", repay_amount.to_string()))
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

/// The reply used to handle all liquidation leftovers. Prioritizes use of the SP, then the sell wall.
#[allow(unused_variables)]
pub fn handle_stability_pool_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    //Initialize Response Attributes
    let mut attrs = vec![];

    match msg.result.into_result() {
        Ok(result) => {
            //1) Parse potential leftover amount and send to sell_wall if there is any
            //Don't need to change state bc the SP will be repaying thru the contract
            //There should only be leftover here if the SP loses funds between the query and the repayment
            //2) Send collateral to the SP in the repay function and call distribute

            let liq_event = result
                .events
                .iter()
                .find(|e| {
                    e.attributes
                        .iter()
                        .any(|attr| attr.key == "leftover_repayment")
                })
                .ok_or_else(|| {
                    StdError::GenericErr { msg: String::from("unable to find stability pool event") }
                })?;

            let leftover = &liq_event
                .attributes
                .iter()
                .find(|attr| attr.key == "leftover_repayment")
                .unwrap()
                .value;

            let leftover_amount = Uint128::from_str(leftover)?;

            let mut liquidation_propagation = LIQUIDATION.load(deps.storage)?;
            let mut submessages = vec![];

            //Success w/ leftovers: Sell Wall combined leftovers
            //Success w/o leftovers: Send LQ leftovers to the SP
            //Error: Sell Wall combined leftovers
            if leftover_amount != Uint128::zero() {
                panic!("{:?}--line 397", liquidation_propagation);
                attrs.push(attr("leftover_amount", leftover_amount.clone().to_string()));

                let repay_amount = liquidation_propagation.clone().liq_queue_leftovers
                + Decimal::from_ratio(leftover_amount, Uint128::new(1u128));
                
                //Sell Wall SP, LQ and User's SP Fund leftovers
                let (sell_wall_msgs, lp_withdraw_msgs) = sell_wall(
                    deps.storage, 
                    deps.querier,
                    env, 
                    &mut liquidation_propagation, 
                    repay_amount
                ).map_err(|err| StdError::GenericErr { msg: err.to_string() } )?;
                //Turn lp withdraw msgs into submessages so they run before the sell_wall_msgs
                let lp_withdraw_msgs = lp_withdraw_msgs.into_iter().map(|msg| SubMsg::new(msg)).collect::<Vec<SubMsg>>();
                submessages.extend(lp_withdraw_msgs);
                submessages.extend(sell_wall_msgs);

                //Save to propagate
                LIQUIDATION.save(deps.storage, &liquidation_propagation)?;

            //Go to SP if LQ has leftovers
            //Using 1 instead of 0 to account for rounding errors
            } else if liquidation_propagation.clone().liq_queue_leftovers > Decimal::one(){

                //Send LQ leftovers to SP
                //This is an SP reply so we don't have to check if the SP is okay to call
                let config: Config = liquidation_propagation.clone().config;

                //Send leftovers to the Stability Pool
                let sp_repay_amount = liquidation_propagation.clone().liq_queue_leftovers;

                attrs.push(attr("sent_to_sp", sp_repay_amount.clone().to_string()));

                //Stability Pool message builder
                let liq_msg = SP_ExecuteMsg::Liquidate {
                    liq_amount: sp_repay_amount
                };

                let msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: config.stability_pool.unwrap().to_string(),
                    msg: to_binary(&liq_msg)?,
                    funds: vec![],
                });

                let sub_msg: SubMsg = SubMsg::reply_always(msg, STABILITY_POOL_REPLY_ID);

                submessages.push(sub_msg);
                
                //Remove repayment from leftovers
                liquidation_propagation.liq_queue_leftovers -= sp_repay_amount;
                
                //If the first stability pool message succeed and needs to call a 2nd here,
                //We set the stability_pool amount in the propogation to the 2nd amount so that...
                //..if the 2nd errors, then it'll sell wall the correct amount
                liquidation_propagation.stability_pool = sp_repay_amount;

                LIQUIDATION.save(deps.storage, &liquidation_propagation)?;
                // }                
            }

            Ok(Response::new()
                .add_submessages(submessages)
                .add_attributes(attrs))
        }
        Err(_) => {
            //If error, sell wall the SP repay amount and LQ leftovers
            let mut liquidation_propagation = LIQUIDATION.load(deps.storage)?;
            panic!("{:?}--line 465", liquidation_propagation);

            let repay_amount = liquidation_propagation.liq_queue_leftovers + liquidation_propagation.stability_pool;
            
            //Sell wall remaining
            let (sell_wall_msgs, lp_withdraw_msgs) = sell_wall(
                deps.storage, 
                deps.querier,
                env, 
                &mut liquidation_propagation, 
                repay_amount
            ).map_err(|err| StdError::GenericErr { msg: err.to_string() } )?;
            //Turn lp withdraw msgs into submessages so they run before the sell_wall_msgs
            let lp_withdraw_msgs = lp_withdraw_msgs.into_iter().map(|msg| SubMsg::new(msg)).collect::<Vec<SubMsg>>();
            
            attrs.push(attr(
                "sent_to_sell_wall",
                (repay_amount)
                    .to_string(),
            ));

            //Set both liq amounts to 0
            liquidation_propagation.liq_queue_leftovers = Decimal::zero();
            liquidation_propagation.stability_pool = Decimal::zero();
            
            LIQUIDATION.save(deps.storage, &liquidation_propagation)?;
            
            Ok(Response::new()
                .add_submessages(lp_withdraw_msgs)
                .add_submessages(sell_wall_msgs)
                .add_attributes(attrs))
        }
    }
}

/// Send the liquidation queue its collateral reward.
/// If the SP wasn't used, send leftovers to the sell wall.
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
                .ok_or_else(|| StdError::GenericErr {  msg: "unable to find liq-queue event".to_string()})?;

            let repay = &liq_event
                .attributes
                .iter()
                .find(|attr| attr.key == "repay_amount")
                .unwrap()
                .value;
            let repay_amount = Uint128::from_str(repay)?;

            let mut prop: LiquidationPropagation = LIQUIDATION.load(deps.storage)?;
            let mut basket = prop.clone().basket;

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

            let token_info: AssetInfo = if asset_info.eq(&"token".to_string()) {
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
            
            //Subtract repaid amount from LQs repay responsibility. If it hits 0 then there were no LQ or User SP fund errors.
            if repay_amount != Uint128::zero() {
                if !prop.liq_queue_leftovers.is_zero() {
                    
                    prop.liq_queue_leftovers = decimal_subtraction(
                        prop.liq_queue_leftovers,
                        Decimal::from_ratio(repay_amount, Uint128::new(1u128)),
                    )?;
                    //SP reply handles LQ_leftovers

                    //Update credit amount based on liquidation's total repaid amount
                    prop.target_position.credit_amount -= repay_amount;
                } else {
                    return Err(StdError::GenericErr { msg: "LQ_leftovers is 0 before finishing LQ liquidations".to_string() })
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
            }

            //If this is the last asset left to send and the SP wasn't used, update position
            //This is because the SP's liq_repay would update the position's collateral otherwise
            //the idea is to only have a single position update to save gas
            if prop.per_asset_repayment.len() == 1 && prop.stability_pool == Decimal::zero(){

                //////Update Basket and save
                if prop.clone().target_position.credit_amount.is_zero(){                
                    //Remove position's assets from Supply caps 
                    update_basket_tally(
                        deps.storage, 
                        deps.querier, 
                        env.clone(), 
                        &mut basket, 
                        prop.clone().target_position.collateral_assets,
                        false, 
                        prop.clone().config,
                        true
                    ).map_err(|err| StdError::GenericErr { msg: err.to_string() })?;
                } else {                    
                    //Remove liquidated assets from Supply caps 
                    update_basket_tally(
                        deps.storage, 
                        deps.querier, 
                        env.clone(), 
                        &mut basket, 
                        prop.clone().liquidated_assets, 
                        false, 
                        prop.clone().config,
                        true
                    ).map_err(|err| StdError::GenericErr { msg: err.to_string() })?;
                }
                BASKET.save(deps.storage, &basket)?;
                
                //LQ rounding errors can cause the repay_amount to be 1e-6 off
                if prop.clone().target_position.credit_amount == Uint128::one(){
                    prop.target_position.credit_amount = Uint128::zero();
                }

                //Update position
                update_position(deps.storage, prop.clone().position_owner, prop.clone().target_position)?;

            }
            //Remove Asset
            prop.per_asset_repayment.remove(0);
            LIQUIDATION.save(deps.storage, &prop)?;

            attrs.extend(vec![
                attr("repay_amount", repay_amount),
                attr("reward_amount", send_amount),
                attr("reward_info", token_info.to_string()),
            ]);

            Ok(Response::new().add_message(msg).add_attributes(attrs))
        }
        Err(string) => {
            //If error, do nothing if the SP was used. The SP reply will handle the sell wall.
            //Else, handle leftovers here

            let mut repay_amount = Decimal::zero();

            let mut prop: LiquidationPropagation = LIQUIDATION.load(deps.storage)?;
            panic!("{:?}--line 662", prop);

            //If SP wasn't called, meaning LQ leftovers can't be handled there, sell wall this asset's leftovers
            //Replies are FIFO so we remove from front
            if prop.stability_pool == Decimal::zero() {
                
                repay_amount = prop.clone().per_asset_repayment[0];

                //Sell wall asset's repayment amount
                let (sell_wall_msgs, lp_withdraw_msgs) = sell_wall(
                    deps.storage,
                    deps.querier,
                    env,
                    &mut prop,
                    repay_amount,
                ).map_err(|err| StdError::GenericErr { msg: err.to_string() } )?;
                //Turn lp withdraw msgs into submessages so they run before the sell_wall_msgs
                let lp_withdraw_msgs = lp_withdraw_msgs.into_iter().map(|msg| SubMsg::new(msg)).collect::<Vec<SubMsg>>();
                                
                return Ok(Response::new()
                    .add_submessages(lp_withdraw_msgs)
                    .add_submessages(sell_wall_msgs)
                    .add_attribute("error", string)
                    .add_attribute("sent_to_sell_wall", repay_amount.to_string()))
               
            }

            prop.per_asset_repayment.remove(0);
            LIQUIDATION.save(deps.storage, &prop)?;
            
            Ok(Response::new()
                .add_attribute("error", string)
                .add_attribute("sent_to_sell_wall", repay_amount.to_string()))
        }
    }
}

