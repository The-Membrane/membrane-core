use std::cmp::min;
use std::str::FromStr;
use std::vec;

use cosmwasm_std::{
    attr, to_json_binary, Addr, Api, BankMsg, Coin, CosmosMsg, Decimal, DepsMut, Env, MessageInfo,
    QuerierWrapper, QueryRequest, Response, StdError, StdResult, Storage, SubMsg, Uint128, WasmMsg,
    WasmQuery,
};

use membrane::helpers::{validate_position_owner, asset_to_coin, withdrawal_msg, get_contract_balances};
use membrane::cdp::{Config, EditBasket, ExecuteMsg, CallbackMsg};
use membrane::oracle::{AssetResponse, PriceResponse};
use membrane::liq_queue::ExecuteMsg as LQ_ExecuteMsg;
use membrane::liquidity_check::ExecuteMsg as LiquidityExecuteMsg;
use membrane::ltv_disco::{ExecuteMsg as LTVDiscoExecuteMsg};
use membrane::staking::{ExecuteMsg as Staking_ExecuteMsg, QueryMsg as Staking_QueryMsg, Config as Staking_Config};
use membrane::oracle::{ExecuteMsg as OracleExecuteMsg, QueryMsg as OracleQueryMsg};
use membrane::osmosis_proxy::{ExecuteMsg as OsmoExecuteMsg, QueryMsg as OsmoQueryMsg };
use membrane::stability_pool::ExecuteMsg as SP_ExecuteMsg;
use membrane::deployable_venue::{ExecuteMsg as DeploymentVenue_ExecuteMsg};
use membrane::math::{decimal_division, decimal_multiplication, Uint256, decimal_subtraction};
use membrane::types::{
    cAsset, AffiliateData, Asset, AssetInfo, AssetOracleInfo, Basket, DeploymentIntent, DeploymentVenue, IndividualCost, LPAssetInfo, LeaveTokens, LiquidityInfo, PendingRevenue, PoolInfo, PoolStateResponse, PoolType, Position, PositionRedemption, PurchaseIntent, RangeBoundUserIntents, RedemptionInfo, StringEntry, SupplyCap, UserDeploymentIntents, UserInfo
};

use crate::contract::set_active_deployment_venues;
use crate::liquidations::get_deployable_venues_user_repay_amount;
use crate::query::{get_cAsset_ratios, get_avg_LTV, insolvency_check};
use crate::rates::accrue;
use crate::risk_engine::update_basket_tally;
use crate::state::{create_collateral_rate_assurance, get_target_position, update_cdt_supply, update_position, update_position_claims, ClosePositionPropagation, CollateralVolatility, Timer, AFFILIATES, BASKET, CLOSE_POSITION, FREEZE_TIMER, REDEMPTION_OPT_IN, STORED_PRICES, VOLATILITY};
use crate::{
    state::{
        WithdrawPropagation, CONFIG, POSITIONS, LIQUIDATION, WITHDRAW, USER_INTENTS
    },
    ContractError,
};

//Liquidation reply ids
pub const LIQ_QUEUE_REPLY_ID: u64 = 1u64;
// pub const SP_REPLY_ID: u64 = 2u64;
// pub const USER_SP_REPAY_REPLY_ID: u64 = 3u64;
pub const WITHDRAW_REPLY_ID: u64 = 4u64;
pub const REVENUE_REPLY_ID: u64 = 5u64;
pub const CLOSE_POSITION_REPLY_ID: u64 = 6u64;
pub const SELL_COLLATERAL_REPLY_ID: u64 = 7u64;
pub const DEPLOYABLE_VENUE_REPLY_ID: u64 = 8u64;
pub const BAD_DEBT_REPLY_ID: u64 = 999999u64;


//Constants
const MAX_POSITIONS_AMOUNT: u32 = 9;


/// Deposit collateral to existing position. New or existing collateral.
/// Anyone can deposit, to any position. Owner restrictions for withdrawals.
pub fn deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_owner: Option<String>,
    position_id: Option<Uint128>,
    cAssets: Vec<cAsset>,
) -> Result<Response, ContractError> {    
    let config = CONFIG.load(deps.storage)?;
    let valid_owner_addr = validate_position_owner(deps.api, info.clone(), position_owner)?;
    let mut basket: Basket = BASKET.load(deps.storage)?;
    let mut set_redemption = false;
    
    //Check if frozen
    if basket.frozen { return Err(ContractError::Frozen {  }) }

    //Set deposit_amounts to double check state storage 
    let deposit_amounts: Vec<Uint128> = cAssets.clone()
        .into_iter()
        .map(|cAsset| cAsset.asset.amount)
        .collect::<Vec<Uint128>>();

    //If any cAsset is a rate_hike asset, force a redemption 
    for cAsset in cAssets.clone(){
        if cAsset.hike_rates.is_some() && cAsset.hike_rates.unwrap() {
            set_redemption = true;
        }
    }

    //Initialize positions_prev_collateral & position_info for deposited assets
    //Used for to double check state storage
    let mut positions_prev_collateral = vec![];
    let position_info: UserInfo;

    if let Ok(mut positions) = POSITIONS.load(deps.storage, valid_owner_addr.clone()){

        //Add collateral to the position_id or Create a new position 
        if let Some(position_id) = position_id {
            //If a rate hike asset is deposited, force a redemption
            if set_redemption {
                edit_redemption_info(
                    deps.storage,
                    info.clone().sender,
                    vec![position_id],
                    Some(true),
                    Some(1),
                    Some(Decimal::one()),
                    None,
                    true,
                )?;
            }
            //Find the position
            if let Some((position_index, mut position)) = positions.clone()
                .into_iter()
                .enumerate()
                .find(|(_i, position)| position.position_id == position_id){

                //Store position_info for reply
                position_info = UserInfo {
                    position_id,
                    position_owner: valid_owner_addr.to_string(),
                };

                for deposit in cAssets.clone(){
                    //Search for cAsset in the position 
                    if let Some((collateral_index, cAsset)) = position.clone().collateral_assets
                        .into_iter()
                        .enumerate()
                        .find(|(_i, cAsset)| cAsset.asset.info.equal(&deposit.asset.info)){
                        //Store positions_prev_collateral
                        positions_prev_collateral.push(cAsset.clone().asset);

                        //Add to existing cAsset
                        position.collateral_assets[collateral_index].asset.amount += deposit.asset.amount;

                    } else { //Add new cAsset object to position
                        position.collateral_assets.push( deposit.clone() );

                        let placeholder_asset = Asset {
                            amount: Uint128::zero(),
                            ..deposit.clone().asset
                        };
                        //Store positions_prev_collateral
                        positions_prev_collateral.push(placeholder_asset.clone());

                    }
                }
                //Set updated position
                positions[position_index] = position.clone();
                
                //Accrue
                accrue(
                    deps.storage,
                    deps.querier,
                    env.clone(),
                    config.clone(),
                    &mut position,
                    &mut basket,
                    valid_owner_addr.to_string(),
                    true,
                )?;
                //Save Updated Vec<Positions> for the user
                POSITIONS.save(deps.storage, valid_owner_addr, &positions)?;

                if !position.credit_amount.is_zero() {
                    //Update Supply caps
                    update_basket_tally(
                        deps.storage, 
                        deps.querier, 
                        env.clone(), 
                        &mut basket, 
                        cAssets.clone(),
                        position.clone().collateral_assets,
                        true,
                        config.clone(),
                        false,
                    )?;
                }
                //Save Basket
                BASKET.save(deps.storage, &basket)?;

            } else {
                //If position_ID is passed but no position is found, Error. 
                //In case its a mistake, don't want to add assets to a new position.
                return Err(ContractError::NonExistentPosition { id: position_id });
            }
        } else { //If user doesn't pass an ID, we create a new position

            //Enforce max positions
            if positions.len() >= MAX_POSITIONS_AMOUNT as usize {
                return Err(ContractError::MaxPositionsReached {});
            }
            
            //Create new position
            let (new_position_info, new_position) = create_position_in_deposit(
                deps.storage,
                deps.querier,
                env.clone(),
                config.clone(),
                valid_owner_addr.clone(),
                cAssets.clone(),
                &mut basket
            )?;

            //Update position_info for state check
            position_info = new_position_info;

            //Add new position to the user's Vec<Positions>
            POSITIONS.update(
                deps.storage,
                valid_owner_addr,
                |positions| -> StdResult<_> {
                    let mut positions = positions.unwrap_or_default();
                    positions.push(new_position);
                    Ok(positions)
                },
            )?;

            
            //If a rate hike asset is deposited, force a redemption
            if set_redemption {
                edit_redemption_info(
                    deps.storage,
                    info.clone().sender,
                    vec![position_info.position_id],
                    Some(true),
                    Some(1),
                    Some(Decimal::one()),
                    None,
                    true,
                )?;
            }
        }
    } else { //No existing positions loaded so new Vec<Position> is created
        let (new_position_info, new_position) = create_position_in_deposit(
            deps.storage,
            deps.querier,
            env.clone(),
            config.clone(),
            valid_owner_addr.clone(),
            cAssets.clone(),
            &mut basket
        )?;

        //Update position_info for state check
        position_info = new_position_info;

        //Add new Vec of Positions to state under the user
        POSITIONS.save(
            deps.storage,
            valid_owner_addr,
            &vec![new_position],
        )?;
                
        //If a rate hike asset is deposited, force a redemption
        if set_redemption {
            edit_redemption_info(
                deps.storage,
                info.clone().sender,
                vec![position_info.position_id],
                Some(true),
                Some(1),
                Some(Decimal::one()),
                None,
                true,
            )?;
        }
    }

    //Double check State storage
    check_deposit_state(deps.storage, deps.api, positions_prev_collateral, deposit_amounts, position_info.clone())?;    

    // Create collateral rate assurance for deposited assets
    let collateral_denoms: Vec<String> = cAssets.iter()
        .map(|c_asset| c_asset.asset.info.to_string())
        .collect();
    let rate_assurance_msgs = create_collateral_rate_assurance(
        deps.storage,
        deps.querier,
        env.clone(),
        collateral_denoms,
        &basket,
    )?;

    Ok(Response::new()
    .add_messages(rate_assurance_msgs)
    .add_attributes(vec![
        attr("method", "deposit"),
        attr("position_owner", position_info.position_owner),
        attr("position_id", position_info.position_id),
        attr("assets", format!("{:?}", cAssets.into_iter().map(|a|a.asset).collect::<Vec<Asset>>())),
    ]))
}

/// Function used to create & save a position, then update state.
/// This is a helper function to reduce the size of the deposit function.
fn create_position_in_deposit(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: Config,
    valid_owner_addr: Addr,
    cAssets: Vec<cAsset>,
    basket: &mut Basket,
) -> Result<(UserInfo, Position), ContractError> {
    let mut new_position = create_position(cAssets, basket)?;

    //Store position_info for reply
    let position_info = UserInfo {
        position_id: new_position.clone().position_id,
        position_owner: valid_owner_addr.to_string(),
    };

    //Accrue, mainly for repayment price
    accrue(
        storage,
        querier,
        env,
        config.clone(),
        &mut new_position,
        basket,
        valid_owner_addr.to_string(),
        true,
    )?;
    //Save Basket
    BASKET.save(storage, basket)?;

    Ok((position_info, new_position))
}

/// Function used to validate the state of the contract after a deposit
fn check_deposit_state(
    storage: &mut dyn Storage,  
    api: &dyn Api,   
    positions_prev_collateral: Vec<Asset>, //Amount of collateral in the position before the deposit
    deposit_amounts: Vec<Uint128>,
    position_info: UserInfo,
) -> Result<(), ContractError>{
    let (_i, target_position) = get_target_position(
        storage, 
        api.addr_validate(&position_info.position_owner)?, 
        position_info.position_id
    )?;

    for (i, asset) in positions_prev_collateral.clone().into_iter().enumerate(){

        if let Some(cAsset) = target_position.clone().collateral_assets
            .into_iter()
            .find(|cAsset| cAsset.asset.info.equal(&asset.info)){

            //Assert cAsset total is equal to the amount deposited + prev_asset_amount
            if cAsset.asset.amount != asset.amount + deposit_amounts[i] {
                return Err(ContractError::CustomError { val: String::from("Conditional 1: Possible state error") })
            }
        }
    }

    //If a deposit to a new position, asset amounts should be exactly what was deposited
    if positions_prev_collateral == vec![] {
        for (i, cAsset) in target_position.collateral_assets.into_iter().enumerate() {
            if cAsset.asset.amount != deposit_amounts[i] {
                return Err(ContractError::CustomError { val: String::from("Deposit Conditional 2: Possible state error") })
            }
        }
    }

    Ok(())
}

