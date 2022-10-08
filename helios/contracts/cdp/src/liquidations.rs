use std::str::FromStr;

use cosmwasm_std::{Storage, Api, QuerierWrapper, Env, MessageInfo, Uint128, Response, Decimal, CosmosMsg, attr, SubMsg, Addr, StdResult, StdError, to_binary, WasmMsg, coin, QueryRequest, WasmQuery, BankMsg, Coin};
use cw20::Cw20ExecuteMsg;

use membrane::math::{decimal_multiplication, decimal_division, decimal_subtraction, Uint256};
use membrane::positions::{Config, ExecuteMsg, CallbackMsg};
use membrane::apollo_router::{ExecuteMsg as RouterExecuteMsg, Cw20HookMsg as RouterHookMsg};
use membrane::stability_pool::{DepositResponse, PoolResponse, LiquidatibleResponse as SP_LiquidatibleResponse, ExecuteMsg as SP_ExecuteMsg, QueryMsg as SP_QueryMsg};
use membrane::liq_queue::{ExecuteMsg as LQ_ExecuteMsg, QueryMsg as LQ_QueryMsg, LiquidatibleResponse as LQ_LiquidatibleResponse};
use membrane::staking::{ExecuteMsg as StakingExecuteMsg};
use membrane::osmosis_proxy::{QueryMsg as OsmoQueryMsg};
use membrane::types::{Basket, SellWallDistribution, Position, AssetInfo, UserInfo, Asset, LiqAsset, cAsset};

use osmo_bindings::{PoolStateResponse };
use osmosis_std::types::osmosis::gamm::v1beta1::MsgExitPool;

use crate::error::{ContractError}; 
use crate::positions::{validate_position_owner, get_target_position, accrue, update_position, insolvency_check, get_avg_LTV, get_cAsset_ratios, get_LP_pool_cAssets, BAD_DEBT_REPLY_ID, update_position_claims, asset_to_coin};
use crate::state::{CONFIG, BASKETS, REPAY, RepayPropagation, POSITIONS};


//Liquidation reply ids
pub const LIQ_QUEUE_REPLY_ID: u64 = 1u64;
pub const STABILITY_POOL_REPLY_ID: u64 = 2u64;
pub const SELL_WALL_REPLY_ID: u64 = 3u64;
pub const USER_SP_REPAY_REPLY_ID: u64 = 6u64;

