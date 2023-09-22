use std::str::FromStr;

#[cfg(not(feature = "library"))]
use cosmwasm_std::{
    attr, to_binary, Addr, Coin, CosmosMsg, Decimal, DepsMut, Env,
    MessageInfo, Response, StdError, StdResult, Storage, Uint128, WasmMsg,
};
use cosmwasm_storage::{Bucket, ReadonlyBucket};
use membrane::math::{Decimal256, Uint256, U256};
use membrane::cdp::ExecuteMsg as CDP_ExecuteMsg;
use membrane::liq_queue::Config;
use membrane::oracle::{PriceResponse256, PriceResponse};
use membrane::types::{Asset, AssetInfo, Bid, BidInput, PremiumSlot, Queue};
use membrane::helpers::{validate_position_owner, withdrawal_msg, asset_to_coin};

use crate::error::ContractError;
use crate::state::{CONFIG, QUEUES};

const MAX_LIMIT: u32 = 2147483646;

static PREFIX_EPOCH_SCALE_SUM: &[u8] = b"epoch_scale_sum";

/// Create Bid and add to the corresponding Slot
pub fn submit_bid(
    deps: DepsMut,
    info: MessageInfo,
    env: Env,
    bid_input: BidInput,
    bid_owner: Option<String>,
) -> Result<Response, ContractError> {

    let config: Config = CONFIG.load(deps.storage)?;

    let valid_owner_addr = validate_position_owner(deps.api, info.clone(), bid_owner)?;

    let mut attrs = vec![
        attr("method", "deposit"),
        attr("bid_owner", valid_owner_addr.to_string()),
        attr("bid_input", bid_input.to_string()),
    ];

    validate_bid_input(deps.storage, bid_input.clone())?;
    let mut queue: Queue = QUEUES.load(deps.storage, bid_input.bid_for.to_string())?;

    let bid_asset: Asset = assert_bid_asset_from_sent_funds(queue.clone().bid_asset.info, &info, config.minimum_bid)?;

    let mut bid: Bid;
    //Add bid to selected premium
    let edited_slot = match queue
        .clone()
        .slots
        .into_iter()
        //Hard coded 1% per slot
        .find(|slot| {
            slot.liq_premium
                == Decimal256::percent(1)
                    * Decimal256::from_uint256(Uint256::from(bid_input.liq_premium as u128))
        }) {
        Some(mut slot) => {
            bid = Bid {
                user: valid_owner_addr.clone(),
                id: queue.current_bid_id,
                amount: Uint256::from(bid_asset.amount.u128()),
                liq_premium: bid_input.liq_premium,
                product_snapshot: Decimal256::one(),
                sum_snapshot: Decimal256::zero(),
                pending_liquidated_collateral: Uint256::zero(),
                wait_end: None,
                epoch_snapshot: Uint128::zero(),
                scale_snapshot: Uint128::zero(),
            };
            
            //Increment bid_id
            queue.current_bid_id += Uint128::new(1u128);

            //Add to total_queue_amount and total_slot_amount if below bid_threshold
            if slot.total_bid_amount <= queue.bid_threshold {
                //If the whole bid + the current bid total is less than the bid threshold + minimum_bid, activate the whole bid
                //This ensures the amount sent to wait is at least the minimum
                if slot.total_bid_amount + bid.amount < queue.bid_threshold + config.minimum_bid.into(){
                    //Add active bid amounts to the queue and slot
                    queue.bid_asset.amount += bid_asset.amount;
                    slot.total_bid_amount += bid.amount;

                    process_bid_activation(&mut bid, &mut slot);
                
                    //Add bid to active bids
                    slot.bids.push(bid.clone());    

                    //Set the (remaining) bid to 0 which will skip the waiting queue logic
                    bid.amount = Uint256::zero();

                    attrs.extend(vec![
                        attr("bid_id", bid.id.to_string()),
                        attr("bid", bid_asset.amount.to_string()),
                    ]);

                } else { //Activate the amount within the bid threshold and send the rest to the waiting queue
                    let amount_sent_to_wait = slot.total_bid_amount + bid.amount - queue.bid_threshold;
                    
                    //Create clone for the active bid
                    let mut bid_clone = bid.clone();
                                       
                    //Set the clone to the remaining active amount
                    bid_clone.amount = bid.amount - amount_sent_to_wait;

                    //Update bid_id to reflect the clone and increment
                    bid.id = queue.current_bid_id;
                    queue.current_bid_id += Uint128::new(1u128);                        

                    //Add active bid amounts to the queue and slot
                    queue.bid_asset.amount += bid_asset.amount - Uint128::new(u128::from(amount_sent_to_wait));
                    slot.total_bid_amount += bid_clone.amount;

                    process_bid_activation(&mut bid_clone, &mut slot);

                    attrs.push(attr("bid_id", bid_clone.id.to_string()));
                    attrs.push(attr("bid", (bid_asset.amount- Uint128::new(u128::from(amount_sent_to_wait))).to_string()));
                
                    //Add bid_clone to active bids
                    slot.bids.push(bid_clone);    

                    //Set the (remaining) bid to the amount to send to the waiting queue
                    bid.amount = amount_sent_to_wait;
                }  
            } 
            
            //Set the (remaining) bid to waiting 
            if !bid.amount.is_zero() {
                //Set wait time
                // calculate wait_end from current time
                bid.wait_end = Some(env.block.time.plus_seconds(config.waiting_period).seconds());

                //Add bid to waiting bids           
                slot.waiting_bids.push(bid.clone());

                //Enforce maximum number of waiting bids
                if slot.waiting_bids.len() > config.maximum_waiting_bids as usize {
                    return Err(ContractError::TooManyWaitingBids {
                        max_waiting_bids: config.maximum_waiting_bids,
                    });
                }

                attrs.extend(vec![
                    attr("bid_id", bid.id.to_string()),
                    attr("bid", bid.amount.to_string()),
                ]);
            }

            slot
        }
        None => return Err(ContractError::InvalidPremium {}), //Shouldn't be reached due to validate_bid_input() above
    };

    //Filter for unedited slot
    let mut new_slots: Vec<PremiumSlot> = queue
        .slots
        .into_iter()
        .filter(|slot| {
            slot.liq_premium
                != Decimal256::percent(1)
                    * Decimal256::from_uint256(Uint256::from(bid_input.liq_premium as u128))
        }) //Hard coded 1% per slot
        .collect::<Vec<PremiumSlot>>();
    //Add edited_slot
    new_slots.push(edited_slot);

    //Assign new slots to queue
    queue.slots = new_slots;

    //Save queue to state
    QUEUES.save(deps.storage, bid_input.bid_for.to_string(), &queue)?;

    //Response build
    let response = Response::new();    

    Ok(response.add_attributes(attrs))
}

