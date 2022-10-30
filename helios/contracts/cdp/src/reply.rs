use std::str::FromStr;

use cosmwasm_std::{DepsMut, Env, Reply, StdResult, Response, SubMsg, Decimal, Uint128, StdError, attr, to_binary, WasmMsg, CosmosMsg, Storage, QuerierWrapper};

use membrane::types::{AssetInfo, Asset, Basket, LiqAsset, SellWallDistribution, Position};
use membrane::stability_pool::{ExecuteMsg as SP_ExecuteMsg};
use membrane::positions::{Config, ExecuteMsg};
use membrane::math::decimal_subtraction;

use crate::state::{RepayPropagation, REPAY, WITHDRAW, CONFIG, BASKETS, CLOSE_POSITION, ClosePositionPropagation};
use crate::contract::get_contract_balances;
use crate::positions::{get_target_position, withdrawal_msg, update_position_claims};
use crate::liquidations::{query_stability_pool_liquidatible, STABILITY_POOL_REPLY_ID, sell_wall_using_ids, SELL_WALL_REPLY_ID};

 //On success....
//Update position claims
//attempt to withdraw leftover using a WithdrawMsg
pub fn handle_close_position_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(_result) => {
            //Load Close Position Prop
            let state_propagation: ClosePositionPropagation = CLOSE_POSITION.load(deps.storage)?;
            
            //Create user info variables
            let valid_position_owner = deps.api.addr_validate(&state_propagation.position_info.position_owner)?;
            let basket_id = state_propagation.position_info.basket_id; 
            let position_id = state_propagation.position_info.position_id; 
            
            //Update position claims for each withdrawn + sold amount
            for withdrawn_collateral in state_propagation.clone().withdrawn_assets{

                update_position_claims(
                    deps.storage, 
                    deps.querier, 
                    env.clone(), 
                    basket_id.clone(), 
                    position_id.clone(), 
                    valid_position_owner.clone(), 
                    withdrawn_collateral.info, 
                    withdrawn_collateral.amount
                )?;
            }

            //Load position
            let target_position = match get_target_position(
                deps.storage, 
                basket_id.clone(), 
                valid_position_owner, 
                position_id.clone(), 
            ){
                Ok(position) => position,
                Err(err) => return Err(StdError::GenericErr { msg: err.to_string() })
            };

            //Withdrawing everything thats left
            let assets_to_withdraw: Vec<Asset> = target_position.collateral_assets
                .into_iter()
                .map(|cAsset| cAsset.asset)
                .collect::<Vec<Asset>>();

            //Create WithdrawMsg
            let withdraw_msg = CosmosMsg::Wasm(WasmMsg::Execute { 
                contract_addr: env.contract.address.to_string(), 
                msg: to_binary(& ExecuteMsg::Withdraw { 
                    basket_id, 
                    position_id, 
                    assets: assets_to_withdraw, 
                    send_to: state_propagation.send_to, 
                })?, 
                funds: vec![],
            });


            //Response 
            Ok(Response::new().add_message(withdraw_msg)
                .add_attribute("sold_assets", format!("{:?}", state_propagation.withdrawn_assets))            
            )
        },
        
        Err(err) => {
            //Its reply on success only
            Ok(Response::new().add_attribute("error", err))
        }
    }
}

pub fn handle_sp_repay_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
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

            let mut prop: RepayPropagation = REPAY.load(deps.storage)?;

            //If SP wasn't called, meaning User's SP funds can't be handled there, sell wall the leftovers
            if prop.stability_pool == Decimal::zero() {
                
                repay_amount = prop.clone().user_repay_amount;

                //Sell wall asset's repayment amount
                sell_wall_in_reply(deps.storage, env.clone(), deps.querier, &mut prop, &mut submessages, repay_amount.clone())?;

            } else {                    
                //Since Error && SP was used (ie there will be a reply later in the execution)...
                //we add the leftovers to the liq_queue_leftovers so the stability pool reply handles it
                prop.liq_queue_leftovers += prop.user_repay_amount;
            }


            REPAY.save(deps.storage, &prop)?;

            Ok(Response::new()
                .add_submessages(submessages)
                .add_attribute("error", string)
                .add_attribute("sent_to_sell_wall", repay_amount.to_string()))
        }
    }
}