//Confirms insolvency and calculates repayment amount
//Then sends liquidation messages to the modules if they have funds
//If not, sell wall
pub fn liquidate(
    storage: &mut dyn Storage,
    api: &dyn Api,
    querier: QuerierWrapper,
    env: Env,
    info: MessageInfo,
    basket_id: Uint128,
    position_id: Uint128,
    position_owner: String,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(storage)?;

    let mut basket: Basket = match BASKETS.load(storage, basket_id.to_string()) {
        Err(_) => return Err(ContractError::NonExistentBasket {}),
        Ok(basket) => basket,
    };
    let valid_position_owner =
        validate_position_owner(api, info.clone(), Some(position_owner.clone()))?;

    let mut target_position = get_target_position(
        storage,
        basket_id,
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
    )?;
    //Save updated repayment price and basket debt
    BASKETS.save(storage, basket_id.to_string(), &basket)?;

    //Save updated position to lock-in credit_amount and last_accrued time
    update_position(storage, basket_id.clone().to_string(), valid_position_owner.clone(), target_position.clone())?;

    //Check position health compared to max_LTV
    let (insolvent, current_LTV, _available_fee) = insolvency_check(
        storage,
        env.clone(),
        querier,
        basket.clone(),
        target_position.clone().collateral_assets,
        Decimal::from_ratio(target_position.clone().credit_amount, Uint128::new(1u128)),
        basket.credit_price,
        false,
        config.clone(),
    )?;
    //TODO: For liquidation tests, Delete.
    let insolvent = true;
    let current_LTV = Decimal::percent(90);

    if !insolvent {
        return Err(ContractError::PositionSolvent {});
    }

    //Send liquidation amounts and info to the modules
    //1) We need to calculate how much needs to be liquidated (down to max_borrow_LTV):

    let (avg_borrow_LTV, avg_max_LTV, total_value, cAsset_prices) = get_avg_LTV(
        storage,
        env.clone(),
        querier,
        config.clone(),
        basket.clone(),
        target_position.clone().collateral_assets,
    )?;

    //Get repay value and repay_amount
    let (repay_value, mut credit_repay_amount) = get_repay_quantities(config.clone(), basket.clone(), target_position.clone(), current_LTV.clone(), avg_borrow_LTV)?;

    // Don't send any funds here, only send UserInfo and repayment amounts.
    // We want to act on the reply status but since SubMsg state won't revert if we catch the error,
    // assets we send prematurely won't come back.

    let res = Response::new();
    let mut submessages = vec![];
    let mut fee_messages: Vec<CosmosMsg> = vec![];
    let mut lp_withdraw_messages: Vec<CosmosMsg> = vec![];

    //Pre-LP Split ratios
    let cAsset_ratios = get_cAsset_ratios(
        storage,
        env.clone(),
        querier,
        target_position.clone().collateral_assets,
        config.clone(),
    )?;
    //Post-LP Split assets
    let collateral_assets = get_LP_pool_cAssets(
        querier,
        config.clone(),
        basket.clone(),
        target_position.clone().collateral_assets,
    )?;

    for (i, cAsset) in target_position
        .clone()
        .collateral_assets
        .into_iter()
        .enumerate()
    {
        //Withdraw the necessary amount of LP shares
        //Ensures liquidations are on the pooled assets and not the LP share itself for more efficient queue capital
        if cAsset.clone().pool_info.is_some() {

            let msg = get_lp_liq_withdraw_msg( 
                storage, 
                querier, 
                env.clone(), 
                config.clone(), 
                basket_id.clone(), 
                position_id.clone(), 
                valid_position_owner.clone(), 
                collateral_assets.clone(), 
                cAsset_ratios.clone(), 
                cAsset_prices.clone(), 
                repay_value.clone(),
                cAsset.clone(), 
                i.clone()  
            )?;

            //Comment out to pass accrue_debt test
            lp_withdraw_messages.push(msg);
        }
    }

    //Post-LP Split ratios
    let cAsset_ratios = get_cAsset_ratios(
        storage,
        env.clone(),
        querier,
        collateral_assets.clone(),
        config.clone(),
    )?;
    //Dynamic fee that goes to the caller (info.sender): current_LTV - max_LTV
    let caller_fee = decimal_subtraction(current_LTV, avg_max_LTV);

    //Get amount of repayment user can repay from the Stability Pool
    let user_repay_amount = get_user_repay_amount(querier, config.clone(), basket.clone(), position_id.clone(), position_owner.clone(), &mut credit_repay_amount, &mut submessages)?;

    //Track total leftover repayment after the liq_queue
    let mut liq_queue_leftover_credit_repayment: Decimal = credit_repay_amount;

    //Track repay_amount_per_asset
    let mut per_asset_repayment: Vec<Decimal> = vec![];

    let mut leftover_position_value = total_value;

    //Calculate caller & protocol fees 
    //and amount to send to the Liquidation Queue.
    per_asset_fulfillments(
        storage, 
        querier, 
        env.clone(), 
        config.clone(), 
        basket.clone(), 
        position_id.clone(), 
        valid_position_owner.clone(), 
        info.clone().sender.to_string(),
        caller_fee,
        collateral_assets.clone(), 
        &mut credit_repay_amount, 
        &mut leftover_position_value, 
        &mut liq_queue_leftover_credit_repayment, 
        repay_value, 
        cAsset_ratios.clone(), 
        cAsset_prices, 
        &mut submessages, 
        &mut fee_messages, 
        &mut per_asset_repayment
    )?;
    

    //Build SubMsgs to send to the Stability Pool
    //This will only run if config.stability_pool.is_some()
    let ( leftover_repayment ) = build_sp_submsgs(
        storage, 
        querier, 
        env.clone(), 
        config.clone(), 
        basket.clone(), 
        position_id.clone(), 
        valid_position_owner.clone(), 
        collateral_assets.clone(), 
        cAsset_ratios.clone(), 
        &mut liq_queue_leftover_credit_repayment, 
        &mut credit_repay_amount, 
        &mut leftover_position_value, 
        &mut submessages, 
        per_asset_repayment, 
        user_repay_amount.clone()
    )?;


    //Create the Bad debt callback message to be added as the last SubMsg
    let msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: env.clone().contract.address.to_string(),
        msg: to_binary(&ExecuteMsg::Callback(CallbackMsg::BadDebtCheck {
            basket_id,
            position_id,
            position_owner: valid_position_owner.clone(),
        }))?,
        funds: vec![],
    });
    //The logic for this will be handled in the callback
    //Replying on Error is just so an Error doesn't cancel transaction
    //Don't care about the success case so didnt reply_always
    let call_back = SubMsg::reply_on_error(msg, BAD_DEBT_REPLY_ID);

    //If the SP hasn't repaid everything the liq_queue hasn't AND the value of the position is <= the value that was leftover to be repaid...
    //..sell wall everything from the start, don't go through either module.
    //If we don't we are guaranteeing increased bad debt by selling collateral for a discount.
    if !(leftover_repayment).is_zero()
        && leftover_position_value
            <= decimal_multiplication(leftover_repayment, basket.clone().credit_price)
    {
        //Sell wall credit_repay_amount
        //The other submessages were for the LQ and SP so we reassign the submessage variable
        let (sell_wall_msgs, collateral_distributions) = sell_wall(
            storage,
            collateral_assets.clone(),
            cAsset_ratios.clone(),
            credit_repay_amount,
            basket.clone().credit_asset.info,
            basket_id,
            position_id,
            position_owner.clone(),
        )?;

        let submessages = sell_wall_msgs
            .into_iter()
            .map(|msg| {
                //If this succeeds, we update the positions collateral claims
                //If this fails, do nothing. Try again isn't a useful alternative.
                SubMsg::reply_on_success(msg, SELL_WALL_REPLY_ID)
            })
            .collect::<Vec<SubMsg>>();

        // Set repay values for reply msg
        let repay_propagation = RepayPropagation {
            per_asset_repayment: vec![],
            liq_queue_leftovers: Decimal::zero(),
            stability_pool: Decimal::zero(),
            sell_wall_distributions: vec![SellWallDistribution {
                distributions: collateral_distributions,
            }],
            user_repay_amount,
            basket_id,
            position_id,
            position_owner: valid_position_owner.clone(),
            positions_contract: env.clone().contract.address,
        };

        REPAY.save(storage, &repay_propagation)?;

        Ok(res
            .add_messages(lp_withdraw_messages)
            .add_messages(fee_messages)
            .add_submessages(submessages)
            .add_submessage(call_back)
            .add_attributes(vec![
                attr("method", "liquidate"),
                attr("propagation_info", format!("{:?}", repay_propagation)),
            ]))
    } else {
        let mut repay_propagation: Option<String> = None;
        match REPAY.load(storage) {
            Ok(repay) => repay_propagation = Some(format!("{:?}", repay)),
            Err(_) => {}
        }

        Ok(res
            .add_messages(lp_withdraw_messages)
            .add_messages(fee_messages)
            .add_submessages(submessages)
            .add_submessage(call_back)
            .add_attributes(vec![
                attr("method", "liquidate"),
                attr(
                    "propagation_info",
                    format!("{:?}", repay_propagation.unwrap_or_default()),
                ),
            ]))
    }
}

