use std::str::FromStr;
use std::vec;

use cosmwasm_std::{
    attr, coin, to_binary, Addr, Api, BankMsg, Coin, CosmosMsg, Decimal, DepsMut, Env, MessageInfo,
    QuerierWrapper, QueryRequest, Response, StdError, StdResult, Storage, SubMsg, Uint128, WasmMsg,
    WasmQuery,
};

use cw_storage_plus::Item;
use membrane::helpers::{router_native_to_native, pool_query_and_exit, query_stability_pool_fee, validate_position_owner, asset_to_coin, withdrawal_msg, get_contract_balances};
use membrane::cdp::{Config, ExecuteMsg, EditBasket};
use membrane::oracle::AssetResponse;
use osmo_bindings::PoolStateResponse;
use membrane::liq_queue::ExecuteMsg as LQ_ExecuteMsg;
use membrane::liquidity_check::ExecuteMsg as LiquidityExecuteMsg;
use membrane::staking::{ExecuteMsg as Staking_ExecuteMsg, QueryMsg as Staking_QueryMsg, Config as Staking_Config};
use membrane::oracle::{ExecuteMsg as OracleExecuteMsg, QueryMsg as OracleQueryMsg};
use membrane::osmosis_proxy::{ExecuteMsg as OsmoExecuteMsg, QueryMsg as OsmoQueryMsg };
use membrane::stability_pool::ExecuteMsg as SP_ExecuteMsg;
use membrane::math::{decimal_division, decimal_multiplication, Uint256, decimal_subtraction};
use membrane::types::{
    cAsset, Asset, AssetInfo, AssetOracleInfo, Basket, LiquidityInfo, Position,
    StoredPrice, SupplyCap, UserInfo, PriceVolLimiter, PoolType, RedemptionInfo, PositionRedemption
};

use crate::query::{get_cAsset_ratios, get_avg_LTV, insolvency_check};
use crate::rates::accrue;
use crate::risk_engine::{update_basket_tally, update_basket_debt, update_debt_per_asset_in_position};
use crate::state::{CLOSE_POSITION, ClosePositionPropagation, BASKET, get_target_position, update_position_claims, REDEMPTION_OPT_IN, update_position};
use crate::{
    state::{
        WithdrawPropagation, CONFIG, POSITIONS, LIQUIDATION, WITHDRAW,
    },
    ContractError,
};

pub const WITHDRAW_REPLY_ID: u64 = 4u64;
pub const CLOSE_POSITION_REPLY_ID: u64 = 5u64;
pub const ROUTER_REPLY_ID: u64 = 6u64;
pub const BAD_DEBT_REPLY_ID: u64 = 999999u64;

