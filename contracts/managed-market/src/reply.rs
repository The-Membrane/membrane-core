use std::str::FromStr;

use cosmwasm_std::{attr, to_json_binary, CosmosMsg, Decimal, DepsMut, Env, Reply, Response, StdError, StdResult, SubMsg, Uint128, WasmMsg};

use membrane::managed_market::{Config, ExecuteMsg, MarketParams};
use membrane::math::{decimal_division, decimal_multiplication, decimal_subtraction};
use membrane::oracle::PriceResponse;
use membrane::types::{cAsset, Asset, AssetInfo, Basket, PurchaseData, UserHistory};
use membrane::helpers::{asset_to_coin, get_contract_balances, withdrawal_msg};

use crate::positions::{CDT_DENOM, LTV_CHECK_REPLY_ID};
use crate::state::{ClosePositionPropagation, LiquidationPropagation, LoopPropagation, CLOSE_POSITION, CONFIG, LIQUIDATION, LOOP_POSITION, MARKET_PARAMS, POSITIONS, POSITION_UX_BOOSTS, USER_HISTORY};
use crate::oracle::{get_cdt_price, get_collateral_price};
//Liquidation reply
//Reply on success to:
// - edit the user position based on the CDT that was swapped for
// - edit the market's total borrowed.
#[allow(unused_variables)]
pub fn handle_liquidation_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(_result) => {
            //Load liquidation propogation
            let liquidation_propagation: LiquidationPropagation = LIQUIDATION.load(deps.storage)?;

            //Query the contract balance of the credit asset
            let current_cdt_balance = deps.querier.query_balance(env.contract.address, CDT_DENOM.to_string())?.amount;

            //Calculate the balance of newly acquired CDT
            let cdt_swapped_for = current_cdt_balance
                .checked_sub(liquidation_propagation.pre_liquidation_cdt_balance)
                .map_err(|_| StdError::generic_err("CDT balance went down"))?;

            //Load user state
            let mut user_position = POSITIONS.load(deps.storage, (liquidation_propagation.position_owner.clone(), liquidation_propagation.collateral_denom.clone()))?;

            //Initialize potential excess swap amount
            let mut excess_swap_amount: Uint128 = Uint128::zero();
            let mut msgs = vec![];

            //Edit user position with the debt purchased
            user_position.debt_amount = match user_position.debt_amount
                .checked_sub(cdt_swapped_for){
                Ok(debt_amount) => {
                    debt_amount
                },
                Err(_) => {
                    //If the amount is more than the current debt, we calculate the excess amount

                    excess_swap_amount = cdt_swapped_for - user_position.debt_amount;

                    //Set the debt to zero
                    Uint128::zero()
                }
                };
            //Save user position
            POSITIONS.save(deps.storage, (liquidation_propagation.position_owner.clone(), liquidation_propagation.collateral_denom.clone()), &user_position)?;

            //If there is an excess amount, we send it back to the user
            if excess_swap_amount > Uint128::zero() {
                //Create bank send msg
                let send_msg = withdrawal_msg(
                    Asset {
                        info: AssetInfo::NativeToken { denom: CDT_DENOM.to_string() },
                        amount: excess_swap_amount,
                    },
                    liquidation_propagation.position_owner.clone(),
                )?;
                msgs.push(send_msg);
            }

            //Edit market's total borrowed
            let mut market: MarketParams = MARKET_PARAMS.load(deps.storage, liquidation_propagation.collateral_denom.clone())?;
            market.total_borrowed = market.total_borrowed
                .checked_sub(cdt_swapped_for - excess_swap_amount)
                .map_err(|_| StdError::generic_err("The amount swapped for is more than the market's total borrowed"))?;
            MARKET_PARAMS.save(deps.storage, liquidation_propagation.collateral_denom.clone(), &market)?;

            //Remove liquidation propagation
            LIQUIDATION.remove(deps.storage);

            
            Ok(Response::new()
                .add_attribute("liquidation_position_owner", liquidation_propagation.position_owner.to_string())
                .add_attribute("debt_recovered", cdt_swapped_for)
                .add_attribute("actual_debt_repaid", cdt_swapped_for - excess_swap_amount)
                .add_attribute("cdt_returned_to_user", excess_swap_amount)
                
                .add_messages(msgs)
        )
        }        
        Err(string) => {
            //Its reply on success only
            Ok(Response::new())
        }
    }
}