/// Activate bid
fn process_bid_activation(bid: &mut Bid, slot: &mut PremiumSlot) {
    bid.product_snapshot = slot.product_snapshot;
    bid.sum_snapshot = slot.sum_snapshot;
    bid.wait_end = None;
    bid.scale_snapshot = slot.current_scale;
    bid.epoch_snapshot = slot.current_epoch;
}

/// Validate sent assets
pub fn assert_bid_asset_from_sent_funds(
    bid_asset: AssetInfo,
    info: &MessageInfo,
    minimum_bid: Uint128,
) -> StdResult<Asset> {

    if info.funds.is_empty() {
        return Err(StdError::GenericErr {
            msg: "No asset provided, only bid asset allowed".to_string(),
        });
    }

    match bid_asset.clone() {
        AssetInfo::NativeToken { denom } => {
            if info.funds[0].denom == denom && info.funds.len() == 1 {
                if info.funds[0].amount < minimum_bid {
                    return Err(StdError::GenericErr {
                        msg: format!("Bid amount too small, minimum is {}", minimum_bid),
                    });
                } else {
                    
                    Ok(Asset {
                        info: bid_asset,
                        amount: info.funds[0].amount,
                    })
                }
            } else {
                Err(StdError::GenericErr {
                    msg: "Invalid asset provided, only bid asset allowed".to_string(),
                })
            }
        }
        AssetInfo::Token { address: _ } => {
            Err(StdError::GenericErr {
                msg: "Bid asset's are native assets".to_string(),
            })
        }
    }
}

