use core::panic;
use std::str::FromStr;

use cosmwasm_std::{Storage, Api, QuerierWrapper, Env, MessageInfo, Uint128, Response, Decimal, CosmosMsg, attr, SubMsg, Addr, StdResult, StdError, to_binary, WasmMsg, QueryRequest, WasmQuery, BankMsg, Coin};

use membrane::helpers::{router_native_to_native, pool_query_and_exit, query_stability_pool_fee, asset_to_coin, validate_position_owner};
use membrane::math::{decimal_multiplication, decimal_division, decimal_subtraction, Uint256};
use membrane::cdp::{Config, ExecuteMsg, CallbackMsg};
use membrane::oracle::PriceResponse;
use membrane::osmosis_proxy::QueryMsg as OsmoQueryMsg;
use membrane::stability_pool::{LiquidatibleResponse as SP_LiquidatibleResponse, ExecuteMsg as SP_ExecuteMsg, QueryMsg as SP_QueryMsg};
use membrane::liq_queue::{ExecuteMsg as LQ_ExecuteMsg, QueryMsg as LQ_QueryMsg, LiquidatibleResponse as LQ_LiquidatibleResponse};
use membrane::staking::ExecuteMsg as StakingExecuteMsg;
use membrane::types::{Basket, Position, AssetInfo, UserInfo, Asset, cAsset, PoolStateResponse, AssetPool};

use crate::error::ContractError; 
use crate::rates::accrue;
use crate::positions::{BAD_DEBT_REPLY_ID, ROUTER_REPLY_ID};
use crate::query::{insolvency_check, get_avg_LTV, get_cAsset_ratios};
use crate::state::{CONFIG, BASKET, LIQUIDATION, LiquidationPropagation, get_target_position, update_position, update_position_claims, ROUTER_REPAY_MSG};