/// Withdraws assets from a position.
/// Validates withdraw amount & updates state.
pub fn withdraw(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_id: Uint128,
    cAssets: Vec<cAsset>,
    send_to: Option<String>,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;
    let mut basket: Basket = BASKET.load(deps.storage)?;
    let mut msgs = vec![];

    //Check if frozen
    if basket.frozen { return Err(ContractError::Frozen {  }) }

    //Set recipient
    let mut recipient = info.clone().sender;
    if let Some(string) = send_to.clone() {
        recipient = deps.api.addr_validate(&string)?;
    } 

    //Set position owner
    let mut valid_position_owner = info.clone().sender;

    //If the contract is withdrawing for a user (i.e. ClosePosition), set the position owner to the recipient
    if info.sender == env.contract.address && send_to.is_some(){
        valid_position_owner = recipient.clone();
    }

    //This forces withdrawals to be done by the info.sender
    let (position_index, mut target_position) = get_target_position(deps.storage, valid_position_owner.clone(), position_id)?;
    //Accrue interest
    accrue(
        deps.storage,
        deps.querier,
        env.clone(),
        config.clone(),
        &mut target_position,
        &mut basket,
        valid_position_owner.to_string(),
        false,
    )?;

    //For supply cap updates
    let mut tally_update_list: Vec<cAsset> = vec![];

    //Set withdrawal prop variables for state checks
    let mut prop_assets = vec![];
    let mut withdraw_amounts: Vec<Uint128> = vec![];

    //For Withdraw Msg
    let mut withdraw_coins: Vec<Coin> = vec![];

    //Check for expunged assets and assert they are being withdrawn
    check_for_expunged(target_position.clone().collateral_assets, cAssets.clone(), basket.clone() )?;

    //Attempt to withdraw each cAsset
    for cAsset in cAssets.clone() {
        let withdraw_asset = cAsset.asset;             

        //Find cAsset in target_position
        if let Some((collateral_index, position_collateral)) = target_position.clone().collateral_assets
            .into_iter()
            .enumerate()
            .find(|(_i, cAsset)| cAsset.asset.info.equal(&withdraw_asset.info)){
            //If the cAsset is found in the position, attempt withdrawal
            //Cant withdraw more than the positions amount
            if withdraw_asset.amount > position_collateral.asset.amount {
                return Err(ContractError::InvalidWithdrawal {});
            } else {
                //Now that its a valid withdrawal and debt has accrued, we can add to tally_update_list
                //This will be used to keep track of Basket supply caps
                tally_update_list.push(cAsset {
                    asset: withdraw_asset.clone(),
                    ..position_collateral.clone()
                });

                //Withdraw Prop: Push the initial asset
                prop_assets.push(position_collateral.clone().asset);

                //Update cAsset data to account for the withdrawal
                let leftover_amount = position_collateral.asset.amount - withdraw_asset.amount;               

                //Delete asset from the position if the amount is being fully withdrawn, otherwise edit.
                if leftover_amount != Uint128::new(0u128) {
                    target_position.collateral_assets[collateral_index].asset.amount = leftover_amount;
                } else {
                    target_position.collateral_assets.remove(collateral_index);
                }

                //If resulting LTV makes the position insolvent, error. If not construct withdrawal_msg
                //This is taking max_borrow_LTV so users can't max borrow and then withdraw to get a higher initial LTV
                let (insolvency_res, _) = insolvency_check(
                    deps.storage,
                    env.clone(),
                    deps.querier,
                    Some(basket.clone()),
                    target_position.clone().collateral_assets,
                    target_position.clone().credit_amount,
                    basket.clone().credit_price,
                    true,
                    config.clone(),
                )?;
                if insolvency_res.0 {
                    return Err(ContractError::PositionInsolvent { insolvency_res });
                } else {
                    //Update Position list
                    POSITIONS.update(deps.storage, valid_position_owner.clone(), |positions: Option<Vec<Position>>| -> Result<Vec<Position>, ContractError>{

                        let mut updating_positions = positions.unwrap_or_else(|| vec![]);

                        //If new position isn't empty, update
                        if !check_for_empty_position(target_position.clone().collateral_assets){
                            updating_positions[position_index] = target_position.clone();
                        } else { // remove position that was withdrawn from
                            updating_positions.remove(position_index);
                        }

                        Ok( updating_positions )                    
                    })?;
                    //load to check if positions list is fully empty
                    let positions = POSITIONS.load(deps.storage, valid_position_owner.clone())?;
                    //Delete if empty
                    if positions.is_empty(){
                        POSITIONS.remove(deps.storage, valid_position_owner.clone());
                    }

                    //If new position is empty, remove from UserIntentState
                    if check_for_empty_position(target_position.clone().collateral_assets){
                        //Remove position from UserIntentState                        
                        let mut intents = USER_INTENTS.load(deps.storage, valid_position_owner.clone().to_string()).unwrap_or_else(|_| UserDeploymentIntents {
                            user: valid_position_owner.clone().to_string(),
                            deployment_intents: vec![],
                        });
                        intents.deployment_intents = intents.deployment_intents.into_iter().filter(|intent| intent.position_id != position_id).collect();

                        if intents.deployment_intents.is_empty(){
                            USER_INTENTS.remove(deps.storage, valid_position_owner.clone().to_string());
                        } else {
                            USER_INTENTS.save(deps.storage, valid_position_owner.clone().to_string(), &intents)?;
                        }
                    }
                }
                
                //Push withdraw asset to list for withdraw prop
                withdraw_amounts.push(withdraw_asset.clone().amount);

                //Add to native token send list
                if let AssetInfo::NativeToken { denom: _ } = withdraw_asset.clone().info {
                    //Push to withdraw_coins
                    withdraw_coins.push(asset_to_coin(withdraw_asset)?);
                }
            }
        }         
    };
    
    //Push aggregated native coin withdrawal
    if withdraw_coins != vec![] {
        let message = CosmosMsg::Bank(BankMsg::Send {
            to_address: recipient.to_string(),
            amount: withdraw_coins,
        });
        msgs.push(SubMsg::reply_on_success(message, WITHDRAW_REPLY_ID));
    }

    //Update supply cap tallies
    if !target_position.clone().credit_amount.is_zero(){        
        //Update basket supply cap tallies after the full withdrawal to improve UX by smoothing debt_cap restrictions
        update_basket_tally(
            deps.storage,
            deps.querier,
            env.clone(),
            &mut basket,
            tally_update_list,
            target_position.clone().collateral_assets,
            false,
            config.clone(),
            false,
        )?;
    } 
    //Save updated repayment price and asset tallies
    BASKET.save(deps.storage, &basket)?;
    
    //Set Withdrawal_Prop
    let prop_assets_info: Vec<AssetInfo> = prop_assets
        .clone()
        .into_iter()
        .map(|asset| asset.info)
        .collect::<Vec<AssetInfo>>();
    
    let withdrawal_prop = WithdrawPropagation {
        positions_prev_collateral: prop_assets,
        withdraw_amounts,
        contracts_prev_collateral_amount: get_contract_balances(
            deps.querier,
            env.clone(),
            prop_assets_info,
        )?,
        position_info: UserInfo {
            position_id,
            position_owner: info.sender.to_string(),
        },
    };
    WITHDRAW.save(deps.storage, &withdrawal_prop)?;

    // Create collateral rate assurance for withdrawn assets
    let collateral_denoms: Vec<String> = cAssets.iter()
        .map(|c_asset| c_asset.asset.info.to_string())
        .collect();
    let rate_assurance_msgs = create_collateral_rate_assurance(
        deps.storage,
        deps.querier,
        env.clone(),
        collateral_denoms,
        &basket,
    )?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "withdraw"),
            attr("position_id", position_id),
            attr("assets", format!("{:?}", cAssets)),
        ])
        .add_submessages(msgs)
        .add_messages(rate_assurance_msgs))
}

/// Use credit to repay outstanding debt in a Position.
/// Validates repayment & updates state.
/// Note: Excess repayment defaults to the sending address.
pub fn repay(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    api: &dyn Api,
    env: Env,
    info: MessageInfo,
    position_id: Uint128,
    position_owner: Option<String>,
    credit_asset: Asset,
    send_excess_to: Option<String>,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(storage)?;
    let mut basket: Basket = BASKET.load(storage)?;

    //Check if frozen
    if basket.frozen { return Err(ContractError::Frozen {  }) }

    //Validate position owner 
    let valid_owner_addr = validate_position_owner(api, info.clone(), position_owner)?;
    
    //Get target_position
    let (position_index, mut target_position) = get_target_position(storage, valid_owner_addr.clone(), position_id)?;

    //SP accrues externally before calling repay, so we only accrue if the sender isn't the SP
    //Accrue interest
    accrue(
        storage,
        querier,
        env.clone(),
        config.clone(),
        &mut target_position,
        &mut basket,
        valid_owner_addr.to_string(),
        false,
    )?;

    //Set prev_credit_amount for state checks
    let prev_credit_amount = target_position.credit_amount;
    
    let mut messages = vec![];
    let mut excess_repayment = Uint128::zero();
    // println!("target_position.credit_amount: {:?}", target_position.credit_amount);

    //Repay amount sent
    target_position.credit_amount = match target_position.credit_amount.checked_sub(credit_asset.amount){
        Ok(difference) => difference,
        Err(_err) => {
            //Set excess_repayment
            excess_repayment = credit_asset.amount - target_position.credit_amount;
            
            Uint128::zero()
        },
    };

    // println!("excess_repayment: {:?}", excess_repayment);
    // println!("credit_asset.amount: {:?}", credit_asset.amount);

    //Update Supply caps if this clears all debt
    if target_position.credit_amount.is_zero(){
        update_basket_tally(
            storage, 
            querier, 
            env.clone(), 
            &mut basket, 
            target_position.collateral_assets.clone(),
            target_position.clone().collateral_assets,
            false,
            config.clone(),
            false,
        )?;
    }

    //Position's resulting debt value can't be below minimum without being fully repaid
    if basket.clone().credit_price.get_value(target_position.credit_amount)? < Decimal::from_ratio(config.debt_minimum, Uint128::one())
        && !target_position.credit_amount.is_zero(){
        //Router contract, Stability Pool & Liquidation Queue are allowed to.
        //Router: We rather $1 of bad debt than $2000 and bad debt comes from swap slippage
        //SP & LQ: If the resulting debt is below the minimum, the whole loan is liquidated so it won't be under the minimum by the end of the liquidation process
        let mut let_pass = false;
        // if let Some(router) = config.clone().dex_router {
        //     if info.sender == router { let_pass = true; }
        // }
        // if let Some(stability_pool) = config.clone().stability_pool {
        //     if info.sender == stability_pool { let_pass = true; }
        // }
        if let Some(liq_queue) = basket.clone().liq_queue {
            if info.sender == liq_queue { let_pass = true; }
        }
        // Any user's deployment venue
        if target_position.clone().deployed_to.iter().any(|venue| venue.address == info.sender) { let_pass = true; }
        //Contract itself
        if info.sender == env.contract.address { let_pass = true; }
        if !let_pass {
            return Err(ContractError::BelowMinimumDebt { minimum: config.debt_minimum, debt: basket.clone().credit_price.get_value(target_position.credit_amount)?.to_uint_floor() });
        }
        //This would also pass for ClosePosition, but since spread is added to collateral amount this should never happen
        //Even if it does, the subsequent withdrawal would then error
    }

    //Get affiliates
    let affiliations = AFFILIATES.load(storage, position_id.to_string())
        .unwrap_or_else(|_| vec![]);

    //Set total_interest_paid
    let total_interest_paid = std::cmp::min(target_position.pending_interest, credit_asset.amount - excess_repayment);
    //Update pending interest
    target_position.pending_interest = match target_position.pending_interest.checked_sub(total_interest_paid){
        Ok(difference) => difference,
        Err(_) => Uint128::zero(),
    };
        

    //Burn repayment & send revenue to stakers & affiliates
    let burn_and_rev_msgs = credit_burn_rev_msg(
        config.clone(),
        env.clone(),
        Asset {
            amount: credit_asset.clone().amount - excess_repayment,
            ..credit_asset.clone()
        },
        &mut basket,
        affiliations.clone(),
        false
    )?;
    messages.extend(burn_and_rev_msgs);

    //Update affiliates.
    //Used during repay to reset the time affiliated & shrink the list to a single affiliate..
    update_affiliates(storage, affiliations, position_id, env.block.time.seconds())?;

    
    //To indicate removed positions during ClosePosition
    let mut removed = false;
    //Update Position
    POSITIONS.update(storage, valid_owner_addr.clone(), |positions: Option<Vec<Position>>| -> Result<Vec<Position>, ContractError> {
        let mut updating_positions = positions.unwrap_or_else(|| vec![]);

        //If new position isn't empty, update
        if !check_for_empty_position(updating_positions[position_index].clone().collateral_assets){
            updating_positions[position_index] = target_position.clone();
        } else { // remove repaying position
            updating_positions.remove(position_index);
            removed = true;
        }
        
        Ok(updating_positions)
    })?;


    //Send back excess repayment, defaults to the repaying address
    if !excess_repayment.is_zero() {
        if let Some(addr) = send_excess_to {
            let valid_addr = api.addr_validate(&addr)?;

            let msg = withdrawal_msg(Asset {
                amount: excess_repayment,
                ..basket.clone().credit_asset
            }, valid_addr )?;

            messages.push(SubMsg::new(msg));
        } else {
            let msg = withdrawal_msg(Asset {
                amount: excess_repayment,
                ..basket.clone().credit_asset
            }, info.sender )?;

            messages.push(SubMsg::new(msg));
        }                                
    }

    //Subtract paid debt from Basket
    basket.credit_asset.amount = match basket.credit_asset.amount.checked_sub(credit_asset.amount - excess_repayment){
        Ok(difference) => difference,
        Err(_err) => Uint128::zero(),
    };

    //Save updated repayment price and debts
    BASKET.save(storage, &basket)?;

    //Update CDT Supply Growth Tracker
    update_cdt_supply(storage, env.clone(), basket.credit_asset.amount)?;

    if !removed {
        //Check that state was saved correctly
        check_repay_state(
            storage,
            credit_asset.amount - excess_repayment, 
            prev_credit_amount, 
            position_id, 
            valid_owner_addr
        )?;
    }
    
    Ok(Response::new()
        .add_submessages(messages)
        .add_attributes(vec![
            attr("method", "repay"),
            attr("position_id", position_id),
            attr("pending_interest", target_position.pending_interest),
            attr("total_interest_accrued", target_position.total_interest_accrued),
            attr("loan_amount", target_position.credit_amount),
            attr("total_interest_paid", total_interest_paid),
    ]))
}

/// Asserts valid state after repay()
fn check_repay_state(
    storage: &mut dyn Storage,
    repay_amount: Uint128,
    prev_credit_amount: Uint128,
    position_id: Uint128,
    position_owner: Addr,
) -> Result<(), ContractError>{

    //Get target_position
    let (_i, target_position) = get_target_position(storage, position_owner, position_id)?;

    //If repay amount should've 0'd the position's debt and it didn't error
    if repay_amount >= prev_credit_amount && target_position.credit_amount != Uint128::zero(){ 
        return Err(ContractError::CustomError { val: String::from("Conditional 1: Possible state error") })
    } else {
        //Assert that the stored credit_amount is equal to the origin - what was repayed
        if target_position.credit_amount != prev_credit_amount - repay_amount {
            return Err(ContractError::CustomError { val: String::from("Conditional 2: Possible state error") })
        }
    }

    Ok(())
}

/// This is what the stability pool contract calls to repay for a liquidation and get its collateral distribution
// pub fn liq_repay(
//     deps: DepsMut,
//     env: Env,
//     info: MessageInfo,
//     mut credit_asset: Asset,
// ) -> Result<Response, ContractError> {
//     //Fetch liquidation info and state propagation
//     let mut liquidation_propagation = LIQUIDATION.load(deps.storage)?;    
//     let config = liquidation_propagation.clone().config;
//     let mut basket = liquidation_propagation.clone().basket;

//     //Can only be called by the SP contract
//     if config.stability_pool.is_none() || info.sender != config.clone().stability_pool.unwrap_or_else(|| Addr::unchecked("")){
//         return Err(ContractError::Unauthorized { owner: config.owner.to_string() });
//     }
//     //This position has collateral & credit_amount updated in the liquidation process...
//     // from LQ replies && fee handling
//     let mut target_position = liquidation_propagation.clone().target_position;
    
//     let mut messages: Vec<SubMsg> = vec![];
//     let mut excess_repayment = Uint128::zero();
//     //Update credit amount in target_position to account for SP's repayment
//     target_position.credit_amount = match target_position.credit_amount.checked_sub(credit_asset.amount){
//         Ok(difference) => {
//             //LQ rounding errors can cause the repay_amount to be 1e-6 off
//             if difference == Uint128::one(){
//                 Uint128::zero()
//             } else {
//                 difference
//             }
//         },
//         Err(_err) => {
//             //Send the excess repayment back to the SP
//             excess_repayment = credit_asset.amount - target_position.credit_amount;

//             let excess_repayment_msg = withdrawal_msg(
//                 Asset {
//                     amount: excess_repayment,
//                     ..basket.clone().credit_asset
//                 },
//                 config.clone().stability_pool.unwrap_or_else(|| Addr::unchecked("")),
//             )?;
//             //Update credit_asset amount so its correct for the burn
//             credit_asset.amount = target_position.credit_amount;

//             //Add msg
//             messages.push(SubMsg::new(excess_repayment_msg));

//             Uint128::zero()
//         },
//     };
    