/// Withdraw bid amount
pub fn retract_bid(
    deps: DepsMut,
    info: MessageInfo,
    _env: Env,
    bid_id: Uint128,
    bid_for: AssetInfo,
    amount: Option<Uint256>,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;
    let mut bid = read_bid(deps.storage, bid_id, bid_for.clone())?;

    //Only owner can withdraw
    if bid.clone().user != info.sender {
        return Err(ContractError::Unauthorized {});
    }
    
    let mut slot: PremiumSlot =
        read_premium_slot(deps.storage, bid_for.clone(), bid.clone().liq_premium)?;

    let withdraw_amount: Uint256 = if bid.wait_end.is_some() {
        // waiting bid amount can be withdrawn without restriction
        let waiting_withdraw_amount = assert_withdraw_amount(amount, bid.amount, Uint256::from(config.minimum_bid))?;
        if waiting_withdraw_amount == bid.amount {
            remove_bid(deps.storage, bid.clone(), bid_for.clone())?;
        } else {
            bid.amount = bid.amount - waiting_withdraw_amount;
            store_bid(deps.storage, bid_for.clone(), bid.clone())?;
        }

        waiting_withdraw_amount
    } else {
        // calculate spent and reward until this moment
        let (withdrawable_amount, residue_bid) = calculate_remaining_bid(&bid, &slot)?;
        let (liquidated_collateral, residue_collateral) = calculate_liquidated_collateral(
            deps.storage,
            &bid,
            bid_for.to_string(),
        )?;

        // accumulate pending reward to be claimed later
        bid.pending_liquidated_collateral += liquidated_collateral;

        // stack residues, will give it to next claimer if it becomes bigger than 1.0
        slot.residue_collateral += residue_collateral;
        slot.residue_bid += residue_bid;

        //Store slot so store_bid() is using the correct slot
        store_premium_slot(deps.storage, bid_for.clone(), slot.clone())?;

        //Check requested amount
        let withdraw_amount = assert_withdraw_amount(amount, withdrawable_amount, Uint256::from(config.minimum_bid))?;

        //remove or update bid
        if withdraw_amount == bid.amount && bid.pending_liquidated_collateral.is_zero() {
            remove_bid(deps.storage, bid.clone(), bid_for.clone())?;
        } else {
            store_bid(
                deps.storage,
                bid_for.clone(),
                Bid {
                    amount: withdrawable_amount - withdraw_amount,
                    product_snapshot: slot.product_snapshot,
                    sum_snapshot: slot.sum_snapshot,
                    scale_snapshot: slot.current_scale,
                    ..bid.clone()
                },
            )?;
        }

        //Reload slot so that store_slot() below doesn't override the above store_bid()
        let mut slot: PremiumSlot =
            read_premium_slot(deps.storage, bid_for.clone(), bid.clone().liq_premium)?;

        slot.total_bid_amount = slot.total_bid_amount - withdraw_amount;

        //User's share
        let refund_amount = withdraw_amount + claim_bid_residue(&mut slot);

        store_premium_slot(deps.storage, bid_for.clone(), slot)?;

        refund_amount
    };

    let mut msgs: Vec<CosmosMsg> = vec![];
    if !withdraw_amount.is_zero() {
        let mut queue = QUEUES.load(deps.storage, bid_for.to_string())?;

        let w_amount: u128 = withdraw_amount.into();

        //Store total bids
        queue.bid_asset.amount -= Uint128::from(w_amount);
        QUEUES.save(deps.storage, bid_for.to_string(), &queue)?;

        msgs.push(withdrawal_msg(
            Asset {
                amount: Uint128::from(w_amount),
                ..queue.bid_asset
            },
            info.sender,
        )?);
    }

    //Response builder
    let response = Response::new();
    Ok(response
        .add_attributes(vec![
            attr("method", "retract_bid"),
            attr("bid_for", bid_for.to_string()),
            attr("bid_id", bid_id.to_string()),
            attr("amount", withdraw_amount.to_string()),
        ])
        .add_messages(msgs))
}

/// Positions contract (owner) executes the liquidation and pays in the msg reply
/// This operation returns a repay_amount based on the available bids on each
/// premium slot, consuming bids from lowest to higher premium slots
#[allow(clippy::too_many_arguments)]
pub fn execute_liquidation(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    //All from Positions Contract
    collateral_amount: Uint256,
    bid_for: AssetInfo, //aka collateral_info
    collateral_price: PriceResponse,
    credit_price: PriceResponse,
    //For Repayment
    position_id: Uint128,
    position_owner: String,
) -> Result<Response, ContractError> {

    let config: Config = CONFIG.load(deps.storage)?;

    //Only Positions contract can execute
    if info.sender != config.positions_contract {
        return Err(ContractError::Unauthorized {});
    }
    
    //Get bid_with asset from Config
    let bid_with: AssetInfo = config.bid_asset;

    let queue = QUEUES.load(deps.storage, bid_for.to_string())?;
    if queue.bid_asset.info != bid_with {
        return Err(ContractError::Unauthorized {});
    }

    let mut price: PriceResponse256 = collateral_price.to_decimal256()?;

    let mut remaining_collateral_to_liquidate = collateral_amount;
    let mut repay_amount = Uint256::zero();
    let mut filled: bool = false;

    let max_premium_plus_1 = (queue.max_premium + Uint128::from(1u128)).u128();

    for premium in 0..max_premium_plus_1 {
        let mut slot: PremiumSlot =
            match read_premium_slot(deps.storage, bid_for.clone(), premium as u8) {
                Ok(slot) => slot,
                Err(_) => continue,
            };
        //Activates necessary bids for a new total
        slot = set_slot_total(deps.storage, slot, env.clone(), bid_for.clone())?;

        if slot.total_bid_amount.is_zero() {
            continue;
        };

        let (pool_repay_amount, pool_liquidated_collateral) = execute_pool_liquidation(
            deps.storage,
            &mut slot,
            premium as u8,
            bid_for.clone().to_string(), 
            remaining_collateral_to_liquidate,
            price.clone(),
            credit_price.to_decimal256()?,
            &mut filled,
        )?;

        store_premium_slot(deps.storage, bid_for.clone(), slot.clone())?;

        repay_amount += pool_repay_amount;

        if filled {
            remaining_collateral_to_liquidate = Uint256::zero();
            break;
        } else {
            remaining_collateral_to_liquidate =
                remaining_collateral_to_liquidate - pool_liquidated_collateral;
        }
    }

    //Because the Positions contract is querying balances beforehand, this should rarely occur
    if !remaining_collateral_to_liquidate.is_zero() {
        return Err(ContractError::InsufficientBids {});
    }

    //Repay for the user
    let r_amount: u128 = repay_amount.into();
    let repay_asset = Asset {
        amount: Uint128::new(r_amount),
        ..queue.bid_asset
    };

    //Have to reload Queue to use newly saved Slots
    let mut queue = QUEUES.load(deps.storage, bid_for.to_string())?;

    //Store total bids
    queue.bid_asset.amount = match queue.bid_asset.amount.checked_sub(Uint128::new(r_amount)) {
        Ok(amount) => amount,
        Err(_) => return Err(ContractError::InsufficientBids {}),
    };

    QUEUES.save(deps.storage, bid_for.to_string(), &queue)?;

    let repay_msg = CDP_ExecuteMsg::Repay {
        position_id,
        position_owner: Some(position_owner),
        send_excess_to: None,
    };

    let coin: Coin = asset_to_coin(repay_asset)?;

    let message = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.positions_contract.to_string(),
        msg: to_binary(&repay_msg)?,
        funds: vec![coin],
    });

    match bid_for {
        AssetInfo::Token { address: _ } => {
            Ok(Response::new().add_message(message).add_attributes(vec![
                attr("action", "execute_bid"),
                attr("denom", queue.bid_asset.info.to_string()),
                attr("repay_amount", repay_amount),
                attr("collateral_token", bid_for.to_string()),
                attr("collateral_info", "token"),
                attr("collateral_amount", collateral_amount),
            ]))
        }
        AssetInfo::NativeToken { denom: _ } => {
            Ok(Response::new().add_message(message).add_attributes(vec![
                attr("action", "execute_bid"),
                attr("denom", queue.bid_asset.info.to_string()),
                attr("repay_amount", repay_amount),
                attr("collateral_token", bid_for.to_string()),
                attr("collateral_info", "native_token"),
                attr("collateral_amount", collateral_amount),
            ]))
        }
    }
}