//Liquidation reply ids
pub const LIQ_QUEUE_REPLY_ID: u64 = 1u64;
pub const STABILITY_POOL_REPLY_ID: u64 = 2u64;
pub const USER_SP_REPAY_REPLY_ID: u64 = 6u64;

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
    let config: Config = CONFIG.load(storage)?;

    let mut basket: Basket = BASKET.load(storage)?;
    let valid_position_owner =
        validate_position_owner(api, info.clone(), Some(position_owner.clone()))?;

    let (_i, mut target_position) = get_target_position(
        storage,
        valid_position_owner.clone(),
        position_id,
    )?;
    
    //Accrue interest
    accrue(
        storage,
        querier,
        env.clone(),
        &mut target_position,
        &mut basket,
        position_owner.clone(),
        false
    )?;
    
    //Save updated repayment price and basket debt
    BASKET.save(storage, &basket)?;

    //Save updated position to lock-in credit_amount and last_accrued time
    update_position(storage, valid_position_owner.clone(), target_position.clone())?;

    //Check position health compared to max_LTV
    let (insolvent, current_LTV, _available_fee) = insolvency_check(
        storage,
        env.clone(),
        querier,
        target_position.clone().collateral_assets,
        Decimal::from_ratio(target_position.clone().credit_amount, Uint128::new(1u128)),
        basket.credit_price,
        false,
        config.clone(),
    )?;
    let insolvent = true;
    let current_LTV = Decimal::percent(90);
    
    if !insolvent {
        return Err(ContractError::PositionSolvent {});
    }

    //Send liquidation amounts and info to the modules
    //Calculate how much needs to be liquidated (down to max_borrow_LTV):
    let (avg_borrow_LTV, avg_max_LTV, total_value, cAsset_prices_res) = get_avg_LTV(
        storage,
        env.clone(),
        querier,
        config.clone(),
        target_position.clone().collateral_assets,
        false,
        true,
    )?;
    //Convert from Response to price (Decimal)
    let cAsset_prices = cAsset_prices_res.clone().into_iter().map(|price| price.price).collect::<Vec<Decimal>>();

    //Get repay value and repay_amount
    let (repay_value, mut credit_repay_amount) = get_repay_quantities(
        config.clone(),
        basket.clone(),
        target_position.clone(),
        current_LTV,
        avg_borrow_LTV,
        total_value,
    )?;

    // Don't send any funds here, only send UserInfo and repayment amounts.
    // We want to act on the reply status but since SubMsg state won't revert if we catch the error,
    // assets we send prematurely won't come back.

    let res = Response::new();
    let mut submessages = vec![];
    let mut caller_fee_messages: Vec<CosmosMsg> = vec![];
    let mut lp_withdraw_messages: Vec<CosmosMsg> = vec![];

    //cAsset_ratios including LP shares
    let (cAsset_ratios, _) = get_cAsset_ratios(
        storage,
        env.clone(),
        querier,
        target_position.clone().collateral_assets,
        config.clone(),
    )?;
    //Set collateral_assets
    let mut collateral_assets = target_position.clone().collateral_assets;

    //Dynamic fee that goes to the caller (info.sender): current_LTV - max_LTV
    let caller_fee = decimal_subtraction(current_LTV, avg_max_LTV)?;

    //Get amount of repayment user can repay from the Stability Pool
    let user_repay_amount = get_user_repay_amount(querier, config.clone(), basket.clone(), position_id, position_owner.clone(), &mut credit_repay_amount, &mut submessages)?;
    
    //Track total leftover repayment after the liq_queue
    let mut leftover_repayment: Decimal = credit_repay_amount;

    //Track repay_amount_per_asset
    let mut per_asset_repayment: Vec<Decimal> = vec![];

    let mut leftover_position_value = total_value;

    //Calculate caller & protocol fees 
    //and amount to send to the Liquidation Queue.
    let (protocol_fee_msg, leftover_repayment) = per_asset_fulfillments(
        storage, 
        querier, 
        env.clone(), 
        config.clone(), 
        basket.clone(), 
        position_id, 
        valid_position_owner.clone(), 
        info.sender.to_string(),
        caller_fee,
        &mut collateral_assets, 
        credit_repay_amount.to_uint_floor(), 
        &mut leftover_position_value, 
        leftover_repayment.to_uint_floor(), 
        repay_value, 
        cAsset_ratios, 
        cAsset_prices_res, 
        &mut submessages, 
        &mut caller_fee_messages, 
        &mut per_asset_repayment
    )?;
    
    //Build SubMsgs to send to the Stability Pool & Sell Wall
    //This will only run if config.stability_pool.is_some()
    let ( leftover_repayment, lp_withdraw_msgs, sell_wall_messages ) = build_sp_sw_submsgs(
        storage, 
        querier,
        api, 
        env.clone(), 
        config, 
        basket.clone(), 
        position_id, 
        valid_position_owner.clone(), 
        collateral_assets.clone(), 
        leftover_repayment, 
        credit_repay_amount, 
        leftover_position_value, 
        &mut submessages, 
        per_asset_repayment, 
        user_repay_amount,
    )?;
    
    //Extend LP withdraw messages
    lp_withdraw_messages.extend(lp_withdraw_msgs);


    //Create the Bad debt callback message to be added as the last SubMsg
    let msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_binary(&ExecuteMsg::Callback(CallbackMsg::BadDebtCheck {
            position_id,
            position_owner: valid_position_owner.clone(),
        }))?,
        funds: vec![],
    });
    //The logic for this will be handled in the callback
    //Replying on Error is just so an Auction error doesn't cancel transaction
    //Don't care about the success case so didnt reply_always
    let call_back = SubMsg::reply_on_error(msg, BAD_DEBT_REPLY_ID);
    
    let mut liquidation_propagation: Option<String> = None;
    if let Ok(repay) = LIQUIDATION.load(storage) { liquidation_propagation = Some(format!("{:?}", repay)) }

    Ok(res
        // .add_messages(lp_withdraw_messages)
        .add_submessages(sell_wall_messages)
        .add_messages(caller_fee_messages)
        .add_message(protocol_fee_msg)
        .add_submessages(submessages)
        .add_submessage(call_back)
        .add_attributes(vec![
            attr("method", "liquidate"),
            attr(
                "propagation_info",
                format!("{:?}", liquidation_propagation.unwrap_or_else(|| String::from("None"))),
            ),
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
    let loan_value = decimal_multiplication(
        basket.credit_price,
        Decimal::from_ratio(target_position.credit_amount, Uint128::new(1u128)),
    )?;

    //repay value = the % of the loan insolvent. Insolvent is anything between current and max borrow LTV.
    //IE, repay what to get the position down to borrow LTV
    //If the position LTV is above 100%, repay using all the collateral 
    let mut repay_value = if current_LTV >= Decimal::one() {
        total_value
    } else {
        decimal_multiplication( decimal_division( decimal_subtraction(current_LTV, borrow_LTV)?, current_LTV)?, loan_value)?
    };

    //Assert repay_value is above the minimum, if not repay at least the minimum
    //Repay the full loan if the resulting leftover credit amount is less than the minimum.
    let decimal_debt_minimum = Decimal::from_ratio(config.debt_minimum, Uint128::new(1u128));
    if repay_value < decimal_debt_minimum {
        //If setting the repay value to the minimum leaves at least the minimum in the position...
        //..then partially liquidate
        if loan_value - decimal_debt_minimum >= decimal_debt_minimum {
            repay_value = decimal_debt_minimum;
        } else {
            //Else liquidate it all
            repay_value = loan_value;
        }
    }

    let credit_repay_amount = match decimal_division(repay_value, basket.credit_price)?{
        //Repay amount has to be above 0, or there is nothing to liquidate and there was a mistake prior
        x if x <= Decimal::new(Uint128::zero()) => return Err(ContractError::PositionSolvent {}),
        //No need to repay more than the debt
        x if x > Decimal::from_ratio(
            target_position.credit_amount,
            Uint128::new(1u128),
        ) =>
        {
            return Err(ContractError::FaultyCalc { msg: "Repay amount is greater than total debt".to_string() })
        }
        x => x,
    };

    Ok((repay_value, credit_repay_amount))
}

/// Calculate amount of debt the User can repay from the Stability Pool
fn get_user_repay_amount(
    querier: QuerierWrapper,
    config: Config,
    basket: Basket,
    position_id: Uint128,
    position_owner: String,
    credit_repay_amount: &mut Decimal,
    submessages: &mut Vec<SubMsg>,
) -> StdResult<Decimal>{

    let mut user_repay_amount = Decimal::zero();
    //Let the user repay their position if they are in the SP
    if config.stability_pool.is_some() {
        //Query Stability Pool to see if the user has funds
        let user_deposits = querier
            .query::<AssetPool>(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: config.clone().stability_pool.unwrap().to_string(),
                msg: to_binary(&SP_QueryMsg::AssetPool { 
                    user: Some(position_owner.clone()),
                    deposit_limit: None, 
                    start_after: None,
                })?,
            }))?
            .deposits;

        let total_user_deposit: Decimal = user_deposits
            .iter()
            .map(|user_deposit| user_deposit.amount)
            .collect::<Vec<Decimal>>()
            .into_iter()
            .sum();
            
        //If the user has funds, tell the SP to repay and subtract from credit_repay_amount
        if !total_user_deposit.is_zero() {
            //Set Repayment amount to what needs to get liquidated or total_deposits
            user_repay_amount = {
                //Repay the full debt
                if total_user_deposit > *credit_repay_amount {
                    *credit_repay_amount
                } else {
                    total_user_deposit
                }
            };

            //Add Repay SubMsg
            let repay_msg = SP_ExecuteMsg::Repay {
                user_info: UserInfo {
                    position_id,
                    position_owner,
                },
                repayment: Asset {
                    amount: user_repay_amount * Uint128::new(1u128),
                    info: basket.credit_asset.info,
                },
            };

            let msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.stability_pool.unwrap().to_string(),
                msg: to_binary(&repay_msg)?,
                funds: vec![],
            });

            //Convert to submsg
            let sub_msg: SubMsg = SubMsg::reply_on_error(msg, USER_SP_REPAY_REPLY_ID);
            submessages.push(sub_msg);

            //Subtract Repay amount from credit_repay_amount for the liquidation
            *credit_repay_amount = decimal_subtraction(*credit_repay_amount, user_repay_amount)?;
        }
    }

    Ok( user_repay_amount )
}