pub fn handle_withdraw_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    //Initialize Response Attributes
    let mut attrs = vec![];

    //Match on msg.result
    match msg.result.into_result() {
        Ok(_result) => {
            let mut withdraw_prop = WITHDRAW.load(deps.storage)?;

            //Assert valid withdrawal for each asset this reply is
            for _i in 0..withdraw_prop.reply_order[0] {
                let asset_info: AssetInfo = withdraw_prop.positions_prev_collateral[0].clone().info;
                let position_amount: Uint128 = withdraw_prop.positions_prev_collateral[0].amount;
                let withdraw_amount: Uint128 = withdraw_prop.withdraw_amounts[0];

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

                //If balance differnce is more than what they tried to withdraw or position amount, error
                if withdraw_prop.contracts_prev_collateral_amount[0] - current_asset_balance
                    > position_amount
                    || withdraw_prop.contracts_prev_collateral_amount[0] - current_asset_balance
                        > withdraw_amount
                {
                    return Err(StdError::GenericErr {
                        msg: format!(
                            "Conditional 1: Invalid withdrawal, possible bug found by {}",
                            withdraw_prop.position_info.position_owner.clone()
                        ),
                    });
                }

                match get_target_position(
                    deps.storage,
                    withdraw_prop.position_info.basket_id,
                    deps.api
                        .addr_validate(&withdraw_prop.position_info.position_owner.clone())?,
                    withdraw_prop.position_info.position_id,
                ) {
                    Ok(user_position) => {
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
                        //Error means the position was deleted from state, assert that
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

                //Remove the first entry from each field
                withdraw_prop.positions_prev_collateral.remove(0);
                withdraw_prop.withdraw_amounts.remove(0);
                withdraw_prop.contracts_prev_collateral_amount.remove(0);
            }

            //Remove used reply_order entry
            withdraw_prop.reply_order.remove(0);

            //Save new prop
            WITHDRAW.save(deps.storage, &withdraw_prop)?;

            //We can go by first entries for these fields bc the replies will come in FIFO in terms of assets sent
            //The reply_order numbers are used to loop the logic on the list of native assets whenever it arrives while still allowing Cw20s to work in the reply
        } //We only reply on success
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }

    Ok(Response::new().add_attributes(attrs))
}

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
                    StdError::generic_err(format!("unable to find stability pool event"))
                })?;

            let leftover = &liq_event
                .attributes
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
            if leftover_amount != Uint128::zero() {
                attrs.push(attr("leftover_amount", leftover_amount.clone().to_string()));

                let repay_amount = repay_propagation.clone().liq_queue_leftovers
                + Decimal::from_ratio(leftover_amount, Uint128::new(1u128));

                //Sell Wall SP, LQ and User's SP Fund leftovers
                sell_wall_in_reply(deps.storage, env.clone(), deps.querier, &mut repay_propagation, &mut submessages, repay_amount.clone())?;

                //Save to propagate
                REPAY.save(deps.storage, &repay_propagation)?;
            } else {
                //Send LQ leftovers to SP
                //This is an SP reply so we don't have to check if the SP is okay to call
                let config: Config = CONFIG.load(deps.storage)?;

                let basket: Basket = BASKETS.load(
                    deps.storage,
                    repay_propagation.clone().basket_id.to_string(),
                )?;

                //Check for stability pool funds before any liquidation attempts
                //Sell wall any leftovers
                let leftover_repayment = query_stability_pool_liquidatible(
                    deps.querier,
                    config.clone(),
                    repay_propagation.clone().liq_queue_leftovers,
                    basket.clone().credit_asset.info,
                )?;

                //If there are leftovers, send to sell wall
                if leftover_repayment > Decimal::zero() {
                    attrs.push(attr(
                        "leftover_amount",
                        leftover_repayment.clone().to_string(),
                    ));
                    
                    //Sell wall remaining
                    sell_wall_in_reply(deps.storage, env.clone(), deps.querier, &mut repay_propagation, &mut submessages, leftover_repayment)?;
                    
                    REPAY.save(deps.storage, &repay_propagation)?;
                   
                }

                //Send whatever is able to the Stability Pool
                let sp_repay_amount = decimal_subtraction(
                    repay_propagation.clone().liq_queue_leftovers,
                    leftover_repayment.clone(),
                );

                if !sp_repay_amount.is_zero() {
                    attrs.push(attr("sent_to_sp", sp_repay_amount.clone().to_string()));

                    //Stability Pool message builder
                    let liq_msg = SP_ExecuteMsg::Liquidate {
                        credit_asset: LiqAsset {
                            amount: sp_repay_amount,
                            info: basket.clone().credit_asset.info,
                        },
                    };

                    let msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: config.stability_pool.unwrap().to_string(),
                        msg: to_binary(&liq_msg)?,
                        funds: vec![],
                    });

                    let sub_msg: SubMsg = SubMsg::reply_always(msg, STABILITY_POOL_REPLY_ID);

                    submessages.push(sub_msg);

                    //Have to reload due to prior saves
                    let mut repay_propagation = REPAY.load(deps.storage)?;

                    //Remove repayment from leftovers
                    repay_propagation.liq_queue_leftovers -= sp_repay_amount;

                    //If the first stability pool message succeed and needs to call a 2nd here,
                    //We set the stability_pool amount in the propogation to the 2nd amount so that...
                    //..if the 2nd errors, then it'll sell wall the correct amount
                    repay_propagation.stability_pool = sp_repay_amount;

                    REPAY.save(deps.storage, &repay_propagation)?;
                }
            }

            Ok(Response::new()
                .add_submessages(submessages)
                .add_attributes(attrs))
        }
        Err(_) => {

            let mut submessages: Vec<SubMsg> = vec![];

            //If error, sell wall the SP repay amount and LQ leftovers
            let mut repay_propagation = REPAY.load(deps.storage)?;

            let repay_amount = repay_propagation.liq_queue_leftovers + repay_propagation.stability_pool;

            //Sell wall remaining
            sell_wall_in_reply(deps.storage, env.clone(), deps.querier, &mut repay_propagation, &mut submessages, repay_amount.clone())?;

            attrs.push(attr(
                "sent_to_sell_wall",
                (repay_amount)
                    .to_string(),
            ));

            //Set both liq amounts to 0
            repay_propagation.liq_queue_leftovers = Decimal::zero();
            repay_propagation.stability_pool = Decimal::zero();

            
            REPAY.save(deps.storage, &repay_propagation)?;

            Ok(Response::new().add_submessages(submessages).add_attributes(attrs))
        }
    }
}