/// Bid owner can claim their share of the liquidated collateral until the
/// bid is consumed
pub fn claim_liquidations(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    bid_for: AssetInfo,
    bid_ids: Option<Vec<Uint128>>,
) -> Result<Response, ContractError> {

    let bids: Vec<Bid> = if let Some(bid_ids) = bid_ids {
        bid_ids
            .into_iter()
            .map(|id| read_bid(deps.storage, id, bid_for.clone()))
            .collect::<Result<Vec<Bid>, StdError>>()?
    } else {
        read_bids_by_user(
            deps.storage,
            bid_for.to_string(),
            info.clone().sender,
            None,
            None,
        )?
    };

    let mut claim_amount = Uint256::zero();

    for bid in bids.into_iter() {
        if bid.user != info.clone().sender {
            return Err(ContractError::Unauthorized {});
        }

        if bid.wait_end.is_some() && bid.wait_end.unwrap() > env.block.time.seconds() {
            // bid not activated
            continue;
        }

        let mut slot: PremiumSlot =
            read_premium_slot(deps.storage, bid_for.clone(), bid.clone().liq_premium)?;

        // calculate remaining bid amount
        let (remaining_bid, residue_bid) = calculate_remaining_bid(&bid, &slot)?;

        // calculate liquidated collateral
        let (liquidated_collateral, residue_collateral) = calculate_liquidated_collateral(
            deps.storage,
            &bid,
            bid_for.to_string(),
        )?;

        // keep residues
        slot.residue_collateral += residue_collateral;
        slot.residue_bid += residue_bid;

        // get claimable amount
        claim_amount += bid.pending_liquidated_collateral
            + liquidated_collateral
            + claim_col_residue(&mut slot);

        // store slot to update residue
        store_premium_slot(deps.storage, bid_for.clone(), slot.clone())?;

        // check if bid has been consumed, include 1 for rounding
        if remaining_bid <= Uint256::one() {
            remove_bid(deps.storage, bid, bid_for.clone())?;
        } else {
            store_bid(
                deps.storage,
                bid_for.clone(),
                Bid {
                    amount: remaining_bid,
                    product_snapshot: slot.product_snapshot,
                    sum_snapshot: slot.sum_snapshot,
                    scale_snapshot: slot.current_scale,
                    pending_liquidated_collateral: Uint256::zero(),
                    ..bid
                },
            )?;
        }
    }

    let mut messages: Vec<CosmosMsg> = vec![];
    if !claim_amount.is_zero() {
        let c_amount: u128 = claim_amount.into();

        let withdrawal_asset = Asset {
            info: bid_for.clone(),
            amount: Uint128::new(c_amount),
        };

        messages.push(withdrawal_msg(withdrawal_asset, info.sender)?);
    }

    Ok(Response::new().add_messages(messages).add_attributes(vec![
        attr("action", "claim_liquidations"),
        attr("collateral_token", bid_for.to_string()),
        attr("collateral_amount", claim_amount),
    ]))
}