/// Calculate & send fees.
/// Send liquidatible amount to Liquidation Queue.
fn per_asset_fulfillments(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: Config,
    basket: Basket,
    position_id: Uint128,
    valid_position_owner: Addr,
    fee_recipient: String,
    caller_fee: Decimal,
    collateral_assets: &mut Vec<cAsset>,
    credit_repay_amount: Uint128,
    leftover_position_value: &mut Decimal,
    mut leftover_repayment: Uint128,
    repay_value: Decimal,
    cAsset_ratios: Vec<Decimal>,
    cAsset_prices: Vec<PriceResponse>,
    submessages: &mut Vec<SubMsg>,
    caller_fee_messages: &mut Vec<CosmosMsg>,
    per_asset_repayment: &mut Vec<Decimal>,
) -> StdResult<(CosmosMsg, Decimal)>{
    
    let mut caller_coins: Vec<Coin> = vec![];
    let mut protocol_coins: Vec<Coin> = vec![];

    for (num, cAsset) in collateral_assets.clone().iter().enumerate() {

        let repay_amount_per_asset = credit_repay_amount * cAsset_ratios[num];


        let collateral_price = cAsset_prices[num].price;
        let collateral_repay_value_for_fees = decimal_multiplication(repay_value, cAsset_ratios[num])?;
        let mut collateral_repay_amount_for_fees: Uint128 = decimal_division(collateral_repay_value_for_fees, collateral_price)?.to_uint_floor();
        //ReAdd decimals to collateral_repay_amount if it was removed in valuation to normalize to 6 decimals
        match cAsset_prices[num].decimals.checked_sub(6u64) {
            Some(decimals) => {
                collateral_repay_amount_for_fees = collateral_repay_amount_for_fees * Uint128::from(10u128.pow(decimals as u32));
            },
            None => {
                return Err(StdError::GenericErr { msg: String::from("Decimals cannot be less than 6") });
            }
        }

        //Subtract Caller fee from Position's claims
        let caller_fee_in_collateral_amount = collateral_repay_amount_for_fees * caller_fee;
            
        update_position_claims(
            storage,
            querier,
            env.clone(),
            position_id,
            valid_position_owner.clone(),
            cAsset.clone().asset.info,
            caller_fee_in_collateral_amount,
        )?;

        //Update collateral_assets to reflect the fee
        collateral_assets[num].asset.amount -= caller_fee_in_collateral_amount;
        
        //Subtract Protocol fee from Position's claims
        let protocol_fee_in_collateral_amount = collateral_repay_amount_for_fees * config.clone().liq_fee;
        update_position_claims(
            storage,
            querier,
            env.clone(),
            position_id,
            valid_position_owner.clone(),
            cAsset.clone().asset.info,
            protocol_fee_in_collateral_amount,
        )?;
        
        //Update collateral_assets to reflect the fee
        collateral_assets[num].asset.amount -= protocol_fee_in_collateral_amount;
        ///These updates are necessary bc the fees are always taken out so if the liquidation is SW only, it'll try to sell more than the position owns.

        //After fees are calculated, set collateral_repay_amount to the amount minus anything the user paid from the SP
        //Has to be after or user_repayment would disincentivize liquidations which would force a non-trivial debt minimum
        let collateral_repay_value = repay_amount_per_asset * basket.clone().credit_price;
        let mut collateral_repay_amount: Uint128 = decimal_division(
            Decimal::from_ratio(collateral_repay_value,Uint128::one()),
            collateral_price
        )?.to_uint_floor();
        //ReAdd decimals to collateral_repay_amount if it was removed in valuation to normalize to 6 decimals
        match cAsset_prices[num].decimals.checked_sub(6u64) {
            Some(decimals) => {
                collateral_repay_amount = collateral_repay_amount * Uint128::from(10u128.pow(decimals as u32));
            },
            None => {
                return Err(StdError::GenericErr { msg: String::from("Decimals cannot be less than 6") });
            }
        }

        //Subtract fees from leftover_position value
        //fee_value = total_fee_collateral_amount * collateral_price
        let mut fee_value = 
                (caller_fee_in_collateral_amount + protocol_fee_in_collateral_amount) * collateral_price;

        //Remove decimals from fee value that were added when correcting for asset decimals
        match cAsset_prices[num].decimals.checked_sub(6u64) {
            Some(decimals) => {
                fee_value = fee_value / Uint128::from(10u128.pow(decimals as u32));
            },
            None => {
                return Err(StdError::GenericErr { msg: String::from("Decimals cannot be less than 6") });
            }
        }
        //Remove fee_value from leftover_position_value
        *leftover_position_value = decimal_subtraction(*leftover_position_value, Decimal::from_ratio(fee_value, Uint128::one()))?;
        
        //Create msgs to caller as well as to liq_queue if.is_some()
        match cAsset.clone().asset.info {
            AssetInfo::Token { address: _ } => { return Err(StdError::GenericErr { msg: "Cw20 assets aren't allowed".to_string() }) },
            AssetInfo::NativeToken { denom: _ } => {
                let asset = Asset {
                    amount: caller_fee_in_collateral_amount,
                    ..cAsset.clone().asset
                };

                caller_coins.push(asset_to_coin(asset)?);

                let asset = Asset {
                    amount: protocol_fee_in_collateral_amount,
                    ..cAsset.clone().asset
                };
                protocol_coins.push(asset_to_coin(asset)?);
            }
        } 

        /////////////LiqQueue calls//////
        if basket.clone().liq_queue.is_some() {
            //if collateral repay amount is more than the Position has in assets, 
            //Set collateral_repay_amount to the amount the Position has in assets
            if collateral_repay_amount > collateral_assets[num].asset.amount {
                collateral_repay_amount = collateral_assets[num].asset.amount;
            }

            //Store repay amount
            per_asset_repayment.push(Decimal::from_ratio(repay_amount_per_asset, Uint128::one()));

            let res: LQ_LiquidatibleResponse =
                querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
                    contract_addr: basket.clone().liq_queue.unwrap().to_string(),
                    msg: to_binary(&LQ_QueryMsg::CheckLiquidatible {
                        bid_for: cAsset.clone().asset.info,
                        collateral_price,
                        collateral_amount: Uint256::from(
                            (collateral_repay_amount).u128(),
                        ),
                        credit_info: basket.clone().credit_asset.info,
                        credit_price: basket.clone().credit_price,
                    })?,
                }))?;

            //Calculate how much collateral we are sending to the liq_queue to liquidate
            let leftover: Uint128 = Uint128::from_str(&res.leftover_collateral)?;
            let mut queue_asset_amount_paid: Uint128 =
                collateral_repay_amount  - leftover;
                
            //Call Liq Queue::Liquidate for the asset
            let liq_msg = LQ_ExecuteMsg::Liquidate {
                credit_price: basket.credit_price,
                collateral_price,
                collateral_amount: Uint256::from(queue_asset_amount_paid.u128()),
                bid_for: cAsset.clone().asset.info,
                position_id,
                position_owner: valid_position_owner.clone().to_string(),
            };

            //Renormalize decimals before we use the amount to compare valuations
            match cAsset_prices[num].decimals.checked_sub(6u64) {
                Some(decimals) => {
                    queue_asset_amount_paid = queue_asset_amount_paid / Uint128::from(10u128.pow(decimals as u32));
                },
                None => {
                    return Err(StdError::GenericErr { msg: String::from("Decimals cannot be less than 6") });
                }
            }

            //Keep track of remaining position value
            //value_paid_to_queue = queue_asset_amount_paid * collateral_price
            let value_paid_to_queue: Uint128 = queue_asset_amount_paid * collateral_price;

            *leftover_position_value = decimal_subtraction(*leftover_position_value, Decimal::from_ratio(value_paid_to_queue, Uint128::one()))?;
            
            //Calculate how much the queue repaid in credit
            let queue_credit_repaid = Uint128::from_str(&res.total_debt_repaid)?;
            
            //Subtract that from the running total for potential leftovers
            //i.e. after this function is over, this value will be the amount of credit that was not repaid
            leftover_repayment = leftover_repayment - queue_credit_repaid;
            
            //Create CosmosMsg
            let msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: basket.clone().liq_queue.unwrap().to_string(),
                msg: to_binary(&liq_msg)?,
                funds: vec![],
            });

            //Convert to submsg
            let sub_msg: SubMsg = SubMsg::reply_always(msg, LIQ_QUEUE_REPLY_ID);
            submessages.push(sub_msg);
        }
    }
    
    //Create Msg to send all native token liq fees for fn caller
    let msg = CosmosMsg::Bank(BankMsg::Send {
        to_address: fee_recipient.clone(),
        amount: caller_coins,
    });
    caller_fee_messages.push(msg);
    
    //Create Msg to send all native token liq fees for MBRN to the staking contract
    let protocol_fee_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.clone().staking_contract.unwrap().to_string(),
        msg: to_binary(&StakingExecuteMsg::DepositFee {})?,
        funds: protocol_coins,
    }); 

    Ok((protocol_fee_msg, Decimal::from_ratio(leftover_repayment, Uint128::one())))
}