//     //Get affiliates
//     let affiliations = AFFILIATES.load(deps.storage, target_position.position_id.to_string())?;

//     //Burn repayment & send revenue to stakers
//     let burn_and_rev_msgs = credit_burn_rev_msg(
//         config.clone(),
//         env.clone(),
//         credit_asset.clone(),
//         &mut basket,
//         affiliations.clone(),
//     )?;
//     messages.extend(burn_and_rev_msgs);

//     //Update affiliates
//     update_affiliates(deps.storage, affiliations, target_position.position_id, env.block.time.seconds())?;

//     //Subtract paid debt from Basket
//     basket.credit_asset.amount = match basket.credit_asset.amount.checked_sub(credit_asset.amount){
//         Ok(difference) => difference,
//         Err(_err) => return Err(ContractError::CustomError { val: String::from("Repay amount is greater than Basket credit amount in liq_repay") }),
//     };
   
//     //Set collateral_assets
//     let collateral_assets = target_position.clone().collateral_assets;

//     //Get position's cAsset ratios
//     let (cAsset_ratios, cAsset_prices) = (liquidation_propagation.clone().cAsset_ratios, liquidation_propagation.clone().cAsset_prices);

//     let repay_value = basket.clone().credit_price.get_value(credit_asset.amount)?;

//     //Add repay amount && user_repay_amount to total repaid
//     //This makes the assumption that if the SP liquidation is successful, the user_repay_amount was too
//     liquidation_propagation.total_repaid += Decimal::from_ratio(credit_asset.amount, Uint128::new(1u128));

//     //Error if the caller fee is more than the total repaid value
//     let repaid_value = basket.clone().credit_price.get_value(liquidation_propagation.clone().total_repaid.to_uint_floor())?;
//     if liquidation_propagation.clone().caller_fee_value_paid > repaid_value {
//         return Err(ContractError::CustomError { val: String::from("Caller fee is greater than total repaid value") });
//     }

//     //Stability Pool receives pro rata assets
//     //Add distribute messages to the message builder, so the contract knows what to do with the received funds
//     let mut distribution_assets = vec![];

//     let mut coins: Vec<Coin> = vec![];    

//     //Get SP liq fee
//     let sp_liq_fee = liquidation_propagation.sp_liq_fee;

//     //Calculate distribution of assets to send from the repaid position
//     for (num, cAsset) in collateral_assets.into_iter().enumerate() {

//         let collateral_repay_value = decimal_multiplication(repay_value, cAsset_ratios[num])?;
//         let collateral_repay_amount = cAsset_prices[num].get_amount(collateral_repay_value)?;

//         //Add fee %
//         let collateral_w_fee = min(
//             collateral_repay_amount * (sp_liq_fee+Decimal::one()),
//             target_position.collateral_assets[num].asset.amount
//         );
//         //This min accounts for rounding errors.

//         //Set distribution asset
//         let distribution_asset: Asset = Asset {
//             amount: collateral_w_fee,
//             ..cAsset.clone().asset
//         };
        
//         //Remove collateral from user's position claims
//         target_position.collateral_assets[num].asset.amount -= collateral_w_fee;
//         liquidation_propagation.liquidated_assets.push(
//             cAsset {
//                 asset: distribution_asset.clone(),
//                 ..cAsset.clone()
//             }
//         );

//         //SP Distribution needs list of cAsset's and is pulling the amount from the Asset object
//         distribution_assets.push(distribution_asset.clone());
//         coins.push(asset_to_coin(distribution_asset)?);
//     }

//     if target_position.credit_amount.is_zero(){                
//         //Remove position's assets from Supply caps 
//         update_basket_tally(
//             deps.storage, 
//             deps.querier, 
//             env.clone(), 
//             &mut basket,
//             [target_position.clone().collateral_assets, liquidation_propagation.clone().liquidated_assets].concat(),
//             target_position.clone().collateral_assets,
//             false, 
//             config.clone(),
//             true,
//         )?;
//     } else {
//         //Remove liquidated assets from Supply caps
//         update_basket_tally(
//             deps.storage, 
//             deps.querier, 
//             env.clone(), 
//             &mut basket,
//             liquidation_propagation.liquidated_assets,
//             target_position.clone().collateral_assets,
//             false,
//             config.clone(),
//             true,
//         )?;
//     }

//     //Update position
//     update_position(deps.storage, liquidation_propagation.position_owner, target_position)?;
//     //Update Basket
//     BASKET.save(deps.storage, &basket)?;

//     //Adds Native token distribution msg to messages
//     let distribution_msg = SP_ExecuteMsg::Distribute {
//         distribution_assets: distribution_assets.clone(),
//         distribution_asset_ratios: cAsset_ratios, //The distributions are based off cAsset_ratios so they shouldn't change
//         distribute_for: credit_asset.amount,
//     };
//     //Build the Execute msg w/ the full list of native tokens
//     let msg = CosmosMsg::Wasm(WasmMsg::Execute {
//         contract_addr: config.stability_pool.unwrap_or_else(|| Addr::unchecked("")).to_string(),
//         msg: to_json_binary(&distribution_msg)?,
//         funds: coins,
//     });
//     messages.push(SubMsg::new(msg));
    
//     Ok(Response::new()
//         .add_submessages(messages)
//         .add_attribute("method", "liq_repay")
//         .add_attribute("distribution_assets", format!("{:?}", distribution_assets))
//         .add_attribute("distribute_for", credit_asset.amount)
//         .add_attribute("excess", excess_repayment))
// }

//Set mint to RBLP Intent
pub fn set_intents(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    deployment_intent: DeploymentIntent,
) -> Result<Response, ContractError> {
    let mut msgs: Vec<CosmosMsg> = vec![];

    //Save user Intent to deployment venue
    //Get Target position to check ownership
    let (_, mut user_position) = get_target_position(deps.storage, info.clone().sender, deployment_intent.position_id)?;

    //Load UserIntents
    let mut user_intents = match USER_INTENTS.load(deps.storage, info.clone().sender.to_string()){
        Ok(user_intents) => user_intents,
        Err(_err) => UserDeploymentIntents {
            user: info.clone().sender.to_string(),
            deployment_intents: vec![],
        },
    };

    //if mint LTV > 1, error.
    if deployment_intent.ltv_to_mint > Decimal::one() {
        return Err(ContractError::CustomError { val: String::from("Mint LTV is above 1, maybe you forgot to add the decimal place?") })
    }


    //Add, or edit intent if position id is the same
    if let Some((index, _)) = user_intents.deployment_intents.iter().enumerate().find(|(_i, intent)| intent.position_id == deployment_intent.position_id && intent.destination == deployment_intent.destination){

        //If ltv_to_mint is 0, remove intent    
        if deployment_intent.ltv_to_mint.is_zero() {
            //Remove intent from the vector
            user_intents.deployment_intents.remove(index);
            //Find venue in user's position 
            let venue = user_position.deployed_to
                .iter()
                .enumerate()
                .find(|(_, venue)| venue.address == deployment_intent.destination);

            if let Some((index, venue)) = venue {
                //Exit vault & send to user
                msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: deployment_intent.destination.clone(),
                    msg: to_json_binary(&DeploymentVenue_ExecuteMsg::RepayUserDebt { 
                        user_info: (UserInfo {
                            position_id: deployment_intent.position_id,
                            position_owner: info.clone().sender.to_string(),
                        }), 
                        repayment: venue.deployed_debt_amount,
                    })?,
                    funds: vec![],
                }));

                //Remove venue from user's position
                user_position.deployed_to.remove(index);
                //Update active deployment venues
                set_active_deployment_venues(deps.storage, vec![StringEntry {
                    entry: deployment_intent.destination.to_string(),
                    remove: true,
                }])?;
                //Save user's position
                update_position(deps.storage, info.clone().sender, user_position)?;
            }

        } else {
            user_intents.deployment_intents[index].ltv_to_mint = deployment_intent.ltv_to_mint;
        }
    } else {
        user_intents.deployment_intents.push(deployment_intent.clone());
    }
    
    //If intent list is empty, remove from state
    if user_intents.deployment_intents.is_empty(){
        USER_INTENTS.remove(deps.storage, info.clone().sender.to_string());
    } 
    //Otherwise save
    else {
        USER_INTENTS.save(deps.storage, info.clone().sender.to_string(), &user_intents)?;
    }

    Ok(Response::new()
        .add_attribute("method", "set_intents")
        .add_attribute("deployment_intent", format!("{:?}", deployment_intent)))
}

/// Fulfill mint to Deployment Venue Intents
pub fn fulfill_intents(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    users: Vec<String>,
) -> Result<Response, ContractError> {
    let mut msgs: Vec<CosmosMsg> = vec![];
    //Load state    
    let basket: Basket = BASKET.load(deps.storage)?;
    let config: Config = CONFIG.load(deps.storage)?;

    for user in users {
        //Load intent for user
        let intents = USER_INTENTS.load(deps.storage, user.clone())?;
        // println!("intents: {:?}", intents);

        for intent in intents.deployment_intents {
            //Get target position
            let (_, mut target_position) = get_target_position(deps.storage, deps.api.addr_validate(&user.clone())?, intent.position_id)?;

            //Get LTV for the target position   
            let ((_, LTV, _), ((max_borrow_LTV, _, total_value, _, _))) = insolvency_check(
                deps.storage,
                env.clone(),
                deps.querier,
                Some(basket.clone()),
                target_position.clone().collateral_assets,
                target_position.clone().credit_amount,
                basket.clone().credit_price,
                false,
                config.clone(),
            )?;

            //Set max mint intent
            let ltv_to_mint = intent.ltv_to_mint;

            //Calc LTV space 
            let ltv_space_to_mint = match max_borrow_LTV.checked_sub(LTV){
                Ok(v) => v,
                Err(_) => return Err(ContractError::CustomError { val: String::from("LTV space to mint is greater than max borrow LTV") }),
            };

            //If the LTV is below the ltv_to_mint, create mint msg
            if ltv_space_to_mint > Decimal::zero() {
                //Get deployment venue
                let deployment_venue = match target_position.deployed_to.clone().into_iter().find(|venue| venue.address == intent.destination){
                    Some(venue) => venue,
                    None => DeploymentVenue { 
                        address: deps.api.addr_validate(&intent.destination)?, 
                        deployed_debt_amount: Uint128::zero(),
                        failed_liquidation: false,
                    },
                };
                //Calc how much ltv the deployment venue is currently using
                let debt_value_used = basket.credit_price.get_value(deployment_venue.deployed_debt_amount)?;
                let ltv_used = decimal_division(debt_value_used, total_value)?;
                ///////
                // Calc how much ltv the deployment venue has left to use
                let usable_ltv = match ltv_to_mint.checked_sub(ltv_used){
                    Ok(v) => v,
                    Err(_) => Decimal::zero(),
                };
                //Calc usable debt value
                let usable_debt_value = decimal_multiplication(total_value, usable_ltv)?;
                //Calc amount of debt to mint
                let usable_debt_amount = basket.credit_price.get_amount(usable_debt_value)?;

                // Only proceed if we need to mint more debt
                if !usable_debt_amount.is_zero() {
                    //Create increase_debt msg to this contract
                    msgs.push(
                        CosmosMsg::Wasm(WasmMsg::Execute {
                            contract_addr: env.contract.address.to_string(),
                            msg: to_json_binary(&ExecuteMsg::IncreaseDebt {
                                position_id: intent.position_id,
                                amount: Some(usable_debt_amount),
                                LTV: None, // Use amount instead of LTV since we calculated the exact amount
                                mint_to_addr: None,
                                deployment_intent: Some(intent.clone()),
                            })?,
                            funds: vec![],
                        })
                    );

                    //Add deployment venue to target position with the debt amount
                    if let Some(mut deployment_venue) = target_position.deployed_to.clone().into_iter().find(|venue| venue.address == intent.destination){
                        deployment_venue.deployed_debt_amount += usable_debt_amount;
                    } else {
                        target_position.deployed_to.push(DeploymentVenue {
                            address: deps.api.addr_validate(&intent.destination)?,
                            deployed_debt_amount: usable_debt_amount,
                            failed_liquidation: false,
                        });
                        //Update active deployment venues
                        set_active_deployment_venues(deps.storage, vec![StringEntry {
                            entry: intent.destination.to_string(),
                            remove: false,
                        }])?;
                    }

                    //Save target position
                    update_position(deps.storage, deps.api.addr_validate(&user.clone())?, target_position)?;
                }
            }
        }
    }

    //Fee to caller?
    // Not until we know the frequency of this call relative to the yield distribution.
    // We don't want the user to get ground between the liquidation (caller) fee & this auto-remint fee.
    // So as the user you still want a relatively conservative LTV to buy time for the yield distrbution to come in. Probably (e.g. 70% max_LTV = 45%)

    Ok(Response::new()
        .add_messages(msgs)
        .add_attribute("method", "fulfill_intents"))
}