/// On each collateral execution the product_snapshot and sum_snapshot are updated
/// to track the expense and reward distribution for biders in the pool
/// More details:
/// https://github.com/liquity/liquity/blob/master/papers/Scalable_Reward_Distribution_with_Compounding_Stakes.pdf
#[allow(clippy::too_many_arguments)]
fn execute_pool_liquidation(
    deps: &mut dyn Storage,
    slot: &mut PremiumSlot,
    premium: u8,
    bid_for: String,
    collateral_to_liquidate: Uint256,
    mut price: PriceResponse256,
    credit_price: PriceResponse256,
    filled: &mut bool,
) -> Result<(Uint256, Uint256), ContractError> {

    //price * (1- premium)
    let premium_price: Decimal256 = price.price * (Decimal256::one() - slot.liq_premium);
    //Update price 
    price.price = premium_price;
    
    let mut pool_collateral_to_liquidate: Uint256 = collateral_to_liquidate;
    
    let mut pool_required_stable: Uint256 = {
        let pool_collateral_value_to_liquidate = price.get_value(pool_collateral_to_liquidate);

        credit_price.get_amount(pool_collateral_value_to_liquidate)
    };

    
    if pool_required_stable > slot.total_bid_amount {
        pool_required_stable = slot.total_bid_amount;
        //Transform required stable to amount of collateral it can liquidate
        pool_collateral_to_liquidate = {
            let pool_required_stable_value = credit_price.get_value(pool_required_stable);

            price.get_amount(pool_required_stable_value)
        };
    } else {
        *filled = true;
    }

    // E / D
    let col_per_bid: Decimal256 = Decimal256::from_uint256(pool_collateral_to_liquidate)
        / Decimal256::from_uint256(slot.total_bid_amount);

    // Q / D
    let expense_per_bid: Decimal256 = Decimal256::from_uint256(pool_required_stable)
        / Decimal256::from_uint256(slot.total_bid_amount);

    ///////// Update sum /////////
    // E / D * P
    let sum = slot.product_snapshot * col_per_bid;

    // S + E / D * P
    slot.sum_snapshot += sum;
    slot.total_bid_amount = slot.total_bid_amount - pool_required_stable;

    
    // save reward sum for current epoch and scale
    store_epoch_scale_sum(
        deps,
        bid_for,
        premium,
        slot.current_epoch,
        slot.current_scale,
        slot.sum_snapshot,
    )?;

    ///////// Update product /////////
    // Check if the pool is emptied, if it is, reset (P = 1, S = 0)
    if expense_per_bid == Decimal256::one() {
        slot.sum_snapshot = Decimal256::zero();
        slot.product_snapshot = Decimal256::one();
        slot.current_scale = Uint128::zero();

        slot.current_epoch += Uint128::from(1u128);
    } else {
        // 1 - Q / D
        let product = Decimal256::one() - expense_per_bid;

        // check if scale needs to be increased (in case product truncates to zero)
        let new_product = slot.product_snapshot * product;
        slot.product_snapshot = if new_product < Decimal256(U256::from(1_000_000_000u64)) {
            slot.current_scale += Uint128::from(1u128);

            Decimal256(slot.product_snapshot.0 * U256::from(1_000_000_000u64)) * product
        } else {
            new_product
        };
    }

    Ok((pool_required_stable, pool_collateral_to_liquidate))
}

/// Calculate & update PremiumSlot total bid amount
pub(crate) fn set_slot_total(
    deps: &mut dyn Storage,
    mut slot: PremiumSlot,
    env: Env,
    bid_for: AssetInfo,
) -> Result<PremiumSlot, ContractError> {

    let mut queue = QUEUES.load(deps, bid_for.to_string())?;

    let block_time = env.block.time.seconds();

    let config = CONFIG.load(deps)?;

    //If elapsed time is less than wait_period && total is above threshold, don't recalculate/activate any bids
    //This can increase wait_period but decreases runtime for recurrent liquidations
    if (block_time - slot.last_total) < config.waiting_period
        && slot.total_bid_amount >= queue.bid_threshold
    {
        return (Ok(slot));
    }

    let edited_bids: Vec<Bid> = slot
        .clone()
        .waiting_bids
        .into_iter()
        .map(|mut bid| {
            //IF the bid is past the wait time, activate it
            if bid.wait_end.unwrap() <= block_time {
                let b_amount: u128 = bid.amount.into();
                queue.bid_asset.amount += Uint128::new(b_amount);

                slot.total_bid_amount += bid.amount;

                process_bid_activation(&mut bid, &mut slot);

                //Add bid to active bid list
                slot.bids.push(bid.clone());

                //Set bid amount to 0 so we can filter it out at the end
                bid.amount = Uint256::zero();

            //IF the slot total is less than the threshold, activate the bid
            } else if slot.total_bid_amount <= queue.bid_threshold {
                let b_amount: u128 = bid.amount.into();
                queue.bid_asset.amount += Uint128::new(b_amount);

                slot.total_bid_amount += bid.amount;

                process_bid_activation(&mut bid, &mut slot);

                //Add bid to active bid list
                slot.bids.push(bid.clone());

                //Set bid amount to 0 so we can filter it out at the end
                bid.amount = Uint256::zero();
            }
            bid
        })
        .collect::<Vec<Bid>>()
        .into_iter()
        .filter(|bid| !bid.amount.is_zero())
        .collect::<Vec<Bid>>();

    slot.waiting_bids = edited_bids;

    QUEUES.save(deps, bid_for.to_string(), &queue)?;

    //Set the last_total time
    slot.last_total = block_time;

    Ok(slot)
}