/// This fucntion is used to build (sub)messages for the Stability Pool and sell wall.
/// Also returns leftover debt repayment amount.
fn build_sp_sw_submsgs(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    api: &dyn Api,
    env: Env,
    config: Config,
    basket: Basket,
    position_id: Uint128,
    valid_position_owner: Addr,
    collateral_assets: Vec<cAsset>,
    mut leftover_repayment: Decimal,
    credit_repay_amount: Decimal,
    mut leftover_position_value: Decimal,
    submessages: &mut Vec<SubMsg>,
    per_asset_repayment: Vec<Decimal>,
    user_repay_amount: Decimal,
) -> Result<(Decimal, Vec<CosmosMsg>,  Vec<SubMsg>), ContractError>{
    
    let sell_wall_repayment_amount: Decimal;
    let mut lp_withdraw_messages = vec![];
    let mut sell_wall_messages = vec![];
    let mut skip_sp = false;
    
    if config.stability_pool.is_some() && !leftover_repayment.is_zero() {
        let sp_liq_fee = query_stability_pool_fee(querier, config.clone().stability_pool.unwrap().to_string())?;

        //If LTV is 90% and the fees are 10%, the position would pay everything to pay the liquidators.
        //So above that, the liquidators are losing the premium guarantee.
        // !( leftover_position_value >= leftover_repay_value * sp_fee)

        //Working on the LQ's leftovers
        let leftover_repayment_value = decimal_multiplication(
            leftover_repayment,
            basket.credit_price,
        )?;

        //SP liq_fee Guarantee check
        //if leftover_position_value is less than leftover_repay value + the SP fee, we liquidate what we can and send the rest to the sell wall
        if leftover_position_value < decimal_multiplication(leftover_repayment_value, (Decimal::one() + sp_liq_fee))?{
            //if liq_Fee is 100%+, skip fee discount and just use Sell Wall
            if sp_liq_fee >= Decimal::one() {
                skip_sp = true;
                if leftover_position_value < leftover_repayment_value {
                    //Set leftover_repayment to the amount of credit the Position value can pay
                    leftover_repayment = decimal_division(leftover_position_value, basket.credit_price)?;
                }
            } else {
                //Set Position value to the discounted value the SP will be distributed
                leftover_position_value = decimal_multiplication(leftover_position_value, (Decimal::one() - sp_liq_fee))?;
                //Set leftover_repayment to the amount of credit the Position value can pay
                leftover_repayment = decimal_division(leftover_position_value, basket.credit_price)?;       
            }     
        }
        
        if !skip_sp {
            //Check for stability pool funds before any liquidation attempts
            //If no funds, go directly to the sell wall
            let sp_leftover_repayment = query_stability_pool_liquidatible(
                querier,
                config.clone(),
                leftover_repayment,
            )?;
            
            if sp_leftover_repayment > Decimal::zero() {
                sell_wall_repayment_amount = sp_leftover_repayment;

                //Sell wall remaining
                let (sell_wall_msgs, lp_withdraw_msgs) = sell_wall(
                    storage,
                    querier,
                    api,
                    env.clone(),
                    collateral_assets,
                    sell_wall_repayment_amount,
                    basket.clone(),
                    position_id,
                    valid_position_owner.to_string(),
                )?;
                lp_withdraw_messages = lp_withdraw_msgs;
                sell_wall_messages = sell_wall_msgs;
                
            }

            //Set Stability Pool repay_amount
            let sp_repay_amount = decimal_subtraction(leftover_repayment, sp_leftover_repayment)?;

            //Leftover's starts as the total LQ is supposed to pay, and is subtracted by every successful LQ reply
            let liq_queue_leftovers =
                decimal_subtraction(credit_repay_amount, leftover_repayment)?;
            
            // Set repay values for reply msg
            let liquidation_propagation = LiquidationPropagation {
                per_asset_repayment,
                liq_queue_leftovers,
                stability_pool: sp_repay_amount,
                user_repay_amount,
                position_info: UserInfo {
                    position_id,
                    position_owner: valid_position_owner.to_string()
                },
                positions_contract: env.contract.address,
            };

            LIQUIDATION.save(storage, &liquidation_propagation)?;

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
        } else {
            if leftover_repayment > Decimal::zero() {
                sell_wall_repayment_amount = leftover_repayment;

                //Sell wall remaining
                let (sell_wall_msgs, lp_withdraw_msgs) = sell_wall(
                    storage,
                    querier,
                    api,
                    env.clone(),
                    collateral_assets,
                    sell_wall_repayment_amount,
                    basket.clone(),
                    position_id,
                    valid_position_owner.to_string(),
                )?;
                lp_withdraw_messages = lp_withdraw_msgs;
                sell_wall_messages = sell_wall_msgs;
                
            }

            //Leftover's starts as the total LQ is supposed to pay, and is subtracted by every successful LQ reply
            let liq_queue_leftovers =
                decimal_subtraction(credit_repay_amount, leftover_repayment)?;

            // Set repay values for reply msg
            let liquidation_propagation = LiquidationPropagation {
                per_asset_repayment,
                liq_queue_leftovers,
                stability_pool: Decimal::zero(),
                user_repay_amount,
                position_info: UserInfo {
                    position_id,
                    position_owner: valid_position_owner.to_string()
                },
                positions_contract: env.contract.address,
            };

            LIQUIDATION.save(storage, &liquidation_propagation)?;
        }

        //Because these are reply always, we can NOT make state changes that we wouldn't allow no matter the tx result, as our altereed state will NOT revert.
        //Errors also won't revert the whole transaction
        //( https://github.com/CosmWasm/cosmwasm/blob/main/SEMANTICS.md#submessages )

        //Collateral distributions get handled in the reply        
    } else {
        //In case SP isn't used, we need to set LiquidationPropagation
        // Set repay values for reply msg
        let liquidation_propagation = LiquidationPropagation {
            per_asset_repayment,
            liq_queue_leftovers: Decimal::zero(),
            stability_pool: Decimal::zero(),
            user_repay_amount,
            position_info: UserInfo {
                position_id,
                position_owner: valid_position_owner.to_string()
            },
            positions_contract: env.contract.address,
        };

        LIQUIDATION.save(storage, &liquidation_propagation)?;
    }

    Ok((leftover_repayment, lp_withdraw_messages, sell_wall_messages))
}