/// Increase debt of a position.
/// Accrue and validate credit amount.
/// Check for insolvency & update basket debt tally.
pub fn increase_debt(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_id: Uint128,
    amount: Option<Uint128>,
    LTV: Option<Decimal>,
    mint_to_addr: Option<String>,
    // Contract uses this to mint for a user into a Deployment Venue
    deployment_intent: Option<DeploymentIntent>,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;
    let mut basket: Basket = BASKET.load(deps.storage)?;
    let mut set_redemption = false;

    //Check if frozen
    if basket.frozen { return Err(ContractError::Frozen {  }) }

    //Only the contract can send deployment_intents
    if deployment_intent.is_some() && info.sender != env.contract.address {
        return Err(ContractError::Unauthorized { owner: config.owner.to_string() });
    }

    //Set position owner
    let position_owner = if let Some(deployment_intent) = deployment_intent.clone() {
        deps.api.addr_validate(&deployment_intent.user)?
    } else {
        info.clone().sender
    };

    //Get Target position
    let (position_index, mut target_position) = get_target_position(deps.storage, position_owner.clone(), position_id)?;

    
    //If any cAsset is a rate_hike asset, force a redemption 
    for cAsset in target_position.collateral_assets.clone(){
        if cAsset.hike_rates.is_some() && cAsset.hike_rates.unwrap() {
            set_redemption = true;
        }
    }    

    //Accrue interest
    accrue(
        deps.storage,
        deps.querier,
        env.clone(),
        config.clone(),
        &mut target_position,
        &mut basket,
        info.sender.to_string(),
        false,
    )?;

    //Set prev_credit_amount
    let prev_credit_amount = target_position.credit_amount;
    let prev_basket_credit = basket.credit_asset.amount;

    //Update Supply caps if this is the first debt taken out
    if prev_credit_amount.is_zero() {
        update_basket_tally(
            deps.storage, 
            deps.querier, 
            env.clone(), 
            &mut basket, 
            target_position.collateral_assets.clone(),
            target_position.clone().collateral_assets,
            true,
            config.clone(),
            false,
        )?;
    }

    //Set amount
    let amount = match amount {
        Some(amount) => amount,
        None => {
            if let Some(LTV) = LTV {
                get_amount_from_LTV(deps.storage, deps.querier, env.clone(), config.clone(), target_position.clone(), basket.clone(), LTV)?
            } else if let Some(intent) = deployment_intent.clone() {
                //Get LTV from intent
                get_amount_from_LTV(deps.storage, deps.querier, env.clone(), config.clone(), target_position.clone(), basket.clone(), intent.ltv_to_mint)?
            } else {
                return Err(ContractError::CustomError { val: String::from("If amount isn't passed, LTV must be passed") })
            }            
        }
    };

    //Add new credit_amount
    target_position.credit_amount += amount;

    //Test for minimum debt requirements
    if  basket.clone().credit_price.get_value(target_position.credit_amount)? < Decimal::from_ratio(config.debt_minimum, Uint128::new(1u128))
    {        
        return Err(ContractError::BelowMinimumDebt { minimum: config.debt_minimum, debt: basket.clone().credit_price.get_value(target_position.credit_amount)?.to_uint_floor() });
    }

    let mut messages: Vec<CosmosMsg> = vec![];

    //Can't take credit before an oracle is set
    if basket.oracle_set {
        //If resulting LTV makes the position insolvent, error. If not construct mint msg
        let (insolvency_res, avg_LTV_res) = insolvency_check(
            deps.storage,
            env.clone(),
            deps.querier,
            Some(basket.clone()),
            target_position.clone().collateral_assets,
            target_position.credit_amount,
            basket.clone().credit_price,
            true,
            config.clone(),
        )?;

        if insolvency_res.0 {
            return Err(ContractError::PositionInsolvent { insolvency_res });
        } else {
            //Set recipient
            let recipient = {
                if let Some(mint_to) = mint_to_addr {
                    deps.api.addr_validate(&mint_to)?
                }               
                else if let Some(_) = deployment_intent.clone() {
                    //mint to the contract so it can send it to the Deployment Venue
                    env.clone().contract.address
                } else {
                    info.clone().sender
                }
            };
            //Add mint msg
            messages.push( credit_mint_msg(
                config.clone(),
                Asset {
                    amount: amount.clone(),
                    ..basket.clone().credit_asset
                },
                recipient,
            )? );
            //If intent, add Deployment Venue entry message
            if let Some(intent) = deployment_intent.clone() {
                //Create compounding purchase intents for all the positions assets
                let purchase_intents: Vec<PurchaseIntent> = target_position.collateral_assets.clone().into_iter().enumerate().map(|(index, cAsset)| PurchaseIntent {
                    desired_asset: cAsset.asset.info.to_string(),
                    route: None,
                    yield_percent: avg_LTV_res.4[index],
                    position_id: Some(intent.position_id.u128() as u64),
                    slippage: None,
                }).collect();

                messages.push( CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: intent.destination.clone(),
                    msg: to_json_binary(&DeploymentVenue_ExecuteMsg::EnterVault { 
                        leave_vault_tokens_in_vault: Some(
                            LeaveTokens {
                                percent_to_leave: Decimal::one(),
                                intent_for_tokens: RangeBoundUserIntents {
                                    user: intent.user,
                                    last_conversion_rate: Uint128::zero(),
                                    purchase_intents,
                                }
                            }
                        )
                    })?,
                    funds: vec![
                        Coin {
                            denom: basket.credit_asset.info.to_string(),
                            amount,
                        }
                    ],
                }) );
            }

            //Add credit amount to the position
            //Update Position
            POSITIONS.update(deps.storage, position_owner.clone(), |positions: Option<Vec<Position>>| -> Result<Vec<Position>, ContractError> {
                let mut updating_positions = positions.unwrap_or_else(|| vec![]);
                updating_positions[position_index] = target_position.clone();

                Ok(updating_positions)
            })?;

            //Add new debt to Basket
            basket.credit_asset.amount += amount;
            
            //Save updated repayment price and debts
            BASKET.save(deps.storage, &basket)?;
        }
    } else {
        return Err(ContractError::NoRepaymentPrice {});
    }

    //Update CDT Supply Growth Tracker
    update_cdt_supply(deps.storage, env.clone(), basket.credit_asset.amount)?;

    //Check state changes
    check_debt_increase_state(
        deps.storage, 
        amount, 
        prev_credit_amount,
        prev_basket_credit,
        position_id, 
        position_owner.clone(),
    )?;
    
    //If a rate hike asset is deposited, force a redemption
    if set_redemption {
        edit_redemption_info(
            deps.storage,
            position_owner.clone(),
            vec![position_id.clone()],
            Some(true),
            Some(1),
            Some(Decimal::one()),
            None,
            true,
        )?;
    }

    let response = Response::new()
        .add_messages(messages)
        .add_attribute("method", "increase_debt")
        .add_attribute("position_id", position_id.to_string())
        .add_attribute("total_loan", target_position.credit_amount.to_string())
        .add_attribute("increased_by", amount.to_string())
        .add_attribute("deployment_intent", format!("{:?}", deployment_intent));

    Ok(response)
}

/// Sell position collateral to fully repay debts.
/// Max spread is used to ensure the full debt is repaid in lieu of slippage.
pub fn close_position(
    deps: DepsMut, 
    env: Env,
    info: MessageInfo,
    position_id: Uint128,
    close_percentage: Option<Decimal>,
    max_spread: Decimal,
    mut send_to: Option<String>,
) -> Result<Response, ContractError>{
    //Load Config
    let config: Config = CONFIG.load(deps.storage)?;

    //Load Basket
    let basket: Basket = BASKET.load(deps.storage)?;

    //Set close_percentage
    let close_percentage = match close_percentage {
        Some(close_percentage) => min(close_percentage, Decimal::one()),
        None => Decimal::one(),
    };

    //Load target_position, restrict to owner
    let (_i, mut target_position) = get_target_position(deps.storage, info.clone().sender, position_id)?;

    //Set close_amount
    let close_amount = target_position.credit_amount * close_percentage;

    // println!("close_amount: {}", close_amount);

    //Try to repay debt from deployable venues first
    let mut credit_repay_amount = Decimal::from_ratio(close_amount, Uint128::one());
    let mut venue_submessages = vec![];
    let mut attrs = vec![];
    
    let user_repay_amount = get_deployable_venues_user_repay_amount(
        deps.storage,
        deps.querier, 
        config.clone(), 
        basket.clone(), 
        position_id, 
        info.sender.to_string(), 
        &mut target_position,
        &mut credit_repay_amount,
        &mut venue_submessages,
        &mut attrs,
    )?;

    //Update close_amount to reflect what was repaid from venues
    let remaining_close_amount = credit_repay_amount.to_uint_floor();

    //Calc collateral to sell
    //credit_amount * credit_price * (1 + max_spread)
    let total_collateral_value_to_sell = {
            decimal_multiplication(
                basket.clone().credit_price.get_value(remaining_close_amount)?, 
                (max_spread + Decimal::one())
            )?
    };

    //Max_spread is added to the collateral amount to ensure enough credit is purchased
    //Excess debt token gets sent back to the position_owner during repayment

    //Get cAsset_ratios for the target_position
    let (cAsset_ratios, cAsset_prices) = get_cAsset_ratios(deps.storage, env.clone(), deps.querier, target_position.clone().collateral_assets, config.clone(), Some(basket.clone()))?;

    let mut router_messages = vec![];
    let mut withdrawn_assets = vec![];
    // println!("credit_repay_amount: {}", credit_repay_amount);

    // println!("remaining_close_amount: {}", remaining_close_amount);

    //Only sell collateral if there's remaining debt to repay
    if !remaining_close_amount.is_zero() {
        //Calc collateral_amount_to_sell per asset & create router msg
        for (i, _collateral_ratio) in cAsset_ratios.clone().into_iter().enumerate(){

            //Calc collateral_amount_to_sell
            let mut collateral_amount_to_sell = {

                let collateral_value_to_sell = decimal_multiplication(total_collateral_value_to_sell, cAsset_ratios[i])?;

                let post_normalized_amount: Uint128 = match cAsset_prices[i].get_amount(collateral_value_to_sell){
                    Ok(amount) => amount,
                    Err(_e) => return Err(ContractError::CustomError { val: String::from("Collateral value to sell is too high to calculate an amount for due to the max spread creating an out of bounds error") })
                };

                post_normalized_amount
            };

            //Collateral to sell can't be more than the position owns
            if collateral_amount_to_sell > target_position.collateral_assets.clone()[i].asset.amount {
                collateral_amount_to_sell = target_position.collateral_assets.clone()[i].asset.amount;
            }

            //Set collateral asset
            let collateral_asset = target_position.clone().collateral_assets[i].clone().asset;

            //Add collateral_amount to list for propagation
            withdrawn_assets.push(Asset{
                amount: collateral_amount_to_sell,
                ..collateral_asset.clone()
            });

            //Create router subMsg to sell, repay in reply on success
            let router_msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.clone().chain_proxy.unwrap().to_string(),
                msg: to_json_binary(&OsmoExecuteMsg::ExecuteSwaps { 
                    token_out: basket.clone().credit_asset.info.to_string(),
                    max_slippage: max_spread,
                })?,
                funds: vec![
                    Coin {
                        denom: collateral_asset.clone().info.to_string(),
                        amount: collateral_amount_to_sell,
                    }
                ],
            });
            router_messages.push(router_msg);
        }
    }

    //Set send_to for WithdrawMsg in Reply
    if send_to.is_none() {
        send_to = Some(info.sender.to_string());
    }

    //Save CLOSE_POSITION_PROPAGATION
    CLOSE_POSITION.save(deps.storage, &ClosePositionPropagation {
        withdrawn_assets: withdrawn_assets.clone(),
        position_info: UserInfo { 
            position_id, 
            position_owner: info.sender.to_string(),
        },
        send_to,
    })?;

    //The last router message is updated to a CLOSE_POSITION_REPLY to close the position after all sales and repayments are done.
    let mut all_submessages = venue_submessages.clone(); // Add venue messages first
    
    if !router_messages.is_empty() {
        let sub_msg = SubMsg::reply_on_success(router_messages.pop().unwrap(), CLOSE_POSITION_REPLY_ID);    
        //Transform Router Msgs into SubMsgs so they run after venue repayments
        let router_submessages = router_messages.into_iter().map(|msg| SubMsg::new(msg)).collect::<Vec<SubMsg>>();
        all_submessages.extend(router_submessages);
        all_submessages.push(sub_msg);
    } else if !venue_submessages.is_empty() {
        //If no router messages but venue messages exist, add a callback to close the position
        let close_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::Callback(CallbackMsg::ClosePositionCallback {
                position_id,
                position_owner: info.sender.to_string(),
            }))?,
            funds: vec![],
        });
        all_submessages.push(SubMsg::reply_on_success(close_msg, CLOSE_POSITION_REPLY_ID));
    }

    // Create collateral rate assurance for closed position assets
    let collateral_denoms: Vec<String> = withdrawn_assets.clone().iter()
        .map(|asset| asset.info.to_string())
        .collect();
    let rate_assurance_msgs = create_collateral_rate_assurance(
        deps.storage,
        deps.querier,
        env.clone(),
        collateral_denoms,
        &basket,
    )?;

    //Combine venue repayment attributes with existing attributes
    let mut response_attrs = vec![
        attr("position_id", position_id),
        attr("user", info.sender),
        attr("venue_repay_amount", user_repay_amount.to_string()),
        attr("remaining_close_amount", remaining_close_amount.to_string()),
    ];
    response_attrs.extend(attrs);

    Ok(Response::new()
        .add_submessages(all_submessages)
        // .add_messages(rate_assurance_msgs)
        .add_attributes(response_attrs)) //If the sale incurred slippage and couldn't repay through the debt minimum, the subsequent withdraw msg will error and revert state 
}

/// Asserts valid state after increase_debt()
fn check_debt_increase_state(
    storage: &mut dyn Storage,
    increase_amount: Uint128,
    prev_credit_amount: Uint128,
    prev_basket_credit: Uint128,
    position_id: Uint128,
    position_owner: Addr,  
) -> Result<(), ContractError>{
    
    //Get target_position & Basket
    let (_i, target_position) = get_target_position(storage, position_owner, position_id)?;
    let basket = BASKET.load(storage)?;

    //Assert that credit_amount is equal to the origin + what was added
    if target_position.credit_amount != prev_credit_amount + increase_amount {
        return Err(ContractError::CustomError { val: String::from("Conditional 1: increase_debt() state error found, saved credit_amount != desired.") })
    }
    //Assert that credit_amount is equal to the origin + what was added
    if basket.credit_asset.amount != prev_basket_credit + increase_amount {
        return Err(ContractError::CustomError { val: String::from("Conditional 2: increase_debt() state error found, saved credit_amount != desired.") })
    }


    Ok(())
}