/// Claim residue bids due to bid type conversions
fn claim_bid_residue(slot: &mut PremiumSlot) -> Uint256 {
    let claimable = slot.residue_bid * Uint256::one();

    if !claimable.is_zero() {
        slot.residue_bid = slot.residue_bid - Decimal256::from_uint256(claimable);
    }
    claimable
}

/// Claim residue collateral due to collateral type conversions
fn claim_col_residue(slot: &mut PremiumSlot) -> Uint256 {
    let claimable = slot.residue_collateral * Uint256::one();

    if !claimable.is_zero() {
        slot.residue_collateral = slot.residue_collateral - Decimal256::from_uint256(claimable);
    }
    claimable
}

/// Calculate the amount of collateral to liquidate
pub fn calculate_liquidated_collateral(
    deps: &dyn Storage,
    bid: &Bid,
    bid_for: String,
) -> StdResult<(Uint256, Decimal256)> {

    let reference_sum_snapshot = read_epoch_scale_sum(
        deps,
        &bid_for,
        bid.liq_premium,
        bid.epoch_snapshot,
        bid.scale_snapshot,
    )
    .unwrap_or_default();

    // reward = reward from first scale + reward from second scale (if any)
    let first_portion = reference_sum_snapshot - bid.sum_snapshot;
    let second_portion = if let Ok(second_scale_sum_snapshot) = read_epoch_scale_sum(
        deps,
        &bid_for,
        bid.liq_premium,
        bid.epoch_snapshot,
        bid.scale_snapshot + Uint128::from(1u128),
    ) {
        Decimal256(
            (second_scale_sum_snapshot.0 - reference_sum_snapshot.0) / U256::from(1_000_000_000u64),
        )
    } else {
        Decimal256::zero()
    };

    let liquidated_collateral_dec = Decimal256::from_uint256(bid.amount)
        * (first_portion + second_portion)
        / bid.product_snapshot;

    let liquidated_collateral = liquidated_collateral_dec * Uint256::one();
    // stacks the residue when converting to integer
    let residue_collateral =
        liquidated_collateral_dec - Decimal256::from_uint256(liquidated_collateral);

    Ok((liquidated_collateral, residue_collateral))
}

/// Store epoch scale sum
pub fn store_epoch_scale_sum(
    deps: &mut dyn Storage,
    bid_for: String,
    premium_slot: u8,
    epoch: Uint128,
    scale: Uint128,
    sum: Decimal256,
) -> StdResult<()> {
    let mut epoch_scale_sum: Bucket<Decimal256> = Bucket::multilevel(
        deps,
        &[
            PREFIX_EPOCH_SCALE_SUM,
            &bid_for.as_bytes(),
            &premium_slot.to_be_bytes(),
            &epoch.u128().to_be_bytes(),
        ],
    );
    epoch_scale_sum.save(&scale.u128().to_be_bytes(), &sum)
}

/// Read epoch scale sum
pub fn read_epoch_scale_sum(
    deps: &dyn Storage,
    bid_for: &String,
    premium: u8,
    epoch: Uint128,
    scale: Uint128,
) -> StdResult<Decimal256> {
    let epoch_scale_sum: ReadonlyBucket<Decimal256> = ReadonlyBucket::multilevel(
        deps,
        &[
            PREFIX_EPOCH_SCALE_SUM,
            bid_for.as_bytes(),
            &premium.to_be_bytes(),
            &epoch.u128().to_be_bytes(),
        ],
    );

    epoch_scale_sum.load(&scale.u128().to_be_bytes())
}