/// Returns LP withdrawal message use in liquidations
fn get_lp_liq_withdraw_msg(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: Config,
    position_id: Uint128,
    position_owner: Addr,
    cAsset_ratios: Vec<Decimal>,
    cAsset_prices: Vec<PriceResponse>,
    repay_value: Decimal,
    cAsset: cAsset,
    i: usize,
) -> StdResult<CosmosMsg>{    
    let pool_info = cAsset.clone().pool_info.unwrap();

    ////Calculate amount of asset to liquidate
    // Amount to liquidate = cAsset_ratio * % of position insolvent * cAsset amount
    let mut lp_liquidate_amount = decimal_division( 
        decimal_multiplication(
            cAsset_ratios[i],
            repay_value)?, 
            cAsset_prices[i].price
        )?.to_uint_floor();
    
    //ReAdd decimals if it was removed in valuation when normalizing to 6 decimals
    match cAsset_prices[i].decimals.checked_sub(6u64) {
        Some(decimals) => {
            lp_liquidate_amount = lp_liquidate_amount * Uint128::from(10u128.pow(decimals as u32));
        },
        None => {
            return Err(StdError::GenericErr { msg: String::from("Decimals cannot be less than 6") });
        }
    }

    //Remove asset from Position claims
    update_position_claims(
        storage,
        querier,
        env.clone(),
        position_id,
        position_owner,
        cAsset.asset.info,
        lp_liquidate_amount,
    )?;   

    Ok( pool_query_and_exit(
        querier, 
        env, 
        config.osmosis_proxy.unwrap().to_string(), 
        pool_info.pool_id, 
        lp_liquidate_amount
    )?.0 )
}