/// Edit and Enable debt token Redemption for any address-owned Positions
pub fn edit_redemption_info(
    storage: &mut dyn Storage, 
    position_owner: Addr,
    // Position IDs to edit
    mut position_ids: Vec<Uint128>,
    // Add or remove redeemability
    redeemable: Option<bool>,
    // Edit premium on the redeemed collateral.
    // Can't set a 100% premium, as that would be a free loan repayment.
    updated_premium: Option<u128>,
    // Edit Max loan repayment %
    max_loan_repayment: Option<Decimal>,    
    // Restricted collateral assets.
    // These aren't used for redemptions.
    restricted_collateral_assets: Option<Vec<String>>,
    // If called by the contract
    called_by_contract: bool,
) -> Result<Response, ContractError>{
    //Check for valid premium
    if let Some(premium) = updated_premium {
        if premium > 99u128 {
            return Err(ContractError::CustomError { val: String::from("Premium can't be greater than 99") })
        }
    }

    //Check for valid max_loan_repayment
    if let Some(max_loan_repayment) = max_loan_repayment {
        if max_loan_repayment > Decimal::one() || max_loan_repayment.is_zero() {
            return Err(ContractError::CustomError { val: String::from("Max loan repayment can't be 0% or greater than 100%") })
        }
    }

    //Position IDs must be specified & unique
    if position_ids.is_empty() {
        return Err(ContractError::CustomError { val: String::from("Position IDs must be specified") })
    } else {
        for id in position_ids.clone() {
            if position_ids.iter().filter(|&n| *n == id).count() > 1 {
                return Err(ContractError::CustomError { val: String::from("Position IDs must be unique") })
            }
        }
    }

    //If a rate hike asset is in the position, USER CAN"T REMOVE REEDMPTIONS
    for id in position_ids.clone() {
        let (_i, target_position) = get_target_position(storage, position_owner.clone(), id)?;
        for cAsset in target_position.collateral_assets.clone(){
            if !called_by_contract && cAsset.hike_rates.is_some() && cAsset.hike_rates.unwrap() {
                return Err(ContractError::CustomError { val: format!("Can't edit redemption for a position with a rate hike asset: {:?}", cAsset.asset.info) })
            }
        }
    }

    //////Additions//////
    //Add PositionRedemption objects under the user in the desired premium while skipping duplicates, if redeemable is true or None
    if (redeemable.is_some() && redeemable.unwrap_or_else(|| false)) || redeemable.is_none(){
        if let Some(updated_premium) = updated_premium {
            //Load premium we are adding to 
            match REDEMPTION_OPT_IN.load(storage, updated_premium){
                Ok(mut users_of_premium)=> {
                    //If the user already has a PositionRedemption, add the Position to the list
                    if let Some ((user_index, mut user_positions)) = users_of_premium.clone().into_iter().enumerate().find(|(_, user)| user.position_owner == position_owner){
                        //Iterate through the Position IDs
                        for id in position_ids.clone() {
                            //If the Position ID is not in the list, add it
                            if !user_positions.position_infos.iter().any(|position| position.position_id == id){
                            
                                //Get target_position
                                let target_position = match get_target_position(storage, position_owner.clone(), id){
                                    Ok((_, pos)) => pos,
                                    Err(_e) => return Err(ContractError::CustomError { val: String::from("User does not own this position id") })
                                };

                                user_positions.position_infos.push(PositionRedemption {
                                    position_id: id,
                                    remaining_loan_repayment: max_loan_repayment.unwrap_or(Decimal::one()) * target_position.credit_amount,
                                    restricted_collateral_assets: restricted_collateral_assets.clone().unwrap_or(vec![]),
                                });

                                //Remove the Position ID from the list, don't want to edit newly added RedemptionInfo
                                position_ids.retain(|&x| x != id);
                            }

                        }

                        //Update the PositionRedemption
                        users_of_premium[user_index] = user_positions;

                        //Save the updated list
                        REDEMPTION_OPT_IN.save(storage, updated_premium, &users_of_premium)?;
                    } //Add user to the premium state
                    else {
                        //Create new RedemptionInfo
                        let new_redemption_info = create_redemption_info(
                            storage,
                            position_ids.clone(), 
                            max_loan_repayment.clone(), 
                            position_owner.clone(),
                            restricted_collateral_assets.clone().unwrap_or(vec![]),
                        )?;

                        //Add the new RedemptionInfo to the list
                        users_of_premium.push(new_redemption_info);

                        //Save the updated list
                        REDEMPTION_OPT_IN.save(storage, updated_premium, &users_of_premium)?;
                    }
                },
                //If no users, create a new list
                Err(_err) => {
                    //Create new RedemptionInfo
                    let new_redemption_info = create_redemption_info(
                        storage,
                        position_ids.clone(), 
                        max_loan_repayment.clone(), 
                        position_owner.clone(),
                        restricted_collateral_assets.clone().unwrap_or(vec![]),
                    )?;

                    //Save the new RedemptionInfo
                    REDEMPTION_OPT_IN.save(storage, updated_premium, &vec![new_redemption_info])?;
                },
            };
        } else if (redeemable.is_some() && redeemable.unwrap_or_else(|| false)) && updated_premium.is_none(){
            return Err(ContractError::CustomError { val: String::from("Can't set redeemable to true without specifying a premium") })
        }
    } 

    //////Edits and Removals//////
    //Parse through premium range to look for the Position IDs
    for premium in 0..100u128 {
        //Load premium we are editing
        let mut users_of_premium: Vec<RedemptionInfo> = match REDEMPTION_OPT_IN.load(storage, premium){
            Ok(list)=> list,
            Err(_err) => vec![], //If no users, return empty vec
        };

        //Query for Users in the premium as long as there are Position IDs left to find && there are users in the premium
        if !position_ids.is_empty() && !users_of_premium.is_empty(){      
            
            //Iterate through users to find the Positions
            if let Some ((user_index, mut user_positions)) = users_of_premium.clone().into_iter().enumerate().find(|(_, user)| user.position_owner == position_owner.clone()){
                
                for id in position_ids.clone() {
                    //If the Position ID is in the list, edit, update and remove from the list
                    if let Some((position_index, _)) = user_positions.clone().position_infos.clone().into_iter().enumerate().find(|(_, position)| position.position_id == id){

                        //Edit or Remove the Position from redeemability
                        if let Some(redeemable) = redeemable {
                            if !redeemable {
                                user_positions.position_infos.remove(position_index);

                                //If the user has no more positions, remove them from the premium
                                if user_positions.position_infos.is_empty() {
                                    users_of_premium.remove(user_index);
                                    
                                    //Save the updated list
                                    REDEMPTION_OPT_IN.save(storage, premium, &users_of_premium)?;
                                    break;
                                }
                            }
                        }
                        
                        //Update maximum loan repayment
                        if let Some(max_loan_repayment) = max_loan_repayment {
                            //Get target_position
                            let target_position = match get_target_position(storage, position_owner.clone(), id){
                                Ok((_, pos)) => pos,
                                Err(_e) => return Err(ContractError::CustomError { val: String::from("User does not own this position id") })
                            };

                            user_positions.position_infos[position_index].remaining_loan_repayment = max_loan_repayment * target_position.credit_amount;
                        }

                        //To switch premiums we remove it from the list, it should've been added to its new list beforehand
                        if let Some(updated_premium) = updated_premium {  
                            if updated_premium != premium {
                                user_positions.position_infos.remove(position_index);

                                //If the user has no more positions, remove them from the premium
                                if user_positions.position_infos.is_empty() {
                                    users_of_premium.remove(user_index);
                                    
                                    //Save the updated list
                                    REDEMPTION_OPT_IN.save(storage, premium, &users_of_premium)?;
                                    break;
                                }
                            }   
                        }
                        
                        //Update restricted collateral assets
                        if let Some(restricted_assets) = restricted_collateral_assets.clone() {
                            //Map collateral assets to String
                            let basket = BASKET.load(storage)?;
                            let collateral = basket.collateral_types.iter().map(|asset| asset.asset.info.to_string()).collect::<Vec<String>>();

                            //If all restricted assets are valid, swap objects
                            if restricted_assets.iter().all(|asset| collateral.contains(asset)) {
                                user_positions.position_infos[position_index].restricted_collateral_assets = restricted_assets.clone();
                            } else {
                                return Err(ContractError::CustomError { val: String::from("Invalid restricted asset, only the position's collateral assets are viable to restrict") })
                            }
                        }

                        //Update the Position
                        users_of_premium[user_index] = user_positions.clone();

                        //Save the updated list
                        REDEMPTION_OPT_IN.save(storage, premium, &users_of_premium)?;

                        //Remove the Position ID from the list
                        position_ids = position_ids
                            .clone()
                            .into_iter()
                            .filter(|stored_id| stored_id != id)
                            .collect::<Vec<Uint128>>();
                    }
                }
            }
        }
    }


    Ok(Response::new().add_attributes(vec![
        attr("method", "edit_redemption_info"),
        attr("positions_not_edited", format!("{:?}", position_ids))
    ]))
}

fn create_redemption_info(
    storage: &dyn Storage,
    position_ids: Vec<Uint128>,
    max_loan_repayment: Option<Decimal>,
    position_owner: Addr,
    restricted_collateral_assets: Vec<String>,
) -> StdResult<RedemptionInfo>{
    //Create list of PositionRedemptions
    let mut position_infos = vec![];
    
    for id in position_ids.clone(){
        //Get target_position
        let target_position = match get_target_position(storage, position_owner.clone(), id){
            Ok((_, pos)) => pos,
            Err(_e) => return Err(StdError::GenericErr { msg: String::from("User does not own this position id") })
        };

        //Add PositionRedemption to list
        position_infos.push(PositionRedemption {
            position_id: id,
            remaining_loan_repayment: max_loan_repayment.unwrap_or(Decimal::one()) * target_position.credit_amount,
            restricted_collateral_assets: restricted_collateral_assets.clone(),
        });
    }

    Ok(RedemptionInfo { 
        position_owner, 
        position_infos 
    })
}

/// Redeem the debt token for collateral for Positions that have opted in 
/// The premium is set by the Position owner, ex: 1% premium = vault is buying CDT at 99% of the peg price
/// 
/// Affiliates are not handled during redemption. If this becomes a large sum of loss revenue, we will find a solution.
pub fn redeem_for_collateral(    
    deps: DepsMut, 
    env: Env,
    info: MessageInfo,
    max_collateral_premium: u128,
) -> Result<Response, ContractError>{
    //Load State
    let config: Config = CONFIG.load(deps.storage)?;
    let mut basket: Basket = BASKET.load(deps.storage)?;

    let mut credit_amount;
    let mut redeemable_credit = Decimal::zero();
    let mut collateral_sends: Vec<Asset> = vec![];
    let mut position_removal_ids: Vec<Uint128> = vec![];    
    let mut user_removal_addrs: Vec<Addr> = vec![];
    
    //Validate asset 
    if info.clone().funds.len() != 1 || info.clone().funds[0].denom != basket.credit_asset.info.to_string(){
        return Err(ContractError::CustomError { val: String::from("Must send only the Basket's debt token") })
    } else {
        credit_amount = Decimal::from_ratio(Uint128::from(info.clone().funds[0].amount), Uint128::one());
    }

    //Set initial credit amount
    let initial_credit_amount = credit_amount.clone();

    //Set debt minimum in amount, not value
    let debt_minimum = basket.credit_price.get_amount(Decimal::from_ratio(config.debt_minimum, Uint128::one()))?;

    //Set premium range
    for premium in 0..=max_collateral_premium {
        //Calc discount ratio
        //(100%-premium)
        let discount_ratio = decimal_subtraction(
            Decimal::one(), 
            Decimal::percent(premium as u64)
        )?;

        //Loop until all credit is redeemed
        if !credit_amount.is_zero(){
            
            //Query for Users in the premium 
            let mut users_of_premium: Vec<RedemptionInfo> = match REDEMPTION_OPT_IN.load(deps.storage, premium){
                Ok(list)=> list,
                Err(_err) => vec![], //If no users, return empty vec
            };

            //Parse thru Users
            for (user_index, mut user) in users_of_premium.clone().into_iter().enumerate() {
                //Parse thru Positions
                for (pos_rdmpt_index, position_redemption_info) in user.clone().position_infos.into_iter().enumerate() {
                    //Query for user Positions in the premium
                    let (_i, mut target_position) = match get_target_position(
                        deps.storage, 
                        user.clone().position_owner, 
                        position_redemption_info.position_id
                    ){
                        Ok(pos) => pos,
                        Err(_e) => {
                            //Add id to removal list for user
                            position_removal_ids.push(position_redemption_info.clone().position_id);
                            continue;
                        }
                    };

                    //Initialize redeemed collateral
                    let mut redeemed_collateral: Vec<cAsset> = vec![];
                    
                    //Accrue
                    // accrue(
                    //     deps.storage,
                    //     deps.querier,
                    //     env.clone(),
                    //     config.clone(),
                    //     &mut target_position,
                    //     &mut basket,
                    //     user.position_owner.to_string(),
                    //     false,
                    // )?;
                    // //Update position to save rate index changes
                    // update_position(
                    //     deps.storage, 
                    //     user.clone().position_owner, 
                    //     target_position.clone()
                    // )?;
                    
                    //Remove restricted collateral assets from target_position.collateral_assets
                    for restricted_asset in position_redemption_info.clone().restricted_collateral_assets {
                        target_position.collateral_assets = target_position.collateral_assets.clone()
                            .into_iter()
                            .filter(|asset| asset.asset.info.to_string() != restricted_asset)
                            .collect::<Vec<cAsset>>();
                    }
                    if target_position.collateral_assets.is_empty() || target_position.credit_amount.is_zero() {
                        //Add id to removal list for user
                        position_removal_ids.push(position_redemption_info.clone().position_id);
                        //Add user to removal list if no more positions
                        if user.position_infos.is_empty() {
                            user_removal_addrs.push(user.clone().position_owner);
                        }
                        continue;
                    }

                    //Get cAsset ratios
                    let (cAsset_ratios, cAsset_prices) = get_cAsset_ratios(
                        deps.storage,
                        env.clone(),
                        deps.querier,
                        target_position.clone().collateral_assets,
                        config.clone(),
                        Some(basket.clone()),
                    )?;
                    let available_credit = if target_position.credit_amount > debt_minimum {
                        target_position.credit_amount - debt_minimum
                    } else {
                        Uint128::zero()
                    };
                    //Calc amount of credit that can be redeemed.
                    //Max we can redeem is the target_position's credit_amount.
                    redeemable_credit = Decimal::min(
                        Decimal::min(
                            Decimal::from_ratio(position_redemption_info.remaining_loan_repayment, Uint128::one()),
                            Decimal::from_ratio(available_credit, Uint128::one())
                        ),
                        credit_amount
                    );

                    
                    //Subtract redeemable from credit_amount 
                    credit_amount = decimal_subtraction(credit_amount, redeemable_credit)?;
                    
                    //Calc & remove redemption fee from redeemable_credit
                    //This is done after the credit_amount subtraction to ensure excess being sent back doesn't forego the fee
                    // let redemption_fee = decimal_multiplication(
                    //     redeemable_credit, 
                    //     config.redemption_fee
                    // )?;

                    //If the remaining credit_amount is less than the redemption fee, subtract the fee from the redeemable_credit.
                    //Ex: 50 sent, 50 is redeemable, 1% fee = 0.5, 50 - 0.5 = 49.5 redeemable
                    // if credit_amount < redemption_fee {
                    //     redeemable_credit = decimal_subtraction(redeemable_credit, redemption_fee)?;
                    // } 
                    //If the remaining credit_amount can fulfill the fee, we take it from there to allow full redemptions to be made
                    //Otherwise there would always be a remainder of credit that can't be redeemed, at the size of the fee.
                    //Ex: 100 sent, 50 is redeemable, 50 remaining credit, 1% fee = 0.5, 50 > 0.5...
                    // 49.5 is sent back as excess. 50 is used to redeem, 0.5 is taken as the fee.
                    // else if credit_amount >= redemption_fee {
                    //     //If the credit_amount is greater than the fee, subtract the fee from the credit_amount
                    //     credit_amount = decimal_subtraction(credit_amount, redemption_fee)?;
                    // }


                    //Subtract redeemed debt from Basket
                    basket.credit_asset.amount = match basket.credit_asset.amount.checked_sub(redeemable_credit.to_uint_floor()) {
                        Ok(difference) => difference,
                        Err(_err) => Uint128::zero(),
                    };

                    //Subtract redeemable from remaining_loan_repayment
                    user.position_infos[pos_rdmpt_index].remaining_loan_repayment = 
                        position_redemption_info.remaining_loan_repayment - 
                        redeemable_credit.to_uint_floor();

                    /////Set and Save user info with updated remaining_loan_repayment////
                    //If remaining_loan_repayment is zero, remove PositionRedemption from user
                    if user.position_infos[pos_rdmpt_index].remaining_loan_repayment.is_zero() {
                        //Add id to removal list for user
                        position_removal_ids.push(position_redemption_info.clone().position_id);
                        //Add user to removal list if no more positions
                        if user.position_infos.is_empty() {
                            user_removal_addrs.push(user.clone().position_owner);
                        }
                    }
                    
                    // Calc credit_value
                    //redeemable_credit * credit_price
                    let credit_value =  basket.clone().credit_price.get_value(redeemable_credit.to_uint_floor())?;
                    // Calc redeemable value
                    //credit_value * discount_ratio 
                    let redeemable_value = decimal_multiplication(
                        credit_value, 
                        discount_ratio
                    )?;

                    //Calc collateral to send for each cAsset
                    for (i, cAsset) in target_position.collateral_assets.iter().enumerate() {
                        //Calc collateral to send
                        let value_to_send = decimal_multiplication(
                            redeemable_value, 
                            cAsset_ratios[i]
                        )?;
                        let collateral_to_send = cAsset_prices[i].get_amount(value_to_send)?;

                        //Add to send list
                        if let Some(asset) = collateral_sends.iter_mut().find(|a| a.info == cAsset.asset.info) {
                            asset.amount += collateral_to_send.clone();
                        } else {
                            collateral_sends.push(Asset {
                                info: cAsset.asset.info.clone(),
                                amount: collateral_to_send.clone(),
                            });
                        }

                        //Add to redeemed_collateral
                        redeemed_collateral.push(
                            cAsset {
                                asset: Asset {
                                    amount: collateral_to_send.clone(),
                                    ..cAsset.asset.clone()
                                },
                                ..cAsset.clone()
                            }
                        );
                        
                        //Update Position totals
                        update_position_claims(
                            deps.storage, 
                            deps.querier, 
                            env.clone(), 
                            config.clone(),
                            position_redemption_info.position_id, 
                            user.clone().position_owner, 
                            cAsset.asset.info.clone(), 
                            collateral_to_send
                        )?;

                    }

                    //Reload target_position
                    let (_i, mut new_target_position) = get_target_position(
                        deps.storage, 
                        user.clone().position_owner, 
                        position_redemption_info.position_id
                    )?;
                    //This allows us to transfer the accrued interest to the position currently in state
                    new_target_position.credit_amount = target_position.credit_amount;

                    //Set position.credit_amount
                    new_target_position.credit_amount -= redeemable_credit.to_uint_floor();

                    //Remove from redemption_info if credit_amount is zero
                    if new_target_position.credit_amount.is_zero() {
                        //Add id to removal list for user
                        position_removal_ids.push(position_redemption_info.clone().position_id);
                        //Add user to removal list if no more positions
                        if user.position_infos.is_empty() {
                            user_removal_addrs.push(user.clone().position_owner);
                        }
                    }

                    //Update position.credit_amount
                    update_position(
                        deps.storage, 
                        user.clone().position_owner, 
                        new_target_position.clone()
                    )?;


                    //Update basket tally
                    update_basket_tally(
                        deps.storage, 
                        deps.querier,
                        env.clone(), 
                        &mut basket, 
                        redeemed_collateral.clone(),
                        target_position.clone().collateral_assets,
                        false, 
                        config.clone(), 
                        false
                    )?;
                }

                //Remove positions from user now that we're post loop
                user.position_infos = user.clone().position_infos
                    .into_iter()
                    .filter(|pos| !position_removal_ids.contains(&pos.position_id))
                    .collect::<Vec<PositionRedemption>>();

                //Update user
                if !users_of_premium.is_empty() && users_of_premium.len() > user_index {
                    users_of_premium[user_index] = user.clone();
                }

                //Add user to removal list if no more positions
                if user.position_infos.is_empty() {
                    user_removal_addrs.push(user.clone().position_owner);
                }
            }
            //Remove users from premium now that we're post loop
            users_of_premium = users_of_premium
                .into_iter()
                .filter(|user| !user_removal_addrs.contains(&user.position_owner))
                .collect::<Vec<RedemptionInfo>>();

            
            REDEMPTION_OPT_IN.save(deps.storage, premium, &users_of_premium)?;
        }
    }

    if credit_amount == initial_credit_amount {
        return Err(ContractError::CustomError { val: String::from("No collateral to redeem with at this max premium") })
    }

    //Convert collateral_sends to coins
    let mut coins: Vec<Coin> = vec![];
    for asset in collateral_sends {
        coins.push(asset_to_coin(asset)?)
    }

    let mut messages: Vec<SubMsg> = vec![];
    //Send collateral to sender
    let collateral_msg: CosmosMsg = BankMsg::Send {
        to_address: info.clone().sender.to_string(),
        amount: coins.clone(),
    }.into();
    messages.push(SubMsg::new(collateral_msg));

    //Burn redeemed credit
    if config.chain_proxy.is_some() {
        //Act if a redemption was made
        if !redeemable_credit.to_uint_floor().is_zero() {            
            //Create rev/burn msgs
            let burn_and_rev_msgs = credit_burn_rev_msg(
                config.clone(),
                env.clone(),
                Asset {
                    amount: redeemable_credit.to_uint_floor(),
                    ..basket.credit_asset.clone()
                },
                &mut basket,
                vec![],
                true,
                
            )?;
            messages.extend(burn_and_rev_msgs);
        }
    }
    
    //Save updated Basket
    BASKET.save(deps.storage, &basket)?;

    // Create collateral rate assurance for redeemed assets
    let collateral_denoms: Vec<String> = coins.iter()
        .map(|coin| coin.denom.clone())
        .collect();
    let rate_assurance_msgs = create_collateral_rate_assurance(
        deps.storage,
        deps.querier,
        env.clone(),
        collateral_denoms,
        &basket,
    )?;

    //If there is excess credit, send it back to sender
    if !credit_amount.is_zero() {
        let credit_msg: CosmosMsg = BankMsg::Send {
            to_address: info.clone().sender.to_string(),
            amount: vec![Coin {
                denom: basket.credit_asset.info.to_string(),
                amount: credit_amount.to_uint_floor(),
            }],
        }.into();
        messages.push(SubMsg::new(credit_msg));


        return Ok(Response::new()
            .add_submessages(messages)
            .add_messages(rate_assurance_msgs)
            .add_attributes(vec![
                attr("action", "redeem_for_collateral"),
                attr("sender", info.clone().sender),
                attr("redeemed_collateral", format!("{:?}", coins)),
                attr("excess_credit", format!("{:?}", credit_amount)),
            ])
        )
    }

    //Response
    Ok(Response::new()
        .add_submessages(messages)
        .add_messages(rate_assurance_msgs)
        .add_attributes(vec![
        attr("action", "redeem_for_collateral"),
        attr("sender", info.clone().sender),
        attr("redeemed_collateral", format!("{:?}", coins)),
        ])
    )
}