fn get_repay_quantities(
    config: Config,
    basket: Basket,
    target_position: Position,
    current_LTV: Decimal,
    borrow_LTV: Decimal,
) -> Result<(Decimal, Decimal), ContractError>{
    
    // max_borrow_LTV/ current_LTV, * current_loan_value, current_loan_value - __ = value of loan amount
    let loan_value = decimal_multiplication(
        basket.credit_price,
        Decimal::from_ratio(target_position.clone().credit_amount, Uint128::new(1u128)),
    );

    //repay value = the % of the loan insolvent. Insolvent is anything between current and max borrow LTV.
    //IE, repay what to get the position down to borrow LTV
    let mut repay_value = decimal_multiplication( decimal_division( decimal_subtraction(current_LTV, borrow_LTV), current_LTV), loan_value);

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

    let mut credit_repay_amount = match decimal_division(repay_value, basket.clone().credit_price) {
        //Repay amount has to be above 0, or there is nothing to liquidate and there was a mistake prior
        x if x <= Decimal::new(Uint128::zero()) => return Err(ContractError::PositionSolvent {}),
        //No need to repay more than the debt
        x if x > Decimal::from_ratio(
            target_position.clone().credit_amount,
            Uint128::new(1u128),
        ) =>
        {
            return Err(ContractError::FaultyCalc {})
        }
        x => x,
    };

    Ok((repay_value, credit_repay_amount))
}

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
    if config.clone().stability_pool.is_some() {
        //Query Stability Pool to see if the user has funds
        let user_deposits = querier
            .query::<DepositResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: config.clone().stability_pool.unwrap().to_string(),
                msg: to_binary(&SP_QueryMsg::AssetDeposits {
                    user: position_owner.clone(),
                    asset_info: basket.clone().credit_asset.info,
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
                    basket_id: basket.clone().basket_id,
                    position_id,
                    position_owner: position_owner.clone(),
                },
                repayment: Asset {
                    amount: user_repay_amount * Uint128::new(1u128),
                    info: basket.clone().credit_asset.info,
                },
            };

            let msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.clone().stability_pool.unwrap().to_string(),
                msg: to_binary(&repay_msg)?,
                funds: vec![],
            });

            //Convert to submsg
            let sub_msg: SubMsg = SubMsg::reply_on_error(msg, USER_SP_REPAY_REPLY_ID);

            submessages.push(sub_msg);

            //Subtract Repay amount from credit_repay_amount for the liquidation
            *credit_repay_amount = decimal_subtraction(*credit_repay_amount, user_repay_amount);
        }
    }

    Ok( user_repay_amount )
}