//Add to the front of the "stack" bc message semantics are depth first
//LIFO
fn add_distributions(
    mut old_distributions: Vec<SellWallDistribution>,
    new_distrbiutions: SellWallDistribution,
) -> Vec<SellWallDistribution> {
    old_distributions.push(new_distrbiutions);

    old_distributions
}

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
                .ok_or_else(|| StdError::generic_err(format!("unable to find liq-queue event")))?;

            let repay = &liq_event
                .attributes
                .iter()
                .find(|attr| attr.key == "repay_amount")
                .unwrap()
                .value;

            let repay_amount = Uint128::from_str(&repay)?;

            let mut prop: RepayPropagation = REPAY.load(deps.storage)?;

            let basket = BASKETS.load(deps.storage, prop.basket_id.to_string())?;

            //Send successfully liquidated amount
            let amount = &liq_event
                .attributes
                .iter()
                .find(|attr| attr.key == "collateral_amount")
                .unwrap()
                .value;

            let send_amount = Uint128::from_str(&amount)?;

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
                    address: deps.api.addr_validate(&token)?,
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
                basket.liq_queue.unwrap(),
            )?;

            //Subtract repaid amount from LQs repay responsibility. If it hits 0 then there were no LQ or User SP fund errors.
            if repay_amount != Uint128::zero() {
                if !prop.liq_queue_leftovers.is_zero() {
                    prop.liq_queue_leftovers = decimal_subtraction(
                        prop.liq_queue_leftovers,
                        Decimal::from_ratio(repay_amount, Uint128::new(1u128)),
                    );

                    //SP reply handles LQ_leftovers
                }

                update_position_claims(
                    deps.storage,
                    deps.querier,
                    env,
                    prop.basket_id,
                    prop.clone().position_id,
                    prop.clone().position_owner,
                    token_info.clone(),
                    send_amount,
                )?;
            }
            //Remove Asset
            prop.per_asset_repayment.remove(0);
            REPAY.save(deps.storage, &prop)?;

            attrs.extend(vec![
                attr("repay_amount", repay_amount),
                attr("reward_amount", send_amount),
                attr("reward_info", token_info.to_string()),
            ]);

            Ok(Response::new().add_message(msg).add_attributes(attrs))
        }
        Err(string) => {
            //If error, do nothing if the SP was used
            //The SP reply will handle the sell wall

            let mut submessages: Vec<SubMsg> = vec![];
            let mut repay_amount = Decimal::zero();

            let mut prop: RepayPropagation = REPAY.load(deps.storage)?;

            //If SP wasn't called, meaning LQ leftovers can't be handled there, sell wall this asset's leftovers
            //Replies are FIFO so we remove from front
            if prop.stability_pool == Decimal::zero() {
                
                repay_amount = prop.clone().per_asset_repayment[0];

                //Sell wall asset's repayment amount
                sell_wall_in_reply(deps.storage, env.clone(), deps.querier, &mut prop, &mut submessages, repay_amount.clone())?;
               
            }

            prop.per_asset_repayment.remove(0);
            REPAY.save(deps.storage, &prop)?;

            Ok(Response::new()
                .add_submessages(submessages)
                .add_attribute("error", string)
                .add_attribute("sent_to_sell_wall", repay_amount.to_string()))
        }
    }
}