/// Create the contract's Basket.
/// Validates params.
 pub fn create_basket(
    deps: DepsMut,
    info: MessageInfo,
    env: Env,
    basket_id: Uint128,
    collateral_types: Vec<cAsset>,
    credit_asset: Asset,
    credit_price: Decimal,
    base_interest_rate: Option<Decimal>,
    credit_pool_infos: Vec<PoolType>,
    liq_queue: Option<String>,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    //Only contract owner can create new baskets. This will likely be governance.
    if info.sender != config.owner {
        return Err(ContractError::NotContractOwner {});
    }
    //One basket per contract
    if let Ok(_basket) = BASKET.load(deps.storage){
        return Err(ContractError::CustomError { val: String::from("Only one basket per contract") })
    }

    let mut new_assets = collateral_types.clone();
    let mut collateral_supply_caps = vec![];
    let mut msgs: Vec<CosmosMsg> = vec![];

    let mut new_liq_queue: Option<Addr> = None;
    if liq_queue.is_some() {
        new_liq_queue = Some(deps.api.addr_validate(&liq_queue.clone().unwrap_or_else(|| String::from("")))?);
    }

    //Minimum viable cAsset parameters
    for (i, asset) in collateral_types.iter().enumerate() {
        new_assets[i].asset.amount = Uint128::zero();
        new_assets[i].rate_index = Decimal::one();

        if asset.max_borrow_LTV >= asset.max_LTV
            && asset.max_borrow_LTV
                >= Decimal::from_ratio(Uint128::new(100u128), Uint128::new(1u128))
        {
            return Err(ContractError::CustomError {
                val: String::from("Max borrow LTV can't be greater or equal to max_LTV nor equal to 100"),
            });
        }

        //No LPs initially. Their pool asset's need to already be added as collateral so they can't come first.
        if asset.pool_info.is_some() {
            return Err(ContractError::CustomError {
                val: String::from("Can't add an LP when creating a basket"),
            });
        } else {
            //Asserting the Collateral Asset has an oracle
            if config.clone().oracle_contract.is_some() {
                //Query Asset Oracle
                deps.querier
                    .query::<Vec<AssetResponse>>(&QueryRequest::Wasm(WasmQuery::Smart {
                        contract_addr: config.clone().oracle_contract.unwrap_or_else(|| Addr::unchecked("")).to_string(),
                        msg: to_json_binary(&OracleQueryMsg::Assets {
                            asset_infos: vec![asset.clone().asset.info],
                        })?,
                    }))?;

                //If it errors it means the oracle doesn't exist
            } else {
                return Err(ContractError::CustomError {
                    val: String::from("Need to setup oracle contract before adding assets"),
                });
            }

            //Create Liquidation Queue for basket assets
            if new_liq_queue.clone().is_some() {
                //Gets Liquidation Queue max premium.
                //The premium has to be at most 5% less than the difference between max_LTV and 100%
                //The ideal variable for the 5% is the avg caller_liq_fee during high traffic periods
                let max_premium = match Uint128::new(95u128).checked_sub( asset.max_LTV * Uint128::new(100u128) ){
                    Ok( diff ) => diff,
                    //A default to 10 assuming that will be the highest sp_liq_fee
                    Err( _err ) => Uint128::new(10u128),
                };
                //We rather the LQ liquidate than the SP if possible so its max_premium will be at most the sp_liq fee...
                //..if the first subtraction fails.
                //If it failed, allowing the LQ premium to be more than the SP fee means less efficient liquidations..
                //Since we are aiming for lowest possible fee

                msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: new_liq_queue.clone().unwrap_or_else(|| Addr::unchecked("")).to_string(),
                    msg: to_json_binary(&LQ_ExecuteMsg::AddQueue {
                        bid_for: asset.clone().asset.info,
                        max_premium,
                        //Bid total before bids go to the waiting queue. 
                        // The cumulative threshold of frequented slots should be larger than the largest single liquidation amount to prevent waiting bids from causing InsufficientBids errors.
                        bid_threshold: Uint256::from(1_000_000_000_000u128), //1 million
                    })?,
                    funds: vec![],
                }));
            }
        }

        let mut lp = false;
        if asset.pool_info.is_some() {
            lp = true;
        }
        //Push the cAsset's asset info
        collateral_supply_caps.push(SupplyCap {
            asset_info: asset.clone().asset.info,
            current_supply: Uint128::zero(),
            supply_cap_ratio: Decimal::zero(),
            debt_total: Uint128::zero(),
            lp,
            stability_pool_ratio_for_debt_cap: None,
        });
    }

    //Set Basket fields
    let base_interest_rate = base_interest_rate.unwrap_or(Decimal::zero());

    //Initialize individual_cost for each asset only if it already exists
    for (i, asset) in collateral_types.iter().enumerate() {
        if let Some(ref mut cost) = new_assets[i].individual_cost {
            // If individual_cost exists, initialize the rate
            let initial_rate = if config.rate_hike_rate.is_some() && asset.hike_rates.is_some() && asset.hike_rates.unwrap() {
                config.rate_hike_rate.unwrap()
            } else {
                decimal_multiplication(
                    base_interest_rate,
                    decimal_division(Decimal::one(), asset.max_LTV)?,
                )?
            };
            cost.rate = initial_rate;
        }
        // Otherwise leave it as None
    }

    let new_basket: Basket = Basket {
        basket_id,
        current_position_id: Uint128::from(1u128),
        collateral_types: new_assets,
        collateral_supply_caps,
        lastest_collateral_rates: vec![], //This will be set in the accrue function
        multi_asset_supply_caps: vec![],
        credit_asset: credit_asset.clone(),
        credit_price: PriceResponse {
            price: credit_price,
            prices: vec![],
            decimals: 6,
        },
        base_interest_rate,
        pending_revenue: PendingRevenue {
            total_pending: Uint128::zero(),
            per_asset_rev: vec![],
        },
        pending_bad_debt: Uint128::zero(),
        credit_last_accrued: env.block.time.seconds(),
        rates_last_accrued: env.block.time.seconds(),
        liq_queue: new_liq_queue,
        negative_rates: true,
        cpc_margin_of_error: Decimal::one(),
        oracle_set: false,
        frozen: false,
        distribute_revenue: true,
    };

    //Denom check
    if let AssetInfo::Token { address :_} = credit_asset.info {
        return Err(ContractError::CustomError {
            val: String::from("Basket credit must be a native token denom"),
        });
    }

    //Add asset to liquidity check contract
    //Liquidity AddAsset Msg
    if let Some(liquidity_contract) = config.liquidity_contract {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: liquidity_contract.to_string(),
            msg: to_json_binary(&LiquidityExecuteMsg::AddAsset {
                asset: LiquidityInfo {
                    asset: new_basket.clone().credit_asset.info,
                    pool_infos: credit_pool_infos,
                },
            })?,
            funds: vec![],
        }));
    }

    //Save Basket
    BASKET.save( deps.storage, &new_basket )?;

    //Response Building
    let response = Response::new();

    Ok(response
        .add_attributes(vec![
            attr("method", "create_basket"),
            attr("basket_id", basket_id),
            attr("credit_asset", credit_asset.to_string()),
            attr("credit_price", credit_price.to_string()),
            attr(
                "liq_queue",
                liq_queue.unwrap_or_else(|| String::from("None")),
            ),
        ])
        .add_messages(msgs))
} 