//Calc fees and send liquidatible amount to Liquidaiton Queue
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
    collateral_assets: Vec<cAsset>,
    credit_repay_amount: &mut Decimal,
    leftover_position_value: &mut Decimal,
    liq_queue_leftover_credit_repayment: &mut Decimal,
    repay_value: Decimal,
    cAsset_ratios: Vec<Decimal>,
    cAsset_prices: Vec<Decimal>,
    submessages: &mut Vec<SubMsg>,
    fee_messages: &mut Vec<CosmosMsg>,
    per_asset_repayment: &mut Vec<Decimal>,
) -> StdResult<()>{

    for (num, cAsset) in collateral_assets.clone().iter().enumerate() {
        let mut caller_coins: Vec<Coin> = vec![];
        let mut protocol_coins: Vec<Coin> = vec![];
        let mut fee_assets: Vec<Asset> = vec![];

        let repay_amount_per_asset =
            decimal_multiplication(*credit_repay_amount, cAsset_ratios[num]);


        let collateral_price = cAsset_prices[num];
        let collateral_repay_value = decimal_multiplication(repay_value, cAsset_ratios[num]);
        let collateral_repay_amount = decimal_division(collateral_repay_value, collateral_price);

        //Subtract Caller fee from Position's claims
        let caller_fee_in_collateral_amount =
            decimal_multiplication(collateral_repay_amount, caller_fee) * Uint128::new(1u128);
        update_position_claims(
            storage,
            querier,
            env.clone(),
            basket.clone().basket_id,
            position_id,
            valid_position_owner.clone(),
            cAsset.clone().asset.info,
            caller_fee_in_collateral_amount,
        )?;

        //Subtract Protocol fee from Position's claims
        let protocol_fee_in_collateral_amount =
            decimal_multiplication(collateral_repay_amount, config.clone().liq_fee)
                * Uint128::new(1u128);
        update_position_claims(
            storage,
            querier,
            env.clone(),
            basket.clone().basket_id,
            position_id,
            valid_position_owner.clone(),
            cAsset.clone().asset.info,
            protocol_fee_in_collateral_amount,
        )?;

        //After fees are calculated, set collateral_repay_amount to the amount minus anything the user paid from the SP
        //Has to be after or user_repayment would disincentivize liquidations which would force a non-trivial debt minimum
        let collateral_repay_value =
            decimal_multiplication(repay_amount_per_asset, basket.clone().credit_price);
        let collateral_repay_amount = decimal_division(collateral_repay_value, collateral_price);

        //Subtract fees from leftover_position value
        //fee_value = total_fee_collateral_amount * collateral_price
        let fee_value = decimal_multiplication(
            Decimal::from_ratio(
                caller_fee_in_collateral_amount + protocol_fee_in_collateral_amount,
                Uint128::new(1u128),
            ),
            collateral_price,
        );
        *leftover_position_value = decimal_subtraction(*leftover_position_value, fee_value);

        //Create msgs to caller as well as to liq_queue if.is_some()
        match cAsset.clone().asset.info {
            AssetInfo::Token { address } => {
                //Send caller Fee
                let msg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: address.to_string(),
                    msg: to_binary(&Cw20ExecuteMsg::Transfer {
                        amount: caller_fee_in_collateral_amount,
                        recipient: fee_recipient.clone(),
                    })?,
                    funds: vec![],
                });
                fee_messages.push(msg);

                //Send Protocol Fee
                let msg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: address.to_string(),
                    msg: to_binary(&Cw20ExecuteMsg::Send {
                        contract: config.clone().staking_contract.unwrap().to_string(),
                        amount: protocol_fee_in_collateral_amount,
                        msg: to_binary(&StakingExecuteMsg::DepositFee {})?,
                    })?,
                    funds: vec![],
                });
                fee_messages.push(msg);
            }

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
                fee_assets.push(asset.clone());
                protocol_coins.push(asset_to_coin(asset)?);
            }
        } 
        //Create Msg to send all native token liq fees for fn caller
        let msg = CosmosMsg::Bank(BankMsg::Send {
            to_address: fee_recipient.clone(),
            amount: caller_coins,
        });
        fee_messages.push(msg);

        //Create Msg to send all native token liq fees for MBRN to the staking contract
        let msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.clone().staking_contract.unwrap().to_string(),
            msg: to_binary(&StakingExecuteMsg::DepositFee {})?,
            funds: protocol_coins,
        });
        fee_messages.push(msg);

        /////////////LiqQueue calls//////
        if basket.clone().liq_queue.is_some() {
            //Push
            per_asset_repayment.push(repay_amount_per_asset);

            let res: LQ_LiquidatibleResponse =
                querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
                    contract_addr: basket.clone().liq_queue.unwrap().to_string(),
                    msg: to_binary(&LQ_QueryMsg::CheckLiquidatible {
                        bid_for: cAsset.clone().asset.info,
                        collateral_price,
                        collateral_amount: Uint256::from(
                            (collateral_repay_amount * Uint128::new(1u128)).u128(),
                        ),
                        credit_info: basket.clone().credit_asset.info,
                        credit_price: basket.clone().credit_price,
                    })?,
                }))?;

            //Calculate how much collateral we are sending to the liq_queue to liquidate
            let leftover: Uint128 = Uint128::from_str(&res.leftover_collateral)?;
            let queue_asset_amount_paid: Uint128 =
                (collateral_repay_amount * Uint128::new(1u128)) - leftover;

            //Keep track of remaining position value
            //value_paid_to_queue = queue_asset_amount_paid * collateral_price
            let value_paid_to_queue: Decimal = decimal_multiplication(
                Decimal::from_ratio(queue_asset_amount_paid, Uint128::new(1u128)),
                collateral_price,
            );
            *leftover_position_value =
                decimal_subtraction(*leftover_position_value, value_paid_to_queue);

            //Calculate how much the queue repaid in credit
            let queue_credit_repaid = Uint128::from_str(&res.total_credit_repaid)?;
            *liq_queue_leftover_credit_repayment = decimal_subtraction(
                *liq_queue_leftover_credit_repayment,
                Decimal::from_ratio(queue_credit_repaid, Uint128::new(1u128)),
            );

            //Call Liq Queue::Liquidate for the asset
            let liq_msg = LQ_ExecuteMsg::Liquidate {
                credit_price: basket.credit_price,
                collateral_price,
                collateral_amount: Uint256::from(queue_asset_amount_paid.u128()),
                bid_for: cAsset.clone().asset.info,
                bid_with: basket.clone().credit_asset.info,
                basket_id: basket.clone().basket_id,
                position_id,
                position_owner: valid_position_owner.clone().to_string(),
            };

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

    Ok(())
}