/// Calculate the remaining bid amount after a scale change, i.e. a liquidation or a bid activation
pub fn calculate_remaining_bid(bid: &Bid, slot: &PremiumSlot) -> StdResult<(Uint256, Decimal256)> {
    let scale_diff: Uint128 = slot.current_scale.checked_sub(bid.scale_snapshot)?;
    let epoch_diff: Uint128 = slot.current_epoch.checked_sub(bid.epoch_snapshot)?;

    let remaining_bid_dec: Decimal256 = if !epoch_diff.is_zero() {
        // pool was emptied, return 0
        Decimal256::zero()
    } else if scale_diff.is_zero() {
        Decimal256::from_uint256(bid.amount) * slot.product_snapshot / bid.product_snapshot
    } else if scale_diff == Uint128::from(1u128) {
        // product has been scaled
        let scaled_remaining_bid =
            Decimal256::from_uint256(bid.amount) * slot.product_snapshot / bid.product_snapshot;

        Decimal256(scaled_remaining_bid.0 / U256::from(1_000_000_000u64))
    } else {
        Decimal256::zero()
    };

    let remaining_bid = remaining_bid_dec * Uint256::one();
    // stacks the residue when converting to integer
    let bid_residue = remaining_bid_dec - Decimal256::from_uint256(remaining_bid);

    Ok((remaining_bid, bid_residue))
}

/// Read premium slot
pub fn read_premium_slot(
    deps: &dyn Storage,
    bid_for: AssetInfo,
    premium: u8,
) -> StdResult<PremiumSlot> {
    let queue = QUEUES.load(deps, bid_for.to_string())?;    

    let slot = match queue.clone()
        .slots
        .into_iter()
        .find(|slot| slot.liq_premium == Decimal256::percent(premium as u64))
    {
        //Hard coded 1% per slot
        Some(slot) => slot,
        None => {
            return Err(StdError::GenericErr {
                msg: "Invalid premium".to_string(),
            })
        }
    };

    Ok(slot)
}

/// Store premium slot
fn store_premium_slot(
    deps: &mut dyn Storage,
    bid_for: AssetInfo,
    slot: PremiumSlot,
) -> Result<(), ContractError> {

    let mut queue = QUEUES.load(deps, bid_for.to_string())?;

    //Filter the old slot out
    let mut new_slots: Vec<PremiumSlot> = queue
        .slots
        .into_iter()
        .filter(|temp_slot| temp_slot.liq_premium != slot.liq_premium)
        .collect::<Vec<PremiumSlot>>();

    //Add updated slot to new_slots
    new_slots.push(slot);

    //Set
    queue.slots = new_slots;

    //Update
    QUEUES.update(
        deps,
        bid_for.to_string(),
        |old_queue| -> Result<Queue, ContractError> {
            match old_queue {
                Some(_old_queue) => Ok(queue),
                None => Err(ContractError::InvalidAsset {}),
            }
        },
    )?;

    Ok(())
}

/// Remove bid from premium slot
fn remove_bid(deps: &mut dyn Storage, bid: Bid, bid_for: AssetInfo) -> Result<(), ContractError> {

    //load Queue
    let mut queue = QUEUES.load(deps, bid_for.to_string())?;

    //Get premium_slot to edit
    let some_slot: Option<PremiumSlot> = queue
        .clone()
        .slots
        .into_iter()
        .filter(|slot| {
            slot.liq_premium
                == Decimal256::percent(1)
                    * Decimal256::from_uint256(Uint256::from(bid.liq_premium as u128))
        }) //Hard coded 1% per slot
        .next();
    let mut slot = match some_slot {
        Some(slot) => slot,
        None => return Err(ContractError::InvalidPremium {}),
    };

    //Filter bid from said slot if active
    let new_bids: Vec<Bid> = slot
        .bids
        .into_iter()
        .filter(|temp_bid| temp_bid.id != bid.id)
        .collect::<Vec<Bid>>();

    //Set
    slot.bids = new_bids;

    //Filter bid from said slot if waiting
    let new_bids: Vec<Bid> = slot
        .waiting_bids
        .into_iter()
        .filter(|temp_bid| temp_bid.id != bid.id)
        .collect::<Vec<Bid>>();

    //Set
    slot.waiting_bids = new_bids;

    //Filter for all slots except the edited one and then push the new slot
    let mut slots: Vec<PremiumSlot> = queue
        .slots
        .into_iter()
        .filter(|slot| {
            slot.liq_premium
                != Decimal256::percent(1)
                    * Decimal256::from_uint256(Uint256::from(bid.liq_premium as u128))
        }) //Hard coded 1% per slot
        .collect::<Vec<PremiumSlot>>();

    slots.push(slot);

    //Set
    queue.slots = slots;

    //Update Queue to save slots
    QUEUES.save(deps, bid_for.to_string(), &queue)?;

    Ok(())
}