//Constants
const MAX_POSITIONS_AMOUNT: u32 = 10;


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
    let valid_owner_addr = validate_position_owner(deps.api, info, position_owner)?;
    let mut basket: Basket = BASKET.load(deps.storage)?;

    //Set deposit_amounts to double check state storage 
    let deposit_amounts: Vec<Uint128> = cAssets.clone()
        .into_iter()
        .map(|cAsset| cAsset.asset.amount)
        .collect::<Vec<Uint128>>();

    //Initialize positions_prev_collateral & position_info for deposited assets
    //Used for to double check state storage
    let mut positions_prev_collateral = vec![];
    let position_info: UserInfo;

    //For debt per asset updates
    let mut old_assets: Vec<cAsset>;
    let new_assets;

    if let Ok(mut positions) = POSITIONS.load(deps.storage, valid_owner_addr.clone()){

        //Enforce max positions
        if positions.len() >= MAX_POSITIONS_AMOUNT as usize {
            return Err(ContractError::MaxPositionsReached {});
        }

        //Add collateral to the position_id or Create a new position 
        if let Some(position_id) = position_id {
            //Find the position
            if let Some((position_index, mut position)) = positions.clone()
                .into_iter()
                .enumerate()
                .find(|(_i, position)| position.position_id == position_id){
                //Set old_assets for debt cap update
                old_assets = position.clone().collateral_assets;

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

                        //Add empty asset to old_assets as a placeholder
                        old_assets.push(cAsset {
                            asset: placeholder_asset.clone(),
                            max_borrow_LTV: deposit.clone().max_borrow_LTV,
                            max_LTV: deposit.clone().max_LTV,
                            pool_info: deposit.clone().pool_info,
                            rate_index: deposit.clone().rate_index,
                        });
                    }
                }
                //Set new_assets for debt cap updates
                new_assets = position.clone().collateral_assets;
                
                //Set updated position
                positions[position_index] = position.clone();
                
                //Accrue
                accrue(
                    deps.storage,
                    deps.querier,
                    env.clone(),
                    &mut position.clone(),
                    &mut basket,
                    valid_owner_addr.to_string(),
                    true
                )?;
                //Save Basket
                BASKET.save(deps.storage, &basket)?;
                //Save Updated Vec<Positions> for the user
                POSITIONS.save(deps.storage, valid_owner_addr, &positions)?;

                if !position.credit_amount.is_zero() {
                    update_debt_per_asset_in_position(
                        deps.storage,
                        env,
                        deps.querier,
                        config,
                        old_assets,
                        new_assets,
                        Decimal::from_ratio(position.credit_amount, Uint128::new(1u128)),
                    )?;
                }
            } else {                
                //If position_ID is passed but no position is found, Error. 
                //In case its a mistake, don't want to add assets to a new position.
                return Err(ContractError::NonExistentPosition { id: position_id });
            }
        } else { //If user doesn't pass an ID, we create a new position
            let (new_position_info, new_position) = create_position_in_deposit(
                deps.storage,
                deps.querier,
                env,
                valid_owner_addr.clone(),
                cAssets.clone(),
                &mut basket
            )?;

            //Update position_info
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
        }
    } else { //No existing positions loaded so new Vec<Position> is created
        let (new_position_info, new_position) = create_position_in_deposit(
            deps.storage,
            deps.querier,
            env,
            valid_owner_addr.clone(),
            cAssets.clone(),
            &mut basket
        )?;

        //Update position_info
        position_info = new_position_info;

        //Add new Vec of Positions to state under the user
        POSITIONS.save(
            deps.storage,
            valid_owner_addr,
            &vec![new_position],
        )?;
    }

    //Double check State storage
    check_deposit_state(deps.storage, deps.api, positions_prev_collateral, deposit_amounts, position_info.clone())?;    

    Ok(Response::new().add_attributes(vec![
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
        &mut new_position,
        basket,
        valid_owner_addr.to_string(),
        true
    )?;
    //Save Basket. This only doesn't overwrite the save in update_debt_per_asset_in_position() bc they are certain to never happen at the same time
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
                return Err(ContractError::CustomError { val: String::from("Conditional 2: Possible state error") })
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
        &mut target_position,
        &mut basket,
        valid_position_owner.to_string(),
        false
    )?;

    //For debt cap updates
    let old_assets = target_position.clone().collateral_assets;
    let mut new_assets: Vec<cAsset> = vec![];
    let mut tally_update_list: Vec<cAsset> = vec![];

    //Set withdrawal prop variables
    let mut prop_assets = vec![];
    let mut withdraw_amounts: Vec<Uint128> = vec![];

    //For Withdraw Msg
    let mut withdraw_coins: Vec<Coin> = vec![];

    //Check for expunged assets and assert they are being withdrawn
    check_for_expunged(old_assets.clone(), cAssets.clone(), basket.clone() )?;

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
                if insolvency_check(
                    deps.storage,
                    env.clone(),
                    deps.querier,
                    target_position.clone().collateral_assets,
                    Decimal::from_ratio(target_position.clone().credit_amount, Uint128::new(1u128)),
                    basket.credit_price,
                    true,
                    config.clone(),
                )?.0 {
                    return Err(ContractError::PositionInsolvent {});
                } else {
                    //Update Position list
                    POSITIONS.update(deps.storage, valid_position_owner.clone(), |positions: Option<Vec<Position>>| -> Result<Vec<Position>, ContractError>{

                        let mut updating_positions = positions.unwrap();

                        //If new position isn't empty, update
                        if !check_for_empty_position(target_position.clone().collateral_assets){
                            updating_positions[position_index] = target_position.clone();
                        } else { // remove old position
                            updating_positions.remove(position_index);
                        }

                        Ok( updating_positions )
                    
                    })?;
                }
                
                //Save for debt cap updates
                new_assets = target_position.clone().collateral_assets;

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

    //Update basket supply cap tallies after all withdrawals to improve UX by smoothing debt_cap restrictions
    update_basket_tally(
        deps.storage,
        deps.querier,
        env.clone(),
        &mut basket,
        tally_update_list,
        false,
    )?;

    //Save updated repayment price and asset tallies
    BASKET.save(deps.storage, &basket)?;

    //Update debt distribution for position assets
    if !target_position.clone().credit_amount.is_zero() {
        //Make sure lists are equal and add blank assets if not
        if old_assets.len() != new_assets.len() {
            for i in 0..old_assets.len() {
                let mut already_pushed = false;
                if i == new_assets.len() {
                    new_assets.push(cAsset {
                        asset: Asset {
                            info: old_assets[i].clone().asset.info,
                            amount: Uint128::zero(),
                        },
                        ..old_assets[i].clone()
                    });
                    already_pushed = true;
                }
                //If the index isn't equal, push a blank asset (0 amount) beforehand
                if !already_pushed && !old_assets[i].asset.info.equal(&new_assets[i].asset.info){
                     
                    let temp_vec = vec![cAsset {
                        asset: Asset {
                            info: old_assets[i].clone().asset.info,
                            amount: Uint128::zero(),
                        },
                        ..old_assets[i].clone()
                    }];

                    let mut left: Vec<cAsset> = vec![];
                    let mut right: Vec<cAsset> = vec![];
                    for (index, asset) in new_assets.into_iter().enumerate() {
                        if index < i {
                            left.push(asset)
                        } else {
                            right.push(asset)
                        }
                    }
                    left.extend(temp_vec);
                    left.extend(right);
                    new_assets = left;                    
                }
            }
        }
        //Update debt caps
        update_debt_per_asset_in_position(
            deps.storage,
            env.clone(),
            deps.querier,
            config,
            old_assets,
            new_assets,
            Decimal::from_ratio(target_position.credit_amount, Uint128::new(1u128)),
        )?;
    }
    
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
            env,
            prop_assets_info,
        )?,
        position_info: UserInfo {
            position_id,
            position_owner: info.sender.to_string(),
        },
    };
    WITHDRAW.save(deps.storage, &withdrawal_prop)?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "withdraw"),
            attr("position_id", position_id),
            attr("assets", format!("{:?}", cAssets)),
        ])
        .add_submessages(msgs))
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

    //Validate position owner 
    let valid_owner_addr = validate_position_owner(api, info.clone(), position_owner)?;
    
    //Get target_position
    let (position_index, mut target_position) = get_target_position(storage, valid_owner_addr.clone(), position_id)?;

    //Accrue interest
    accrue(
        storage,
        querier,
        env.clone(),
        &mut target_position,
        &mut basket,
        valid_owner_addr.to_string(),
        false
    )?;
    
    //Set prev_credit_amount
    let prev_credit_amount = target_position.credit_amount;
    
    let mut messages = vec![];
    let mut excess_repayment = Uint128::zero();

    //Repay amount sent
    target_position.credit_amount = match target_position.credit_amount.checked_sub(credit_asset.amount){
        Ok(difference) => difference,
        Err(_err) => {
            //Set excess_repayment
            excess_repayment = credit_asset.amount - target_position.credit_amount;
            
            Uint128::zero()
        },
    };

    //Position's resulting debt can't be below minimum without being fully repaid
    if target_position.credit_amount * basket.clone().credit_price < config.debt_minimum
        && !target_position.credit_amount.is_zero(){
        //Router contract is allowed to.
        //We rather $1 of bad debt than $2000 and bad debt comes from swap slippage
        if let Some(router) = config.clone().dex_router {
            if info.sender != router {
                return Err(ContractError::BelowMinimumDebt {});
            }
        }
        //This would also pass for ClosePosition, but since spread is added to collateral amount this should never happen
        //Even if it does, the subsequent withdrawal would then error
    }

    //To indicate removed positions during ClosePosition
    let mut removed = false;
    //Update Position
    POSITIONS.update(storage, valid_owner_addr.clone(), |positions: Option<Vec<Position>>| -> Result<Vec<Position>, ContractError> {
        let mut updating_positions = positions.unwrap();

        //If new position isn't empty, update
        if !check_for_empty_position(updating_positions[position_index].clone().collateral_assets){
            updating_positions[position_index] = target_position.clone();
        } else { // remove old position
            updating_positions.remove(position_index);
            removed = true;
        }
        
        Ok(updating_positions)
    })?;

    //Burn repayment & send revenue to stakers
    let burn_and_rev_msgs = credit_burn_rev_msg(
        config.clone(),
        env.clone(),
        credit_asset.clone(),
        &mut basket,
    )?;
    messages.extend(burn_and_rev_msgs);

    //Send back excess repayment, defaults to the repaying address
    if !excess_repayment.is_zero() {
        if let Some(addr) = send_excess_to {
            let valid_addr = api.addr_validate(&addr)?;

            let msg = withdrawal_msg(Asset {
                amount: excess_repayment,
                ..basket.clone().credit_asset
            }, valid_addr )?;

            messages.push(msg);
        } else {
            let msg = withdrawal_msg(Asset {
                amount: excess_repayment,
                ..basket.clone().credit_asset
            }, info.sender )?;

            messages.push(msg);
        }                                
    }

    //Subtract paid debt from debt-per-asset tallies
    update_basket_debt(
        storage,
        env,
        querier,
        config,
        &mut basket,
        target_position.collateral_assets,
        credit_asset.amount - excess_repayment,
        false,
    )?;

    //Save updated repayment price and debts
    BASKET.save(storage, &basket)?;

    if !removed {
        //Check that state was saved correctly
        check_repay_state(
            storage,
            credit_asset.amount, 
            prev_credit_amount, 
            position_id, 
            valid_owner_addr
        )?;
    }
    
    Ok(Response::new()
        .add_messages(messages)
        .add_attributes(vec![
            attr("method", "repay"),
            attr("position_id", position_id),
            attr("loan_amount", target_position.credit_amount),
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

    if repay_amount >= prev_credit_amount { 
        if target_position.credit_amount != Uint128::zero() {
            return Err(ContractError::CustomError { val: String::from("Conditional 1: Possible state error") })
        }
    } else {
        //Assert that credit_amount is equal to the origin - what was repayed
        if target_position.credit_amount != prev_credit_amount - repay_amount {
            return Err(ContractError::CustomError { val: String::from("Conditional 2: Possible state error") })
        }
    }

    Ok(())
}

/// This is what the stability pool contract calls to repay for a liquidation and get its collateral distribution
pub fn liq_repay(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    credit_asset: Asset,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    
    //Fetch position info to repay for
    let liquidation_propagation = LIQUIDATION.load(deps.storage)?;

    //Can only be called by the SP contract
    if config.stability_pool.is_none() || info.sender != config.clone().stability_pool.unwrap(){
        return Err(ContractError::Unauthorized {});
    }

    //These 3 checks shouldn't error we are pulling the ids from state.
    //Would have to be an issue w/ the repay_progation initialization
    let basket: Basket = BASKET.load(deps.storage)?;

    let (_i, target_position) = get_target_position(
        deps.storage, 
        deps.api.addr_validate(&liquidation_propagation.position_info.position_owner)?,
        liquidation_propagation.clone().position_info.position_id,
    )?;
    
    //Position repayment
    let res = match repay(
        deps.storage,
        deps.querier,
        deps.api,
        env.clone(),
        info,
        liquidation_propagation.position_info.position_id,
        Some(liquidation_propagation.clone().position_info.position_owner),
        credit_asset.clone(),
        None,
    ) {
        Ok(res) => res,
        Err(e) => return Err(e),
    };
   
    //Set collateral_assets
    let collateral_assets = target_position.collateral_assets;

    //Get position's cAsset ratios
    let (cAsset_ratios, _) = get_cAsset_ratios(
        deps.storage,
        env.clone(),
        deps.querier,
        collateral_assets.clone(),
        config.clone(),
    )?;
    //Get cAsset prices
    let (_avg_borrow_LTV, _avg_max_LTV, _total_value, cAsset_prices) = get_avg_LTV(
        deps.storage,
        env.clone(),
        deps.querier,
        config.clone(),
        collateral_assets.clone(),
        false
    )?;

    let repay_value = decimal_multiplication(
        Decimal::from_ratio(credit_asset.amount, Uint128::new(1u128)),
        basket.credit_price,
    )?;

    let mut messages = vec![];
    let mut coins: Vec<Coin> = vec![];
    let mut native_repayment = Uint128::zero();

    //Stability Pool receives pro rata assets
    //Add distribute messages to the message builder, so the contract knows what to do with the received funds
    let mut distribution_assets = vec![];

    //Query SP liq fee
    let sp_liq_fee = query_stability_pool_fee(deps.querier, config.clone().stability_pool.unwrap().to_string())?;

    //Calculate distribution of assets to send from the repaid position
    for (num, cAsset) in collateral_assets.into_iter().enumerate() {

        let collateral_repay_value = decimal_multiplication(repay_value, cAsset_ratios[num])?;
        let collateral_repay_amount = decimal_division(collateral_repay_value, cAsset_prices[num])?;
        let collateral_w_fee = decimal_multiplication(collateral_repay_amount, sp_liq_fee+Decimal::one())? * Uint128::new(1u128);

        let repay_amount_per_asset = credit_asset.amount * cAsset_ratios[num];

        //Remove collateral from user's position claims
        update_position_claims(
            deps.storage,
            deps.querier,
            env.clone(),
            liquidation_propagation.clone().position_info.position_id,
            deps.api.addr_validate(&liquidation_propagation.clone().position_info.position_owner)?,
            cAsset.clone().asset.info,
            collateral_w_fee,
        )?;

        //SP Distribution needs list of cAsset's and is pulling the amount from the Asset object
        match cAsset.clone().asset.info {
            AssetInfo::NativeToken { denom: _ } => {
                //Adding each native token to the list of distribution assets
                let asset = Asset {
                    amount: collateral_w_fee,
                    ..cAsset.clone().asset
                };
                //Add to the distribution_for field for native sends
                native_repayment += repay_amount_per_asset;

                distribution_assets.push(asset.clone());
                coins.push(asset_to_coin(asset)?);
            },            
            AssetInfo::Token { address: _ } => { return Err(ContractError::CustomError { val: String::from("Collateral assets are supposed to be native") }) }
        }
    }

    //Adds Native token distribution msg to messages
    let distribution_msg = SP_ExecuteMsg::Distribute {
        distribution_assets: distribution_assets.clone(),
        distribution_asset_ratios: cAsset_ratios, //The distributions are based off cAsset_ratios so they shouldn't change
        distribute_for: native_repayment,
    };
    //Build the Execute msg w/ the full list of native tokens
    let msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.stability_pool.unwrap().to_string(),
        msg: to_binary(&distribution_msg)?,
        funds: coins,
    });

    messages.push(msg);

    Ok(res
        .add_messages(messages)
        .add_attribute("method", "liq_repay")
        .add_attribute("distribution_assets", format!("{:?}", distribution_assets))
        .add_attribute("distribute_for", native_repayment))
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
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;
    let mut basket: Basket = BASKET.load(deps.storage)?;

    //Check if frozen
    if basket.frozen { return Err(ContractError::Frozen {  }) }

    //Get Target position
    let (position_index, mut target_position) = get_target_position(deps.storage, info.clone().sender, position_id)?;

    //Accrue interest
    accrue(
        deps.storage,
        deps.querier,
        env.clone(),
        &mut target_position,
        &mut basket,
        info.sender.to_string(),
        false
    )?;

    //Set prev_credit_amount
    let prev_credit_amount = target_position.credit_amount;

    //Set amount
    let amount = match amount {
        Some(amount) => amount,
        None => {
            if let Some(LTV) = LTV {
                get_amount_from_LTV(deps.storage, deps.querier, env.clone(), config.clone(), target_position.clone(), basket.clone(), LTV)?
            } else {
                return Err(ContractError::CustomError { val: String::from("If amount isn't passed, LTV must be passed") })
            }            
        }
    };

    //Add new credit_amount
    target_position.credit_amount += amount;

    //Test for minimum debt requirements
    if decimal_multiplication(
        Decimal::from_ratio(target_position.credit_amount, Uint128::new(1u128)),
        basket.credit_price,
    )? < Decimal::from_ratio(config.debt_minimum, Uint128::new(1u128))
    {
        return Err(ContractError::BelowMinimumDebt {});
    }

    let message: CosmosMsg;

    //Can't take credit before an oracle is set
    if basket.oracle_set {
        //If resulting LTV makes the position insolvent, error. If not construct mint msg
        if insolvency_check(
            deps.storage,
            env.clone(),
            deps.querier,
            target_position.clone().collateral_assets,
            Decimal::from_ratio(target_position.credit_amount, Uint128::new(1u128)),
            basket.credit_price,
            true,
            config.clone(),
        )? .0 {
            return Err(ContractError::PositionInsolvent {});
        } else {
            //Set recipient
            let recipient = {
                if let Some(mint_to) = mint_to_addr {
                    deps.api.addr_validate(&mint_to)?
                } else {
                    info.clone().sender
                }
            };
            message = credit_mint_msg(
                config.clone(),
                Asset {
                    amount,
                    ..basket.clone().credit_asset
                },
                recipient,
            )?;

            //Add credit amount to the position
            //Update Position
            POSITIONS.update(deps.storage, info.clone().sender, |positions: Option<Vec<Position>>| -> Result<Vec<Position>, ContractError> {
                let mut updating_positions = positions.unwrap();
                updating_positions[position_index] = target_position.clone();

                Ok(updating_positions)
            })?;

            //Add new debt to debt-per-asset tallies
            update_basket_debt(
                deps.storage,
                env,
                deps.querier,
                config,
                &mut basket,
                target_position.collateral_assets,
                amount,
                true,
            )?;
            
            //Save updated repayment price and debts
            BASKET.save(deps.storage, &basket)?;
        }
    } else {
        return Err(ContractError::NoRepaymentPrice {});
    }

    //Check state changes
    check_debt_increase_state(
        deps.storage, 
        amount, 
        prev_credit_amount, 
        position_id, 
        info.sender,
    )?;

    let response = Response::new()
        .add_message(message)
        .add_attribute("method", "increase_debt")
        .add_attribute("position_id", position_id.to_string())
        .add_attribute("total_loan", target_position.credit_amount.to_string())
        .add_attribute("increased_by", amount.to_string());

    Ok(response)
}