/// Edit the contract's Basket.
/// Can't edit basket id, current_position_id or credit_asset.
/// Credit price can only be changed thru the accrue function.
/// Validates parameters and updates the basket.
pub fn edit_basket(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    editable_parameters: EditBasket,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    if info.sender != config.owner {
        return Err(ContractError::Unauthorized { owner: config.owner.to_string() });
    }

    let mut new_queue: Option<Addr> = None;
    if let Some(liq_queue) = editable_parameters.clone().liq_queue {
        new_queue = Some(deps.api.addr_validate(&liq_queue)?);
    }

    //Blank cAsset
    //This never gets added unless its edited. Here due to uninitialized errors.
    let mut new_cAsset = cAsset {
        asset: Asset {
            info: AssetInfo::NativeToken {
                denom: String::from("None"),
            },
            amount: Uint128::zero(),
        },
        max_borrow_LTV: Decimal::zero(),
        max_LTV: Decimal::zero(),
        pool_info: None,
        rate_index: Decimal::one(),
        hike_rates: Some(false),
        individual_cost: Some(IndividualCost {
            rate: Decimal::zero(),
            updater_address: None,
        }),
    };

    let mut msgs: Vec<CosmosMsg> = vec![];    
    let mut attrs = vec![attr("method", "edit_basket")];

    let mut basket = BASKET.load(deps.storage)?;
    //cAsset check
    if let Some(added_cAsset) = editable_parameters.clone().added_cAsset {
        let mut check = true;
        new_cAsset = added_cAsset.clone();

        //new_cAsset can't be the basket credit_asset or MBRN 
        if let Some(staking_contract) = config.clone().staking_contract {
            let mbrn_denom = deps.querier.query::<Staking_Config>(&QueryRequest::Wasm(WasmQuery::Smart { 
                contract_addr: staking_contract.to_string(), 
                msg: to_json_binary(&Staking_QueryMsg::Config { })? 
            }))?
            .mbrn_denom;

            if new_cAsset.asset.info.to_string() == mbrn_denom {
                return Err(ContractError::InvalidCollateral {  } )
            }
        }
        if new_cAsset.asset.info == basket.clone().credit_asset.info {
            return Err(ContractError::InvalidCollateral {  } )
        }
        ////
        
        //Each cAsset has to initialize amount as 0..
        new_cAsset.asset.amount = Uint128::zero();
        
        //..and index at 1
        new_cAsset.rate_index = Decimal::one();

        //Initialize individual_cost with base formula: base_interest_rate * (1/max_LTV)
        let initial_rate = if config.rate_hike_rate.is_some() && new_cAsset.hike_rates.is_some() && new_cAsset.hike_rates.unwrap() {
            config.rate_hike_rate.unwrap()
        } else {
            decimal_multiplication(
                basket.base_interest_rate,
                decimal_division(Decimal::one(), new_cAsset.max_LTV)?,
            )?
        };
        
        //Initialize individual_cost if not already set
        if new_cAsset.individual_cost.is_none() {
            // No individual_cost set, so we don't initialize it
            new_cAsset.individual_cost = None;
        } else if let Some(ref mut cost) = new_cAsset.individual_cost {
            // If individual_cost exists, initialize the rate
            cost.rate = initial_rate;
        }

        //No duplicates
        if let Some(_duplicate) = basket
            .clone()
            .collateral_types
            .into_iter()
            .find(|cAsset| cAsset.asset.info.equal(&new_cAsset.asset.info))
        {
            return Err(ContractError::CustomError {
                val: String::from("Attempting to add duplicate asset"),
            });
        }

        //Specific logic for direct adds of Osmosis xyk LPs (Maybe we reuse for Astroport LPs)
        if let Some(mut pool_info) = added_cAsset.pool_info {

            //Query share asset amount
            let pool_state = match deps.querier.query::<PoolStateResponse>(&QueryRequest::Wasm(
                WasmQuery::Smart {
                    contract_addr: config.clone().chain_proxy.unwrap_or_else(|| Addr::unchecked("")).to_string(),
                    msg: match to_json_binary(&OsmoQueryMsg::PoolState {
                        id: pool_info.pool_id,
                    }) {
                        Ok(binary) => binary,
                        Err(err) => {
                            return Err(ContractError::CustomError {
                                val: err.to_string(),
                            })
                        }
                    },
                },
            )) {
                Ok(resp) => resp,
                Err(err) => {
                    return Err(ContractError::CustomError {
                        val: err.to_string(),
                    })
                }
            };
            let pool_assets = pool_state.assets;

            //Set correct shares denom
            new_cAsset.asset.info = AssetInfo::NativeToken {
                denom: pool_state.shares.denom,
            };

            //Set pool_assets in PoolInfo object
            //Assert pool_assets are already in the basket, which confirms an oracle and adequate parameters for them
            for (i, asset) in pool_assets.iter().enumerate() {

                //Set pool assets 
                pool_info.asset_infos[i].info = AssetInfo::NativeToken { denom: asset.clone().denom };               
               
                //Asserting that its pool assets are already added as collateral types
                if !basket.clone().collateral_types.into_iter().any(|cAsset| {
                    cAsset.asset.info.equal(&AssetInfo::NativeToken {
                        denom: asset.clone().denom,
                    })
                }){
                    return Err(ContractError::CustomError {
                        val: String::from("Need to add all pool assets before adding the LP"),
                    });
                }
            }

            //Update pool_info
            new_cAsset.pool_info = Some(pool_info.clone());

            //Add share_token to the oracle
            msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.clone().oracle_contract.unwrap_or_else(|| Addr::unchecked("")).to_string(),
                msg: to_json_binary(&OracleExecuteMsg::AddAsset { 
                    asset_info: new_cAsset.clone().asset.info,
                    oracle_info: AssetOracleInfo { 
                        basket_id: Uint128::one(), 
                        pools_for_osmo_twap: vec![],
                        is_usd_par: false,
                        lp_pool_info: Some(
                            PoolInfo { 
                                pool_id: pool_info.pool_id,
                                asset_infos: vec![
                                    LPAssetInfo { 
                                        info: AssetInfo::NativeToken { denom: pool_assets[0].clone().denom  }, 
                                        decimals: 6, 
                                        ratio: Decimal::percent(50),
                                    },
                                    LPAssetInfo { 
                                        info: AssetInfo::NativeToken { denom: pool_assets[1].clone().denom  }, 
                                        decimals: 6, 
                                        ratio: Decimal::percent(50),
                                    },
                                ],
                            }
                        ),                       
                        decimals: 18,
                        pyth_price_feed_id: None,
                        vault_info: None,
                    },
                })?,
                funds: vec![],
            }));

        } else {
            //Asserting the Collateral Asset has an oracle
            if config.oracle_contract.is_some() {
                //Query Asset Oracle
                deps.querier
                    .query::<Vec<AssetResponse>>(&QueryRequest::Wasm(WasmQuery::Smart {
                        contract_addr: config.clone().oracle_contract.unwrap_or_else(|| Addr::unchecked("")).to_string(),
                        msg: to_json_binary(&OracleQueryMsg::Assets {
                            asset_infos: vec![new_cAsset.clone().asset.info],
                        })?,
                    }))?;

                //If it errors it means the oracle doesn't exist
            } else {
                return Err(ContractError::CustomError {
                    val: String::from("Need to setup oracle contract before adding assets"),
                });
            }
        }        

        //Create Liquidation Queue for the asset
        if basket.clone().liq_queue.is_some() {
            //Gets Liquidation Queue max premium.
            //The premium has to be at most 5% less than the difference between max_LTV and 100%
            //The ideal variable for the 5% is the avg caller_liq_fee during high traffic periods
            let max_premium = match Uint128::new(95u128).checked_sub( new_cAsset.max_LTV * Uint128::new(100u128) ){
                Ok( diff ) => diff,
                //A default to 10 assuming that will be the highest sp_liq_fee
                Err( _err ) => Uint128::new(10u128),
            };

            msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: basket.clone().liq_queue.unwrap_or_else(|| Addr::unchecked("")).into_string(),
                msg: to_json_binary(&LQ_ExecuteMsg::AddQueue {
                    bid_for: new_cAsset.clone().asset.info,
                    max_premium,
                    //Bid total before bids go to the waiting queue. 
                    //Threshold should be larger than the largest single liquidation amount to prevent waiting bids from causing InsufficientBids errors.
                    bid_threshold: Uint256::from(1_000_000_000_000u128), //1 million
                })?,
                funds: vec![],
            }));
        } else if let Some(new_queue) = new_queue.clone() {
            //Gets Liquidation Queue max premium.
            //The premium has to be at most 5% less than the difference between max_LTV and 100%
            //The ideal variable for the 5% is the avg caller_liq_fee during high traffic periods
            let max_premium = match Uint128::new(95u128).checked_sub( new_cAsset.max_LTV * Uint128::new(100u128) ){
                Ok( diff ) => diff,
                //A default to 10 assuming that will be the highest sp_liq_fee
                Err( _err ) => Uint128::new(10u128) 
                ,
            };

            msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: new_queue.into_string(),
                msg: to_json_binary(&LQ_ExecuteMsg::AddQueue {
                    bid_for: new_cAsset.clone().asset.info,
                    max_premium,
                    //Bid total before bids go to the waiting queue. 
                    //Threshold should be larger than the largest single liquidation amount to prevent waiting bids from causing InsufficientBids errors.
                    bid_threshold: Uint256::from(1_000_000_000_000u128), //1 million
                })?,
                funds: vec![],
            }));
        }

        //Create LTV Disco queue for the asset
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.ltv_disco.to_string(),
            msg: to_json_binary(&LTVDiscoExecuteMsg::CreateQueue {
                asset: new_cAsset.clone().asset.info.to_string(),
            })?,
            funds: vec![],
        }));

        //..needs minimum viable LTV parameters
        if new_cAsset.max_borrow_LTV >= new_cAsset.max_LTV
            || new_cAsset.max_borrow_LTV
                >= Decimal::from_ratio(Uint128::new(100u128), Uint128::new(1u128))
        {
            check = false;
        }

        if !check {
            return Err(ContractError::CustomError {
                val: "Max borrow LTV can't be greater or equal to max_LTV nor equal to 100"
                    .to_string(),
            });
        }

        let mut lp = false;
        if new_cAsset.pool_info.is_some() {
            lp = true;
        }
        //Push the cAsset's asset info
        basket.collateral_supply_caps.push(SupplyCap {
            asset_info: new_cAsset.clone().asset.info,
            current_supply: Uint128::zero(),
            supply_cap_ratio: Decimal::zero(),
            debt_total: Uint128::zero(),
            lp,
            stability_pool_ratio_for_debt_cap: None,
        });

        //Create Volatility Index for the asset
        VOLATILITY.save(deps.storage, new_cAsset.clone().asset.info.to_string(), &CollateralVolatility {
            index: Decimal::one(),
            volatility_list: vec![],
        })?;
    }
    
    //Save basket's new collateral_supply_caps
    BASKET.save(deps.storage, &basket)?;

    //Send credit_asset TWAP info to Oracle Contract
    let mut oracle_set = basket.oracle_set;

    if let Some(credit_twap) = editable_parameters.clone().credit_asset_twap_price_source {
        if let Some(oracle_contract) = config.clone().oracle_contract {
            //Set the credit Oracle. Using EditAsset updates or adds.
            msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: oracle_contract.to_string(),
                msg: to_json_binary(&OracleExecuteMsg::EditAsset {
                    asset_info: basket.clone().credit_asset.info,
                    oracle_info: Some(AssetOracleInfo {
                        basket_id: basket.clone().basket_id,
                        pools_for_osmo_twap: vec![credit_twap],
                        is_usd_par: false,
                        lp_pool_info: None,
                        decimals: 6,
                        pyth_price_feed_id: None,
                        vault_info: None,
                    }),
                    remove: false,
                })?,
                funds: vec![],
            }));

            oracle_set = true;
        }
    };

    //Add pool_infos to the Liquidity contract
    if let Some(pool_infos) = editable_parameters.clone().credit_pool_infos {
        attrs.push(attr("new_pool_infos", format!("{:?}", pool_infos)));

        if let Some(liquidity_contract) = config.liquidity_contract {
            msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: liquidity_contract.to_string(),
                msg: to_json_binary(&LiquidityExecuteMsg::EditAsset {
                    asset: LiquidityInfo {
                        asset: basket.clone().credit_asset.info,
                        pool_infos,
                    },
                })?,
                funds: vec![],
            }));
        }
    }

    //If updating frozen, set timer
    if let Some(frozen) = editable_parameters.clone().frozen {
        let mut timer = match FREEZE_TIMER.load(deps.storage){
            Ok(timer) => timer,
            Err(_err) => Timer {
                start_time: 0,
                end_time: 0,
            },
        };

        if frozen && !basket.frozen{
            //If we are freezing, set start timer
            timer.start_time = env.block.time.seconds();
            
            //Save timer
            FREEZE_TIMER.save(deps.storage, &timer)?;
        } else  if !frozen && basket.frozen {
            //If we are unfreezing, set end timer
            timer.end_time = env.block.time.seconds();

            //Save timer
            FREEZE_TIMER.save(deps.storage, &timer)?;
        }
    }
    //Reset the Volatility Index for any edited supply caps
    if let Some(caps) = editable_parameters.clone().collateral_supply_caps {
        for cap in caps {
            VOLATILITY.update(deps.storage, cap.asset_info.to_string(), |mut vol| -> StdResult<CollateralVolatility> {
                match vol {
                    Some(mut vol) => {
                        vol.index = Decimal::one();
                        Ok(vol)
                    },
                    None => {
                        let mut vol = CollateralVolatility {
                            index: Decimal::one(),
                            volatility_list: vec![],
                        };
                        Ok(vol)
                    }
                }
            })?;
        }
    }
    if let Some(caps) = editable_parameters.clone().multi_asset_supply_caps {
        for cap in caps {
            for asset in cap.assets {
                VOLATILITY.update(deps.storage, asset.to_string(), |mut vol| -> StdResult<CollateralVolatility> {
                    match vol {
                        Some(mut vol) => {
                            vol.index = Decimal::one();
                            Ok(vol)
                        },
                        None => {
                            let mut vol = CollateralVolatility {
                                index: Decimal::one(),
                                volatility_list: vec![],
                            };
                            Ok(vol)
                        }
                    }
                })?;
            }
        }
    }

    //Handle individual_costs updates
    if let Some(individual_costs) = editable_parameters.clone().individual_costs {
        for (asset_string, new_rate) in individual_costs {
            // Find the asset in basket
            if let Some((_index, c_asset)) = basket.collateral_types.iter_mut().enumerate()
                .find(|(_, asset)| asset.asset.info.to_string() == asset_string) {
                // Check authority
                if let Some(ref cost) = c_asset.individual_cost {
                    if let Some(ref updater_addr) = cost.updater_address {
                        // Updater address is set, must match sender
                        if info.sender != *updater_addr {
                            return Err(ContractError::Unauthorized { owner: updater_addr.to_string() });
                        }
                    } else {
                        // No updater_address set, only owner can update
                        if info.sender != config.owner {
                            return Err(ContractError::Unauthorized { owner: config.owner.to_string() });
                        }
                    }
                } else {
                    // No individual_cost set, only owner can update
                    if info.sender != config.owner {
                        return Err(ContractError::Unauthorized { owner: config.owner.to_string() });
                    }
                }
                
                // Update or create the individual_cost
                if let Some(ref mut cost) = c_asset.individual_cost {
                    cost.rate = new_rate;
                } else {
                    // Create new individual_cost with the rate
                    c_asset.individual_cost = Some(IndividualCost {
                        rate: new_rate,
                        updater_address: None,
                    });
                }
            } else {
                return Err(ContractError::CustomError { 
                    val: format!("Asset {} not found in basket", asset_string) 
                });
            }
        }
        // Save updated basket
        BASKET.save(deps.storage, &basket)?;
    }

    //Handle individual_cost_updaters updates
    if let Some(individual_cost_updaters) = editable_parameters.clone().individual_cost_updaters {
        // Only owner can set or change updater addresses
        if info.sender != config.owner {
            return Err(ContractError::Unauthorized { owner: config.owner.to_string() });
        }
        
        for (asset_string, new_updater_address) in individual_cost_updaters {
            // Find the asset in basket
            if let Some((_index, c_asset)) = basket.collateral_types.iter_mut().enumerate()
                .find(|(_, asset)| asset.asset.info.to_string() == asset_string) {
                
                // Update the updater_address
                c_asset.individual_cost = match (new_updater_address, c_asset.individual_cost.clone()) {
                    (Some(addr_string), Some(mut existing_cost)) => {
                        // Update existing individual_cost with new updater address
                        existing_cost.updater_address = Some(deps.api.addr_validate(&addr_string)?);
                        Some(existing_cost)
                    },
                    (Some(addr_string), None) => {
                        // Create new individual_cost
                        Some(IndividualCost {
                            rate: Decimal::zero(),
                            updater_address: Some(deps.api.addr_validate(&addr_string)?),
                        })
                    },
                    (None, Some(existing_cost)) => {
                        // Clear the updater_address but keep the cost with the rate
                        Some(IndividualCost {
                            rate: existing_cost.rate,
                            updater_address: None,
                        })
                    },
                    (None, None) => None, // No change
                };
            } else {
                return Err(ContractError::CustomError { 
                    val: format!("Asset {} not found in basket", asset_string) 
                });
            }
        }
        // Save updated basket
        BASKET.save(deps.storage, &basket)?;
    }

    //Update Basket
    BASKET.update(deps.storage, |mut basket| -> Result<Basket, ContractError> {
        //Set all optional parameters
        editable_parameters.edit_basket(&mut basket, new_cAsset, new_queue, oracle_set)?;        

        Ok(basket)
    })?;
    attrs.push(attr("updated_basket", format!("{:?}", basket.clone())));

    //Return Response
    Ok(Response::new().add_attributes(attrs).add_messages(msgs))
}