/// Uses Position info to create sell wall msgs
pub fn sell_wall_using_ids(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    api: &dyn Api,
    env: Env,
    position_id: Uint128,
    position_owner: Addr,
    repay_amount: Decimal,
) -> StdResult<(Vec<SubMsg>, Vec<CosmosMsg>)> {
    let basket: Basket = BASKET.load(storage)?;

    let (_i, target_position) = match get_target_position(storage, position_owner.clone(), position_id){
        Ok(position) => position,
        Err(_err) => return Err(StdError::GenericErr { msg: String::from("Non_existent position") })
    };    
    let collateral_assets = target_position.collateral_assets;

    match sell_wall(
        storage,
        querier,
        api,
        env,
        collateral_assets,
        repay_amount,
        basket,
        position_id,
        position_owner.to_string(),
    ) {
        Ok(res) => Ok(res),
        Err(err) => {
            Err(StdError::GenericErr {
                msg: err.to_string(),
            })
        }
    }
}

/// Returns router & lp withdraw messages for use in liquidations
pub fn sell_wall(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    api: &dyn Api,
    env: Env,
    collateral_assets: Vec<cAsset>,
    repay_amount: Decimal,
    basket: Basket,
    //For Repay msg
    position_id: Uint128,
    position_owner: String,
) -> Result<(Vec<SubMsg>, Vec<CosmosMsg>), ContractError> {
    //Load Config
    let config: Config = CONFIG.load(storage)?;   

    let mut router_messages = vec![];
    let mut lp_withdraw_messages = vec![];
    let position_owner_addr = api.addr_validate(&position_owner)?;

    let repay_value = decimal_multiplication(repay_amount, basket.credit_price)?;
    
    //Get Pre-Split cAsset_ratios & prices
    let (cAsset_ratios, cAsset_prices) = get_cAsset_ratios(storage, env.clone(), querier, collateral_assets.clone(), config.clone())?;   

    for (i, cAsset) in collateral_assets
        .clone()    
        .into_iter()
        .enumerate()
    {
        //Withdraw the necessary amount of LP shares
        //Ensures liquidations are on the pooled assets and not the LP share itself
        if cAsset.clone().pool_info.is_some() {

            let msg = get_lp_liq_withdraw_msg( 
                storage,
                querier, 
                env.clone(), 
                config.clone(), 
                position_id,
                position_owner_addr.clone(),
                cAsset_ratios.clone(), 
                cAsset_prices.clone(), 
                repay_value,
                cAsset.clone(), 
                i, 
            )?;

            lp_withdraw_messages.push(msg);            
        } else {
            
            //Calc collateral_repay_amount        
            let collateral_price = cAsset_prices[i].price;
            let collateral_repay_value = decimal_multiplication(repay_value, cAsset_ratios[i])?;
            let mut collateral_repay_amount = decimal_division(collateral_repay_value, collateral_price)?.to_uint_floor();
            //The repay_amount per asset may be greater after LP splits so the amount used to update claims isn't necessary the total amount that'll get sold
            //ReAdd decimals to collateral_repay_amount if it was removed in valuation to normalize to 6 decimals
            match cAsset_prices[i].decimals.checked_sub(6u64) {
                Some(decimals) => {
                    collateral_repay_amount = collateral_repay_amount * Uint128::from(10u128.pow(decimals as u32));
                },
                None => {
                    return Err(ContractError::Std(StdError::GenericErr { msg: String::from("Decimals cannot be less than 6") }));
                }
            }
            
            //Remove assets from Position claims before spliting the LP cAsset to ensure excess claims aren't removed
            //Avoid a situation where the user's LP token claims are reduced && it's pool asset claims are reduced, doubling the "loss" of funds due to state mismanagement
            update_position_claims(
                storage,
                querier,
                env.clone(),
                position_id,
                position_owner_addr.clone(),
                cAsset.clone().asset.info,
                collateral_repay_amount,
            )?;    
        }
    }    

    //Split LP into assets
    let collateral_assets = get_LP_pool_cAssets(
        querier,
        config.clone(),
        basket.clone(),
        collateral_assets,
    )?;

    //Post-LP Split ratios
    let (cAsset_ratios, cAsset_prices) = get_cAsset_ratios(
        storage,
        env,
        querier,
        collateral_assets.clone(),
        config.clone(),
    )?;

    //Create Router Msgs for each asset
    //The LP will be sold as pool assets so individual ratios may increase
    for (index, ratio) in cAsset_ratios.into_iter().enumerate() {

        //Calc collateral_repay_amount        
        let collateral_price = cAsset_prices[index].price;
        let collateral_repay_value = decimal_multiplication(repay_value, ratio)?;
        let mut collateral_repay_amount = decimal_division(collateral_repay_value, collateral_price)?.to_uint_floor(); 
        //ReAdd decimals to collateral_repay_amount if it was removed in valuation to normalize to 6 decimals
        match cAsset_prices[index].decimals.checked_sub(6u64) {
            Some(decimals) => {
                collateral_repay_amount = collateral_repay_amount * Uint128::from(10u128.pow(decimals as u32));
            },
            None => {
                return Err(ContractError::Std(StdError::GenericErr { msg: String::from("Decimals cannot be less than 6") }));
            }
        }              

        let hook_msg = to_binary(&ExecuteMsg::Repay {
            position_id,
            position_owner: Some(position_owner.clone()),
            send_excess_to: Some(position_owner.clone()),
        })?;

        //Save Repay msg to be executed in the reply
        ROUTER_REPAY_MSG.save(storage, &hook_msg)?;

        //Create router reply msg to repay debt after sales
        let router_msg = router_native_to_native(
            config.clone().dex_router.unwrap().into(), 
            collateral_assets[index].clone().asset.info, 
            basket.clone().credit_asset.info, 
            None, 
            collateral_repay_amount.into()
        )?;
        let router_msg = SubMsg::new(router_msg);
        router_messages.push(router_msg);        
    }

    //The last router message is updated to a ROUTER_REPLY_ID to repay the position after all sales are done.
    let index = router_messages.clone().len()-1;
    router_messages[index].id = ROUTER_REPLY_ID;
    
    Ok((router_messages, lp_withdraw_messages))
}