/// On success
/// - Repay the position
/// - Withdraw the assets if the repayment would leave no debt left
pub fn handle_close_position_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(_result) => {
            //Init msgs
            let mut msgs: Vec<SubMsg> = vec![];

            //Load Close Position Prop
            let close_prop: ClosePositionPropagation = CLOSE_POSITION.load(deps.storage)?;

            //Load position
            let target_position = match POSITIONS.load(deps.storage, (deps.api.addr_validate(&close_prop.position_owner)?, close_prop.collateral_denom.clone())) {
                Ok(position) => position,
                Err(err) => return Err(StdError::GenericErr { msg: err.to_string() })
            };

            //Load position ux boosts
            if let Ok(target_position_ux_boosts) = POSITION_UX_BOOSTS.load(deps.storage, (deps.api.addr_validate(&close_prop.position_owner)?, close_prop.collateral_denom.clone())) {
                
                //If the collateral bought list is a single instance, we calculate proft made or loss from the swap.
                //A single instance means this is the data from the final loop.
                if target_position_ux_boosts.collateral_bought_from_loops.len() == 1 {
                    //Load user history 
                    let mut user_history = match USER_HISTORY.load(deps.storage, close_prop.position_owner.clone()) {
                        Ok(history) => history,
                        Err(_) => {
                            vec![UserHistory {
                                collateral_denom: close_prop.collateral_denom.clone(),
                                user: close_prop.position_owner.clone(),
                                alias: None,
                                profits: Decimal::zero(),
                                losses: Decimal::zero(),
                                volume: Decimal::zero(),
                            }]
                        }
                    };
                    //Get the average purchase price
                    let average_purchase_price = target_position_ux_boosts.collateral_bought_from_loops[0].post_purchase_price;

                    //Get the current price of the collateral
                    let current_price = get_collateral_price(
                        deps.storage,
                        deps.querier,
                        env.clone(), 
                        MARKET_PARAMS.load(deps.storage, close_prop.collateral_denom.clone())?,
                    ).map_err(|_| StdError::generic_err("Failed to get collateral price"))?;
                    //set loss to false
                    let mut loss = false;
                    //Calculate the profit made or loss from the swap
                    let price_difference = match decimal_subtraction(
                        current_price.price,
                        average_purchase_price
                    ){
                        Ok(diff) => diff,
                        Err(_) => {
                            //Set as a loss
                            loss = true;
                            //Calculate the price difference
                            decimal_subtraction(    
                                average_purchase_price,
                                current_price.price
                            ).map_err(|_| StdError::generic_err("Failed to get price difference"))?
                        }
                        
                    };
                    //Create new PriceResponse
                    let price_response = PriceResponse {
                        price: price_difference,
                        decimals: current_price.decimals,
                        prices: vec![],
                    };

                    //Calculate the value of the profit or loss
                    let value_realized = price_response.get_value(close_prop.collateral_swapped)?;
                    //Calculate the volume based on the current price
                    let volume = current_price.get_value(close_prop.collateral_swapped)?;

                    if loss {
                        //Find the user history for the collateral denom and add to losses & volume
                        user_history.iter_mut().enumerate().for_each(|(index, history)| {
                            if history.collateral_denom == close_prop.collateral_denom {
                                //Add to losses
                                history.losses += value_realized;
                                //Add to volume
                                history.volume += volume;
                            }
                        });
                    } else {    
                        //Find the user history for the collateral denom and add to profits & volume
                        user_history.iter_mut().enumerate().for_each(|(index, history)| {
                            if history.collateral_denom == close_prop.collateral_denom {
                                //Add to profits
                                history.profits += value_realized;
                                //Add to volume
                                history.volume += volume;
                            }
                        });
                    }

                    //Save user history
                    USER_HISTORY.save(deps.storage, close_prop.position_owner.clone(), &user_history)?;
                }
                
            };


            //Load State
            // let config: Config = CONFIG.load(deps.storage)?;

            //Query contract balance of the debt denom
            let post_close_debt_balance = get_contract_balances(
                deps.querier, 
                env.clone(), 
                vec![
                    AssetInfo::NativeToken { denom: CDT_DENOM.to_string() }
                ]
            )?[0];

            //Calculate the amount of debt tokens earned from the swap
            let amount_swapped_for = match post_close_debt_balance.checked_sub(close_prop.pre_close_debt_balance){
                Ok(amount) => amount,
                Err(_) => {
                    return Err(StdError::generic_err(format!("The new debt token balance is less than the previous debt token balance. {} < {}", post_close_debt_balance, close_prop.pre_close_debt_balance) ))
                }
            };

            //Create repay_msg
            let repay_msg = ExecuteMsg::Repay { 
                collateral_denom: close_prop.collateral_denom.clone(),
                send_excess_to: close_prop.send_to.clone(),
            };


            //Create repay_msg with swapped for funds
            let repay_msg = CosmosMsg::Wasm(WasmMsg::Execute { 
                contract_addr: env.contract.address.to_string(), 
                msg: to_json_binary(&repay_msg)?, 
                funds: vec![asset_to_coin(
                    Asset { 
                        info: AssetInfo::NativeToken { denom: CDT_DENOM.to_string() },
                        amount: amount_swapped_for,
                    })?]
            });


            //Create WithdrawMsg
            //We only withdraw if the debt will be zero post repaymentt
            let withdraw_msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute { 
                contract_addr: env.contract.address.to_string(), 
                msg: to_json_binary(& ExecuteMsg::WithdrawCollateral { 
                    collateral_denom: close_prop.collateral_denom.clone(), 
                    send_to: close_prop.send_to.clone(), 
                    //We set the withdraw amount to None, so we withdraw all assets
                    withdraw_amount: None,
                } )?, 
                funds: vec![],
            });

            let is_new_debt_zero = amount_swapped_for >= target_position.debt_amount;

            //Add to msgs
            //If new debt is 0 do the LTV check in the withdraw msg,
            //Otherwise do the LTV check in the repay msg
            if is_new_debt_zero {
                msgs.push(SubMsg::new(repay_msg.clone()));
                msgs.push(SubMsg::reply_on_success(withdraw_msg.clone(), LTV_CHECK_REPLY_ID));
            } else {
                msgs.push(SubMsg::reply_on_success(repay_msg.clone(), LTV_CHECK_REPLY_ID));
            }

            //Create response
            let response = Response::new()
                .add_submessages(msgs)
                .add_attribute("action", "close_position")
                .add_attribute("position_owner", close_prop.position_owner)
                .add_attribute("debt_recovered", amount_swapped_for)
                .add_attribute("is_new_debt_zero", is_new_debt_zero.to_string())
                .add_attribute("assets_sent_to", format!("{:?}", close_prop.send_to.clone()));

            Ok(response)

        },

        Err(err) => {
            //Its reply on success only
            Ok(Response::new().add_attribute("error", err))
        }
    }
}