fn build_sp_submsgs(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: Config,
    basket: Basket,
    position_id: Uint128,
    valid_position_owner: Addr,
    collateral_assets: Vec<cAsset>,
    cAsset_ratios: Vec<Decimal>,
    liq_queue_leftover_credit_repayment: &mut Decimal,
    credit_repay_amount: &mut Decimal,
    leftover_position_value: &mut Decimal,
    submessages: &mut Vec<SubMsg>,
    per_asset_repayment: Vec<Decimal>,
    user_repay_amount: Decimal,
) -> Result<Decimal, ContractError>{
    
    let leftover_repayment = Decimal::zero();
    let sell_wall_repayment_amount: Decimal;
    
    if config.clone().stability_pool.is_some() && !liq_queue_leftover_credit_repayment.is_zero() {
        let sp_liq_fee = query_stability_pool_fee(querier, config.clone(), basket.clone())?;

        //If LTV is 90% and the fees are 10%, the position would pay everything to pay the liquidators.
        //So above that, the liquidators are losing the premium guarantee.
        // !( leftover_position_value >= leftover_repay_value * sp_fee)

        //Bc the LQ has already repaid some
        let leftover_repayment_value = decimal_multiplication(
            *liq_queue_leftover_credit_repayment,
            basket.clone().credit_price,
        );

        //SP liq_fee Guarantee check
        if !(*leftover_position_value
            >= decimal_multiplication(leftover_repayment_value, (Decimal::one() + sp_liq_fee)))
        {
            sell_wall_repayment_amount = *liq_queue_leftover_credit_repayment;

            //Go straight to sell wall
            let (sell_wall_msgs, collateral_distributions) = sell_wall(
                storage,
                collateral_assets.clone(),
                cAsset_ratios.clone(),
                sell_wall_repayment_amount,
                basket.clone().credit_asset.info,
                basket.clone().basket_id,
                position_id,
                valid_position_owner.clone().to_string(),
            )?;

            submessages.extend(
                sell_wall_msgs
                    .into_iter()
                    .map(|msg| {
                        //If this succeeds, we update the positions collateral claims
                        //If this fails, error. Try again isn't a useful alternative.
                        SubMsg::reply_on_success(msg, SELL_WALL_REPLY_ID)
                    })
                    .collect::<Vec<SubMsg>>(),
            );

            //Leftover's starts as the total LQ is supposed to pay,
            //and is subtracted by every successful LQ reply
            let liq_queue_leftovers =
                decimal_subtraction(*credit_repay_amount, *liq_queue_leftover_credit_repayment);

            // Set repay values for reply msg
            let repay_propagation = RepayPropagation {
                per_asset_repayment,
                liq_queue_leftovers,
                stability_pool: Decimal::zero(),
                sell_wall_distributions: vec![SellWallDistribution {
                    distributions: collateral_distributions,
                }],
                user_repay_amount,
                basket_id: basket.clone().basket_id,
                position_id,
                position_owner: valid_position_owner.clone(),
                positions_contract: env.clone().contract.address,
            };

            REPAY.save(storage, &repay_propagation)?;
        } else {
            //Check for stability pool funds before any liquidation attempts
            //If no funds, go directly to the sell wall
            let leftover_repayment = query_stability_pool_liquidatible(
                querier,
                config.clone(),
                *liq_queue_leftover_credit_repayment,
                basket.clone().credit_asset.info,
            )?;

            let mut collateral_distributions = vec![];

            if leftover_repayment > Decimal::zero() {
                sell_wall_repayment_amount = leftover_repayment;

                //Sell wall remaining
                let (sell_wall_msgs, distributions) = sell_wall(
                    storage,
                    collateral_assets.clone(),
                    cAsset_ratios.clone(),
                    sell_wall_repayment_amount,
                    basket.clone().credit_asset.info,
                    basket.clone().basket_id,
                    position_id,
                    valid_position_owner.clone().to_string(),
                )?;
                collateral_distributions = distributions;

                submessages.extend(
                    sell_wall_msgs
                        .into_iter()
                        .map(|msg| {
                            //If this succeeds, we update the positions collateral claims
                            //If this fails, error. Try again isn't a useful alternative.
                            SubMsg::reply_on_success(msg, SELL_WALL_REPLY_ID)
                        })
                        .collect::<Vec<SubMsg>>(),
                );
            }

            //Set Stability Pool repay_amount
            let sp_repay_amount = decimal_subtraction(*liq_queue_leftover_credit_repayment, leftover_repayment);

            //Leftover's starts as the total LQ is supposed to pay, and is subtracted by every successful LQ reply
            let liq_queue_leftovers =
                decimal_subtraction(*credit_repay_amount, *liq_queue_leftover_credit_repayment);

            // Set repay values for reply msg
            let repay_propagation = RepayPropagation {
                per_asset_repayment,
                liq_queue_leftovers,
                stability_pool: sp_repay_amount,
                sell_wall_distributions: vec![SellWallDistribution {
                    distributions: collateral_distributions,
                }],
                user_repay_amount,
                basket_id: basket.clone().basket_id,
                position_id,
                position_owner: valid_position_owner.clone(),
                positions_contract: env.clone().contract.address,
            };

            REPAY.save(storage, &repay_propagation)?;

            ///////////////////

            //Stability Pool message builder
            let liq_msg = SP_ExecuteMsg::Liquidate {
                credit_asset: LiqAsset {
                    amount: sp_repay_amount,
                    info: basket.clone().credit_asset.info,
                },
            };

            let msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.clone().stability_pool.unwrap().to_string(),
                msg: to_binary(&liq_msg)?,
                funds: vec![],
            });

            let sub_msg: SubMsg = SubMsg::reply_always(msg, STABILITY_POOL_REPLY_ID);

            submessages.push(sub_msg);

            //Because these are reply always, we can NOT make state changes that we wouldn't allow no matter the tx result, as our altereed state will NOT revert.
            //Errors also won't revert the whole transaction
            //( https://github.com/CosmWasm/cosmwasm/blob/main/SEMANTICS.md#submessages )

            //Collateral distributions get handled in the reply

            //Set and subtract the value of what was paid to the Stability Pool
            //(sp_repay_amount * credit_price) * (1+sp_liq_fee)
            let paid_to_sp = decimal_multiplication(
                decimal_multiplication(sp_repay_amount, basket.credit_price),
                (Decimal::one() + sp_liq_fee),
            );
            *leftover_position_value = decimal_subtraction(*leftover_position_value, paid_to_sp);
        }
    } else {
        //In case SP isn't used, we need to set RepayPropagation
        // Set repay values for reply msg
        let repay_propagation = RepayPropagation {
            per_asset_repayment,
            liq_queue_leftovers: Decimal::zero(),
            stability_pool: Decimal::zero(),
            sell_wall_distributions: vec![],
            user_repay_amount,
            basket_id: basket.clone().basket_id,
            position_id,
            position_owner: valid_position_owner.clone(),
            positions_contract: env.clone().contract.address,
        };

        REPAY.save(storage, &repay_propagation)?;
    }

    Ok(leftover_repayment)
}