pub fn handle_sell_wall_reply(deps: DepsMut, msg: Reply, env: Env) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(_result) => {
            //On success we update the position owner's claims bc it means the protocol sent assets on their behalf
            let mut repay_propagation = REPAY.load(deps.storage)?;

            let res = Response::new();
            let mut attrs = vec![];

            //We use the distribution at the end of the list bc new ones were appended in the SP or LQ replies, and msgs are fulfilled depth first.
            match repay_propagation.sell_wall_distributions.pop() {
                Some(distribution) => {
                    //Update position claims for each distributed asset
                    for (asset, amount) in distribution.distributions {
                        update_position_claims(
                            deps.storage,
                            deps.querier,
                            env.clone(),
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
                        attrs.push(("distribution", res_asset.to_string()));
                    }
                }
                None => {
                    //If None it means the distribution wasn't added when the sell wall msg was added which should be impossible
                    //Either way, Error
                    return Err(StdError::GenericErr {
                        msg: "Distributions were added to the state propagation incorrectly"
                            .to_string(),
                    });
                }
            }

            //Save propagation w/ removed tail
            REPAY.save(deps.storage, &repay_propagation)?;

            Ok(res.add_attributes(attrs))
        }
        Err(string) => {
            //This is only reply_on_success so this shouldn't be reached
            Ok(Response::new().add_attribute("error", string))
        }
    }
}

//Adds sell wall submessages to list of submessages
pub fn sell_wall_in_reply(
    storage: &mut dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    prop: &mut RepayPropagation,
    submessages: &mut Vec<SubMsg>,
    repay_amount: Decimal,
) -> StdResult<()>{
    //Sell wall asset's repayment amount
    let (sell_wall_msgs, collateral_distributions) = sell_wall_using_ids(
        storage,
        env,
        querier,
        prop.clone().basket_id,
        prop.clone().position_id,
        prop.clone().position_owner,
        repay_amount,
    )?;
    

    //Save new distributions from this liquidation
    prop.sell_wall_distributions = add_distributions(
        prop.clone().sell_wall_distributions,
        SellWallDistribution {
            distributions: collateral_distributions,
        },
    );

    submessages.extend(
        sell_wall_msgs
            .into_iter()
            .map(|msg| {
                //If this succeeds, we update the positions collateral claims
                //If this fails, revert. Try again isn't a useful alternative.
                SubMsg::reply_on_success(msg, SELL_WALL_REPLY_ID)
            })
            .collect::<Vec<SubMsg>>(),
    );

    Ok( () )
}