pub fn handle_ltv_check_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(_result) => {
                        //Load Config
                        let mut config: Config = CONFIG.load(deps.storage)?;
        
                        //Load Close Position Prop
                        let close_prop: ClosePositionPropagation = CLOSE_POSITION.load(deps.storage)?;
        
                        let market = match MARKET_PARAMS.load(deps.storage, close_prop.collateral_denom.clone()){
                            Ok(market) => market,
                            Err(_) => return Err(StdError::generic_err(format!("Collateral asset ({:?}) not supported", close_prop.collateral_denom) )),
                        };
                
                        //Validate position owner
                        let position_owner = deps.api.addr_validate(&close_prop.position_owner)?;
                
                        //Load user position
                        let mut user_position = POSITIONS.load(deps.storage, (position_owner.clone(), close_prop.collateral_denom.clone()))?;
                
                        // accrue(
                        //     deps.storage,
                        //     get_total_debt_tokens(config.clone())?,
                        //     total_vault_tokens, 
                        //     env.clone(), 
                        //     &mut config, 
                        //     &mut user_position,
                        //     &mut msgs,
                        //     markets_manager_fee
                        // )?;    
                
                
                        //Check if the position is insolvent
                        let collateral_price = match get_collateral_price(deps.storage, deps.querier, env.clone(), market.clone()){
                            Ok(price) => price,
                            Err(err) => return Err(StdError::generic_err(format!("Failed to get collateral price in ltv check reply: {}", err) ))
                        };
                        let debt_price = match get_cdt_price(deps.querier, env.clone()){
                            Ok(price) => price,
                            Err(err) => return Err(StdError::generic_err(format!("Failed to get debt price in ltv check reply: {}", err) ))
                        };
                        let collateral_value = collateral_price.get_value(user_position.collateral_amount)?;
                        if collateral_value.is_zero() {
                            return Err(StdError::generic_err("Collateral value is zero; cannot calculate LTV".to_string()));
                        }
                        let debt_value = debt_price.get_value(user_position.debt_amount)?;
                        let position_LTV = decimal_division(debt_value, collateral_value)?;
                        if position_LTV < market.collateral_params.liquidation_LTV {
                            Ok(Response::new())
                        } else {
                            Err(StdError::generic_err(format!("Position is insolvent. Position LTV: {} is above the liquidation LTV: {}", position_LTV, market.collateral_params.liquidation_LTV) ))
                        }
            },

            Err(err) => {
                //Its reply on success only
                Ok(Response::new().add_attribute("error", err))
            }
    }
}