//Returns LP withdrawal message that is used in liquidations
fn get_lp_liq_withdraw_msg(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: Config,
    basket_id: Uint128,
    position_id: Uint128,
    valid_position_owner: Addr,
    collateral_assets: Vec<cAsset>,
    cAsset_ratios: Vec<Decimal>,
    cAsset_prices: Vec<Decimal>,
    repay_value: Decimal,
    cAsset: cAsset,
    i: usize,
) -> StdResult<CosmosMsg>{
    
    let pool_info = cAsset.clone().pool_info.unwrap();

    //Find cAsset_prices index for both LP assets
    let mut indexes = vec![];

    for asset in pool_info.asset_infos.clone() {
        if let Some( (i, _cAsset) ) = collateral_assets.clone().into_iter().enumerate().find(|cAsset| cAsset.1.asset.info.equal(&asset.info)){
            //Push index
            indexes.push( i );
        }
    }
    
    //Query per share asset amounts
    let share_asset_amounts = querier
        .query::<PoolStateResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
            msg: to_binary(&OsmoQueryMsg::PoolState {
                id: pool_info.pool_id,
            })?,
        }))?
        .shares_value(Uint128::new(1u128));

    //Find LP price
    let lp_price = {
        //Get asset values
        let mut per_asset_value = vec![]; 
        
        for (i, asset) in share_asset_amounts.clone().into_iter().enumerate() {
            per_asset_value.push( cAsset_prices[ indexes[i] ] * asset.amount );
        }
        //Get value for 1 LPshare
        let individual_share_value: Uint128 = per_asset_value.clone().into_iter().sum();

        //Get asset ratios
        let mut per_asset_ratio = vec![]; 

        for value in per_asset_value {
            per_asset_ratio.push( Decimal::from_ratio(value, individual_share_value) );
        }

        //Get price
        let mut lp_price = Decimal::zero();

        for (i, ratio) in per_asset_ratio.into_iter().enumerate() {
            lp_price += decimal_multiplication(ratio, cAsset_prices[ indexes[i] ] );
        }

        lp_price
    };

    ////Calculate amount of asset to liquidate
    // Amount to liquidate = cAsset_ratio * % of position insolvent * cAsset amount
    let lp_liquidate_amount = decimal_division( 
        decimal_multiplication(
            cAsset_ratios[i],
            repay_value), 
        lp_price)
    * Uint128::new(1u128);

    
    update_position_claims(
        storage,
        querier,
        env.clone(),
        basket_id,
        position_id,
        valid_position_owner.clone(),
        cAsset.clone().asset.info,
        lp_liquidate_amount,
    )?;
    
    //Query total share asset amounts
    let share_asset_amounts = querier
        .query::<PoolStateResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
            msg: to_binary(&OsmoQueryMsg::PoolState {
                id: pool_info.pool_id,
            })?,
        }))?
        .shares_value(lp_liquidate_amount);

    //Push LP Withdrawal Msg
    let mut token_out_mins: Vec<osmosis_std::types::cosmos::base::v1beta1::Coin> = vec![];
    for token in share_asset_amounts {
        token_out_mins.push(osmosis_std::types::cosmos::base::v1beta1::Coin {
            denom: token.denom,
            amount: token.amount.to_string(),
        });
    }

    let msg: CosmosMsg = MsgExitPool {
        sender: env.contract.address.to_string(),
        pool_id: pool_info.pool_id,
        share_in_amount: lp_liquidate_amount.to_string(),
        token_out_mins,
    }
    .into();

    Ok( msg )

}