/// Calculate desired amount of credit to borrow to reach target LTV
pub fn get_amount_from_LTV(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: Config,
    position: Position,
    basket: Basket,
    target_LTV: Decimal,
) -> Result<Uint128, ContractError>{
    //Get avg_borrow_LTV & total_value
    let (avg_borrow_LTV, _avg_max_LTV, total_value, _cAsset_prices, _cAsset_ratios) = get_avg_LTV(
        storage, 
        env, 
        querier, 
        config, 
        Some(basket.clone()),
        position.clone().collateral_assets,
        false,
    )?;

    //Target LTV can't be greater than possible borrowable LTV for the Position
    if target_LTV > avg_borrow_LTV {
        return Err(ContractError::InvalidLTV { target_LTV })
    }

    //Calc current LTV
    let current_LTV = {
        let credit_value = basket.clone().credit_price.get_value(position.credit_amount)?;

        decimal_division(credit_value, total_value)?
    };

    //If target_LTV is <= current_LTV there is no room to increase
    if target_LTV <= current_LTV {
        return Err(ContractError::InvalidLTV { target_LTV })
    }

    //Calculate amount of credit to get to target_LTV
    let credit_amount: Uint128 = {        
        //Calc spread between current LTV and target_LTV
        let LTV_spread = target_LTV - current_LTV;

        //Calc the value LTV_spread represents
        let increased_credit_value = decimal_multiplication(total_value, LTV_spread)?;
        
        //Calc the amount of credit needed to reach the increased_credit_value
        basket.credit_price.get_amount(increased_credit_value)?
    };

    Ok( credit_amount )
}


/// Checks if any Basket caps are set to 0.
/// If so the withdrawal assets have to either fully withdraw the asset from the position or only withdraw said asset.
/// Otherwise users could just fully withdrawal other assets and create a new position.
/// In a LUNA situation this would leave debt backed by an asset whose solvency Membrane has no faith in.
fn check_for_expunged(
    position_assets: Vec<cAsset>,
    withdrawal_assets: Vec<cAsset>,
    basket: Basket
)-> StdResult<()>{
    //Extract the Asset from the cAssets
    let position_assets: Vec<Asset> = position_assets
        .into_iter()
        .map(|cAsset| cAsset.asset)
        .collect::<Vec<Asset>>();

    let withdrawal_assets: Vec<Asset> = withdrawal_assets
        .into_iter()
        .map(|cAsset| cAsset.asset)
        .collect::<Vec<Asset>>();

    let mut passed = true;
    let mut invalid_withdraws = vec![];

    //Does the position have expunged assets?
    //If so, the withdrawal assets have to either fully withdraw the asset from the position or only withdraw expunged assets.


    // Step 1: Gather all expunged assets (supply cap == 0)
    let expunged_assets: Vec<AssetInfo> = basket.collateral_supply_caps
        .iter()
        .filter(|cap| cap.supply_cap_ratio.is_zero())
        .map(|cap| cap.asset_info.clone())
        .collect::<Vec<AssetInfo>>();

    // Step 2: Check if all withdrawal assets are expunged
    let all_withdrawn_assets_expunged = withdrawal_assets
        .iter()
        .all(|w| expunged_assets.contains(&w.info));


    //For any supply cap at 0
    for cap in basket.collateral_supply_caps {

        if cap.supply_cap_ratio.is_zero(){

            //If in the position
            if let Some( asset ) = position_assets.clone().into_iter().find(|asset| asset.info.equal(&cap.asset_info)){

                //Withdraw asset has to either..
                //1) Only withdraw the asset
                if withdrawal_assets[0].info.equal(&asset.info) && withdrawal_assets.len() == 1_usize {
                    passed = true;
                
                //2) Fully withdraw the asset
                } else if let Some( withdrawal_asset ) = withdrawal_assets.clone().into_iter().find(|w_asset| w_asset.info.equal(&asset.info)){

                    if withdrawal_asset.amount == asset.amount {
                        passed = true;
                    } else {
                        passed = false;
                        invalid_withdraws.push( asset.info.to_string() );
                    } 
                } else {
                    passed = false;
                    invalid_withdraws.push( asset.info.to_string() );
                }
            }
        }
    }

    // Step 3: For every expunged asset in position, require it to either be
    //         fully withdrawn, partially withdrawn, or not withdrawn, but never
    //         left behind while other assets are withdrawn.
    if all_withdrawn_assets_expunged {
        passed = true;
    } 

    if !passed {
        return Err( StdError::GenericErr { msg: format!("These assets need to be expunged from the position: {:?}", invalid_withdraws) } )
    }

    Ok(())
}

/// Create Position instance
pub fn create_position(
    cAssets: Vec<cAsset>, //Assets being added into the position
    basket: &mut Basket,
) -> Result<Position, ContractError> {   
    let new_position = Position {
        position_id: basket.current_position_id,
        collateral_assets: cAssets,
        credit_amount: Uint128::zero(),
        deployed_to: vec![],
        pending_interest: Uint128::zero(),
        total_interest_accrued: Uint128::zero(),
    };

    //increment position id
    basket.current_position_id += Uint128::from(1u128);

    Ok(new_position)
}

/// Creates a CosmosMsg to mint tokens
pub fn credit_mint_msg(
    config: Config,
    credit_asset: Asset,
    recipient: Addr,
) -> StdResult<CosmosMsg> {
    match credit_asset.clone().info {
        AssetInfo::Token { address: _ } => {
            Err(StdError::GenericErr {
                msg: String::from("Credit has to be a native token"),
            })
        }
        AssetInfo::NativeToken { denom } => {
            if config.chain_proxy.is_some() {
                let message = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: config.chain_proxy.unwrap_or_else(|| Addr::unchecked("")).to_string(),
                    msg: to_json_binary(&OsmoExecuteMsg::MintTokens {
                        denom,
                        amount: credit_asset.amount,
                        mint_to_address: recipient.to_string(),
                    })?,
                    funds: vec![],
                });
                Ok(message)
            } else {
                Err(StdError::GenericErr {
                    msg: String::from("No proxy contract setup"),
                })
            }
        }
    }
}

/// Updates the affiliates for a position.
/// Used during repay to reset the time affiliated & shrink the list to a single affiliate.
fn update_affiliates(
    storage: &mut dyn Storage,
    affiliates: Vec<AffiliateData>,
    position_id: Uint128,
    current_time: u64,
) -> StdResult<()> {
    if affiliates.is_empty() {
        return Ok(())
    }
    //Only save the last affiliate
    let mut affiliates = vec![affiliates[affiliates.len() - 1].clone()];
    //Set this affiliate's time affiliated to the current block time
    affiliates[0].time_affiliated = current_time;
    //Update affiliates
    AFFILIATES.save(storage, position_id.to_string(), &affiliates)?;

    Ok(())
}

/// Creates a CosmosMsg to distribute debt tokens using the revenue distributor
pub fn credit_burn_rev_msg(
    config: Config, 
    env: Env, 
    credit_asset: Asset,
    basket: &mut Basket,
    position_affiliates: Vec<AffiliateData>,
    //Burn all 
    burn_all: bool
) -> StdResult<Vec<SubMsg>> {

    //Calculate the amount to burn
    let (burn_amount, revenue_amount) = {
        //If not distributing revenue, burn all
        if !basket.distribute_revenue || burn_all {
            (credit_asset.amount, Uint128::zero())

            //if pending rev is != 0
        } else if !basket.pending_revenue.total_pending.is_zero() {
            //If pending_revenue && repay amount are more than 50 CDT, send all to stakers
            //Limits Repay gas costs for smaller users & frequent management costs for larger
            if basket.pending_revenue.total_pending >= Uint128::new(50_000_000) && credit_asset.amount >= Uint128::new(50_000_000){
                if basket.pending_revenue.total_pending >= credit_asset.amount {
                    //if pending rev is greater send the full repayment
                    (Uint128::zero(), credit_asset.amount)
                } else {
                    //if pending rev is less send the full pending rev
                    //Burn the remainder
                    (credit_asset.amount - basket.pending_revenue.total_pending, basket.pending_revenue.total_pending)
                }
            } else {
                (credit_asset.amount, Uint128::zero())
            }

        } else {
            (credit_asset.amount, Uint128::zero())
        }        
    };


    //Initialize messages
    let mut messages: Vec<SubMsg> = vec![];
    if let AssetInfo::NativeToken { denom } = credit_asset.clone().info {
        if let Some(chain_proxy_addr) = config.chain_proxy {
            //Initialize promises
            let mut promises = vec![];
            let mut promised_revenue = Uint128::zero();

            //If we have revenue to distribute and a revenue distributor is configured
            if !revenue_amount.is_zero() && config.revenue_distributor.is_some() {

                if !position_affiliates.is_empty() {
                    //Calculate affiliate fees
                    let affiliate_fees = split_affiliate_fee(position_affiliates.clone(), config.affiliate_fee_max, env.block.time.seconds())?;
                    let affiliate_fees_amount = decimal_multiplication(
                        Decimal::from_ratio(revenue_amount, Uint128::from(1u128)), 
                        affiliate_fees.iter().sum::<Decimal>())?.to_uint_floor();


                    //Create affiliate promises
                    for (i, affiliate_fee) in affiliate_fees.into_iter().enumerate() {
                        //Calc the amount of revenue to promise
                        let amount = affiliate_fee * affiliate_fees_amount;
                        //Subtract affiliate fees from revenue
                        promised_revenue += amount;
                        //Create promise
                        promises.push(membrane::revenue_distributor::RevenuePromise {
                            address: position_affiliates[i].affiliate_address.to_string(),
                            amount: amount,
                        });
                    }
                }
                //Set non_promised_revenue to the revenue amount minus the promised revenue
                let revenue_to_distribute = revenue_amount - promised_revenue;

                // Compute get asset ratio from each asset in the basket's pending revenue struct
                let ltv_disco_distribution = basket.pending_revenue.per_asset_rev.clone()
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        let ratio = decimal_division(
                            Decimal::from_ratio(a.amount, Uint128::one()), 
                            Decimal::from_ratio(basket.pending_revenue.total_pending, Uint128::one())
                        )?;
                        let amount = decimal_multiplication(ratio, 
                            Decimal::from_ratio(revenue_to_distribute, Uint128::one())
                        )?.to_uint_floor();
                        //Set amount to the max as the per asset pending amount 
                        let amount = std::cmp::min(amount, basket.pending_revenue.per_asset_rev[i].amount);

                        //Update promised revenue
                        promised_revenue += amount;
                        //Update per-asset revenue
                        basket.pending_revenue.per_asset_rev[i].amount -= amount;

                        //Return
                        Ok(Asset { info: a.info.clone(), amount: amount })
                    })
                    .collect::<StdResult<Vec<Asset>>>()?;
                //Update pending_revenue.
                //We update here from a promsed tracker to account for any rounding, instead of just subtracting revenue_amount.
                basket.pending_revenue.total_pending -= promised_revenue;
                //Send revenue to distributor
                let revenue_distributor_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: config.revenue_distributor.unwrap().to_string(),
                    msg: to_json_binary(&membrane::revenue_distributor::ExecuteMsg::SetPromises {
                        promises,
                        ltv_disco_distribution: Some(ltv_disco_distribution),
                    })?,
                    funds: vec![asset_to_coin(Asset {
                        amount: revenue_amount,
                        info: credit_asset.info.clone(),
                    })?],
                });
                messages.push(SubMsg::new(revenue_distributor_msg));

            }

            if !burn_amount.is_zero() {    
                //Create burn msg
                let burn_message = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: chain_proxy_addr.to_string(),
                    msg: to_json_binary(&OsmoExecuteMsg::BurnTokens {
                        denom: denom.clone(),
                        amount: burn_amount,
                        burn_from_address: env.contract.address.to_string(),
                    })?,
                    funds: vec![],
                });
                messages.push(SubMsg::new(burn_message));
            }

            Ok(messages)
        } else {
            Err(StdError::GenericErr { msg: String::from("No proxy contract setup")})
        }
    } else { Err(StdError::GenericErr { msg: String::from("Cw20 assets aren't allowed") }) }
}


//Split affiliate fee % between affiliates based on time affiliated
fn split_affiliate_fee(affiliates: Vec<AffiliateData>, max_affiliate_fee: Decimal, current_time: u64) -> StdResult<Vec<Decimal>> {
    //Calculate total time affiliated since last repayment 
    let time_since_last_repayment = current_time - affiliates[0].time_affiliated;

    let mut affiliate_fees = vec![];

    //Calc time affiliated for each affiliate by subtracting their block time from the block time of the affiliate in the index +1
    for i in 0..affiliates.len() - 1 {
        let time_affiliated =  affiliates[i + 1].time_affiliated - affiliates[i].time_affiliated;
        let ratio_affiliated = Decimal::from_ratio(time_affiliated, time_since_last_repayment);
        let per_affiliate_fee = decimal_multiplication(affiliates[i].affiliate_fee, ratio_affiliated)?;
        affiliate_fees.push(per_affiliate_fee);
    }

    //Add the last affiliate fee
    let last_affiliates_time =  current_time - affiliates.last().unwrap().time_affiliated;
    let ratio_affiliated = Decimal::from_ratio(last_affiliates_time, time_since_last_repayment);
    let per_affiliate_fee = decimal_multiplication(affiliates.last().unwrap().affiliate_fee, ratio_affiliated)?;
    affiliate_fees.push(per_affiliate_fee);

    //Assert that the sum of the affiliate fees is equal or less than the affiliate fee
    let sum_of_affiliate_fees = affiliate_fees.iter().sum::<Decimal>();
    if sum_of_affiliate_fees > max_affiliate_fee {
        return Err(StdError::GenericErr { msg: format!("Sum of affiliate fees is greater than the max affiliate fee: {} > {}", sum_of_affiliate_fees, max_affiliate_fee) })
    }

    Ok(affiliate_fees)
    
}

/// Checks if any cAsset amount is zero or if asset list is empty
pub fn check_for_empty_position( collateral_assets: Vec<cAsset> )-> bool {
    
    if collateral_assets.len() == 0 {
        return true
    }

    //Checks if any cAsset amount is not zero
    for asset in collateral_assets {    
        if !asset.asset.amount.is_zero(){
            return false
        }
    }
    true 
}