//Deposit collateral earned swapped for 
// - Load the difference so we aren't using other user's collateral
// Update collateral_bought_in_loopd
// Save user position 
pub fn handle_loop_position_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(_result) => {
            //Init msgs
            let mut msgs: Vec<CosmosMsg> = vec![];

            //Load Loop Position Prop
            let loop_prop: LoopPropagation = LOOP_POSITION.load(deps.storage)?;

            //Load position ux boosts
            let mut target_position_ux_boosts = match POSITION_UX_BOOSTS.load(deps.storage, (deps.api.addr_validate(&loop_prop.position_owner)?, loop_prop.collateral_denom.clone())) {
                Ok(position) => position,
                Err(err) => return Err(StdError::GenericErr { msg: err.to_string() })
            };

            //Load position
            let target_position = match POSITIONS.load(deps.storage, (deps.api.addr_validate(&loop_prop.position_owner)?, loop_prop.collateral_denom.clone())) {
                Ok(position) => position,
                Err(err) => return Err(StdError::GenericErr { msg: err.to_string() })
            };

            //Load State
            // let config: Config = CONFIG.load(deps.storage)?;

            //Query contract balance of the collateral denom
            let post_loop_collateral_balance = get_contract_balances(
                deps.querier, 
                env.clone(), 
                vec![
                    AssetInfo::NativeToken { denom: loop_prop.collateral_denom.clone().to_string() }
                ]
            )?[0];

            //Calculate the amount of debt tokens earned from the swap
            let amount_swapped_for = match post_loop_collateral_balance.checked_sub(loop_prop.pre_loop_collateral_balance){
                Ok(amount) => amount,
                Err(_) => {
                    return Err(StdError::generic_err(format!("The new collateral token balance is less than the previous collateral token balance. {} < {}", post_loop_collateral_balance, loop_prop.pre_loop_collateral_balance) ))
                }
            };


            //Create deposit_msg
            let deposit_msg = ExecuteMsg::SupplyCollateral { owner: Some(loop_prop.position_owner.clone()) };


            //Create deposit_msg with swapped_for funds
            let deposit_msg = CosmosMsg::Wasm(WasmMsg::Execute { 
                contract_addr: env.contract.address.to_string(), 
                msg: to_json_binary(&deposit_msg)?, 
                funds: vec![asset_to_coin(
                    Asset { 
                        info: AssetInfo::NativeToken { denom: loop_prop.collateral_denom.to_string() },
                        amount: amount_swapped_for,
                    })?]
            });
            //Add to msgs
            msgs.push(deposit_msg.clone());

            //Get post purchase price
            let post_purchase_price = get_collateral_price(
                deps.storage,
                deps.querier,
                env, 
                MARKET_PARAMS.load(deps.storage, loop_prop.collateral_denom.clone())?,
            ).map_err(|_| StdError::generic_err("Failed to get collateral price"))?;

            //Update user position's collateral bought in loops
            target_position_ux_boosts.collateral_bought_from_loops.push(PurchaseData {
                amount_purchased: amount_swapped_for,
                post_purchase_price: post_purchase_price.price,
            });

            //Add the value purchased to the user history volume
            let mut user_history = match USER_HISTORY.load(deps.storage, loop_prop.position_owner.clone()) {
                Ok(history) => history,
                Err(_) => {
                    vec![UserHistory {
                        collateral_denom: loop_prop.collateral_denom.clone(),
                        user: loop_prop.position_owner.clone(),
                        alias: None,
                        profits: Decimal::zero(),
                        losses: Decimal::zero(),
                        volume: Decimal::zero(),
                    }]
                }
            };
            //Calc value
            let value_purchased = post_purchase_price.get_value(amount_swapped_for)?;
            //Add to volume
            user_history.iter_mut().enumerate().for_each(|(index, history)| {
                if history.collateral_denom == loop_prop.collateral_denom {
                    history.volume += value_purchased;
                }
            });
            //Save user history
            USER_HISTORY.save(deps.storage, loop_prop.position_owner.clone(), &user_history)?;

            //////////////CALCS TO CHECK IF THIS IS THE FINAL LOOP/////////////////////
            //Get the sum of collateral bought from loops
            let total_bought_from_loops = target_position_ux_boosts.collateral_bought_from_loops.iter().map(|collateral| collateral.amount_purchased).sum::<Uint128>();

            //Calculate the looped exposure the position currently has
            let current_multiplier = decimal_division(
                Decimal::from_ratio(total_bought_from_loops + target_position.collateral_amount, Uint128::one()),
                Decimal::from_ratio(target_position.collateral_amount, Uint128::one())
            )?;

            //////If this is the final loop, save the sum of purchases and the average purchase price/////
            //This is the last loop if the multiplier is within 3% of the intended
            if current_multiplier >= decimal_multiplication(loop_prop.intended_multiplier, Decimal::percent(97))? {

                //Get the average purchase price
                let average_purchase_price = match decimal_division(
                    target_position_ux_boosts.collateral_bought_from_loops.iter().map(|collateral| collateral.post_purchase_price).sum::<Decimal>(),
                     Decimal::from_ratio(target_position_ux_boosts.collateral_bought_from_loops.len() as u128, Uint128::one())
                ){
                    Ok(price) => price,
                    Err(_) => {
                        return Err(StdError::generic_err("Failed to get average purchase price"))
                    }
                };

                //Set the list of collateral bought from loops to the average & the total bought
                target_position_ux_boosts.collateral_bought_from_loops = vec![PurchaseData {
                    amount_purchased: total_bought_from_loops,
                    post_purchase_price: average_purchase_price,
                }];
                

            }
            //Save user position
            POSITION_UX_BOOSTS.save(deps.storage, (deps.api.addr_validate(&loop_prop.position_owner)?, loop_prop.collateral_denom.clone()), &target_position_ux_boosts)?;

            //Create response
            let response = Response::new()
                .add_messages(msgs)
                .add_attribute("action", "loop_position")
                .add_attribute("position_owner", loop_prop.position_owner)
                .add_attribute("collateral_bought_in_loops", format!("{:?}", target_position_ux_boosts.collateral_bought_from_loops ));

            Ok(response)

        },

        Err(err) => {
            //Its reply on success only
            Ok(Response::new().add_attribute("error", err))
        }
    }
}