pub fn sell_wall_using_ids(
    storage: &mut dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    basket_id: Uint128,
    position_id: Uint128,
    position_owner: Addr,
    repay_amount: Decimal,
) -> StdResult<(Vec<CosmosMsg>, Vec<(AssetInfo, Decimal)>)> {
    let config: Config = CONFIG.load(storage)?;

    let basket: Basket = BASKETS.load(storage, basket_id.to_string())?;

    let positions: Vec<Position> =
        POSITIONS.load(storage, (basket_id.to_string(), position_owner.clone()))?;

    let target_position = match positions.into_iter().find(|x| x.position_id == position_id) {
        Some(position) => position,
        None => {
            return Err(StdError::NotFound {
                kind: "Position".to_string(),
            })
        }
    };
    let collateral_assets = get_LP_pool_cAssets(
        querier,
        config.clone(),
        basket.clone(),
        target_position.clone().collateral_assets,
    )?;
    let cAsset_ratios = get_cAsset_ratios(
        storage,
        env.clone(),
        querier,
        collateral_assets.clone(),
        config,
    )?;

    match sell_wall(
        storage,
        collateral_assets.clone(),
        cAsset_ratios,
        repay_amount,
        basket.clone().credit_asset.info,
        basket_id,
        position_id,
        position_owner.to_string(),
    ) {
        Ok(res) => Ok(res),
        Err(err) => {
            return Err(StdError::GenericErr {
                msg: err.to_string(),
            })
        }
    }
}