/// Store bid in premium slot
fn store_bid(deps: &mut dyn Storage, bid_for: AssetInfo, bid: Bid) -> Result<(), ContractError> {
    let mut queue = QUEUES.load(deps, bid_for.to_string())?;

    //Get premium_slot to edit
    let some_slot: Option<PremiumSlot> = queue
        .clone()
        .slots
        .into_iter()
        .filter(|slot| {
            slot.liq_premium
                == Decimal256::percent(1)
                    * Decimal256::from_uint256(Uint256::from(bid.liq_premium as u128))
        }) //Hard coded 1% per slot
        .next();
    let mut slot = match some_slot {
        Some(slot) => slot,
        None => return Err(ContractError::InvalidPremium {}),
    };

    //Store bid in slot list depending on if it is active or waiting
    if bid.wait_end.is_some(){
        //Filter bid from said slot if waiting
        let mut new_bids: Vec<Bid> = slot
            .waiting_bids
            .into_iter()
            .filter(|temp_bid| temp_bid.id != bid.id)
            .collect::<Vec<Bid>>();
        //Push new bid
        new_bids.push(bid.clone());

        //Set
        slot.waiting_bids = new_bids;
    } else {
        //Filter bid from said slot and push new bid
        let mut new_bids: Vec<Bid> = slot
            .bids
            .into_iter()
            .filter(|temp_bid| temp_bid.id != bid.id)
            .collect::<Vec<Bid>>();
        //Push new bid
        new_bids.push(bid.clone());

        //Set
        slot.bids = new_bids;
    }


    //Filter for all slots except the edited one and then push the new slot
    let mut slots: Vec<PremiumSlot> = queue
        .slots
        .into_iter()
        .filter(|slot| {
            slot.liq_premium
                != Decimal256::percent(1)
                    * Decimal256::from_uint256(Uint256::from(bid.liq_premium as u128))
        }) //Hard coded 1% per slot
        .collect::<Vec<PremiumSlot>>();

    slots.push(slot);

    //Set
    queue.slots = slots;

    //Update Queue to save slots
    QUEUES.save(deps, bid_for.to_string(), &queue)?;

    Ok(())
}

/// Validate withdrawal amount
fn assert_withdraw_amount(
    withdraw_amount: Option<Uint256>,
    withdrawable_amount: Uint256,
    minimum_bid: Uint256,
) -> Result<Uint256, ContractError> {
    let withdrawal_amount = match withdraw_amount {
        Some(withdraw_amount) => {
            if withdraw_amount > withdrawable_amount {
                return Err(ContractError::InvalidWithdrawal {});
            //Less than minimum bid & greater than 0
            } else if withdrawable_amount - withdraw_amount < minimum_bid && withdrawable_amount - withdraw_amount > Uint256::zero(){
                return Err(ContractError::InvalidWithdrawal {});
            }
            
            withdraw_amount
        }
        None => withdrawable_amount,
    };

    Ok(withdrawal_amount)
}

/// Return Bid from storage
pub fn read_bid(deps: &dyn Storage, bid_id: Uint128, bid_for: AssetInfo) -> StdResult<Bid> {
    let mut read_bid: Option<Bid> = None;

    let queue = QUEUES.load(deps, bid_for.to_string())?;

    let premium_range = 0..(queue.max_premium.u128() as u8 + 1u8);

    for premium in premium_range {
        let slot = read_premium_slot(deps, bid_for.clone(), premium)?;

        match slot.bids.into_iter().find(|bid| bid.id.eq(&bid_id)) {
            Some(bid) => read_bid = Some(bid),
            None => {
                //Check in waiting bids
                match slot.waiting_bids.into_iter().find(|bid| bid.id.eq(&bid_id)) {
                    Some(bid) => read_bid = Some(bid),
                    None => { }
                }
            }
        }

        if read_bid.is_some() {
            break;
        }
    }

    if read_bid.is_none() {
        return Err(StdError::GenericErr {
            msg: "Bid not found".to_string(),
        });
    }

    Ok(read_bid.unwrap())
}

/// Return Bids for a user
pub fn read_bids_by_user(
    deps: &dyn Storage,
    bid_for: String,
    user: Addr,
    limit: Option<u32>,
    start_after: Option<Uint128>, //bid.id
) -> StdResult<Vec<Bid>> {
    
    let mut read_bids: Vec<Bid> = vec![];
    let limit = limit.unwrap_or(MAX_LIMIT) as usize;
    let start = start_after.unwrap_or_else(Uint128::zero);

    let queue = QUEUES.load(deps, bid_for)?;

    for slot in queue.slots {
        read_bids.extend(
            slot.bids
                .into_iter()
                .filter(|bid| bid.id > start)
                .filter(|bid| bid.user == user)
                .collect::<Vec<Bid>>(),
        );
    }

    read_bids = read_bids.into_iter().take(limit).collect::<Vec<Bid>>();

    Ok(read_bids)
}

/// Validate bid input
pub fn validate_bid_input(deps: &dyn Storage, bid_input: BidInput) -> Result<(), ContractError> {
    match QUEUES.load(deps, bid_input.bid_for.to_string()) {
        Ok(queue) => {
            if bid_input.liq_premium <= queue.max_premium.u128() as u8
            {
                Ok(())
            } else {
                Err(ContractError::InvalidPremium {})
            }
        }
        Err(_) => Err(ContractError::InvalidAsset {}),
    }
}