/// Returns leftover liquidatible amount from the stability pool
pub fn query_stability_pool_liquidatible(
    querier: QuerierWrapper,
    config: Config,
    amount: Decimal,
) -> StdResult<Decimal> {
    let query_res: SP_LiquidatibleResponse =
        querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: config.stability_pool.unwrap().to_string(),
            msg: to_binary(&SP_QueryMsg::CheckLiquidatible {
                amount
            })?,
        }))?;

    Ok(query_res.leftover)
}

/// If cAssets include an LP, remove the LP share denom and add its paired assets
pub fn get_LP_pool_cAssets(
    querier: QuerierWrapper,
    config: Config,
    basket: Basket,
    position_assets: Vec<cAsset>,
) -> StdResult<Vec<cAsset>> {
    let mut new_assets = position_assets
        .clone()
        .into_iter()
        .filter(|asset| asset.pool_info.is_none())
        .collect::<Vec<cAsset>>();

    //Add LP's Assets as cAssets
    //Remove LP share token
    for cAsset in position_assets {
        if let Some(pool_info) = cAsset.pool_info {

            //Query share asset amount
            let share_asset_amounts = querier
                .query::<PoolStateResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                    contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
                    msg: to_binary(&OsmoQueryMsg::PoolState {
                        id: pool_info.pool_id,
                    })?,
                }))?
                .shares_value(cAsset.asset.amount);

            for pool_coin in share_asset_amounts {
                let info = AssetInfo::NativeToken {
                    denom: pool_coin.denom,
                };
                //Find the coin in the basket
                if let Some(basket_cAsset) = basket
                    .clone()
                    .collateral_types
                    .into_iter()
                    .find(|cAsset| cAsset.asset.info.equal(&info))
                {
                    //Check if its already in the position asset list
                    if let Some((i, _cAsset)) =
                        new_assets
                            .clone()
                            .into_iter()
                            .enumerate()
                            .find(|(_index, cAsset)| {
                                cAsset.asset.info.equal(&basket_cAsset.clone().asset.info)
                            })
                    {
                        //Add to assets
                        new_assets[i].asset.amount += Uint128::from_str(&pool_coin.amount).unwrap();
                    } else {
                        //Push to list
                        new_assets.push(cAsset {
                            asset: Asset {
                                amount: Uint128::from_str(&pool_coin.amount).unwrap(),
                                info,
                            },
                            ..basket_cAsset
                        })
                    }
                }
                //No reason to error bc LPs can't be added if their assets aren't added first
            }
        }
    }

    Ok(new_assets)
}