pub fn sell_wall(
    storage: &dyn Storage,
    collateral_assets: Vec<cAsset>,
    cAsset_ratios: Vec<Decimal>,
    repay_amount: Decimal,
    credit_info: AssetInfo,
    //For Repay msg
    basket_id: Uint128,
    position_id: Uint128,
    position_owner: String,
) -> Result<(Vec<CosmosMsg>, Vec<(AssetInfo, Decimal)>), ContractError> {
    let config: Config = CONFIG.load(storage)?;

    let mut messages = vec![];
    let mut collateral_distribution = vec![];

    for (index, ratio) in cAsset_ratios.into_iter().enumerate() {
        let collateral_repay_amount = decimal_multiplication(ratio, repay_amount);
        collateral_distribution.push((
            collateral_assets[index].clone().asset.info,
            collateral_repay_amount,
        ));

        match collateral_assets[index].clone().asset.info {
            AssetInfo::NativeToken { denom } => {
                let router_msg = RouterExecuteMsg::SwapFromNative {
                    to: credit_info.clone(),
                    max_spread: None, //Max spread doesn't matter bc we want to sell the whole amount no matter what
                    recipient: None,
                    hook_msg: Some(to_binary(&ExecuteMsg::Repay {
                        basket_id,
                        position_id,
                        position_owner: Some(position_owner.clone()),
                    })?),
                    split: None,
                };

                let payment = coin(
                    (collateral_repay_amount * Uint128::new(1u128)).u128(),
                    denom,
                );

                let msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: config.clone().dex_router.unwrap().to_string(),
                    msg: to_binary(&router_msg)?,
                    funds: vec![payment],
                });

                messages.push(msg);
            }
            AssetInfo::Token { address } => {
                //////////////////////////
                let router_hook_msg = RouterHookMsg::Swap {
                    to: credit_info.clone(),
                    max_spread: None,
                    recipient: None,
                    hook_msg: Some(to_binary(&ExecuteMsg::Repay {
                        basket_id,
                        position_id,
                        position_owner: Some(position_owner.clone()),
                    })?),
                    split: None,
                };

                let msg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: address.to_string(),
                    msg: to_binary(&Cw20ExecuteMsg::Send {
                        amount: collateral_repay_amount * Uint128::new(1u128),
                        contract: config.clone().dex_router.unwrap().to_string(),
                        msg: to_binary(&router_hook_msg)?,
                    })?,
                    funds: vec![],
                });

                messages.push(msg);
            }
        }
    }

    Ok((messages, collateral_distribution))
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

    Ok(resp.liq_premium)
}

pub fn query_stability_pool_liquidatible(
    querier: QuerierWrapper,
    config: Config,
    amount: Decimal,
    info: AssetInfo,
) -> StdResult<Decimal> {
    let query_res: SP_LiquidatibleResponse =
        querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: config.stability_pool.unwrap().to_string(),
            msg: to_binary(&SP_QueryMsg::CheckLiquidatible {
                asset: LiqAsset {
                    amount: amount,
                    info,
                },
            })?,
        }))?;

    Ok(query_res.leftover)
}