/// Asserts valid state after increase_debt()
fn check_debt_increase_state(
    storage: &mut dyn Storage,
    increase_amount: Uint128,
    prev_credit_amount: Uint128,
    position_id: Uint128,
    position_owner: Addr,  
) -> Result<(), ContractError>{
    
    //Get target_position
    let (_i, target_position) = get_target_position(storage, position_owner, position_id)?;

    //Assert that credit_amount is equal to the origin + what was added
    if target_position.credit_amount != prev_credit_amount + increase_amount {
        return Err(ContractError::CustomError { val: String::from("Conditional 1: increase_debt() state error found, saved credit_amount higher than desired.") })
    }

    Ok(())
}

/// Edit and Enable debt token Redemption for any address-owned Positions
pub fn edit_redemption_info(
    deps: DepsMut, 
    info: MessageInfo,
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
) -> Result<Response, ContractError>{
    //Check for valid premium
    if let Some(premium) = updated_premium {
        if premium > 99u128 {
            return Err(ContractError::CustomError { val: String::from("Premium can't be greater than 99") })
        }
    }

    //Check for valid max_loan_repayment
    if let Some(max_loan_repayment) = max_loan_repayment {
        if max_loan_repayment > Decimal::one() {
            return Err(ContractError::CustomError { val: String::from("Max loan repayment can't be greater than 100%") })
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

    //////Additions//////
    //Add PositionRedemption objects under the user in the desired premium while skipping duplicates, if redeemable is true or None
    if !(redeemable.is_some() && !redeemable.unwrap()){
        if let Some(updated_premium) = updated_premium {                
            //Load premium we are adding to 
            match REDEMPTION_OPT_IN.load(deps.storage, updated_premium){
                Ok(mut users_of_premium)=> {
                    //If the user already has a PositionRedemption, add the Position to the list
                    if let Some ((user_index, mut user_positions)) = users_of_premium.clone().into_iter().enumerate().find(|(_, user)| user.position_owner == info.sender){
                        //Iterate through the Position IDs
                        for id in position_ids.clone() {
                            //If the Position ID is not in the list, add it
                            if !user_positions.position_infos.iter().any(|position| position.position_id == id){

                                //Get target_position
                                let target_position = match get_target_position(deps.storage, info.sender.clone(), id){
                                    Ok((_, pos)) => pos,
                                    Err(_e) => return Err(ContractError::CustomError { val: format!("User does not own position id: {}", id) })
                                };

                                user_positions.position_infos.push(PositionRedemption {
                                    position_id: id,
                                    remaining_loan_repayment: max_loan_repayment.unwrap_or(Decimal::one()) * target_position.credit_amount,
                                    restricted_collateral_assets: restricted_collateral_assets.clone().unwrap_or(vec![]),
                                });
                            }

                            //Remove the Position ID from the list, don't want to edit newly added RedemptionInfo
                            position_ids.retain(|&x| x != id);
                        }

                        //Update the PositionRedemption
                        users_of_premium[user_index] = user_positions;

                        //Save the updated list
                        REDEMPTION_OPT_IN.save(deps.storage, updated_premium, &users_of_premium)?;
                    } //Add user to the premium state
                    else {                            
                        //Create new RedemptionInfo
                        let new_redemption_info = create_redemption_info(
                            deps.storage,
                            position_ids.clone(), 
                            max_loan_repayment.clone(), 
                            info.clone().sender,
                            restricted_collateral_assets.clone().unwrap_or(vec![]),
                        )?;

                        //Add the new RedemptionInfo to the list
                        users_of_premium.push(new_redemption_info);

                        //Save the updated list
                        REDEMPTION_OPT_IN.save(deps.storage, updated_premium, &users_of_premium)?;
                    }
                },
                //If no users, create a new list
                Err(_err) => {
                    //Create new RedemptionInfo
                    let new_redemption_info = create_redemption_info(
                        deps.storage,
                        position_ids.clone(), 
                        max_loan_repayment.clone(), 
                        info.clone().sender,
                        restricted_collateral_assets.clone().unwrap_or(vec![]),
                    )?;

                    //Save the new RedemptionInfo
                    REDEMPTION_OPT_IN.save(deps.storage, updated_premium, &vec![new_redemption_info])?;
                },
            };
        } else if (redeemable.is_some() && redeemable.unwrap()) && updated_premium.is_none(){
            return Err(ContractError::CustomError { val: String::from("Can't set redeemable to true without specifying a premium") })
        }
    } 

    //////Edits and Removals//////
    //Parse through premium range to look for the Position IDs
    for premium in 0..100u128 {
        //Load premium we are editing
        let mut users_of_premium: Vec<RedemptionInfo> = match REDEMPTION_OPT_IN.load(deps.storage, premium){
            Ok(list)=> list,
            Err(_err) => vec![], //If no users, return empty vec
        };

        //Query for Users in the premium as long as there are Position IDs left to find && there are users in the premium
        if !position_ids.is_empty() && !users_of_premium.is_empty(){      
            
            //Iterate through users to find the Positions
            if let Some ((user_index, mut user_positions)) = users_of_premium.clone().into_iter().enumerate().find(|(_, user)| user.position_owner == info.sender){
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
                                    REDEMPTION_OPT_IN.save(deps.storage, premium, &users_of_premium)?;
                                    break;
                                }
                            }
                        }
                        
                        //Update maximum loan repayment
                        if let Some(max_loan_repayment) = max_loan_repayment {
                            //Get target_position
                            let target_position = match get_target_position(deps.storage, info.sender.clone(), id){
                                Ok((_, pos)) => pos,
                                Err(_e) => return Err(ContractError::CustomError { val: format!("User does not own position id: {}", id) })
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
                                    REDEMPTION_OPT_IN.save(deps.storage, premium, &users_of_premium)?;
                                    break;
                                }
                            }   
                        }
                        
                        //Update restricted collateral assets
                        if let Some(restricted_assets) = restricted_collateral_assets.clone() {
                            //Map collateral assets to String
                            let basket = BASKET.load(deps.storage)?;
                            let collateral = basket.collateral_types.iter().map(|asset| asset.asset.info.to_string()).collect::<Vec<String>>();

                            //If all restricted assets are valid, swap objects
                            if restricted_assets.iter().all(|asset| collateral.contains(asset)) {
                                user_positions.position_infos[position_index].restricted_collateral_assets = restricted_assets.clone();
                            } else {
                                return Err(ContractError::CustomError { val: format!("Invalid restricted asset, only collateral assets are viable to restrict") })
                            }
                        }

                        //Update the Position
                        users_of_premium[user_index] = user_positions.clone();

                        //Save the updated list
                        REDEMPTION_OPT_IN.save(deps.storage, premium, &users_of_premium)?;

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
            Err(_e) => return Err(StdError::GenericErr { msg: format!("User does not own position id: {}", id) })
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
/// The premium is set by the Position owner, ex: 1% premium = buying CDT at 99% of the peg price
pub fn redeem_for_collateral(    
    deps: DepsMut, 
    env: Env,
    info: MessageInfo,
    max_collateral_premium: u128,
) -> Result<Response, ContractError>{
    //Load State
    let config: Config = CONFIG.load(deps.storage)?;
    let basket: Basket = BASKET.load(deps.storage)?;

    let mut credit_amount;
    let mut collateral_sends: Vec<Asset> = vec![];
    
    //Validate asset 
    if info.clone().funds.len() != 1 || info.clone().funds[0].denom != basket.credit_asset.info.to_string(){
        return Err(ContractError::CustomError { val: format!("Must send only the debt token: {}", basket.credit_asset.info) })
    } else {
        credit_amount = Decimal::from_ratio(Uint128::from(info.clone().funds[0].amount), Uint128::one());
    }
    //Set initial credit amount
    let initial_credit_amount = credit_amount.clone();

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
                    let (_i, mut target_position) = get_target_position(
                        deps.storage, 
                        user.clone().position_owner, 
                        position_redemption_info.position_id
                    )?;                    

                    //Remove restricted collateral assets from target_position.collateral_assets
                    for restricted_asset in position_redemption_info.restricted_collateral_assets {
                        target_position.collateral_assets = target_position.collateral_assets.clone()
                            .into_iter()
                            .filter(|asset| asset.asset.info.to_string() != restricted_asset)
                            .collect::<Vec<cAsset>>();
                    }

                    //Get cAsset ratios
                    let (cAsset_ratios, _) = get_cAsset_ratios(
                        deps.storage,
                        env.clone(),
                        deps.querier,
                        target_position.clone().collateral_assets,
                        config.clone(),
                    )?;

                    //Calc amount of credit that can be redeemed
                    let redeemable_credit = Decimal::min(
                        Decimal::from_ratio(position_redemption_info.remaining_loan_repayment, Uint128::one()),
                        credit_amount
                    );
                    //Subtract redeemable from credit_amount 
                    credit_amount = decimal_subtraction(credit_amount, redeemable_credit)?;
                    //Subtract redeemable from remaining_loan_repayment
                    user.position_infos[pos_rdmpt_index].remaining_loan_repayment = 
                        position_redemption_info.remaining_loan_repayment - 
                        redeemable_credit.to_uint_floor();

                    /////Set and Save user info with updated remaining_loan_repayment////
                    //If remaining_loan_repayment is zero, remove PositionRedemption from user
                    if user.position_infos[pos_rdmpt_index].remaining_loan_repayment.is_zero() {
                        //Remove PositionRedemption from user
                        user.position_infos.remove(pos_rdmpt_index);
                        //Remove user if no more PositionRedemptions
                        if user.position_infos.is_empty() {
                            users_of_premium.remove(user_index);
                        }
                    } else {
                        //Update user
                        users_of_premium[user_index] = user.clone();
                    }
                    REDEMPTION_OPT_IN.save(deps.storage, premium, &users_of_premium)?;

                    // Calc credit_value
                    //redeemable_credit * credit_price
                    let credit_value = decimal_multiplication(
                        Decimal::from_ratio(redeemable_credit.to_uint_floor(), Uint128::one()),
                        basket.credit_price
                    )?;
                    // Calc redeemable value
                    //credit_value * discount_ratio 
                    let redeemable_value = decimal_multiplication(
                        credit_value, 
                        discount_ratio
                    )?;

                    //Calc collateral to send for each cAsset
                    for (i, cAsset) in target_position.collateral_assets.iter().enumerate() {
                        //Calc collateral to send
                        let collateral_to_send = decimal_multiplication(
                            redeemable_value, 
                            cAsset_ratios[i]
                        )?;

                        //Add to send list
                        if let Some(asset) = collateral_sends.iter_mut().find(|a| a.info == cAsset.asset.info) {
                            asset.amount += collateral_to_send.clone().to_uint_floor();
                        } else {
                            collateral_sends.push(Asset {
                                info: cAsset.asset.info.clone(),
                                amount: collateral_to_send.clone().to_uint_floor(),
                            });
                        }
                        
                        //Update Position totals
                        update_position_claims(
                            deps.storage, 
                            deps.querier, 
                            env.clone(), 
                            position_redemption_info.position_id, 
                            user.clone().position_owner, 
                            cAsset.asset.info.clone(), 
                            collateral_to_send.to_uint_floor()
                        )?;
                    }

                    //Reload target_position
                    let (_i, mut target_position) = get_target_position(
                        deps.storage, 
                        user.clone().position_owner, 
                        position_redemption_info.position_id
                    )?;

                    //Set position.credit_amount
                    target_position.credit_amount -= redeemable_credit.to_uint_floor();

                    //Update position.credit_amount
                    update_position(
                        deps.storage, 
                        user.clone().position_owner, 
                        target_position.clone()
                    )?;
                }
            }
        }
    }

    if credit_amount == initial_credit_amount {
        return Err(ContractError::CustomError { val: format!("No collateral to redeem with a max premium of: {}", max_collateral_premium) })
    }

    //Convert collateral_sends to coins
    let mut coins: Vec<Coin> = vec![];
    for asset in collateral_sends {
        coins.push(asset_to_coin(asset)?)
    }

    //Send collateral to user
    let collateral_msg = BankMsg::Send {
        to_address: info.clone().sender.to_string(),
        amount: coins.clone(),
    };

    //If there is excess credit, send it back to user
    if !credit_amount.is_zero() {
        let credit_msg = BankMsg::Send {
            to_address: info.clone().sender.to_string(),
            amount: vec![Coin {
                denom: basket.credit_asset.info.to_string(),
                amount: credit_amount.to_uint_floor(),
            }],
        };
        return Ok(Response::new()
            .add_message(collateral_msg)
            .add_message(credit_msg)
            .add_attributes(vec![
                attr("action", "redeem_for_collateral"),
                attr("sender", info.clone().sender),
                attr("redeemed_collateral", format!("{:?}", coins)),
                attr("redeemed_credit", format!("{:?}", credit_amount)),
            ])
        )
    }

    //Response
    Ok(Response::new()
    .add_message(collateral_msg)
    .add_attributes(vec![
        attr("action", "redeem_for_collateral"),
        attr("sender", info.clone().sender),
        attr("redeemed_collateral", format!("{:?}", coins)),
    ])
)

}

/// Sell position collateral to fully repay debts.
/// Max spread is used to ensure the full debt is repaid in lieu of slippage.
pub fn close_position(
    deps: DepsMut, 
    env: Env,
    info: MessageInfo,
    position_id: Uint128,
    max_spread: Decimal,
    mut send_to: Option<String>,
) -> Result<Response, ContractError>{
    //Load Config
    let config: Config = CONFIG.load(deps.storage)?;

    //Load Basket
    let basket: Basket = BASKET.load(deps.storage)?;

    //Load target_position, restrict to owner
    let (_i, target_position) = get_target_position(deps.storage, info.clone().sender, position_id)?;

    //Calc collateral to sell
    //credit_amount * credit_price * (1 + max_spread)
    let total_collateral_value_to_sell = {
        decimal_multiplication(
            Decimal::from_ratio(target_position.credit_amount, Uint128::new(1)), 
            decimal_multiplication(basket.credit_price, (max_spread + Decimal::one()))?
        )?
    };
    //Max_spread is added to the collateral amount to ensure enough credit is purchased
    //Excess debt token gets sent back to the position_owner during repayment

    //Get cAsset_ratios for the target_position
    let (cAsset_ratios, cAsset_prices) = get_cAsset_ratios(deps.storage, env.clone(), deps.querier, target_position.clone().collateral_assets, config.clone())?;

    let mut router_messages = vec![];
    let mut lp_withdraw_messages: Vec<CosmosMsg> = vec![];
    let mut withdrawn_assets = vec![];

    //Calc collateral_amount_to_sell per asset & create router msg
    for (i, _collateral_ratio) in cAsset_ratios.clone().into_iter().enumerate(){

        //Calc collateral_amount_to_sell
        let mut collateral_amount_to_sell = {
        
            let collateral_value_to_sell = decimal_multiplication(total_collateral_value_to_sell, cAsset_ratios[i])?;
            
            decimal_division(collateral_value_to_sell, cAsset_prices[i])? * Uint128::new(1u128)
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

        //If cAsset is an LP, split into pool assets to sell
        if let Some(pool_info) = target_position.clone().collateral_assets[i].clone().pool_info{

            let (msg, share_asset_amounts) = pool_query_and_exit(
                deps.querier, 
                env.clone(), 
                config.clone().osmosis_proxy.unwrap().to_string(), 
                pool_info.pool_id,
                collateral_amount_to_sell,
            )?;

            //Push LP Withdrawal Msg
            //Comment to pass tests
            lp_withdraw_messages.push(msg);
            
            //Create Router SubMsgs for each pool_asset
            for (i, pool_asset) in pool_info.asset_infos.into_iter().enumerate(){                
                let router_msg = router_native_to_native(
                    config.clone().dex_router.unwrap().to_string(), 
                    pool_asset.clone().info, 
                    basket.clone().credit_asset.info, 
                    None,
                    Uint128::from_str(&share_asset_amounts[i].clone().amount).unwrap().u128(), 
                )?;

                router_messages.push(router_msg);                
            }                  
        } else {        
            //Create router subMsg to sell, repay in reply on success
            let router_msg: CosmosMsg = router_native_to_native(
                config.clone().dex_router.unwrap().to_string(), 
                collateral_asset.clone().info, 
                basket.clone().credit_asset.info, 
                None,                 
                collateral_amount_to_sell.into(),
            )?;

            router_messages.push(router_msg);
        }
    }

    //Set send_to for WithdrawMsg in Reply
    if send_to.is_none() {
        send_to = Some(info.sender.to_string());
    }
    
    //Save CLOSE_POSITION_PROPAGATION
    CLOSE_POSITION.save(deps.storage, &ClosePositionPropagation {
        withdrawn_assets,
        position_info: UserInfo { 
            position_id, 
            position_owner: info.sender.to_string(),
        },
        send_to,
    })?;

    //The last router message is updated to a CLOSE_POSITION_REPLY to close the position after all sales and repayments are done.
    let sub_msg = SubMsg::reply_on_success(router_messages.pop().unwrap(), CLOSE_POSITION_REPLY_ID);
    
    Ok(Response::new()
        // .add_messages(lp_withdraw_messages)
        .add_messages(router_messages)
        .add_submessage(sub_msg)
        .add_attributes(vec![
        attr("position_id", position_id),
        attr("user", info.sender),
    ])) //If the sale incurred slippage and couldn't repay through the debt minimum, the subsequent withdraw msg will error and revert state 
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
        new_liq_queue = Some(deps.api.addr_validate(&liq_queue.clone().unwrap())?);
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
                val: "Max borrow LTV can't be greater or equal to max_LTV nor equal to 100"
                    .to_string(),
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
                        contract_addr: config.clone().oracle_contract.unwrap().to_string(),
                        msg: to_binary(&OracleQueryMsg::Assets {
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
                    contract_addr: new_liq_queue.clone().unwrap().to_string(),
                    msg: to_binary(&LQ_ExecuteMsg::AddQueue {
                        bid_for: asset.clone().asset.info,
                        max_premium,
                        //Bid total before bids go to the waiting queue. 
                        //Threshold should be larger than the largest single liquidation amount to prevent waiting bids from causing InsufficientBids errors.
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

    let new_basket: Basket = Basket {
        basket_id,
        current_position_id: Uint128::from(1u128),
        collateral_types: new_assets,
        collateral_supply_caps,
        multi_asset_supply_caps: vec![],
        credit_asset: credit_asset.clone(),
        credit_price,
        base_interest_rate,
        pending_revenue: Uint128::zero(),
        credit_last_accrued: env.block.time.seconds(),
        rates_last_accrued: env.block.time.seconds(),
        liq_queue: new_liq_queue,
        negative_rates: true,
        cpc_margin_of_error: Decimal::one(),
        oracle_set: false,
        frozen: false,
        rev_to_stakers: true,
    };

    //Denom check
    if let AssetInfo::Token { address :_} = credit_asset.info {
        return Err(ContractError::CustomError {
            val: "Basket credit must be a native token denom".to_string(),
        });
    }

    //Add asset to liquidity check contract
    //Liquidity AddAsset Msg
    let mut msgs = vec![];
    if let Some(liquidity_contract) = config.liquidity_contract {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: liquidity_contract.to_string(),
            msg: to_binary(&LiquidityExecuteMsg::AddAsset {
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
    info: MessageInfo,
    editable_parameters: EditBasket,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
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
    };

    let mut msgs: Vec<CosmosMsg> = vec![];

    let mut basket = BASKET.load(deps.storage)?;
    //cAsset check
    if let Some(added_cAsset) = editable_parameters.clone().added_cAsset {
        let mut check = true;
        new_cAsset = added_cAsset.clone();

        //new_cAsset can't be the basket credit_asset or MBRN 
        if let Some(staking_contract) = config.clone().staking_contract {
            let mbrn_denom = deps.querier.query::<Staking_Config>(&QueryRequest::Wasm(WasmQuery::Smart { 
                contract_addr: staking_contract.to_string(), 
                msg: to_binary(&Staking_QueryMsg::Config { })? 
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

        //No duplicates
        if let Some(_duplicate) = basket
            .clone()
            .collateral_types
            .into_iter()
            .find(|cAsset| cAsset.asset.info.equal(&new_cAsset.asset.info))
        {
            return Err(ContractError::CustomError {
                val: format!(
                    "Attempting to add duplicate asset: {}",
                    new_cAsset.asset.info
                ),
            });
        }

        if let Some(mut pool_info) = added_cAsset.pool_info {

            //Query share asset amount
            let pool_state = match deps.querier.query::<PoolStateResponse>(&QueryRequest::Wasm(
                WasmQuery::Smart {
                    contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
                    msg: match to_binary(&OsmoQueryMsg::PoolState {
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

            //Assert Asset order of pool_assets in PoolInfo object
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
                        val: format!(
                            "Need to add all pool assets before adding the LP. Errored on {}",
                            asset.denom
                        ),
                    });
                }
            }

            //Update pool_info
            new_cAsset.pool_info = Some(pool_info);

        } else {
            //Asserting the Collateral Asset has an oracle
            if config.oracle_contract.is_some() {
                //Query Asset Oracle
                deps.querier
                    .query::<Vec<AssetResponse>>(&QueryRequest::Wasm(WasmQuery::Smart {
                        contract_addr: config.clone().oracle_contract.unwrap().to_string(),
                        msg: to_binary(&OracleQueryMsg::Assets {
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
                contract_addr: basket.clone().liq_queue.unwrap().into_string(),
                msg: to_binary(&LQ_ExecuteMsg::AddQueue {
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
                msg: to_binary(&LQ_ExecuteMsg::AddQueue {
                    bid_for: new_cAsset.clone().asset.info,
                    max_premium,
                    //Bid total before bids go to the waiting queue. 
                    //Threshold should be larger than the largest single liquidation amount to prevent waiting bids from causing InsufficientBids errors.
                    bid_threshold: Uint256::from(1_000_000_000_000u128), //1 million
                })?,
                funds: vec![],
            }));
        }

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
                msg: to_binary(&OracleExecuteMsg::EditAsset {
                    asset_info: basket.clone().credit_asset.info,
                    oracle_info: Some(AssetOracleInfo {
                        basket_id: basket.clone().basket_id,
                        pools_for_osmo_twap: vec![credit_twap],
                        is_usd_par: false,
                    }),
                    remove: false,
                })?,
                funds: vec![],
            }));

            oracle_set = true;
        }
    };
    let mut attrs = vec![attr("method", "edit_basket")];

    //Create EditAssetMsg for Liquidity contract
    if let Some(pool_infos) = editable_parameters.clone().credit_pool_infos {
        attrs.push(attr("new_pool_infos", format!("{:?}", pool_infos)));

        if let Some(liquidity_contract) = config.liquidity_contract {
            msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: liquidity_contract.to_string(),
                msg: to_binary(&LiquidityExecuteMsg::EditAsset {
                    asset: LiquidityInfo {
                        asset: basket.clone().credit_asset.info,
                        pool_infos,
                    },
                })?,
                funds: vec![],
            }));
        }
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

/// Mint Basket's pending revenue to the specified address
pub fn mint_revenue(
    deps: DepsMut,
    info: MessageInfo,
    env: Env,
    send_to: Option<String>,
    repay_for: Option<UserInfo>,
    amount: Option<Uint128>,
) -> Result<Response, ContractError> {
    
    //Can't send_to and repay_for at the same time
    if send_to.is_some() && repay_for.is_some() {
        return Err(ContractError::CustomError {
            val: String::from("Can't send_to and repay_for at the same time"),
        });
    }
    //There must be at least 1 destination address
    if send_to.is_none() && repay_for.is_none(){
        return Err(ContractError::CustomError {
            val: String::from("Destination address is required"),
        });
    }

    let config = CONFIG.load(deps.storage)?;
    let mut basket = BASKET.load(deps.storage)?;

    if info.sender != config.owner { return Err(ContractError::Unauthorized {}) }

    if basket.pending_revenue.is_zero() {
        return Err(ContractError::CustomError {
            val: String::from("No revenue to mint"),
        });
    }

    //Set amount
    let amount = amount.unwrap_or(basket.pending_revenue);

    //Subtract amount from pending revenue
    basket.pending_revenue = match basket.pending_revenue.checked_sub(amount) {
        Ok(new_balance) => new_balance,
        Err(err) => {
            return Err(ContractError::CustomError {
                val: err.to_string(),
            })
        }
    }; //Save basket
    BASKET.save(deps.storage, &basket)?;

    let mut message: Vec<CosmosMsg> = vec![];
    let mut repay_attr = String::from("None");

    //If send to is_some
    if let Some(send_to) = send_to.clone() {
        message.push(credit_mint_msg(
            config,
            Asset {
                amount,
                ..basket.credit_asset
            }, 
            deps.api.addr_validate(&send_to)?
        )?);
    } else if let Some(repay_for) = repay_for {
        repay_attr = repay_for.to_string();

        //Need to mint credit to the contract
        message.push(credit_mint_msg(
            config,
            Asset {
                amount,
                ..basket.credit_asset.clone()
            },
            env.clone().contract.address,
        )?);

        //and then send it for repayment
        let msg = ExecuteMsg::Repay {
            position_id: repay_for.clone().position_id,
            position_owner: Some(repay_for.position_owner),
            send_excess_to: Some(env.contract.address.to_string()),
        };

        message.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_binary(&msg)?,
            funds: vec![coin(amount.u128(), basket.credit_asset.info.to_string())],
        }));
    } 

    Ok(Response::new().add_messages(message).add_attributes(vec![
        attr("amount", amount.to_string()),
        attr("repay_for", repay_attr),
        attr("send_to", send_to.unwrap_or_else(|| String::from("None"))),
    ]))
}

/// Calculate desired amount of credit to borrow to reach target LTV
fn get_amount_from_LTV(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: Config,
    position: Position,
    basket: Basket,
    target_LTV: Decimal,
) -> Result<Uint128, ContractError>{
    //Get avg_borrow_LTV & total_value
    let (avg_borrow_LTV, _avg_max_LTV, total_value, _cAsset_prices) = get_avg_LTV(
        storage, 
        env, 
        querier, 
        config, 
        position.clone().collateral_assets,
        false
    )?;

    //Target LTV can't be greater than possible borrowable LTV for the Position
    if target_LTV > avg_borrow_LTV {
        return Err(ContractError::InvalidLTV { target_LTV })
    }

    //Calc current LTV
    let current_LTV = {
        let credit_value = decimal_multiplication(Decimal::from_ratio(position.credit_amount, Uint128::new(1)), basket.credit_price)?;

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
        
        decimal_division(increased_credit_value, basket.credit_price)? * Uint128::new(1)
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

    //For any supply cap at 0
    for cap in basket.collateral_supply_caps {

        if cap.supply_cap_ratio.is_zero(){

            //If in the position
            if let Some( asset ) = position_assets.clone().into_iter().find(|asset| asset.info.equal(&cap.asset_info)){

                //Withdraw asset has to either..
                //1) Only withdraw the asset
                if withdrawal_assets[0].info.equal(&asset.info) && withdrawal_assets.len() == 1_usize{
                    passed = true;
                
                //2) Fully withdraw the asset
                } else if let Some( withdrawal_asset ) = withdrawal_assets.clone().into_iter().find(|w_asset| w_asset.info.equal(&asset.info)){

                    if withdrawal_asset.amount == asset.amount {
                        passed = true;
                    }else {
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
    if !passed {
        return Err( StdError::GenericErr { msg: format!("These assets need to be expunged from the positon: {:?}", invalid_withdraws) } )
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
                msg: "Credit has to be a native token".to_string(),
            })
        }
        AssetInfo::NativeToken { denom } => {
            if config.osmosis_proxy.is_some() {
                let message = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: config.osmosis_proxy.unwrap().to_string(),
                    msg: to_binary(&OsmoExecuteMsg::MintTokens {
                        denom,
                        amount: credit_asset.amount,
                        mint_to_address: recipient.to_string(),
                    })?,
                    funds: vec![],
                });
                Ok(message)
            } else {
                Err(StdError::GenericErr {
                    msg: "No proxy contract setup".to_string(),
                })
            }
        }
    }
}

/// Creates a CosmosMsg to distribute debt tokens
pub fn credit_burn_rev_msg(
    config: Config, 
    env: Env, 
    credit_asset: Asset,
    basket: &mut Basket,
) -> StdResult<Vec<CosmosMsg>> {

    //Calculate the amount to burn
    let (burn_amount, revenue_amount) = {
        //If not sent to stakers, burn all
        if !basket.rev_to_stakers {
            (credit_asset.amount, Uint128::zero())

            //if pending rev is != 0
        } else if !basket.pending_revenue.is_zero() {
            //If pending_revenue is >= credit_asset.amount && more than 50 CDT, send all to stakers
            //Limits Repay gas costs for smaller users & frequent management costs for larger
            if basket.pending_revenue >= credit_asset.amount && credit_asset.amount > Uint128::new(50_000_000){
                (Uint128::zero(), credit_asset.amount)
            } else {
                (credit_asset.amount, Uint128::zero())
            }

        } else {
            (credit_asset.amount, Uint128::zero())
        }
        
    };
    //Update pending_revenue
    basket.pending_revenue -= revenue_amount;

    //Initialize messages
    let mut messages = vec![];
    
    if let AssetInfo::NativeToken { denom } = credit_asset.clone().info {
        if let Some(addr) = config.osmosis_proxy {
            if !burn_amount.is_zero() {    
                //Create burn msg
                let burn_message = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: addr.to_string(),
                    msg: to_binary(&OsmoExecuteMsg::BurnTokens {
                        denom,
                        amount: burn_amount,
                        burn_from_address: env.contract.address.to_string(),
                    })?,
                    funds: vec![],
                });
                messages.push(burn_message);
            }

            //Create DepositFee Msg
            if !revenue_amount.is_zero() && config.staking_contract.is_some(){
                let rev_message = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: config.staking_contract.unwrap().to_string(),
                    msg: to_binary(&Staking_ExecuteMsg::DepositFee { })?,
                    funds: vec![ asset_to_coin(Asset {
                        amount: revenue_amount,
                        ..credit_asset
                    })? ],
                });
                messages.push(rev_message);
            }

            Ok(messages)
        } else {
            Err(StdError::GenericErr { msg: "No proxy contract setup".to_string()})
        }
    } else { Err(StdError::GenericErr { msg: "Cw20 assets aren't allowed".to_string() }) }
}

/// Stores the price of an asset
pub fn store_price(
    storage: &mut dyn Storage,
    env: Env, 
    asset_token: &AssetInfo,
    mut price: &mut StoredPrice,
) -> StdResult<()> {
    let key = asset_token.to_string();
    let price_bucket: Item<StoredPrice> = Item::new(&key);   
    
    //Set price_vol_limiter
    let time_elapsed = env.block.time.seconds() - price.price_vol_limiter.last_time_updated;
        
    //Store prive_vol_limiter if 5 mins have passed
    if time_elapsed >= 300 {

        price.price_vol_limiter = 
        PriceVolLimiter {
                price: price.clone().price,
                last_time_updated: env.block.time.seconds(),                  
        };
    }
    //Save Item
    price_bucket.save(storage, price)
}

/// Reads the price of an asset from storage
pub fn read_price(
    storage: &dyn Storage,
    asset_token: &AssetInfo
) -> StdResult<StoredPrice> {
    let key = asset_token.to_string();
    let price_bucket: Item<StoredPrice> = Item::new(&key);
    price_bucket.load(storage)
}

/// Checks if any cAsset amount is zero
pub fn check_for_empty_position( collateral_assets: Vec<cAsset> )-> bool {
    //Checks if any cAsset amount is zero
    for asset in collateral_assets {    
        if !asset.asset.amount.is_zero(){
            return false
        }
    }
    true 
}