use cosmwasm_std::{
    attr, to_json_binary, Addr, BankMsg, Coin, CosmosMsg, Decimal, DepsMut, Env, Int128, MessageInfo, QueryRequest, Response, StdError, StdResult, Storage, SubMsg, Uint128, WasmMsg, WasmQuery, QuerierWrapper
};
use std::str::FromStr;
use std::collections::HashMap;
use membrane::cdp::{LiquidationStatResponse, QueryMsg as CDP_QueryMsg};
use membrane::emissions_voting::{HasAnyVotesResponse, QueryMsg as EmissionsVotingQueryMsg};
use membrane::ltv_disco::{
    BackingDeposit, BackingDepositInput, RevenueTrackingEntry, RevenueEvent, UserLifetimeRevenueEntry, Config, DecimalMinMax, Dispersal, ActiveDispersal, LTVQueue, MaxBorrowLTVGroup, MaxLTVSlot, ExecuteMsg, TVLEntry, LTVEntry, CompoundAction, LVTTimeCliff, DepositLVTTracking, GroupLVTTracking, InsuranceEntry
};
use membrane::math::{decimal_division, decimal_multiplication};
use membrane::types::{Asset, Basket, DepositDenom, AssetInfo};
use membrane::stability_pool_vault::{calculate_base_tokens, calculate_vault_tokens};
use membrane::cdp::ExecuteMsg as CDP_ExecuteMsg;
use membrane::oracle::{QueryMsg as Oracle_QueryMsg, PriceResponse};
use membrane::osmosis_proxy::ExecuteMsg as OsmosisProxy_ExecuteMsg;
use membrane::neutron_proxy::ExecuteMsg as NeutronProxy_ExecuteMsg;
use membrane::revenue_distributor::{QueryMsg as RevenueDistributorQueryMsg, EpochCountdownResponse};

use crate::error::ContractError;
use crate::state::{SwapPropagation, SWAP_PROPAGATION, CompoundPropagation, COMPOUND_PROPAGATION, REVENUE_TRACKING, RATE_ASSURANCE, REVENUE_EVENTS, USER_LIFETIME_REVENUE, BACKING_DEPOSITS, USER_DEPOSITS, CONFIG, DISPERSAL, LTV_QUEUES, DAILY_TVL_TRACKER, DAILY_LTV_TRACKER, DAILY_INSURANCE_TRACKER, USER_TOTAL_DEPOSITS, MANAGER_FEE, MANAGED_DEPOSITS};

const REVENUE_TRACKING_LIMIT: usize = 100; // Limit for revenue tracking vectors
const LIFETIME_REVENUE_LIMIT: usize = 100; // Limit for user lifetime revenue tracking
const TVL_TRACKER_LIMIT: usize = 100; // Limit for TVL tracker entries
const LTV_TRACKER_LIMIT: usize = 100; // Limit for LTV tracker entries
const INSURANCE_TRACKER_LIMIT: usize = 100; // Limit for insurance tracker entries
const ONE_DAY_SECONDS: u64 = 86400; // 24 hours in seconds

use crate::state::USER_LOCKED_DEPOSITS;
use membrane::ltv_disco::LockedDeposit;

/// Calculate lock days for a deposit
/// Returns the number of days remaining in the lock, or 0 if not locked or expired
fn calculate_lock_days(deposit: &BackingDeposit, env: &Env) -> u64 {
    if let Some(ref locked) = deposit.locked {
        let current_time = env.block.time.seconds();
        if locked.locked_until > current_time {
            return (locked.locked_until - current_time) / ONE_DAY_SECONDS;
        }
    }
    0
}

/// Calculate base locked vault tokens for a deposit (without epoch discount)
/// Formula: vault_tokens * (lock_days + 1)
/// This ensures unlocked deposits (lock_days = 0) contribute vault_tokens * 1 = vault_tokens
/// Locked deposits get boosted: vault_tokens * (lock_days + 1)
/// 
/// Note: This function calculates the base value. Epoch-based discounts are applied separately
/// in `calculate_unused_locked_vault_tokens` when epoch information is available.
fn calculate_locked_vault_tokens(deposit: &BackingDeposit, env: &Env) -> Uint128 {
    let lock_days = calculate_lock_days(deposit, env);
    deposit.vault_tokens * Uint128::from(lock_days + 1)
}

/// Calculate unused locked vault tokens (lost weight from late deposit)
/// This is used when we have epoch information from the revenue distributor
/// Formula: base_locked_vt * penalty_weight, where penalty_weight = (deposit_time - epoch_start) / epoch_duration
/// Returns the amount of locked vault tokens that should NOT count towards revenue distribution
fn calculate_unused_locked_vault_tokens(
    deposit: &BackingDeposit,
    env: &Env,
    epoch_start: u64,
    epoch_end: u64,
    _current_time: u64,
) -> Uint128 {
    let lock_days = calculate_lock_days(deposit, env);
    let base_locked_vt = deposit.vault_tokens * Uint128::from(lock_days + 1);

    // Calculate unused (lost weight) if deposit_time is available
    if let Some(deposit_time) = deposit.deposit_time {
        let epoch_duration = epoch_end.saturating_sub(epoch_start);
        if epoch_duration > 0 {
            let time_passed = deposit_time.saturating_sub(epoch_start);
            // Calculate penalty: time_passed / epoch_duration
            // The later in the epoch the deposit was made, the more weight is lost
            // Use Decimal for precision
            let penalty_weight = if time_passed > 0 && epoch_duration > 0 {
                Decimal::from_ratio(time_passed, epoch_duration)
            } else {
                Decimal::zero()  // Deposit at epoch start loses nothing
            };

            // Calculate unused: base * penalty_weight
            let unused = decimal_multiplication(
                Decimal::from_ratio(base_locked_vt, Uint128::one()),
                penalty_weight,
            ).unwrap_or(Decimal::zero());

            return unused.to_uint_floor();
        }
    }

    // No deposit_time or invalid epoch: no penalty (deposit at epoch start)
    Uint128::zero()
}

// ============= LVT TIME-CLIFF TRACKING FUNCTIONS =============

/// Calculate LVT at a specific timestamp from tracking data
/// Supports reverse time (timestamp < reference_time) for late claimers
///
/// `reference_time` is the timestamp where `base_lvt` and `base_daily_delta` are known.
/// This is the starting point for all calculations. If `timestamp == reference_time`,
/// the function returns `base_lvt` directly. Otherwise, it applies the daily delta
/// and processes time cliffs to calculate LVT at the requested timestamp.
///
/// The function supports both forward time (timestamp > reference_time) and reverse
/// time (timestamp < reference_time), enabling accurate revenue calculations for
/// late claimers who claim revenue for past events.
fn calculate_lvt_at_time(
    base_lvt: Uint128,
    reference_time: u64,
    base_daily_delta: Int128,
    time_cliffs: &[LVTTimeCliff],
    timestamp: u64,
) -> Result<Uint128, ContractError> {
    if timestamp == reference_time {
        return Ok(base_lvt);
    }
    
    let mut current_lvt = Int128::from(base_lvt.u128() as i128);
    let mut current_daily_delta = base_daily_delta;
    
    if timestamp > reference_time {
        // Forward time: process cliffs up to timestamp
        let mut last_time = reference_time;
        
        for cliff in time_cliffs {
            if cliff.timestamp > timestamp {
                break;
            }
            
            // Apply delta from last_time to cliff.timestamp
            let days_elapsed = (cliff.timestamp - last_time) / ONE_DAY_SECONDS;
            current_lvt = current_lvt + (current_daily_delta * Int128::from(days_elapsed as i128));
            
            // Update daily delta
            current_daily_delta = current_daily_delta + cliff.delta_change;
            last_time = cliff.timestamp;
        }
        
        // Apply delta from last cliff to timestamp
        let days_elapsed = (timestamp - last_time) / ONE_DAY_SECONDS;
        current_lvt = current_lvt + (current_daily_delta * Int128::from(days_elapsed as i128));
    } else {
        // Reverse time: go backwards from reference_time
        let mut last_time = reference_time;
        
        // Process cliffs in reverse order
        for cliff in time_cliffs.iter().rev() {
            if cliff.timestamp > last_time {
                continue;
            }
            if cliff.timestamp <= timestamp {
                break;
            }
            
            // Reverse the delta from last_time to cliff.timestamp
            let days_elapsed = (last_time - cliff.timestamp) / ONE_DAY_SECONDS;
            // Use current delta for the interval after the cliff
            current_lvt = current_lvt - (current_daily_delta * Int128::from(days_elapsed as i128));
            
            // Update daily delta (reverse the change at the cliff)
            current_daily_delta = current_daily_delta - cliff.delta_change;
            last_time = cliff.timestamp;
        }
        
        // Apply delta from last cliff to timestamp
        let days_elapsed = (last_time - timestamp) / ONE_DAY_SECONDS;
        current_lvt = current_lvt - (current_daily_delta * Int128::from(days_elapsed as i128));
    }
    
    // Ensure non-negative and convert to Uint128
    if current_lvt.is_negative() {
        Ok(Uint128::zero())
    } else {
        Ok(Uint128::from(current_lvt.i128() as u128))
    }
}

/// Calculate deposit's contribution to LVT tracking
/// Returns: (base_lvt, daily_delta, cliffs)
fn calculate_deposit_contribution(
    deposit: &BackingDeposit,
    env: &Env,
    lock_ceiling: u64,
) -> Result<(Uint128, Int128, Vec<LVTTimeCliff>), ContractError> {
    let vault_tokens = deposit.vault_tokens;
    let current_time = env.block.time.seconds();
    
    // Handle perpetual locks: treat as constant until explicitly changed
    let (locked_until, is_perpetual) = if let Some(ref locked) = deposit.locked {
        if let Some(perpetual_days) = locked.perpetual_lock {
            // Perpetual: treat as constant boost (locked_until stays in future)
            let virtual_locked_until = current_time + perpetual_days * ONE_DAY_SECONDS;
            (virtual_locked_until, true)
        } else {
            (locked.locked_until, false)
        }
    } else {
        // Unlocked: only time boost applies
        return Ok((
            vault_tokens, // Base: 1x multiplier
            Int128::from(vault_tokens.u128() as i128), // Daily delta: +vault_tokens/day
            vec![] // No cliffs
        ));
    };
    
    // Calculate current lock days
    let lock_days = if locked_until > current_time {
        (locked_until - current_time) / ONE_DAY_SECONDS
    } else {
        0
    };
    
    // Current LVT using formula: vault_tokens * (lock_days + 1)
    let current_lvt = vault_tokens * Uint128::from(lock_days + 1);
    
    // Calculate daily delta
    let daily_delta = if is_perpetual {
        // Perpetual lock: lock_ratio constant, time_ratio increases
        // Daily delta = +vault_tokens (time boost increases)
        Int128::from(vault_tokens.u128() as i128)
    } else if locked_until > current_time {
        // During lock: lock decaying (-vault_tokens/day), time increasing (+vault_tokens/day)
        // Net = 0 during lock period
        Int128::zero()
    } else {
        // After lock expires: only time boost
        Int128::from(vault_tokens.u128() as i128)
    };
    
    // Create cliffs
    let mut cliffs = Vec::new();
    
    if !is_perpetual && locked_until > current_time {
        // Regular lock will expire, create cliff
        cliffs.push(LVTTimeCliff {
            timestamp: locked_until,
            delta_change: Int128::from(vault_tokens.u128() as i128), // Changes from 0 to +vault_tokens
        });
    }
    
    Ok((current_lvt, daily_delta, cliffs))
}

/// Initialize deposit's LVT tracking for a new deposit
fn initialize_deposit_lvt_tracking(
    deposit: &mut BackingDeposit,
    env: &Env,
    lock_ceiling: u64,
) -> Result<(), ContractError> {
    let reference_time = env.block.time.seconds();
    let (base_lvt, daily_delta, cliffs) = calculate_deposit_contribution(
        deposit,
        env,
        lock_ceiling,
    )?;
    
    deposit.lvt_tracking = DepositLVTTracking {
        base_lvt,
        reference_time,
        daily_delta,
        time_cliffs: cliffs,
    };
    
    Ok(())
}

/// Update deposit's LVT tracking when it changes
fn update_deposit_lvt_tracking(
    deposit: &mut BackingDeposit,
    env: &Env,
    lock_ceiling: u64,
) -> Result<(), ContractError> {
    let reference_time = env.block.time.seconds();
    let (base_lvt, daily_delta, cliffs) = calculate_deposit_contribution(
        deposit,
        env,
        lock_ceiling,
    )?;
    
    // Adjust base_lvt to new reference_time if needed
    let adjusted_base_lvt = if reference_time != deposit.lvt_tracking.reference_time {
        calculate_lvt_at_time(
            deposit.lvt_tracking.base_lvt,
            deposit.lvt_tracking.reference_time,
            deposit.lvt_tracking.daily_delta,
            &deposit.lvt_tracking.time_cliffs,
            reference_time,
        )?
    } else {
        base_lvt
    };
    
    deposit.lvt_tracking = DepositLVTTracking {
        base_lvt: adjusted_base_lvt,
        reference_time,
        daily_delta,
        time_cliffs: cliffs,
    };
    
    Ok(())
}

/// Update group LVT tracking when a deposit changes
/// old_deposit: None if new deposit, Some if deposit existed
/// new_deposit: None if deposit removed, Some if deposit exists
fn update_group_lvt_tracking_for_deposit(
    storage: &mut dyn Storage,
    env: &Env,
    asset: &str,
    ltv: Decimal,
    max_borrow_ltv: Decimal,
    old_deposit: Option<&BackingDeposit>,
    new_deposit: Option<&BackingDeposit>,
) -> Result<(), ContractError> {
    let mut queue = LTV_QUEUES.load(storage, asset.to_string())?;
    let slot_index = queue.slots.iter().position(|s| s.ltv == ltv)
        .ok_or_else(|| ContractError::CustomError { val: "Slot not found".to_string() })?;
    let mut slot = queue.slots[slot_index].clone();
    let group_index = slot.deposit_groups.iter().position(|g| g.max_borrow_ltv == max_borrow_ltv)
        .ok_or_else(|| ContractError::CustomError { val: "Group not found".to_string() })?;
    let mut group = slot.deposit_groups[group_index].clone();
    
    let reference_time = env.block.time.seconds();
    let mut tracking = group.lvt_tracking.clone();
    
    // Remove old contribution if deposit existed
    if let Some(old_dep) = old_deposit {
        let old_tracking = &old_dep.lvt_tracking;
        
        // Adjust old tracking to current reference_time
        let old_lvt_at_ref = calculate_lvt_at_time(
            old_tracking.base_lvt,
            old_tracking.reference_time,
            old_tracking.daily_delta,
            &old_tracking.time_cliffs,
            reference_time,
        )?;
        
        // Subtract from base
        tracking.base_total = tracking.base_total.saturating_sub(old_lvt_at_ref);
        tracking.base_daily_delta = tracking.base_daily_delta - old_tracking.daily_delta;
        
        // Remove old cliffs (subtract their delta_change)
        for old_cliff in &old_tracking.time_cliffs {
            if let Some(pos) = tracking.time_cliffs.iter().position(|c| c.timestamp == old_cliff.timestamp) {
                tracking.time_cliffs[pos].delta_change = tracking.time_cliffs[pos].delta_change - old_cliff.delta_change;
                // Remove if delta_change becomes zero
                if tracking.time_cliffs[pos].delta_change.is_zero() {
                    tracking.time_cliffs.remove(pos);
                }
            } else {
                // Add inverse cliff
                tracking.time_cliffs.push(LVTTimeCliff {
                    timestamp: old_cliff.timestamp,
                    delta_change: Int128::zero() - old_cliff.delta_change,
                });
            }
        }
    }
    
    // Add new contribution if deposit exists
    if let Some(new_dep) = new_deposit {
        let new_tracking = &new_dep.lvt_tracking;
        
        // Adjust new tracking to current reference_time
        let new_lvt_at_ref = calculate_lvt_at_time(
            new_tracking.base_lvt,
            new_tracking.reference_time,
            new_tracking.daily_delta,
            &new_tracking.time_cliffs,
            reference_time,
        )?;
        
        // Add to base
        tracking.base_total = tracking.base_total.checked_add(new_lvt_at_ref)
            .map_err(|e| ContractError::CustomError { 
                val: format!("Overflow adding to base_total: {}", e) 
            })?;
        tracking.base_daily_delta = tracking.base_daily_delta + new_tracking.daily_delta;
        
        // Add new cliffs
        for new_cliff in &new_tracking.time_cliffs {
            if let Some(pos) = tracking.time_cliffs.iter().position(|c| c.timestamp == new_cliff.timestamp) {
                tracking.time_cliffs[pos].delta_change = tracking.time_cliffs[pos].delta_change + new_cliff.delta_change;
                // Remove if delta_change becomes zero
                if tracking.time_cliffs[pos].delta_change.is_zero() {
                    tracking.time_cliffs.remove(pos);
                }
            } else {
                tracking.time_cliffs.push(new_cliff.clone());
            }
        }
    }
    
    // Update reference_time
    tracking.reference_time = reference_time;
    
    // Sort cliffs by timestamp
    tracking.time_cliffs.sort_by_key(|c| c.timestamp);
    
    // Save updated tracking
    group.lvt_tracking = tracking;
    slot.deposit_groups[group_index] = group;
    queue.slots[slot_index] = slot;
    LTV_QUEUES.save(storage, asset.to_string(), &queue)?;
    
    Ok(())
}

// ============= END LVT TIME-CLIFF TRACKING FUNCTIONS =============

/// Update group's total_unused_locked_vault_tokens when a deposit changes
/// This should be called whenever a deposit's locked vault tokens change
/// Only edits if the deposit is made within the current epoch.
/// Resets unused totals when the epochs change
fn update_group_unused_total(
    storage: &mut dyn Storage,
    querier: &QuerierWrapper,
    env: &Env,
    config: &Config,
    asset: &str,
    ltv: Decimal,
    max_borrow_ltv: Decimal,
    old_unused_vt: Uint128,
    new_unused_vt: Uint128,
    deposit_time: Option<u64>,
) -> Result<(), ContractError> {
    // Query current epoch from revenue distributor if available
    let epoch_info = if let Some(revenue_distributor_addr) = &config.revenue_distributor {
        querier.query_wasm_smart::<EpochCountdownResponse>(
            revenue_distributor_addr,
            &RevenueDistributorQueryMsg::EpochCountdown {},
        )
        .ok()
        .map(|countdown| (countdown.epoch_start, countdown.epoch_end, env.block.time.seconds()))
    } else {
        None
    };

    let mut queue = LTV_QUEUES.load(storage, asset.to_string())?;
    let slot_index = queue.slots.iter().position(|s| s.ltv == ltv)
        .ok_or_else(|| ContractError::CustomError { val: "Slot not found".to_string() })?;
    let mut slot = queue.slots[slot_index].clone();
    let group_index = find_or_create_borrow_group(&mut slot, max_borrow_ltv, false)?;
    let mut group = slot.deposit_groups[group_index].clone();

    // Check if epoch changed - if so, reset unused total
    if let Some((epoch_start, _, _)) = epoch_info {
        if group.effective_epoch_start.map(|e| e != epoch_start).unwrap_or(true) {
            // New epoch detected - reset unused total to zero
            group.total_unused_locked_vault_tokens = Uint128::zero();
            group.effective_epoch_start = Some(epoch_start);
        }
    }

    // Update unused total by delta (only if deposit is in current epoch)
    if let Some((epoch_start, epoch_end, _)) = epoch_info {
        // Check if deposit was made in current epoch
        let is_in_current_epoch = if let Some(dep_time) = deposit_time {
            dep_time >= epoch_start && dep_time < epoch_end
        } else {
            false
        };

        if is_in_current_epoch {
            // Only update if deposit is in current epoch
            if new_unused_vt > old_unused_vt {
                let delta = new_unused_vt.checked_sub(old_unused_vt)
                    .map_err(|e| ContractError::CustomError {
                        val: format!("Overflow calculating unused_vt delta: {}", e)
                    })?;
                group.total_unused_locked_vault_tokens = group.total_unused_locked_vault_tokens
                    .checked_add(delta)
                    .map_err(|e| ContractError::CustomError {
                        val: format!("Overflow updating total_unused_locked_vault_tokens: {}", e)
                    })?;
            } else if new_unused_vt < old_unused_vt {
                let delta = old_unused_vt.checked_sub(new_unused_vt)
                    .map_err(|e| ContractError::CustomError {
                        val: format!("Overflow calculating unused_vt delta: {}", e)
                    })?;
                group.total_unused_locked_vault_tokens = group.total_unused_locked_vault_tokens
                    .saturating_sub(delta);  // Use saturating_sub to avoid underflow
            }
        }
    }

    slot.deposit_groups[group_index] = group;
    queue.slots[slot_index] = slot;
    LTV_QUEUES.save(storage, asset.to_string(), &queue)?;

    Ok(())
}

/// Refresh deposit lock if it has perpetual_lock
fn refresh_deposit_lock(
    deposit: &mut BackingDeposit,
    env: &Env,
    lock_ceiling: u64,
) -> Result<(), ContractError> {
    if let Some(ref mut locked) = deposit.locked {
        if let Some(perpetual_days) = locked.perpetual_lock {
            // Calculate new locked_until: current_time + perpetual_lock days
            let new_locked_until = env.block.time.seconds() + perpetual_days * ONE_DAY_SECONDS;
            
            // Calculate max allowed lock time (from start_time)
            let max_lock_time = env.block.time.seconds() + (lock_ceiling * ONE_DAY_SECONDS);
            
            // Extend lock, but don't exceed ceiling
            locked.locked_until = std::cmp::min(new_locked_until, max_lock_time);
        }
    }
    Ok(())
}

/// Add a locked deposit to the user's locked deposits list
fn add_locked_deposit(
    storage: &mut dyn Storage,
    user: &Addr,
    asset: &str,
    ltv: Decimal,
    max_borrow_ltv: Decimal,
    deposit_id: Uint128,
    deposit: &BackingDeposit,
) -> Result<(), ContractError> {
    let mut locked_deposits = USER_LOCKED_DEPOSITS
        .may_load(storage, user.clone())?
        .unwrap_or_default();
    
    // Check if already exists
    if locked_deposits.iter().any(|ld| 
        ld.asset == asset && 
        ld.ltv == ltv && 
        ld.max_borrow_ltv == max_borrow_ltv && 
        ld.deposit_id == deposit_id
    ) {
        return Ok(()); // Already exists
    }
    
    locked_deposits.push(LockedDeposit {
        asset: asset.to_string(),
        ltv,
        max_borrow_ltv,
        deposit_id,
        deposit: deposit.clone(),
    });
    
    USER_LOCKED_DEPOSITS.save(storage, user.clone(), &locked_deposits)?;
    Ok(())
}

/// Update a locked deposit in the user's locked deposits list
fn update_locked_deposit(
    storage: &mut dyn Storage,
    user: &Addr,
    asset: &str,
    ltv: Decimal,
    max_borrow_ltv: Decimal,
    deposit_id: Uint128,
    deposit: &BackingDeposit,
) -> Result<bool, ContractError> {
    let mut locked_deposits = USER_LOCKED_DEPOSITS
        .may_load(storage, user.clone())?
        .unwrap_or_default();
    
    // Find and update
    let mut found = false;
    for ld in &mut locked_deposits {
        if ld.asset == asset && 
           ld.ltv == ltv && 
           ld.max_borrow_ltv == max_borrow_ltv && 
            ld.deposit_id == deposit_id {
            ld.deposit = deposit.clone();
            found = true;
            break;
        }
    }
    
    if found {
        // Remove if no longer locked - need to check before move
        let should_remove = deposit.clone().locked.is_none();
        if should_remove {
            locked_deposits.retain(|ld| 
                !(ld.asset == asset && 
                  ld.ltv == ltv && 
                  ld.max_borrow_ltv == max_borrow_ltv && 
                  ld.deposit_id == deposit_id)
            );
        }
        USER_LOCKED_DEPOSITS.save(storage, user.clone(), &locked_deposits)?;
    }
    
    Ok(found)
}

/// Remove a locked deposit from the user's locked deposits list
fn remove_locked_deposit(
    storage: &mut dyn Storage,
    user: &Addr,
    asset: &str,
    ltv: Decimal,
    max_borrow_ltv: Decimal,
    deposit_id: Uint128,
) -> Result<(), ContractError> {
    let mut locked_deposits = USER_LOCKED_DEPOSITS
        .may_load(storage, user.clone())?
        .unwrap_or_default();
    
    locked_deposits.retain(|ld| 
        !(ld.asset == asset && 
          ld.ltv == ltv && 
          ld.max_borrow_ltv == max_borrow_ltv && 
          ld.deposit_id == deposit_id)
    );
    
    USER_LOCKED_DEPOSITS.save(storage, user.clone(), &locked_deposits)?;
    Ok(())
}

/// Add lost vault tokens from early withdrawal to contract's own deposit
fn add_lost_amount_to_contract_deposit(
    storage: &mut dyn Storage,
    querier: &QuerierWrapper,
    env: &Env,
    config: &Config,
    asset: &str,
    ltv: Decimal,
    max_borrow_ltv: Decimal,
    lost_vault_tokens: Uint128,
    group: &mut MaxBorrowLTVGroup,
    _queue: &mut LTVQueue,
) -> Result<(), ContractError> {
    if lost_vault_tokens.is_zero() {
        return Ok(());
    }

    let contract_addr = env.contract.address.clone();
    let asset_str = asset.to_string();
    let ltv_str = ltv.to_string();
    let max_borrow_ltv_str = max_borrow_ltv.to_string();
    let contract_str = contract_addr.to_string();
    
    // Query epoch start time from revenue distributor
    let epoch_start_time = if let Some(revenue_distributor_addr) = &config.revenue_distributor {
        querier.query_wasm_smart::<EpochCountdownResponse>(
            revenue_distributor_addr,
            &RevenueDistributorQueryMsg::EpochCountdown {},
        )
        .map(|countdown| countdown.epoch_start)
        .unwrap_or_else(|_| env.block.time.seconds())
    } else {
        env.block.time.seconds()
    };
    
    // Find or create deposit_id for contract's deposit
    // Use deposit_id 0 for contract's deposit (or find existing)
    let contract_deposit_id = Uint128::zero();
    let contract_deposit_key = make_deposit_key(&asset_str, &ltv_str, &max_borrow_ltv_str, &contract_str, &contract_deposit_id, epoch_start_time);
    
    // Calculate base tokens for lost vault tokens
    let lost_base_tokens = calculate_base_tokens(
        lost_vault_tokens,
        group.total_deposit_tokens,
        group.total_vault_tokens,
    )?;
    
    if let Some(mut contract_deposit) = BACKING_DEPOSITS.may_load(storage, contract_deposit_key.clone())? {
        // Update existing contract deposit
        let old_contract_locked_vt = contract_deposit.locked_vault_tokens;
        contract_deposit.vault_tokens += lost_vault_tokens;
        contract_deposit.locked_vault_tokens = calculate_locked_vault_tokens(&contract_deposit, env);
        BACKING_DEPOSITS.save(storage, contract_deposit_key.clone(), &contract_deposit)?;
        
        // Update group total_locked_vault_tokens by delta
        let delta = contract_deposit.locked_vault_tokens.checked_sub(old_contract_locked_vt)
            .unwrap_or(Uint128::zero());
        group.total_locked_vault_tokens = group.total_locked_vault_tokens.checked_add(delta)
            .map_err(|e| ContractError::CustomError { 
                val: format!("Overflow updating contract deposit locked_vault_tokens: {}", e) 
            })?;
    } else {
        // Create new contract deposit
        let temp_deposit = BackingDeposit {
            user: contract_addr.clone(),
            vault_tokens: lost_vault_tokens,
            locked_vault_tokens: Uint128::zero(), // Will be calculated below
            max_borrow_ltv,
            last_claimed: env.block.time.seconds(),
            locked: None,
            start_time: env.block.time.seconds(),
            deposit_time: Some(env.block.time.seconds()),
            compound_claims: false,
            manager: None,
            depositor: None,
            withdrawals_enabled: true,
            lvt_tracking: DepositLVTTracking {
                base_lvt: Uint128::zero(),
                reference_time: env.block.time.seconds(),
                daily_delta: Int128::zero(),
                time_cliffs: vec![],
            },
            revenue_destination: None,
        };
        let locked_vt = calculate_locked_vault_tokens(&temp_deposit, env);
        let mut contract_deposit = BackingDeposit {
            user: contract_addr.clone(),
            vault_tokens: lost_vault_tokens,
            locked_vault_tokens: locked_vt,
            max_borrow_ltv,
            last_claimed: env.block.time.seconds(),
            locked: None,
            start_time: env.block.time.seconds(),
            deposit_time: Some(env.block.time.seconds()),
            compound_claims: false,
            manager: None,
            depositor: None,
            withdrawals_enabled: true,
            lvt_tracking: DepositLVTTracking {
                base_lvt: Uint128::zero(),
                reference_time: env.block.time.seconds(),
                daily_delta: Int128::zero(),
                time_cliffs: vec![],
            },
            revenue_destination: None,
        };
        // Initialize LVT tracking for contract deposit
        initialize_deposit_lvt_tracking(&mut contract_deposit, env, config.lock_duration_ceiling)?;
        BACKING_DEPOSITS.save(storage, contract_deposit_key.clone(), &contract_deposit)?;
        
        // Add to USER_DEPOSITS index
        let mut keys = USER_DEPOSITS
            .may_load(storage, (contract_addr.clone(), asset.to_string()))?
            .unwrap_or_default();
        keys.push(contract_deposit_key.clone());
        USER_DEPOSITS.save(storage, (contract_addr.clone(), asset.to_string()), &keys)?;
        
        // Update locked_vault_tokens for new contract deposit
        group.total_locked_vault_tokens = group.total_locked_vault_tokens.checked_add(contract_deposit.locked_vault_tokens)
            .map_err(|e| ContractError::CustomError { 
                val: format!("Overflow adding contract deposit locked_vault_tokens: {}", e) 
            })?;
    }
    
    // Update group totals
    group.total_deposit_tokens += lost_base_tokens;
    group.total_vault_tokens += lost_vault_tokens;
    
    Ok(())
}

/// Update deposit lock state: recalculate locked_vault_tokens and update group totals
/// Returns the old locked_vault_tokens value for use in tracking updates
fn update_deposit_lock_state(
    storage: &mut dyn Storage,
    env: &Env,
    deposit: &mut BackingDeposit,
    asset: String,
    ltv: Decimal,
    max_borrow_ltv: Decimal,
) -> Result<Uint128, ContractError> {
    // Store old locked_vault_tokens before any updates
    let old_locked_vault_tokens = deposit.locked_vault_tokens;
    
    // Check if lock expired and remove it if so
    if let Some(ref lock_info) = deposit.locked {
        if lock_info.locked_until <= env.block.time.seconds() {
            deposit.locked = None;
        }
    }
    
    // Recalculate locked_vault_tokens after potential lock expiration
    deposit.locked_vault_tokens = calculate_locked_vault_tokens(deposit, env);
    
    // Update group's total_locked_vault_tokens if changed
    if deposit.locked_vault_tokens != old_locked_vault_tokens {
        let mut queue = LTV_QUEUES.load(storage, asset.clone())?;
        let slot_index = queue.slots.iter().position(|s| s.ltv == ltv)
            .ok_or_else(|| ContractError::CustomError { val: "Slot not found".to_string() })?;
        let mut slot = queue.slots[slot_index].clone();
        let group_index = find_or_create_borrow_group(&mut slot, max_borrow_ltv, false)?;
        let mut group = slot.deposit_groups[group_index].clone();
        
        // Update by delta
        if deposit.locked_vault_tokens > old_locked_vault_tokens {
            // Add delta to group.total_locked_vault_tokens
            let delta = deposit.locked_vault_tokens.checked_sub(old_locked_vault_tokens)
                .map_err(|e| ContractError::CustomError { 
                    val: format!("Overflow calculating locked_vault_tokens delta: {}", e) 
                })?;
            group.total_locked_vault_tokens = group.total_locked_vault_tokens.checked_add(delta)
                .map_err(|e| ContractError::CustomError { 
                    val: format!("Overflow updating total_locked_vault_tokens: {}", e) 
                })?;
        } else if deposit.locked_vault_tokens < old_locked_vault_tokens {
            // Subtract delta from group.total_locked_vault_tokens
            let delta = old_locked_vault_tokens.checked_sub(deposit.locked_vault_tokens)
                .map_err(|e| ContractError::CustomError { 
                    val: format!("Overflow calculating locked_vault_tokens delta: {}", e) 
                })?;
            group.total_locked_vault_tokens = group.total_locked_vault_tokens.checked_sub(delta)
                .map_err(|e| ContractError::CustomError { 
                    val: format!("Overflow updating total_locked_vault_tokens: {}", e) 
                })?;
        }
        
        // Update slot and queue
        slot.deposit_groups[group_index] = group;
        queue.slots[slot_index] = slot;
        LTV_QUEUES.save(storage, asset.clone(), &queue)?;
    }
    
    Ok(old_locked_vault_tokens)
}

/// Helper to create composite key for BACKING_DEPOSITS map
/// Format: asset:ltv:max_borrow_ltv:user:deposit_id:epoch_timestamp
/// For backward compatibility, epoch_timestamp can be 0 for old deposits
pub fn make_deposit_key(asset: &str, ltv: &str, max_borrow_ltv: &str, user: &str, deposit_id: &Uint128, epoch_timestamp: u64) -> String {
    format!("{}:{}:{}:{}:{}:{}", asset, ltv, max_borrow_ltv, user, deposit_id, epoch_timestamp)
}


/// Create a new LTV queue for an asset
pub fn create_queue(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    asset: String,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    // Owner or CDP contract can create queues
    if info.sender != config.owner && info.sender != config.cdp_contract {
        return Err(ContractError::Unauthorized {});
    }

    // Check if queue already exists
    if LTV_QUEUES.has(deps.storage, asset.clone()) {
        return Err(ContractError::CustomError {
            val: "Queue already exists".to_string(),
        });
    }

    // Query CDP contract for asset's max_LTV to set as minimum
    let basket: Basket = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.cdp_contract.to_string(),
        msg: to_json_binary(&CDP_QueryMsg::GetBasket {})?,
    }))?;

    // Find the asset in the basket to get its max_LTV
    let (min_liquidation_ltv, min_borrow_ltv) = basket.collateral_types
        .iter()
        .find(|c_asset| c_asset.asset.info.to_string() == asset)
        .map(|c_asset| (c_asset.max_LTV, c_asset.max_borrow_LTV))
        .ok_or_else(|| ContractError::CustomError {
            val: "Asset not found in CDP basket".to_string(),
        })?;

    // Create the LTV queue with empty slots (slots created on demand)
    let queue = LTVQueue {
        slots: Vec::new(),
        borrow_ltv: DecimalMinMax {
            min: min_borrow_ltv,
            max: config.max_ltv.clone(),
        },
        liquidation_ltv: DecimalMinMax {
            min: min_liquidation_ltv,
            max: config.max_ltv.clone(),
        },
        current_deposit_id: Uint128::new(1),
        percent_to_disperse: None,
    };

    LTV_QUEUES.save(deps.storage, asset.clone(), &queue)?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "create_queue"),
            attr("asset", asset),
            attr("min_borrow_ltv", min_borrow_ltv.to_string()),
            attr("min_liquidation_ltv", min_liquidation_ltv.to_string()),
            attr("max_ltv", config.max_ltv.to_string()),
        ]))
}

/// Update an existing LTV queue
pub fn update_queue(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    asset: String,
    mut max_ltv: Option<Decimal>,
    mut percent_to_disperse: Option<Decimal>,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    // Only owner can set max LTV and percent_to_disperse
    if info.sender != config.owner {
        max_ltv = None;
        percent_to_disperse = None;
    }

    let mut queue: LTVQueue = LTV_QUEUES.load(deps.storage, asset.clone())?;

    // Query CDP contract for updated minimum LTV
    let basket: Basket = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.cdp_contract.to_string(),
        msg: to_json_binary(&CDP_QueryMsg::GetBasket {})?,
    }))?;

    //Min LTV is set to the asset's max LTV bc we are using this to get HIGHER LTVs than what is set in the CDP
    let (new_min_liquidation_ltv, new_min_borrow_ltv) = basket.collateral_types
        .iter()
        .find(|c_asset| c_asset.asset.info.to_string() == asset)
        .map(|c_asset| (c_asset.max_LTV, c_asset.max_borrow_LTV))
        .ok_or_else(|| ContractError::CustomError {
            val: "Asset not found in CDP basket".to_string(),
        })?;

    let new_max_ltv = max_ltv.unwrap_or(config.max_ltv);

    // Update queue parameters
    queue.borrow_ltv.min = new_min_borrow_ltv;
    queue.borrow_ltv.max = new_max_ltv;
    queue.liquidation_ltv.min = new_min_liquidation_ltv;
    queue.liquidation_ltv.max = new_max_ltv;

    // Update percent_to_disperse if provided
    if let Some(percent) = percent_to_disperse {
        queue.percent_to_disperse = Some(percent);
    }

    // Remove empty deposit groups (no vault tokens)
    queue.slots.iter_mut().for_each(|slot| {
        slot.deposit_groups.retain(|group| !group.total_vault_tokens.is_zero());
    });

    LTV_QUEUES.save(deps.storage, asset.clone(), &queue)?;

    let mut attrs = vec![
        attr("method", "update_queue"),
        attr("asset", asset),
        attr("min_liquidation_ltv", new_min_liquidation_ltv.to_string()),
        attr("min_borrow_ltv", new_min_borrow_ltv.to_string()),
        attr("max_ltv", new_max_ltv.to_string()),
    ];

    if let Some(percent) = queue.percent_to_disperse {
        attrs.push(attr("percent_to_disperse", percent.to_string()));
    }

    Ok(Response::new()
        .add_attributes(attrs))
}

/// Submit a backing deposit
pub fn submit_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    deposit_input: BackingDepositInput,
    deposit_owner: Option<String>,
    locked: Option<membrane::types::Locked>,
    deposit_id: Option<Uint128>,
    manager: Option<String>,
    affiliate_address: Option<String>,
    revenue_destination: Option<String>,
) -> Result<Response, ContractError> {
    let mut msgs: Vec<CosmosMsg> = vec![];
    let config: Config = CONFIG.load(deps.storage)?;
    
    let valid_owner_addr = validate_deposit_owner(deps.api, info.clone(), deposit_owner)?;

    // Validate deposit amount
    if info.funds.len() != 1 {
        return Err(ContractError::TooManyAssets { valid: config.deposit_denom.denom.clone() });
    }

    // Validate asset denomination matches
    if info.funds[0].denom != deposit_input.asset {
        return Err(ContractError::CustomError {
            val: "Asset denomination mismatch".to_string(),
        });
    }

    let mut queue: LTVQueue = LTV_QUEUES.load(deps.storage, deposit_input.asset.clone())?;

    // Validate deposit input (asset & LTV checks)
    validate_deposit_input(deps.storage, deposit_input.clone())?;

    // Find or create the appropriate LTV slot
    let slot_index = find_or_create_ltv_slot(&mut queue, deposit_input.ltv)?;
    let mut slot = queue.slots[slot_index].clone();


    // Find or create the maxBorrowLTV group within the slot
    let group_index = find_or_create_borrow_group(&mut slot, deposit_input.max_borrow_ltv, true)?;
    let mut group = slot.deposit_groups[group_index].clone();

    // Calculate vault tokens to mint using vault-like mechanism
    let deposit_amount = info.funds[0].amount.clone();
    let vault_tokens = calculate_vault_tokens(
        deposit_amount,
        group.total_deposit_tokens,
        group.total_vault_tokens,
    )?; 


    // Query epoch info
    let epoch_info = if let Some(revenue_distributor_addr) = &config.revenue_distributor {
        deps.querier.query_wasm_smart::<EpochCountdownResponse>(
            revenue_distributor_addr,
            &RevenueDistributorQueryMsg::EpochCountdown {},
        )
        .ok()
        .map(|countdown| (countdown.epoch_start, countdown.epoch_end, env.block.time.seconds()))
    } else {
        None
    };

    // Event-based deposit storage key strings
    let asset_str = deposit_input.asset.clone();
    let ltv_str = slot.ltv.to_string();
    let max_borrow_ltv_str = deposit_input.max_borrow_ltv.to_string();
    let user_str = valid_owner_addr.to_string();
    
    // Get epoch_start_time from deposit_input or query it
    let epoch_start_time = if let Some(epoch_start) = deposit_input.epoch_start_time {
        epoch_start
    } else {
        // Query epoch start time from revenue distributor if not provided
        if let Some((epoch_start, _, _)) = epoch_info {
            epoch_start
        } else {
            env.block.time.seconds()
        }
    };
    
    
    // Determine deposit_id - use provided or create new
    let deposit_id = if let Some(id) = deposit_id {
        // Validate deposit exists if ID provided - use epoch_start_time from deposit_input
        let check_key = make_deposit_key(&asset_str, &ltv_str, &max_borrow_ltv_str, &user_str, &id, epoch_start_time);
        if !BACKING_DEPOSITS.has(deps.storage, check_key.clone()) {
            return Err(ContractError::CustomError {
                val: format!("Deposit with id {} not found", id),
            });
        }
        id
    } else {
        // Create new deposit_id
        let new_id = queue.current_deposit_id;
        queue.current_deposit_id += Uint128::one();
        LTV_QUEUES.save(deps.storage, deposit_input.asset.clone(), &queue)?;
        new_id
    };
    
    let deposit_time = env.block.time.seconds();
    let deposit_key = make_deposit_key(&asset_str, &ltv_str, &max_borrow_ltv_str, &user_str, &deposit_id, epoch_start_time);

    // Validate deposit amount
    if info.funds[0].amount < config.minimum_deposit {
        return Err(ContractError::InvalidDepositAmount {});
    }

    // Track if this is a new deposit or existing, and locked_vault_tokens for new deposits
    let is_new_deposit = !BACKING_DEPOSITS.has(deps.storage, deposit_key.clone());
    let mut new_deposit_locked_vault_tokens = Uint128::zero();
    
    // If user already has a deposit in this group, auto-claim and top-up
    if let Some(mut existing) = BACKING_DEPOSITS.may_load(deps.storage, deposit_key.clone())? {
        // Claim revenues for existing deposit and send
        let claimed = claim_revenue_for_deposit(
            deps.storage,
            &deps.querier,
            &env,
            &config,
            &mut existing,
            deposit_input.asset.clone(),
            slot.ltv,
            deposit_input.max_borrow_ltv,
            None,
        )?;
        if !claimed.is_zero() {
            // Send to revenue_destination if set, otherwise to deposit owner
            let recipient = existing.revenue_destination.as_ref().unwrap_or(&valid_owner_addr);
            msgs.push(BankMsg::Send {
                to_address: recipient.to_string(),
                amount: vec![Coin { denom: config.cdt_denom.clone(), amount: claimed }],
            }.into());
        }
        // Calculate old locked_vault_tokens before updating
        let old_locked_vault_tokens = existing.locked_vault_tokens;
        
        // Refresh perpetual locks before recalculating
        refresh_deposit_lock(&mut existing, &env, config.lock_duration_ceiling)?;
        
        // Add new vault tokens to existing deposit
        existing.vault_tokens += vault_tokens.clone();
        
        // Recalculate locked_vault_tokens after vault_tokens change
        existing.locked_vault_tokens = calculate_locked_vault_tokens(&existing, &env);
        
        // Update manager if provided
        if let Some(manager_str) = manager {
            //Validate address
            let manager_addr = deps.api.addr_validate(&manager_str)?;
            //Set manager
            existing.manager = Some(manager_addr.clone());
            //Add to state of the manager
            crate::state::add_managed_deposit(deps.storage, &manager_addr, deposit_key.clone())?;
        }
        
        // Update revenue_destination if provided
        if let Some(revenue_dest_str) = revenue_destination {
            let revenue_dest_addr = deps.api.addr_validate(&revenue_dest_str)?;
            existing.revenue_destination = Some(revenue_dest_addr);
        }
        
        BACKING_DEPOSITS.save(deps.storage, deposit_key.clone(), &existing)?;
        
        // Update group.total_locked_vault_tokens by delta
        if existing.locked_vault_tokens > old_locked_vault_tokens {
            //Add delta to group.total_locked_vault_tokens
            let delta = existing.locked_vault_tokens.checked_sub(old_locked_vault_tokens)
                .map_err(|e| ContractError::CustomError { 
                    val: format!("Overflow calculating locked_vault_tokens delta: {}", e) 
                })?;
            group.total_locked_vault_tokens = group.total_locked_vault_tokens.checked_add(delta)
                .map_err(|e| ContractError::CustomError { 
                    val: format!("Overflow updating total_locked_vault_tokens: {}", e) 
                })?;
        } else if existing.locked_vault_tokens < old_locked_vault_tokens {
            //Subtract delta from group.total_locked_vault_tokens
            let delta = old_locked_vault_tokens.checked_sub(existing.locked_vault_tokens)
                .map_err(|e| ContractError::CustomError { 
                    val: format!("Overflow calculating locked_vault_tokens delta: {}", e) 
                })?;
            group.total_locked_vault_tokens = group.total_locked_vault_tokens.checked_sub(delta)
                .map_err(|e| ContractError::CustomError { 
                    val: format!("Overflow updating total_locked_vault_tokens: {}", e) 
                })?;
        }
        
        // Update effective total for existing deposit
        
        // Calculate old and new effective vault tokens
        let old_unused_vt = if let Some((epoch_start, epoch_end, current_time)) = epoch_info {
            // Create a temporary deposit with old vault_tokens to calculate old effective
            let mut old_deposit = existing.clone();
            old_deposit.vault_tokens -= vault_tokens.clone();
            old_deposit.locked_vault_tokens = old_locked_vault_tokens;
            calculate_unused_locked_vault_tokens(&old_deposit, &env, epoch_start, epoch_end, current_time)
        } else {
            old_locked_vault_tokens
        };
        
        let new_unused_vt = if let Some((epoch_start, epoch_end, current_time)) = epoch_info {
            calculate_unused_locked_vault_tokens(&existing, &env, epoch_start, epoch_end, current_time)
        } else {
            existing.locked_vault_tokens
        };
        
        // Update effective total
        update_group_unused_total(
            deps.storage,
            &deps.querier,
            &env,
            &config,
            &deposit_input.asset,
            slot.ltv,
            deposit_input.max_borrow_ltv,
            old_unused_vt,
            new_unused_vt,
            existing.deposit_time,
        )?;
        
        // Update locked deposits tracking if still locked
        if existing.locked.is_some() {
            // Get deposit_id from key
            let parts: Vec<&str> = deposit_key.split(':').collect();
            if parts.len() == 6 {
                if let Ok(deposit_id) = Uint128::from_str(parts[4]) {
                    update_locked_deposit(
                        deps.storage,
                        &valid_owner_addr,
                        &deposit_input.asset,
                        deposit_input.ltv,
                        deposit_input.max_borrow_ltv,
                        deposit_id,
                        &existing,
                    )?;
                }
            }
        }
    } else {
        // New deposit: initialize last_claimed to now
        let locked_info = if let Some(ref l) = locked {
            let intended_lock_days = (l.locked_until - env.block.time.seconds()) / ONE_DAY_SECONDS;
            Some(membrane::types::Locked {
                locked_until: l.locked_until,
                perpetual_lock: l.perpetual_lock,
                intended_lock_days: Some(intended_lock_days),
            })
        } else {
            None
        };
        
        // Validate manager if provided
        let manager_addr = if let Some(manager_str) = manager {
            Some(deps.api.addr_validate(&manager_str)?)
        } else {
            None
        };
        
        // Validate revenue_destination if provided
        let revenue_destination_addr = if let Some(revenue_dest_str) = revenue_destination {
            Some(deps.api.addr_validate(&revenue_dest_str)?)
        } else {
            None
        };
        
        // Determine depositor: if depositing for another user, set depositor to sender
        // Otherwise, set to None for self-deposits
        let depositor = if valid_owner_addr != info.sender {
            Some(info.sender.clone())
        } else {
            None
        };
        
        // Calculate locked_vault_tokens for new deposit
        let temp_deposit = BackingDeposit {
            user: valid_owner_addr.clone(),
            vault_tokens: vault_tokens.clone(),
            locked_vault_tokens: Uint128::zero(), // Will be calculated below
            max_borrow_ltv: deposit_input.max_borrow_ltv,
            last_claimed: env.block.time.seconds(),
            locked: locked_info.clone(),
            start_time: env.block.time.seconds(),
            deposit_time: Some(deposit_time),
            compound_claims: false,
            manager: manager_addr.clone(),
            depositor: depositor.clone(),
            withdrawals_enabled: true,
            lvt_tracking: DepositLVTTracking {
                base_lvt: Uint128::zero(),
                reference_time: env.block.time.seconds(),
                daily_delta: Int128::zero(),
                time_cliffs: vec![],
            },
            revenue_destination: revenue_destination_addr.clone(),
        };
        new_deposit_locked_vault_tokens = calculate_locked_vault_tokens(&temp_deposit, &env);
        
        let mut deposit = BackingDeposit {
            user: valid_owner_addr.clone(),
            vault_tokens: vault_tokens.clone(),
            locked_vault_tokens: new_deposit_locked_vault_tokens,
            max_borrow_ltv: deposit_input.max_borrow_ltv,
            last_claimed: env.block.time.seconds(),
            locked: locked_info.clone(),
            start_time: env.block.time.seconds(),
            deposit_time: Some(deposit_time),
            compound_claims: false,
            manager: manager_addr.clone(),
            depositor: depositor.clone(),
            withdrawals_enabled: true,
            lvt_tracking: DepositLVTTracking {
                base_lvt: Uint128::zero(),
                reference_time: env.block.time.seconds(),
                daily_delta: Int128::zero(),
                time_cliffs: vec![],
            },
            revenue_destination: revenue_destination_addr.clone(),
        };
        // Initialize LVT tracking for new deposit
        initialize_deposit_lvt_tracking(&mut deposit, &env, config.lock_duration_ceiling)?;
        BACKING_DEPOSITS.save(deps.storage, deposit_key.clone(), &deposit)?;
        
        // Add to USER_DEPOSITS index
        let mut keys = USER_DEPOSITS
            .may_load(deps.storage, (valid_owner_addr.clone(), deposit_input.asset.clone()))?
            .unwrap_or_else(Vec::new);
        //
        keys.push(deposit_key.clone());
        //
        USER_DEPOSITS.save(deps.storage, (valid_owner_addr.clone(), deposit_input.asset.clone()), &keys)?;
        
        // Add to MANAGED_DEPOSITS
        if let Some(manager) = manager_addr {
            crate::state::add_managed_deposit(deps.storage, &manager, deposit_key.clone())?;
        }
        
        // Add to locked deposits tracking if locked
        if deposit.locked.is_some() {
            add_locked_deposit(
                deps.storage,
                &valid_owner_addr,
                &deposit_input.asset,
                deposit_input.ltv,
                deposit_input.max_borrow_ltv,
                deposit_id,
                &deposit,
            )?;
        }
        
        // Update effective total for new deposit
        // Calculate effective vault tokens for new deposit
        let new_unused_vt = if let Some((epoch_start, epoch_end, current_time)) = epoch_info {
            calculate_unused_locked_vault_tokens(&deposit, &env, epoch_start, epoch_end, current_time)
        } else {
            new_deposit_locked_vault_tokens
        };
        
        // Save group to storage first so update_group_unused_total can find it
        // Update group totals first (before effective total update)
        group.total_deposit_tokens += deposit_amount;
        group.total_vault_tokens += vault_tokens.clone();
        group.total_locked_vault_tokens = group.total_locked_vault_tokens.checked_add(new_deposit_locked_vault_tokens)
            .map_err(|e| ContractError::CustomError { 
                val: format!("Overflow adding to total_locked_vault_tokens: {}", e) 
            })?;
        
        // Update slot totals
        slot.total_deposit_tokens += deposit_amount;
        slot.deposit_groups[group_index] = group.clone();
        queue.slots[slot_index] = slot.clone();
        LTV_QUEUES.save(deps.storage, deposit_input.asset.clone(), &queue)?;
        
        // Update group LVT tracking for new deposit
        update_group_lvt_tracking_for_deposit(
            deps.storage,
            &env,
            &deposit_input.asset,
            slot.ltv,
            deposit_input.max_borrow_ltv,
            None, // old_deposit: None for new deposit
            Some(&deposit), // new_deposit: the newly created deposit
        )?;
        
        // Update effective total (old is 0 for new deposits)
        update_group_unused_total(
            deps.storage,
            &deps.querier,
            &env,
            &config,
            &deposit_input.asset,
            slot.ltv,
            deposit_input.max_borrow_ltv,
            Uint128::zero(),
            new_unused_vt,
            Some(deposit_time),
        )?;
        
        // Reload group after effective total update (it may have been modified)
        let mut queue = LTV_QUEUES.load(deps.storage, deposit_input.asset.clone())?;
        let slot_index = queue.slots.iter().position(|s| s.ltv == slot.ltv)
            .ok_or_else(|| ContractError::CustomError { val: "Slot not found".to_string() })?;
        let mut slot = queue.slots[slot_index].clone();
        let group_index = find_or_create_borrow_group(&mut slot, deposit_input.max_borrow_ltv, false)?;
        group = slot.deposit_groups[group_index].clone();
    }


    //Must do this before updating totals for Rate Assurance checks
    update_rate_assurance(deps.storage, deposit_input.asset.clone(), deposit_input.ltv, deposit_input.max_borrow_ltv, &group)?;

    //Add rate assurance callback msg
    if !group.total_deposit_tokens.is_zero() && !group.total_vault_tokens.is_zero() {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::RateAssurance {
                asset: deposit_input.asset.clone(),
                max_ltv: deposit_input.ltv,
                max_borrow_ltv: deposit_input.max_borrow_ltv,
            })?,
            funds: vec![],
        }));
    }

    // Update group totals (only for existing deposits, new deposits already updated above at lines 955-966)
    // Note: For new deposits, we already updated group totals and saved the queue above (lines 955-966)
    // We do NOT add new_deposit_locked_vault_tokens here for new deposits because it was already added at line 957
    if !is_new_deposit {
        group.total_deposit_tokens += deposit_amount;
        group.total_vault_tokens += vault_tokens.clone();
    }

    // Update slot totals, just for easier global tracking.
    // We use slot totals to distribute revenue more efficiently.
    slot.total_deposit_tokens += deposit_amount;
    // slot.total_vault_tokens += deposit.vault_tokens; //No need to update this bc VTS are per group

    // Update queue
    slot.deposit_groups[group_index] = group.clone();
    queue.slots[slot_index] = slot;

    LTV_QUEUES.save(deps.storage, deposit_input.asset.clone(), &queue)?;

    // Update daily TVL tracker
    update_daily_tvl_tracker(deps.storage, &env, &deps.querier)?;
    
    // Update daily LTV tracker
    update_daily_ltv_tracker(deps.storage, &env, deposit_input.asset.clone())?;

    // Update daily insurance tracker
    update_daily_insurance_tracker(deps.storage, &env, deposit_input.asset.clone())?;

    // Update user total deposits
    let user_key = valid_owner_addr.to_string();
    let current_total = USER_TOTAL_DEPOSITS
        .may_load(deps.storage, user_key.clone())?
        .unwrap_or(Uint128::zero());
    USER_TOTAL_DEPOSITS.save(deps.storage, user_key.clone(), &(current_total + deposit_amount))?;

    // Handle affiliate if provided
    if let Some(affiliate_addr) = affiliate_address {
        add_affiliate_from_deposit(
            deps.storage,
            deps.api,
            user_key,
            affiliate_addr,
            config.affiliate_fee,
            env.block.time.seconds(),
        )?;
    }

    Ok(Response::new()
        .add_messages(msgs)
        .add_attributes(vec![
            attr("method", "submit_deposit"),
            attr("deposit_owner", valid_owner_addr.to_string()),
            attr("asset", deposit_input.asset),
            attr("ltv", deposit_input.ltv.to_string()),
            attr("max_borrow_ltv", deposit_input.max_borrow_ltv.to_string()),
            attr("amount", deposit_amount.to_string()),
            attr("vault_tokens", vault_tokens.to_string()),
            attr("action", "submitted"),
        ]))
}

/// Withdraw a backing deposit
pub fn withdraw_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    asset: String,
    ltv: Decimal,
    max_borrow_ltv: Decimal,
    deposit_id: Uint128,
    amount: Option<Uint128>, //Amount of base tokens to withdraw
    epoch_start_time: u64,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    // Block withdrawal if user has active emissions votes
    if let Some(emissions_voting_contract) = &config.emissions_voting_contract {
        let has_votes_response: HasAnyVotesResponse = deps.querier.query_wasm_smart(
            emissions_voting_contract.to_string(),
            &EmissionsVotingQueryMsg::HasAnyVotes { user: info.sender.to_string() },
        )?;
        if has_votes_response.has_votes {
            return Err(ContractError::CustomError {
                val: String::from("Cannot withdraw while you have active emissions votes. Remove your votes first."),
            });
        }
    }

    let mut queue = LTV_QUEUES.load(deps.storage, asset.clone())?;
    
    // Construct deposit key using epoch_start_time
    let asset_str = asset.clone();
    let ltv_str = ltv.to_string();
    let max_borrow_ltv_str = max_borrow_ltv.to_string();
    let user_str = info.sender.to_string();
    let deposit_key = make_deposit_key(&asset_str, &ltv_str, &max_borrow_ltv_str, &user_str, &deposit_id, epoch_start_time);
    
    // Load deposit from BACKING_DEPOSITS map
    let mut deposit = BACKING_DEPOSITS.load(deps.storage, deposit_key.clone())?;

    // Check if withdrawals are enabled
    // Allow withdrawal if withdrawals_enabled is true or depositor is None (self-deposit)
    if !deposit.withdrawals_enabled && deposit.depositor.is_some() {
        return Err(ContractError::WithdrawalsDisabled {});
    }

    // Refresh lock on deposit before processing
    refresh_deposit_lock(&mut deposit, &env, config.lock_duration_ceiling)?;

    // Claim revenue before withdrawal and send to user
    let claimed_revenue = claim_revenue_for_deposit(
        deps.storage,
        &deps.querier,
        &env,
        &config,
        &mut deposit,
        asset.clone(),
        ltv,
        max_borrow_ltv,
        None, //no limit = 100
    )?;

    // Get Slot
    let slot_index = queue.slots.iter().position(|s| s.ltv == ltv)
        .ok_or_else(|| ContractError::CustomError { val: "Slot not found".to_string() })?;
    let mut slot = queue.slots[slot_index].clone();
    
    // Get Group
    let group_index = find_or_create_borrow_group(&mut slot, max_borrow_ltv, false)?;
    let mut group = slot.deposit_groups[group_index].clone();

    //Must do this before updating group totals for Rate Assurance checks
    update_rate_assurance(deps.storage, asset.clone(), slot.ltv, max_borrow_ltv, &group)?;

    // Calculate base tokens to withdraw
    let vault_tokens_to_withdraw = if let Some(amount) = amount {
        calculate_vault_tokens(
            amount,
            group.total_deposit_tokens,
            group.total_vault_tokens,
        )?
    } else {
        deposit.vault_tokens
    };

    let withdraw_vault_tokens = std::cmp::min(vault_tokens_to_withdraw, deposit.vault_tokens);

    // Calculate old locked_vault_tokens before withdrawal
    let old_locked_vault_tokens = deposit.locked_vault_tokens;
    
    // Handle early withdrawal if deposit is locked
    let (actual_withdraw_vault_tokens, lost_vault_tokens) = if let Some(ref lock_info) = deposit.locked {
        if lock_info.locked_until > env.block.time.seconds() {
            // Early withdrawal: calculate proportional loss
            let current_time = env.block.time.seconds();
            let lock_start_time = deposit.start_time;
            
            // Calculate fulfilled days
            let fulfilled_seconds = current_time.saturating_sub(lock_start_time);
            let fulfilled_days = fulfilled_seconds / ONE_DAY_SECONDS;
            
            // Get intended lock days (use perpetual_lock if set, otherwise use intended_lock_days)
            let intended_days = if let Some(perpetual_days) = lock_info.perpetual_lock {
                perpetual_days
            } else if let Some(intended) = lock_info.intended_lock_days {
                intended
            } else {
                // Fallback: calculate from locked_until - start_time (for backward compatibility)
                (lock_info.locked_until - lock_start_time) / ONE_DAY_SECONDS
            };
            
            // Calculate ratio (capped at 1.0)
            let ratio = if intended_days > 0 {
                let ratio_decimal = Decimal::from_ratio(fulfilled_days, intended_days);
                ratio_decimal.min(Decimal::one())
            } else {
                Decimal::one() // If intended_days is 0, return 100%
            };
            
            // Calculate returned and lost amounts
            let returned_vault_tokens_decimal = decimal_multiplication(
                Decimal::from_ratio(withdraw_vault_tokens, Uint128::one()),
                ratio,
            )?;
            let returned_vault_tokens = returned_vault_tokens_decimal.to_uint_floor();
            let lost_vault_tokens = withdraw_vault_tokens.checked_sub(returned_vault_tokens)
                .unwrap_or(Uint128::zero());
            
            (returned_vault_tokens, lost_vault_tokens)
        } else {
            // Lock expired, normal withdrawal
            (withdraw_vault_tokens, Uint128::zero())
        }
    } else {
        // Not locked, normal withdrawal
        (withdraw_vault_tokens, Uint128::zero())
    };
    
    // Calculate base tokens to withdraw using actual withdrawal amount
    let base_tokens_to_withdraw = calculate_base_tokens(
        actual_withdraw_vault_tokens,
        group.total_deposit_tokens,
        group.total_vault_tokens,
    )?;
    
    // Update group totals (only for actual withdrawal, lost amount handled separately)
    group.total_deposit_tokens -= base_tokens_to_withdraw;
    group.total_vault_tokens -= actual_withdraw_vault_tokens;
    
    // Subtract old locked_vault_tokens from group total
    group.total_locked_vault_tokens = group.total_locked_vault_tokens.checked_sub(old_locked_vault_tokens)
        .map_err(|e| ContractError::CustomError { 
            val: format!("Underflow subtracting locked_vault_tokens from group: {}", e) 
        })?;
    
    // Update effective total before withdrawal
    // Query epoch info to calculate effective vault tokens
    let epoch_info = if let Some(revenue_distributor_addr) = &config.revenue_distributor {
        deps.querier.query_wasm_smart::<EpochCountdownResponse>(
            revenue_distributor_addr,
            &RevenueDistributorQueryMsg::EpochCountdown {},
        )
        .ok()
        .map(|countdown| (countdown.epoch_start, countdown.epoch_end, env.block.time.seconds()))
    } else {
        None
    };
    
    // Calculate old effective vault tokens
    let old_unused_vt = if let Some((epoch_start, epoch_end, current_time)) = epoch_info {
        calculate_unused_locked_vault_tokens(&deposit, &env, epoch_start, epoch_end, current_time)
    } else {
        old_locked_vault_tokens
    };
    
    // Add lost amount to contract's deposit if any
    if !lost_vault_tokens.is_zero() {
        add_lost_amount_to_contract_deposit(
            deps.storage,
            &deps.querier,
            &env,
            &config,
            &asset,
            ltv,
            max_borrow_ltv,
            lost_vault_tokens,
            &mut group,
            &mut queue,
        )?;
    }
    
    // Remove or update deposit
    // Check if user is withdrawing all their vault tokens (not the actual returned amount after loss)
    // Save old deposit state for LVT tracking
    let old_deposit_for_tracking = deposit.clone();
    
    let fully_withdrawn = withdraw_vault_tokens == deposit.vault_tokens;
    if fully_withdrawn {
        // Remove deposit from BACKING_DEPOSITS
        BACKING_DEPOSITS.remove(deps.storage, deposit_key.clone());

        // Remove from USER_DEPOSITS index
        let mut user_keys = USER_DEPOSITS
            .may_load(deps.storage, (info.sender.clone(), asset.clone()))?
            .unwrap_or_else(Vec::new);
        user_keys.retain(|k| k != &deposit_key);
        if user_keys.is_empty() {
            USER_DEPOSITS.remove(deps.storage, (info.sender.clone(), asset.clone()));
        } else {
            USER_DEPOSITS.save(deps.storage, (info.sender.clone(), asset.clone()), &user_keys)?;
        }

        // Remove from locked deposits tracking if was locked
        if deposit.locked.is_some() {
            remove_locked_deposit(
                deps.storage,
                &info.sender,
                &asset,
                ltv,
                max_borrow_ltv,
                deposit_id,
            )?;
        }
        
        // Update group LVT tracking for removed deposit
        update_group_lvt_tracking_for_deposit(
            deps.storage,
            &env,
            &asset,
            ltv,
            max_borrow_ltv,
            Some(&old_deposit_for_tracking), // old_deposit: the deposit being removed
            None, // new_deposit: None since fully withdrawn
        )?;

        // Update effective total for fully withdrawn deposit (new effective is 0)
        // NOTE: Must be called before reloading queue, as it saves the queue
        update_group_unused_total(
            deps.storage,
            &deps.querier,
            &env,
            &config,
            &asset,
            ltv,
            max_borrow_ltv,
            old_unused_vt,
            Uint128::zero(),
            deposit.deposit_time,
        )?;

        // Reload queue after update_group_unused_total modified it
        queue = LTV_QUEUES.load(deps.storage, asset.clone())?;
        let slot_index = queue.slots.iter().position(|s| s.ltv == ltv)
            .ok_or_else(|| ContractError::CustomError { val: "Slot not found after update".to_string() })?;
        slot = queue.slots[slot_index].clone();
        let group_index = slot.deposit_groups.iter().position(|g| g.max_borrow_ltv == max_borrow_ltv)
            .ok_or_else(|| ContractError::CustomError { val: "Group not found after update".to_string() })?;
        group = slot.deposit_groups[group_index].clone();
    } else {
        // Update deposit (reduce by full withdrawal amount, lost portion already handled)
        deposit.vault_tokens -= withdraw_vault_tokens;
        
        // If lock expired, remove it
        if let Some(ref lock_info) = deposit.locked {
            if lock_info.locked_until <= env.block.time.seconds() {
                deposit.locked = None;
                // Remove from locked deposits tracking
                remove_locked_deposit(
                    deps.storage,
                    &info.sender,
                    &asset,
                    ltv,
                    max_borrow_ltv,
                    deposit_id,
                )?;
            }
        }

        // Recalculate locked_vault_tokens for remaining deposit
        deposit.locked_vault_tokens = calculate_locked_vault_tokens(&deposit, &env);
        
        // Add back the new locked_vault_tokens to group total
        group.total_locked_vault_tokens = group.total_locked_vault_tokens.checked_add(deposit.locked_vault_tokens)
            .map_err(|e| ContractError::CustomError { 
                val: format!("Overflow adding locked_vault_tokens to group: {}", e) 
            })?;
        
        // Calculate new effective vault tokens for remaining deposit
        let new_unused_vt = if let Some((epoch_start, epoch_end, current_time)) = epoch_info {
            calculate_unused_locked_vault_tokens(&deposit, &env, epoch_start, epoch_end, current_time)
        } else {
            deposit.locked_vault_tokens
        };
        
        // Calculate remaining base tokens
        let remaining_base_tokens = calculate_base_tokens(
            deposit.vault_tokens,
            group.total_deposit_tokens,
            group.total_vault_tokens,
        )?;

        // Validate withdrawal amount
        if remaining_base_tokens < config.minimum_deposit {
            return Err(ContractError::InvalidWithdrawal {
                minimum: config.minimum_deposit,
            });
        }

        // Update deposit's LVT tracking after modification
        update_deposit_lvt_tracking(&mut deposit, &env, config.lock_duration_ceiling)?;
        
        // Save updated deposit
        BACKING_DEPOSITS.save(deps.storage, deposit_key.clone(), &deposit)?;

        // Update locked deposits tracking if still locked
        if deposit.locked.is_some() {
            update_locked_deposit(
                deps.storage,
                &info.sender,
                &asset,
                ltv,
                max_borrow_ltv,
                deposit_id,
                &deposit,
            )?;
        }
        
        // Update group LVT tracking for modified deposit
        update_group_lvt_tracking_for_deposit(
            deps.storage,
            &env,
            &asset,
            ltv,
            max_borrow_ltv,
            Some(&old_deposit_for_tracking), // old_deposit: before withdrawal
            Some(&deposit), // new_deposit: after partial withdrawal
        )?;

        // Update effective total for partially withdrawn deposit
        // NOTE: This must be done BEFORE saving the queue, as it modifies and saves the queue
        update_group_unused_total(
            deps.storage,
            &deps.querier,
            &env,
            &config,
            &asset,
            ltv,
            max_borrow_ltv,
            old_unused_vt,
            new_unused_vt,
            deposit.deposit_time,
        )?;

        // Reload queue after update_group_unused_total modified it
        queue = LTV_QUEUES.load(deps.storage, asset.clone())?;
        let slot_index = queue.slots.iter().position(|s| s.ltv == ltv)
            .ok_or_else(|| ContractError::CustomError { val: "Slot not found after update".to_string() })?;
        slot = queue.slots[slot_index].clone();
        let group_index = slot.deposit_groups.iter().position(|g| g.max_borrow_ltv == max_borrow_ltv)
            .ok_or_else(|| ContractError::CustomError { val: "Group not found after update".to_string() })?;
        group = slot.deposit_groups[group_index].clone();
    }

    // Update slot totals
    slot.total_deposit_tokens -= base_tokens_to_withdraw;
    // Update queue
    slot.deposit_groups[group_index] = group.clone();
    queue.slots[slot_index] = slot.clone();
    LTV_QUEUES.save(deps.storage, asset.clone(), &queue)?;

    // Update daily TVL tracker
    update_daily_tvl_tracker(deps.storage, &env, &deps.querier)?;
    
    // Update daily LTV tracker
    update_daily_ltv_tracker(deps.storage, &env, asset.clone())?;

    // Update daily insurance tracker
    update_daily_insurance_tracker(deps.storage, &env, asset.clone())?;

    // Update user total deposits
    let user_key = info.sender.to_string();
    let current_total = USER_TOTAL_DEPOSITS
        .may_load(deps.storage, user_key.clone())?
        .unwrap_or(Uint128::zero());
    let new_total = current_total.saturating_sub(base_tokens_to_withdraw);
    if new_total.is_zero() {
        USER_TOTAL_DEPOSITS.remove(deps.storage, user_key);
    } else {
        USER_TOTAL_DEPOSITS.save(deps.storage, user_key, &new_total)?;
    }

    // Send tokens back to user (base tokens + claimed revenue)
    let mut msgs: Vec<CosmosMsg> = vec![];
    if !base_tokens_to_withdraw.is_zero() {
        msgs.push(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![Coin {
                denom: asset.clone(),
                amount: base_tokens_to_withdraw,
            }],
        }.into());
    } else {
        return Err(ContractError::InvalidWithdrawal {
            minimum: Uint128::zero(),
        });
    }
    
    // Send claimed revenue
    if !claimed_revenue.is_zero() {
        msgs.push(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![Coin {
                denom: config.cdt_denom.clone(),
                amount: claimed_revenue,
            }],
        }.into());
    }

    // Add rate assurance callback msg if remaining tokens are non-zero
    if !group.total_deposit_tokens.is_zero() && !group.total_vault_tokens.is_zero() {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::RateAssurance {
                asset: asset.clone(),
                max_ltv: slot.ltv,
                max_borrow_ltv: max_borrow_ltv,
            })?,
            funds: vec![],
        }));
    }

    let mut attrs = vec![
        attr("method", "withdraw_deposit"),
        attr("asset", asset),
        attr("ltv", ltv.to_string()),
        attr("max_borrow_ltv", max_borrow_ltv.to_string()),
        attr("vault_tokens_withdrawn", actual_withdraw_vault_tokens.to_string()),
        attr("base_tokens", base_tokens_to_withdraw.to_string()),
        attr("claimed_revenue", claimed_revenue.to_string()),
    ];
    
    if !lost_vault_tokens.is_zero() {
        attrs.push(attr("early_withdrawal_lost_vault_tokens", lost_vault_tokens.to_string()));
    }
    
    Ok(Response::new()
        .add_messages(msgs)
        .add_attributes(attrs))
}

/// Add bad debt to an LTV queue (CDP contract only)
pub fn add_bad_debt(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    asset: String,
    amount: Uint128,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;
    let mut msgs: Vec<SubMsg> = vec![];

    // Only CDP contract can add bad debt
    if info.sender != config.cdp_contract {
        return Err(ContractError::Unauthorized {});
    }
    /////if the deposit denom is the Transmuter's vault token///////
    // DON'T DELETE.
    // if let Some(vault_info) = config.deposit_denom.vault_info.clone(){
    //     //1) Query how many vault tokens is the bad debt amount worth in the vault contract
    //     let bad_debt_as_vault_tokens = deps.querier.query_wasm_smart::<Uint128>(
    //         vault_info.vault_contract.clone(),
    //         &Transmuter_QueryMsg::DepositTokenConversion { deposit_token_amount: amount },
    //     )?;
    //     //2) Update amount to denominate as vault tokens
    //     amount = bad_debt_as_vault_tokens;
    //     //3) Withdraw the vault tokens from the vault contract
    //     msgs.push(SubMsg::reply_on_error(CosmosMsg::Wasm(WasmMsg::Execute {
    //         contract_addr: vault_info.vault_contract.clone(),
    //         msg: to_json_binary(&Transmuter_ExecuteMsg::ExitVault { 
    //             recipient: None, //set to none so we don't have to conditionally add the CDP_ExecuteMsg::FulfillBadDebt Msg at the end of this fn
    //             withdraw_as: Some(vault_info.underlying_token.clone()),
    //          })?,
    //         funds: vec![
    //             Coin {
    //                 denom: config.deposit_denom.denom.clone(),
    //                 amount: bad_debt_as_vault_tokens,
    //             }
    //         ],
    //     }), TRANSMUTER_REPLY_ID));
    //     //We reply on error, and add the errored amount to PENDING_BAD_DEBT using BAD_DEBT_PROPAGATION data 
    //     BAD_DEBT_PROPAGATION.save(deps.storage, &BadDebtPropagation {
    //         asset: asset.clone(),
    //         amount: amount,
    //     })?;
    // }

    //Bad debt is sent per asset
    let mut queue: LTVQueue = LTV_QUEUES.load(deps.storage, asset.clone())?;

    // Step 1: Handle bad debt waterfall (dispersals → revenue events)
    let (revenue_fulfilled, mut remaining_bad_debt) = handle_bad_debt_waterfall(
        deps.storage,
        &queue,
        asset.clone(),
        amount,
    )?;

    // Step 2: If revenue was used to fulfill bad debt, send it to CDP
    if !revenue_fulfilled.is_zero() {
        msgs.push(SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.cdp_contract.to_string(),
            msg: to_json_binary(&CDP_ExecuteMsg::FulfillBadDebt {})?,
            funds: vec![Coin {
                denom: config.cdt_denom.clone(),
                amount: revenue_fulfilled,
            }],
        })));
    }

    // Step 3: If there's remaining bad debt, slash deposits
    let mut total_slashed = Uint128::zero();

    if !remaining_bad_debt.is_zero() {
        // Convert remaining CDT bad debt to equivalent collateral amount using oracle
        let asset_info = AssetInfo::NativeToken { denom: asset.clone() };
        let cdt_info = AssetInfo::NativeToken { denom: config.cdt_denom.clone() };
        let asset_infos = vec![asset_info.clone(), cdt_info.clone()];
        
        let price_response: Vec<PriceResponse> = deps.querier.query_wasm_smart(
            config.oracle_contract.to_string(),
            &Oracle_QueryMsg::Prices {
                asset_infos,
                twap_timeframe: 0,
                oracle_time_limit: 0,
            },
        )?;

        // Convert CDT bad debt to collateral amount
        // CDT value -> asset value -> asset amount
        let cdt_value = price_response[1].get_value(remaining_bad_debt)?; // CDT bad debt in USD value
        let collateral_amount_needed = price_response[0].get_amount(cdt_value)?; // Convert USD value to collateral amount

        let mut remaining_collateral_to_slash = collateral_amount_needed;

        // Sort slots by LTV in descending order for deposit slashing
        let mut sorted_slots: Vec<(usize, MaxLTVSlot)> = queue.slots
            .iter()
            .enumerate()
            //Skip empty slots
            .filter(|(_, slot)| !slot.total_deposit_tokens.is_zero())
            .map(|(i, slot)| (i, slot.clone()))
            .collect();
        sorted_slots.sort_by(|a, b| b.1.ltv.cmp(&a.1.ltv));

        for (slot_index, mut slot) in sorted_slots {
            if remaining_collateral_to_slash.is_zero() {
                break;
            }

            // Track deposits seen in this slot for early exit optimization
            let mut total_deposits_seen_per_slot = Uint128::zero();

            // Sort groups by max_borrow_ltv in descending order
            slot.deposit_groups.sort_by(|a, b| b.max_borrow_ltv.cmp(&a.max_borrow_ltv));

            for group in &mut slot.deposit_groups {
                if remaining_collateral_to_slash.is_zero() {
                    break;
                }

                //Skip empty groups
                if group.total_deposit_tokens.is_zero() {
                    continue;
                }
                // Track seen deposits for optimization
                total_deposits_seen_per_slot += group.total_deposit_tokens;

                // Calculate collateral to slash from this group
                let group_slash_amount = std::cmp::min(remaining_collateral_to_slash, group.total_deposit_tokens);

                // Update Group
                group.total_deposit_tokens -= group_slash_amount;
                slot.bad_debt += price_response[1].get_amount(price_response[0].get_value(group_slash_amount)?)?;
                total_slashed += group_slash_amount;
                remaining_collateral_to_slash -= group_slash_amount;

                // Update remaining_bad_debt by converting slashed collateral to CDT.
                // For attribute accuracy only.
                if !total_slashed.is_zero() {
                    let slashed_cdt_value = price_response[1].get_amount(price_response[0].get_value(group_slash_amount)?)?;
                    remaining_bad_debt = remaining_bad_debt.saturating_sub(slashed_cdt_value);
                }

                

                // Early exit if we've seen all deposits in this slot
                if total_deposits_seen_per_slot >= slot.total_deposit_tokens {
                    break;
                }
            }

            queue.slots[slot_index] = slot;
        }
        
    }

    // Step 4: If deposits were slashed, create liquidation swap message
    if !total_slashed.is_zero() {
        let swap_msg = create_liquidation_swap_msg(
            deps.storage,
            &deps.querier,
            &env,
            &config,
            asset.clone(),
            total_slashed,
        )?;
        msgs.push(swap_msg);
    }

    // Save queue
    LTV_QUEUES.save(deps.storage, asset.clone(), &queue)?;

    Ok(Response::new()
        .add_submessages(msgs)
        .add_attributes(vec![
            attr("method", "add_bad_debt"),
            attr("asset", asset),
            attr("total_bad_debt_cdt", amount.to_string()),
            attr("fulfilled_from_revenue_cdt", revenue_fulfilled.to_string()),
            attr("slashed_collateral_amount", total_slashed.to_string()),
            attr("remaining_bad_debt_cdt", remaining_bad_debt.to_string()),
        ]))
}

/// Add revenue to an asset's LTV queue.
/// Distributes rewards to users based on their vault token holdings
pub fn add_revenue(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    asset: String,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    // Validate that only the configured CDT token is sent
    if info.funds.len() != 1 || info.funds[0].denom != config.cdt_denom {
        return Err(ContractError::CustomError {
            val: "Invalid CDT token denomination".to_string(),
        });
    }

    let mut revenue_amount = info.funds[0].amount;
    let queue: LTVQueue = LTV_QUEUES.load(deps.storage, asset.clone())?;

    let response = Response::new();

    // Calculate total deposit tokens to check if any deposits exist
    let total_deposit_tokens: Uint128 = queue.slots
        .iter()
        .map(|slot| slot.total_deposit_tokens)
        .sum();

    let mut dispersal = match DISPERSAL.load(deps.storage, asset.clone()){
        Ok(dispersal) => dispersal,
        Err(_) => Dispersal {
            total_to_disperse: Uint128::zero(),
            dispersal_window: config.dispersal_window,
            active_dispersal: ActiveDispersal {
                dispersal_start: 0,
                amount_dispersed: Uint128::zero(),
            },
            pending_dispersal: Uint128::zero()
        },
    };

    // If no deposits exist, send all revenue to dispersal
    if total_deposit_tokens.is_zero() {
        if dispersal.active_dispersal.dispersal_start == 0 {
            // When dispersal is not active, add to the total to disperse for the upcoming dispersal period
            dispersal.total_to_disperse += revenue_amount;
        } else {
            // When dispersal is active, add to the pending dispersal for the next dispersal period
            dispersal.pending_dispersal += revenue_amount;
        }
        DISPERSAL.save(deps.storage, asset.clone(), &dispersal)?;
        
        return Ok(response
            .add_attributes(vec![
                attr("method", "add_revenue"),
                attr("asset", asset),
                attr("amount", revenue_amount.to_string()),
                attr("routed_to_dispersal", "all"),
            ]));
    }

    // If deposits exist, calculate portion to add to dispersal
    // Use queue's percent_to_disperse if set, otherwise fall back to config value
    let percent_to_disperse = queue.percent_to_disperse.unwrap_or(config.percent_to_disperse);
    let revenue_decimal = Decimal::from_ratio(revenue_amount, Uint128::one());
    let disperse_percent = decimal_multiplication(revenue_decimal, percent_to_disperse)?;
    let disperse_amount = disperse_percent.to_uint_floor();
    if !disperse_amount.is_zero() && dispersal.active_dispersal.dispersal_start == 0 {
        //When dispersal is not active, we add to the total to disperse for the upcoming dispersal period
        revenue_amount -= disperse_amount;
        dispersal.total_to_disperse += disperse_amount;
        DISPERSAL.save(deps.storage, asset.clone(), &dispersal)?;
    } else if !disperse_amount.is_zero() && dispersal.active_dispersal.dispersal_start != 0 {
        //When dispersal is active, we add to the pending dispersal for the next dispersal period
        revenue_amount -= disperse_amount;
        dispersal.pending_dispersal += disperse_amount;
        DISPERSAL.save(deps.storage, asset.clone(), &dispersal)?;
    }
    
    // Distribute remaining revenue to users based on their vault token holdings
    distribute_revenue_to_users(deps.storage, &deps.querier, &env, &config, &queue, asset.clone(), revenue_amount)?;

    Ok(response
        .add_attributes(vec![
            attr("method", "add_revenue"),
            attr("asset", asset),
            attr("amount", revenue_amount.to_string()),
        ]))
}

/// Add deposit token revenue from auction with per-asset distribution
/// Adds directly to total_deposit_tokens to compound for all depositors
pub fn add_deposit_token_revenue(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    per_asset_distribution: Vec<Asset>,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    // Validate sender is auction contract
    // if let Some(auction_addr) = &config.auction_contract {
    //     if info.sender != *auction_addr {
    //         return Err(ContractError::Unauthorized {});
    //     }
    // } else {
    //     return Err(ContractError::CustomError {
    //         val: "Auction contract not configured".to_string(),
    //     });
    // }

    // Validate deposit token is sent
    let deposit_denom = config.deposit_denom.denom.clone();

    if info.funds.len() != 1 || info.funds[0].denom != deposit_denom {
        return Err(ContractError::CustomError {
            val: format!("Invalid deposit token denomination. Expected: {}", deposit_denom),
        });
    }

    let total_deposit_token_revenue = info.funds[0].amount;
    
    if per_asset_distribution.is_empty() {
        return Err(ContractError::CustomError {
            val: "per_asset_distribution cannot be empty".to_string(),
        });
    }

    // Calculate total weight from distribution
    let total_weight: Uint128 = per_asset_distribution.iter()
        .map(|a| a.amount)
        .sum();

    if total_weight.is_zero() {
        return Err(ContractError::CustomError {
            val: "Total distribution weight cannot be zero".to_string(),
        });
    }

    let mut response = Response::new();
    let mut total_distributed = Uint128::zero();

    // Distribute deposit tokens to each asset's queue proportionally
    for asset_entry in per_asset_distribution.iter() {
        let asset_denom = asset_entry.info.to_string();
        let revenue_share = total_deposit_token_revenue.multiply_ratio(asset_entry.amount, total_weight);
        
        if revenue_share.is_zero() {
            continue;
        }

        // Load queue for this asset
        match LTV_QUEUES.may_load(deps.storage, asset_denom.clone())? {
            Some(mut queue) => {
                // Calculate total deposit tokens across all slots in the queue
                let queue_total_deposit_tokens: Uint128 = queue.slots
                    .iter()
                    .map(|slot| slot.total_deposit_tokens)
                    .sum();

                if queue_total_deposit_tokens.is_zero() {
                    // No deposits for this asset, skip (tokens will stay in contract)
                    continue;
                }

                // Distribute revenue proportionally to each slot and group
                for slot in queue.slots.iter_mut() {
                    if slot.total_deposit_tokens.is_zero() {
                        continue;
                    }
                    
                    // Calculate slot's share of the revenue
                    let slot_share = revenue_share.multiply_ratio(
                        slot.total_deposit_tokens, 
                        queue_total_deposit_tokens
                    );
                    
                    if slot_share.is_zero() {
                        continue;
                    }
                    
                    let slot_total = slot.total_deposit_tokens;
                    
                    // Distribute to each group within the slot
                    for group in slot.deposit_groups.iter_mut() {
                        if group.total_deposit_tokens.is_zero() {
                            continue;
                        }
                        
                        // Calculate group's share of the slot's revenue
                        let group_share = slot_share.multiply_ratio(
                            group.total_deposit_tokens,
                            slot_total
                        );
                        
                        // Add to group's total_deposit_tokens (this compounds for all depositors)
                        group.total_deposit_tokens += group_share;
                    }
                    
                    // Update slot's total_deposit_tokens
                    slot.total_deposit_tokens += slot_share;
                }

                // Save the updated queue
                LTV_QUEUES.save(deps.storage, asset_denom.clone(), &queue)?;

                total_distributed += revenue_share;

                response = response.add_attribute(
                    format!("revenue_to_{}", asset_denom),
                    revenue_share.to_string()
                );
            },
            None => {
                // Queue doesn't exist for this asset, skip
                continue;
            }
        }
    }

    Ok(response
        .add_attributes(vec![
            attr("method", "add_deposit_token_revenue"),
            attr("total_revenue", total_deposit_token_revenue.to_string()),
            attr("total_distributed", total_distributed.to_string()),
        ]))
}

/// Distribute revenue to users based on their vault token holdings
/// Creates RevenueEvent structs instead of immediately distributing to users
/// Uses effective locked vault tokens (with epoch discount) for revenue distribution.
/// This allows us to individualize discounts from late deposits
fn distribute_revenue_to_users(
    storage: &mut dyn Storage,
    querier: &QuerierWrapper,
    env: &Env,
    config: &Config,
    queue: &LTVQueue,
    asset: String,
    revenue_amount: Uint128,
) -> Result<(), ContractError> {
    // Query epoch info from revenue distributor if available
    let epoch_info = if let Some(revenue_distributor_addr) = &config.revenue_distributor {
        match querier.query::<cosmwasm_std::Binary>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: revenue_distributor_addr.to_string(),
            msg: to_json_binary(&RevenueDistributorQueryMsg::EpochCountdown {})?,
        })) {
            Ok(response) => {
                let countdown: EpochCountdownResponse = cosmwasm_std::from_json(response)?;
                Some((countdown.epoch_start, countdown.epoch_end, env.block.time.seconds()))
            },
            Err(_) => None,
        }
    } else {
        None
    };

    // Load contract owned deposit keys from USER_DEPOSITS index
    // Will use them later to subtract the contract's locked vault tokens from the group's totals
    let contract_deposit_keys = USER_DEPOSITS.may_load(storage, (env.contract.address.clone(), asset.clone()))?
        .unwrap_or_else(Vec::new);
    // Calculate total deposit tokens across all slots
    let total_deposit_tokens: Uint128 = queue.slots
        .iter()
        .map(|slot| slot.total_deposit_tokens)
        .sum();
    
    if total_deposit_tokens.is_zero() {
        return Ok(());
    }

    // First tier: Distribute revenue to slots based on their deposit tokens
    for slot in &queue.slots {
        if slot.total_deposit_tokens.is_zero() {
            continue;
        }
        
        let slot_share_ratio = Decimal::from_ratio(
            slot.total_deposit_tokens.u128(), 
            total_deposit_tokens.u128()
        );
        let slot_revenue_decimal = decimal_multiplication(
            Decimal::from_ratio(revenue_amount, Uint128::one()),
            slot_share_ratio,
        )?;
        let slot_revenue = slot_revenue_decimal.to_uint_floor();
        
        if slot_revenue.is_zero() {
            continue;
        }
        
        // Second tier: Distribute slot revenue to groups based on their deposit tokens
        for group in &slot.deposit_groups {
            if group.total_deposit_tokens.is_zero() || group.total_vault_tokens.is_zero() {
                continue;
            }
            
            let group_share_ratio = Decimal::from_ratio(
                group.total_deposit_tokens.u128(),
                slot.total_deposit_tokens.u128()
            );
            let group_revenue_decimal = decimal_multiplication(
                Decimal::from_ratio(slot_revenue, Uint128::one()),
                group_share_ratio,
            )?;
            let group_revenue = group_revenue_decimal.to_uint_floor();
            
            if group_revenue.is_zero() {
                continue;
            }
            
            // Check if epoch changed and update group if needed
            // We need to load the queue, update the group, and save it back
            let mut queue_for_update = LTV_QUEUES.load(storage, asset.clone())?;
            let slot_index_for_update = queue_for_update.slots.iter().position(|s| s.ltv == slot.ltv)
                .ok_or_else(|| ContractError::CustomError { val: "Slot not found".to_string() })?;
            let mut slot_for_update = queue_for_update.slots[slot_index_for_update].clone();
            let group_index_for_update = find_or_create_borrow_group(&mut slot_for_update, group.max_borrow_ltv, false)?;
            let mut group_for_update = slot_for_update.deposit_groups[group_index_for_update].clone();
            

            // Check if epoch changed - if so, reset unused total
            if let Some((epoch_start, _, _)) = epoch_info {
                if group_for_update.effective_epoch_start.map(|e| e != epoch_start).unwrap_or(true) {
                    // Reset unused total for new epoch
                    group_for_update.total_unused_locked_vault_tokens = Uint128::zero();
                    group_for_update.effective_epoch_start = Some(epoch_start);

                    // Save updated group
                    slot_for_update.deposit_groups[group_index_for_update] = group_for_update.clone();
                    queue_for_update.slots[slot_index_for_update] = slot_for_update;
                    LTV_QUEUES.save(storage, asset.clone(), &queue_for_update)?;
                }
            }

            // Calculate effective denominator by subtracting unused amounts
            // Use LVT tracking to get the accurate LVT at current timestamp
            let calculated_lvt = calculate_lvt_at_time(
                group_for_update.lvt_tracking.base_total,
                group_for_update.lvt_tracking.reference_time,
                group_for_update.lvt_tracking.base_daily_delta,
                &group_for_update.lvt_tracking.time_cliffs,
                env.block.time.seconds(),
            )?;
            
            let base_denominator = if calculated_lvt > Uint128::zero() {
                calculated_lvt
            } else if group_for_update.total_locked_vault_tokens > Uint128::zero() {
                // Fallback to static value if tracking not initialized
                group_for_update.total_locked_vault_tokens
            } else {
                group_for_update.total_vault_tokens  // Fallback if no locks
            };

            // Subtract unused locked vault tokens from late deposits
            let mut effective_denominator = base_denominator.saturating_sub(
                group_for_update.total_unused_locked_vault_tokens
            );

            // Calculate contract deposits' locked vault tokens and subtract them
            // Contract deposits should not receive any revenue
            let contract_deposit_keys_for_group: Vec<_> = contract_deposit_keys.iter()
                .filter(|k| {
                    k.split(':').nth(1).unwrap_or("") == slot.ltv.to_string() &&
                    k.split(':').nth(2).unwrap_or("") == group.max_borrow_ltv.to_string()
                })
                .collect();

            // Sum up contract deposits' total locked vault tokens (they get zero weight)
            let mut contract_unused_total = Uint128::zero();
            for contract_key in contract_deposit_keys_for_group {
                if let Some(contract_deposit) = BACKING_DEPOSITS.may_load(storage, contract_key.clone())? {
                    // Contract deposits contribute their FULL locked vault tokens as "unused"
                    contract_unused_total += contract_deposit.locked_vault_tokens;
                }
            }

            // Subtract contract deposits' weight from denominator
            effective_denominator = effective_denominator.saturating_sub(contract_unused_total);

            // Ensure denominator is not zero
            if effective_denominator.is_zero() {
                effective_denominator = Uint128::one();
            }
            
            // Calculate amount per 1 locked vault token (as Decimal)
            let amount_per_locked_vt = Decimal::from_ratio(
                group_revenue.u128(),
                effective_denominator.u128()
            );
            
            // Create revenue event with epoch info
            let event = RevenueEvent {
                timestamp: env.block.time.seconds(),
                epoch_start: epoch_info.map(|(start, _, _)| start).unwrap_or(0),
                epoch_end: epoch_info.map(|(_, end, _)| end).unwrap_or(0),
                amount_per_locked_vt,  // Store as Decimal for direct multiplication
                amount_to_be_claimed: group_revenue,
            };
            
            // Store event
            let key = (asset.clone(), slot.ltv.to_string(), group.max_borrow_ltv.to_string());
            let mut events = REVENUE_EVENTS
                .may_load(storage, key.clone())?
                .unwrap_or_else(Vec::new);
            events.push(event);
            REVENUE_EVENTS.save(storage, key, &events)?;
            
            // Track cumulative revenue
            add_revenue_tracking_entry(
                storage,
                env.clone(),
                asset.clone(),
                slot.ltv,
                group.max_borrow_ltv,
                group_revenue
            )?;
        }
    }

    Ok(())
}

/// Add revenue tracking entry for a specific slot/group combination
fn add_revenue_tracking_entry(
    storage: &mut dyn Storage,
    env: Env,
    asset: String,
    max_ltv: Decimal,
    max_borrow_ltv: Decimal,
    revenue_amount: Uint128,
) -> Result<(), ContractError> {
    let timestamp = env.block.time.seconds();
    let key = (asset, max_ltv.to_string(), max_borrow_ltv.to_string());
    
    // Load existing entries
    let mut entries = REVENUE_TRACKING
        .may_load(storage, key.clone())?
        .unwrap_or_else(Vec::new);
    
    // Calculate new total (add to previous total or start fresh)
    let new_total = if let Some(last_entry) = entries.last() {
        last_entry.total_revenue + revenue_amount
    } else {
        revenue_amount
    };
    
    // Create new entry
    let entry = RevenueTrackingEntry {
        timestamp,
        total_revenue: new_total,
    };
    
    entries.push(entry);
    
    // Apply limit
    if entries.len() > REVENUE_TRACKING_LIMIT {
        entries.drain(0..entries.len() - REVENUE_TRACKING_LIMIT);
    }
    
    REVENUE_TRACKING.save(storage, key, &entries)?;
    Ok(())
}

/// Condense deposits from completed epochs to prevent state bloat
/// Merges deposits that have identical settings and are from epochs before the current epoch
fn condense_deposits_for_user(
    deps: DepsMut,
    env: &Env,
    config: &Config,
    user_addr: &Addr,
    asset: &str,
) -> Result<Vec<String>, ContractError> {
    // Query current epoch info
    let epoch_info = if let Some(revenue_distributor_addr) = &config.revenue_distributor {
        deps.querier.query_wasm_smart::<EpochCountdownResponse>(
            revenue_distributor_addr,
            &RevenueDistributorQueryMsg::EpochCountdown {},
        )
        .ok()
        .map(|countdown| (countdown.epoch_start, countdown.epoch_end, env.block.time.seconds()))
    } else {
        None
    };
    
    let current_epoch_start = if let Some((epoch_start, _, _)) = epoch_info {
        epoch_start
    } else {
        // No revenue distributor, can't determine epochs - skip condensing
        return Ok(vec![]);
    };
    
    // Load all deposit keys for this user and asset
    let deposit_keys = USER_DEPOSITS
        .may_load(deps.storage, (user_addr.clone(), asset.to_string()))?
        .unwrap_or_default();
    
    if deposit_keys.len() < 2 {
        // Need at least 2 deposits to condense
        return Ok(vec![]);
    }
    
    // Group deposits by matching criteria
    // Key: (ltv, max_borrow_ltv, lock_key, manager_key, depositor_key, withdrawals_enabled, compound_claims)
    // where lock_key, manager_key, depositor_key are string representations for grouping
    let mut deposit_groups: HashMap<String, Vec<(String, BackingDeposit)>> = HashMap::new();
    
    for deposit_key_str in deposit_keys.iter() {
        // Parse deposit key: "asset:ltv:max_borrow_ltv:user:deposit_id:epoch_timestamp"
        let parts: Vec<&str> = deposit_key_str.split(':').collect();
        if parts.len() != 6 {
            continue;
        }
        
        // Load deposit
        let deposit = match BACKING_DEPOSITS.load(deps.storage, deposit_key_str.clone()) {
            Ok(d) => d,
            Err(_) => continue,
        };
        
        // Skip deposits without deposit_time (old deposits, can't determine epoch)
        let deposit_time = match deposit.deposit_time {
            Some(dt) => dt,
            None => continue,
        };
        
        // Only condense deposits from epochs before current epoch
        if deposit_time >= current_epoch_start {
            continue;
        }
        
        // Create grouping key
        let ltv_str = parts[1].to_string();
        let max_borrow_ltv_str = parts[2].to_string();
        let lock_key = if let Some(ref locked) = deposit.locked {
            format!("{:?}", locked)
        } else {
            "None".to_string()
        };
        let manager_key = deposit.manager.as_ref()
            .map(|m| m.to_string())
            .unwrap_or_else(|| "None".to_string());
        let depositor_key = deposit.depositor.as_ref()
            .map(|d| d.to_string())
            .unwrap_or_else(|| "None".to_string());
        let withdrawals_enabled_str = deposit.withdrawals_enabled.to_string();
        let compound_claims_str = deposit.compound_claims.to_string();
        
        let group_key = format!(
            "{}:{}:{}:{}:{}:{}:{}",
            ltv_str, max_borrow_ltv_str, lock_key, manager_key, depositor_key, withdrawals_enabled_str, compound_claims_str
        );
        
        deposit_groups
            .entry(group_key)
            .or_insert_with(Vec::new)
            .push((deposit_key_str.clone(), deposit));
    }
    
    let mut merged_keys = Vec::new();
    
    // Process each group
    for (group_key, deposits) in deposit_groups.iter() {
        if deposits.len() < 2 {
            continue; // Need at least 2 deposits to merge
        }
        
        // Parse group key to get ltv and max_borrow_ltv
        let group_parts: Vec<&str> = group_key.split(':').collect();
        if group_parts.len() != 7 {
            continue;
        }
        let ltv = Decimal::from_str(group_parts[0])
            .map_err(|_| ContractError::CustomError { val: "Invalid LTV format".to_string() })?;
        let max_borrow_ltv = Decimal::from_str(group_parts[1])
            .map_err(|_| ContractError::CustomError { val: "Invalid max_borrow_ltv format".to_string() })?;
        
        // Calculate merged deposit
        let mut merged_vault_tokens = Uint128::zero();
        let mut earliest_deposit_time = u64::MAX;
        let mut earliest_start_time = u64::MAX;
        let mut latest_last_claimed = 0u64;
        let first_deposit = &deposits[0].1;
        
        for (_, deposit) in deposits.iter() {
            merged_vault_tokens = merged_vault_tokens.checked_add(deposit.vault_tokens)
                .map_err(|e| ContractError::CustomError {
                    val: format!("Overflow adding vault_tokens: {}", e)
                })?;
            
            if let Some(dt) = deposit.deposit_time {
                earliest_deposit_time = earliest_deposit_time.min(dt);
            }
            earliest_start_time = earliest_start_time.min(deposit.start_time);
            latest_last_claimed = latest_last_claimed.max(deposit.last_claimed);
        }
        
        // Create merged deposit
        let temp_merged_deposit = BackingDeposit {
            user: user_addr.clone(),
            vault_tokens: merged_vault_tokens,
            locked_vault_tokens: Uint128::zero(), // Will be calculated below
            max_borrow_ltv: first_deposit.max_borrow_ltv,
            last_claimed: latest_last_claimed,
            locked: first_deposit.locked.clone(),
            start_time: earliest_start_time,
            deposit_time: Some(earliest_deposit_time),
            compound_claims: first_deposit.compound_claims,
            manager: first_deposit.manager.clone(),
            depositor: first_deposit.depositor.clone(),
            withdrawals_enabled: first_deposit.withdrawals_enabled,
            lvt_tracking: DepositLVTTracking {
                base_lvt: Uint128::zero(),
                reference_time: env.block.time.seconds(),
                daily_delta: Int128::zero(),
                time_cliffs: vec![],
            },
            revenue_destination: first_deposit.revenue_destination.clone(),
        };
        
        let merged_locked_vt = calculate_locked_vault_tokens(&temp_merged_deposit, env);
        let mut merged_deposit = BackingDeposit {
            locked_vault_tokens: merged_locked_vt,
            ..temp_merged_deposit
        };
        // Initialize LVT tracking for merged deposit
        let config = CONFIG.load(deps.storage)?;
        initialize_deposit_lvt_tracking(&mut merged_deposit, env, config.lock_duration_ceiling)?;
        
        // Calculate old locked_vault_tokens total from deposits being merged
        let mut old_total_locked_vt = Uint128::zero();
        for (_, deposit) in deposits.iter() {
            old_total_locked_vt = old_total_locked_vt.checked_add(deposit.locked_vault_tokens)
                .map_err(|e| ContractError::CustomError {
                    val: format!("Overflow adding old locked_vault_tokens: {}", e)
                })?;
        }
        
        // Update group totals
        let mut queue = LTV_QUEUES.load(deps.storage, asset.to_string())?;
        let slot_index = queue.slots.iter().position(|s| s.ltv == ltv)
            .ok_or_else(|| ContractError::CustomError { val: "Slot not found".to_string() })?;
        let mut slot = queue.slots[slot_index].clone();
        let group_index = find_or_create_borrow_group(&mut slot, max_borrow_ltv, false)?;
        let mut group = slot.deposit_groups[group_index].clone();
        
        // Subtract old deposits' locked_vault_tokens, add merged deposit's
        group.total_locked_vault_tokens = group.total_locked_vault_tokens
            .checked_sub(old_total_locked_vt)
            .map_err(|e| ContractError::CustomError {
                val: format!("Overflow subtracting old locked_vault_tokens: {}", e)
            })?;
        group.total_locked_vault_tokens = group.total_locked_vault_tokens
            .checked_add(merged_locked_vt)
            .map_err(|e| ContractError::CustomError {
                val: format!("Overflow adding merged locked_vault_tokens: {}", e)
            })?;
        
        // Update effective totals
        // Calculate old and new effective vault tokens
        let epoch_info_for_effective = epoch_info;
        let old_unused_vt = if let Some((epoch_start, epoch_end, current_time)) = epoch_info_for_effective {
            // Sum effective vault tokens from all deposits being merged
            let mut total_old_effective = Uint128::zero();
            for (_, deposit) in deposits.iter() {
                let dep_effective = if let Some(dt) = deposit.deposit_time {
                    if dt >= epoch_start && dt < epoch_end {
                        calculate_unused_locked_vault_tokens(deposit, env, epoch_start, epoch_end, current_time)
                    } else {
                        deposit.locked_vault_tokens
                    }
                } else {
                    deposit.locked_vault_tokens
                };
                total_old_effective = total_old_effective.checked_add(dep_effective)
                    .map_err(|e| ContractError::CustomError {
                        val: format!("Overflow adding old effective_vt: {}", e)
                    })?;
            }
            total_old_effective
        } else {
            old_total_locked_vt
        };
        
        let new_unused_vt = if let Some((epoch_start, epoch_end, current_time)) = epoch_info_for_effective {
            if let Some(dt) = merged_deposit.deposit_time {
                if dt >= epoch_start && dt < epoch_end {
                    calculate_unused_locked_vault_tokens(&merged_deposit, env, epoch_start, epoch_end, current_time)
                } else {
                    merged_locked_vt
                }
            } else {
                merged_locked_vt
            }
        } else {
            merged_locked_vt
        };
        
        // Save updated group totals first
        slot.deposit_groups[group_index] = group;
        queue.slots[slot_index] = slot;
        LTV_QUEUES.save(deps.storage, asset.to_string(), &queue)?;
        
        // Update effective total using helper (this will load and save the queue again)
        update_group_unused_total(
            deps.storage,
            &deps.querier,
            env,
            &config,
            asset,
            ltv,
            max_borrow_ltv,
            old_unused_vt,
            new_unused_vt,
            merged_deposit.deposit_time,
        )?;
        
        // Create new deposit key with earliest deposit_time
        let new_deposit_id = queue.current_deposit_id;
        queue.current_deposit_id += Uint128::one();
        LTV_QUEUES.save(deps.storage, asset.to_string(), &queue)?;
        
        let new_deposit_key = make_deposit_key(
            asset,
            &ltv.to_string(),
            &max_borrow_ltv.to_string(),
            &user_addr.to_string(),
            &new_deposit_id,
            earliest_deposit_time,
        );
        
        // Save merged deposit
        BACKING_DEPOSITS.save(deps.storage, new_deposit_key.clone(), &merged_deposit)?;
        
        // Delete old deposits
        for (old_key, _) in deposits.iter() {
            BACKING_DEPOSITS.remove(deps.storage, old_key.clone());
        }
        
        // Update USER_DEPOSITS index
        // Reload current keys to get the latest state (in case previous groups were condensed)
        let mut updated_keys = USER_DEPOSITS
            .may_load(deps.storage, (user_addr.clone(), asset.to_string()))?
            .unwrap_or_default();
        for (old_key, _) in deposits.iter() {
            updated_keys.retain(|k| k != old_key);
        }
        updated_keys.push(new_deposit_key.clone());
        USER_DEPOSITS.save(deps.storage, (user_addr.clone(), asset.to_string()), &updated_keys)?;
        
        // Update MANAGED_DEPOSITS if manager exists
        if let Some(manager) = &merged_deposit.manager {
            // Remove old keys from manager's list
            for (old_key, _) in deposits.iter() {
                crate::state::remove_managed_deposit(deps.storage, manager, old_key)?;
            }
            // Add new merged key
            crate::state::add_managed_deposit(deps.storage, manager, new_deposit_key.clone())?;
        }
        
        // Update locked deposits tracking if needed
        if merged_deposit.locked.is_some() {
            // Remove old locked deposits
            for (old_key, old_deposit) in deposits.iter() {
                if old_deposit.locked.is_some() {
                    let old_parts: Vec<&str> = old_key.split(':').collect();
                    if old_parts.len() == 6 {
                        if let Ok(old_deposit_id) = Uint128::from_str(old_parts[4]) {
                            remove_locked_deposit(
                                deps.storage,
                                user_addr,
                                asset,
                                ltv,
                                max_borrow_ltv,
                                old_deposit_id,
                            )?;
                        }
                    }
                }
            }
            // Add new merged locked deposit
            add_locked_deposit(
                deps.storage,
                user_addr,
                asset,
                ltv,
                max_borrow_ltv,
                new_deposit_id,
                &merged_deposit,
            )?;
        }
        
        merged_keys.push(new_deposit_key);
    }
    
    Ok(merged_keys)
}

/// Claim revenue for a specific deposit (internal helper)
/// Uses effective locked vault tokens (with epoch discount) when calculating user share
fn claim_revenue_for_deposit(
    storage: &mut dyn Storage,
    _querier: &QuerierWrapper,
    env: &Env,
    _config: &Config,
    deposit: &mut BackingDeposit,
    asset: String,
    max_ltv: Decimal,
    max_borrow_ltv: Decimal,
    limit: Option<u32>,
) -> Result<Uint128, ContractError> {
    let key = (asset.clone(), max_ltv.to_string(), max_borrow_ltv.to_string());
    let mut events = REVENUE_EVENTS
        .may_load(storage, key.clone())?
        .unwrap_or_else(Vec::new);
    
    let mut total_claimed = Uint128::zero();
    let mut events_processed = 0u32;
    let max_events = limit.unwrap_or(100);
    
    for event in events.iter_mut() {
        if events_processed >= max_events {
            break;
        }
        
        if event.timestamp <= deposit.last_claimed {
            continue;
        }
        
        if event.amount_to_be_claimed.is_zero() {
            continue;
        }
        
        // Calculate LVT at event timestamp using deposit's tracking data
        let deposit_lvt_at_event = calculate_lvt_at_time(
            deposit.lvt_tracking.base_lvt,
            deposit.lvt_tracking.reference_time,
            deposit.lvt_tracking.daily_delta,
            &deposit.lvt_tracking.time_cliffs,
            event.timestamp,
        )?;
        
        // Determine unused locked vault tokens for this event
        let unused_locked_vt = if let Some(deposit_time) = deposit.deposit_time {
            // Check if deposit was made within this event's epoch
            if deposit_time >= event.epoch_start && deposit_time < event.epoch_end {
                // Deposit was made in this event's epoch - calculate lost weight
                calculate_unused_locked_vault_tokens(
                    deposit,
                    env,
                    event.epoch_start,
                    event.epoch_end,
                    env.block.time.seconds(),
                )
            } else {
                // Deposit made in different epoch - no penalty
                Uint128::zero()
            }
        } else {
            // No deposit_time - no penalty (backward compatibility)
            Uint128::zero()
        };

        // Calculate effective by subtracting unused from calculated LVT at event time
        let effective_locked_vt: Uint128 = deposit_lvt_at_event.saturating_sub(unused_locked_vt);

        // Calculate user share: effective_locked_vault_tokens * amount_per_locked_vt (Decimal)
        // Use decimal_multiplication helper to ensure correct type handling
        let user_share_decimal = decimal_multiplication(
            Decimal::from_ratio(effective_locked_vt, Uint128::one()),
            event.amount_per_locked_vt,
        )?;
        let mut user_share: Uint128 = user_share_decimal.to_uint_floor();
        
        if !user_share.is_zero() {
            //If the user share is greater than the amount to be claimed, set the user share to the amount to be claimed and set the amount to be claimed to zero
            //This is to prevent overflow errors
            if event.amount_to_be_claimed < user_share {
                user_share = event.amount_to_be_claimed;
                event.amount_to_be_claimed = Uint128::zero();
            } else {
                event.amount_to_be_claimed = event.amount_to_be_claimed.checked_sub(user_share)
                    .map_err(|e| ContractError::CustomError { val: format!("Overflow subtracting user share: {}", e) })?;
            }
            //Add to total claimed
            total_claimed = total_claimed.checked_add(user_share)
                .map_err(|e| ContractError::CustomError { val: format!("Overflow adding user share: {}", e) })?;
        }
        //Increment events processed
        events_processed += 1;
    }
    
    deposit.last_claimed = env.block.time.seconds();
    
    // Trim events with zero amount_to_be_claimed
    events.retain(|e| !e.amount_to_be_claimed.is_zero());
    REVENUE_EVENTS.save(storage, key, &events)?;
    
    // Update user lifetime revenue
    if !total_claimed.is_zero() {
        update_user_lifetime_revenue(storage, deposit.user.clone(), asset.clone(), total_claimed, env.block.time.seconds())?;
    }
    
    Ok(total_claimed)
}

/// Update user lifetime revenue tracking with limit
fn update_user_lifetime_revenue(
    storage: &mut dyn Storage,
    user: Addr,
    asset: String,
    amount: Uint128,
    timestamp: u64,
) -> Result<(), ContractError> {
    let mut entries = USER_LIFETIME_REVENUE
        .may_load(storage, (user.clone(), asset.clone()))?
        .unwrap_or_else(Vec::new);
    
    // Calculate new cumulative total
    let new_total = if let Some(last_entry) = entries.last() {
        last_entry.total_claimed.checked_add(amount)
            .map_err(|e| ContractError::CustomError { val: format!("Overflow adding to lifetime revenue: {}", e) })?
    } else {
        amount
    };
    
    // Create new entry
    let entry = UserLifetimeRevenueEntry {
        timestamp,
        total_claimed: new_total,
    };
    
    //Add to entries
    entries.push(entry);
    
    // Apply limit
    if entries.len() > LIFETIME_REVENUE_LIMIT {
        entries.drain(0..entries.len() - LIFETIME_REVENUE_LIMIT);
    }
    
    USER_LIFETIME_REVENUE.save(storage, (user, asset), &entries)?;
    Ok(())
}

/// Claim accumulated revenue rewards for a user (public execute)
/// 
/// This function processes revenue claims for all deposits belonging to a user for a specific asset.
/// It supports optional compound functionality where claimed CDT can be automatically swapped back
/// into deposit tokens and added to the original deposits.
/// 
/// # Compound Logic
/// - If `compound_action.compound_now` is true, those deposits will compound this claim
/// - If `deposit.compound_claims` is true, the deposit will always compound (ongoing intent)
/// - If `compound_action.set_ongoing` is true, all deposits will have `compound_claims` set to true
/// - Compound deposits contribute their CDT to a swap, remaining CDT goes to user
/// 
/// # Flow
/// 1. Load all user deposits for the asset
/// 2. For each deposit: claim revenue, check compound settings, track contributions
/// 3. If any deposits compound: save balance before swap, create swap submsg via neutron_proxy
/// 4. Send remaining CDT to user (if any)
/// 5. Reply handler will process the swap and distribute deposit tokens back
/// 
/// Uses USER_DEPOSITS map to find all deposits for the user
pub fn claim_revenue_for_user(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    user: String,
    asset: String,
    limit: Option<u32>,
    mut compound_action: Option<CompoundAction>,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;
    let user_addr = deps.api.addr_validate(&user)?;

    //If caller isn't user, set compound_action to None
    if info.sender != user_addr {
        compound_action = None;
    }
    
    // Load all deposit keys for this user and asset
    let deposit_keys = USER_DEPOSITS
        .may_load(deps.storage, (user_addr.clone(), asset.clone()))?
        .unwrap_or_else(Vec::new);
    
    if deposit_keys.is_empty() {
        return Err(ContractError::CustomError {
            val: "No deposits found for user".to_string(),
        });
    }
    
    let mut total_claimed = Uint128::zero();
    // Track which deposits are compounding and how much CDT each contributed
    // Format: (deposit_key, cdt_amount)
    let mut compound_contributions: Vec<(String, Uint128)> = Vec::new();
    let mut total_to_compound = Uint128::zero();
    // Track manager fees per manager: Map<Addr, Uint128>
    let mut manager_fees: HashMap<Addr, Uint128> = HashMap::new();
    
    // Iterate through all deposits and claim from each
    for deposit_key_str in deposit_keys.clone() {
        let deposit_key_for_save = deposit_key_str.clone();
        if let Ok(mut deposit) = BACKING_DEPOSITS.load(deps.storage, deposit_key_str.clone()) {
            // Refresh lock on deposit before processing
            refresh_deposit_lock(&mut deposit, &env, config.lock_duration_ceiling)?;
            
            // Parse LTV values from the key string (format: "asset:ltv:max_borrow_ltv:user:deposit_id:epoch_timestamp")
            let parts: Vec<&str> = deposit_key_str.split(':').collect();
            if parts.len() != 6 {
                continue;
            }
            let max_ltv = Decimal::from_str(parts[1])
                .map_err(|_| ContractError::CustomError { val: "Invalid LTV format".to_string() })?;
            let max_borrow_ltv = Decimal::from_str(parts[2])
                .map_err(|_| ContractError::CustomError { val: "Invalid max_borrow_ltv format".to_string() })?;
            
            let mut claimed = claim_revenue_for_deposit(
                deps.storage,
                &deps.querier,
                &env,
                &config,
                &mut deposit,
                asset.clone(),
                max_ltv,
                max_borrow_ltv,
                limit,
            )?;
            
            // Calculate and deduct manager fee if deposit has a manager
            if let Some(ref manager_addr) = deposit.manager {
                // Load manager fee (default to 0 if not set)
                let manager_fee_rate = MANAGER_FEE
                    .may_load(deps.storage, manager_addr.clone())?
                    .unwrap_or(Decimal::zero());
                
                if !manager_fee_rate.is_zero() && !claimed.is_zero() {
                    // Calculate manager fee amount
                    let manager_fee_amount = decimal_multiplication(
                        Decimal::from_ratio(claimed, Uint128::one()),
                        manager_fee_rate
                    )?.to_uint_floor();
                    
                    if !manager_fee_amount.is_zero() {
                        // Deduct manager fee from claimed amount
                        claimed = claimed.checked_sub(manager_fee_amount)
                            .map_err(|e| ContractError::CustomError {
                                val: format!("Overflow subtracting manager fee: {}", e)
                            })?;
                        
                        // Track manager fee
                        let current_fee = manager_fees.get(manager_addr).copied().unwrap_or(Uint128::zero());
                        manager_fees.insert(manager_addr.clone(), current_fee.checked_add(manager_fee_amount)
                            .map_err(|e| ContractError::CustomError {
                                val: format!("Overflow adding manager fee: {}", e)
                            })?);
                    }
                }
            }
            
            // Determine if this deposit should compound
            // Priority: compound_now (one-time) > compound_claims (ongoing)
            // If compound_now is true, compound this claim regardless of deposit setting
            // If compound_claims is true, always compound (ongoing intent)
            let should_compound = if let Some(ref action) = compound_action {
                action.compound_now || deposit.compound_claims
            } else {
                deposit.compound_claims
            };
            
            // Update compound_claims if set_ongoing is true
            // This sets the ongoing intent for future claims without compound_now
            if let Some(ref action) = compound_action {
                if action.set_ongoing {
                    deposit.compound_claims = true;
                }
            }
            
            total_claimed = total_claimed.checked_add(claimed)
                .map_err(|e| ContractError::CustomError { val: format!("Overflow adding claimed revenue: {}", e) })?;
            
            // Track compound contributions (using claimed amount after manager fee deduction)
            // Only track deposits that want to compound AND have non-zero claimed amount
            // Store deposit key and CDT amount for proportional distribution in reply handler
            if should_compound && !claimed.is_zero() {
                compound_contributions.push((deposit_key_for_save.clone(), claimed));
                total_to_compound = total_to_compound.checked_add(claimed)
                    .map_err(|e| ContractError::CustomError { val: format!("Overflow adding compound amount: {}", e) })?;
            }
            
            // Update lock state (recalculate locked_vault_tokens and update group totals)
            let old_locked_vault_tokens = update_deposit_lock_state(
                deps.storage,
                &env,
                &mut deposit,
                asset.clone(),
                max_ltv,
                max_borrow_ltv,
            )?;
            
            // Save updated deposit
            BACKING_DEPOSITS.save(deps.storage, deposit_key_for_save.clone(), &deposit)?;
            
            // Update locked deposits tracking
            if let Ok(deposit_id) = Uint128::from_str(parts[4]) {
                if deposit.locked.is_some() {
                    // Deposit is still locked, update tracking
                    update_locked_deposit(
                        deps.storage,
                        &user_addr,
                        &asset,
                        max_ltv,
                        max_borrow_ltv,
                        deposit_id,
                        &deposit,
                    )?;
                } else {
                    // Deposit is not locked - check if it was previously locked and needs removal
                    // If old_locked_vault_tokens > vault_tokens, it was locked (with lock_days > 0)
                    // Even if it was expired (lock_days = 0), old_locked_vault_tokens = vault_tokens,
                    // so we check if it was in tracking by attempting removal (safe if not present)
                    // We only remove if old_locked_vault_tokens indicates it was locked
                    if old_locked_vault_tokens > deposit.vault_tokens {
                        // Was locked with lock_days > 0, remove from tracking
                        remove_locked_deposit(
                            deps.storage,
                            &user_addr,
                            &asset,
                            max_ltv,
                            max_borrow_ltv,
                            deposit_id,
                        )?;
                    }
                }
            }
        }
    }
    
    // Validate recipient address early
    let recipient_addr = if let Some(ref action) = compound_action {
        if let Some(ref recipient) = action.recipient_address {
            deps.api.addr_validate(recipient)
                .map_err(|e| ContractError::CustomError {
                    val: format!("Invalid recipient address: {}", e),
                })?
        } else {
            user_addr.clone()
        }
    } else {
        user_addr.clone()
    };
    
    let mut msgs: Vec<CosmosMsg> = vec![];
    let mut submsgs: Vec<SubMsg> = vec![];
    
    // Send manager fees to managers
    let mut total_manager_fees = Uint128::zero();
    for (manager_addr, fee_amount) in manager_fees.iter() {
        if !fee_amount.is_zero() {
            total_manager_fees = total_manager_fees.checked_add(*fee_amount)
                .map_err(|e| ContractError::CustomError {
                    val: format!("Overflow adding total manager fees: {}", e)
                })?;
            msgs.push(BankMsg::Send {
                to_address: manager_addr.to_string(),
                amount: vec![Coin {
                    denom: config.cdt_denom.clone(),
                    amount: *fee_amount,
                }],
            }.into());
            
            // Award points to manager if points system is configured
            if let Some(points_system) = &config.points_system_contract {
                let points_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: points_system.to_string(),
                    msg: to_json_binary(&membrane::points_system::ExecuteMsg::GivePointsForManagerFee {
                        manager: manager_addr.to_string(),
                        fee_amount: *fee_amount,
                    })?,
                    funds: vec![],
                });
                // Use reply_on_error so points failure doesn't fail the claim
                submsgs.push(SubMsg::reply_on_error(points_msg, 0));
            }
        }
    }
    
    // If there are deposits to compound, create swap message
    // This swap converts all compounding deposits' CDT into deposit tokens
    // The reply handler will distribute the new tokens proportionally back to deposits
    if !total_to_compound.is_zero() && !compound_contributions.is_empty() {
        // Query deposit token balance before swap
        // This is critical: we need to know the balance BEFORE to calculate how much we received AFTER
        let deposit_token_balance_before: Coin = deps.querier.query_balance(
            env.contract.address.clone(),
            config.deposit_denom.denom.clone(),
        )?;
        
        // Save compound propagation state for reply handler
        // This stores which deposits contributed and how much, so we can distribute proportionally
        COMPOUND_PROPAGATION.save(deps.storage, &CompoundPropagation {
            deposit_contributions: compound_contributions.clone(),
            deposit_token_balance_before: deposit_token_balance_before.amount,
            asset: asset.clone(),
        })?;
        
        // Create swap message via chain proxy (neutron_proxy) to swap CDT to deposit token
        // The swap will execute asynchronously, and the reply handler processes the result
        let swap_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.chain_proxy_contract.to_string(),
            msg: to_json_binary(&NeutronProxy_ExecuteMsg::ExecuteSwaps {
                token_out: config.deposit_denom.denom.clone(),
                max_slippage: Decimal::percent(90), // 90% max slippage - allows for market volatility
            })?,
            funds: vec![Coin {
                denom: config.cdt_denom.clone(),
                amount: total_to_compound,
            }],
        });
        
        // Use reply_on_success so we only process if swap succeeds
        submsgs.push(SubMsg::reply_on_success(swap_msg, crate::contract::COMPOUND_SWAP_REPLY_ID));
    }
    
    // Handle affiliate fees if applicable
    let mut affiliate_fees_total = Uint128::zero();
    
    if !total_claimed.is_zero() {
        let mut affiliates = crate::state::AFFILIATES.load(deps.storage, user.clone()).unwrap_or_else(|_| vec![]);
        
        // Early skip/reset affiliates with zero fee and zero time
        affiliates.retain(|a| !a.affiliate_fee.is_zero() || a.time_affiliated != 0);
        
        if !affiliates.is_empty() {
            // Calculate affiliate fees
            let affiliate_fees = split_affiliate_fee(affiliates.clone(), config.affiliate_fee, env.block.time.seconds())?;
            let total_affiliate_fee_ratio: Decimal = affiliate_fees.iter().sum();
            affiliate_fees_total = decimal_multiplication(
                Decimal::from_ratio(total_claimed, Uint128::one()),
                total_affiliate_fee_ratio
            )?.to_uint_floor();
            
            // Send affiliate fees directly to affiliate addresses
            for (i, affiliate_fee_ratio) in affiliate_fees.into_iter().enumerate() {
                let affiliate_amount = decimal_multiplication(
                    Decimal::from_ratio(affiliate_fees_total, Uint128::one()),
                    affiliate_fee_ratio
                )?.to_uint_floor();
                
                if !affiliate_amount.is_zero() {
                    msgs.push(BankMsg::Send {
                        to_address: affiliates[i].affiliate_address.clone(),
                        amount: vec![Coin {
                            denom: config.cdt_denom.clone(),
                            amount: affiliate_amount,
                        }],
                    }.into());
                }
            }
            
            // Update affiliates: preserve historic flows (up to 10), reset time_affiliated
            update_affiliates(deps.storage, affiliates, user.clone(), env.block.time.seconds())?;
        }
    }
    
    // Condense deposits from completed epochs to prevent state bloat
    // Do this after all other deps operations to avoid borrow issues
    let condensed_deposits = condense_deposits_for_user(
        deps,
        &env,
        &config,
        &user_addr,
        &asset,
    )?;

    // Redundant check: verify that manager_fees + affiliate_fees_total + compound_amount + user_amount <= total_claimed (before manager fee deduction)
    // Note: total_claimed already has manager fees deducted, so we need to add them back for the check
    let total_claimed_before_fees = total_claimed.checked_add(total_manager_fees)
        .map_err(|_| ContractError::CustomError {
            val: "Overflow calculating total claimed before fees".to_string(),
        })?;
    let user_amount_before_affiliate = total_claimed.checked_sub(total_to_compound)
        .unwrap_or(Uint128::zero());
    let total_to_user = user_amount_before_affiliate.checked_sub(affiliate_fees_total)
        .map_err(|_| ContractError::CustomError {
            val: "Affiliate fees exceed total claimed amount".to_string(),
        })?;
    
    if !total_to_user.is_zero() {
        // Use recipient address validated earlier
        
        msgs.push(BankMsg::Send {
            to_address: recipient_addr.to_string(),
            amount: vec![Coin {
                denom: config.cdt_denom.clone(),
                amount: total_to_user,
            }],
        }.into());
    }
    
    Ok(Response::new()
        .add_messages(msgs)
        .add_submessages(submsgs)
        .add_attributes(vec![
            attr("method", "claim_revenue_for_user"),
            // attr("caller", info.sender.to_string()),
            attr("user", user_addr.to_string()),
            attr("asset", asset),
            // attr("claimed_amount", total_claimed_before_fees.to_string()),
            attr("revenue_claimed", total_claimed_before_fees),
            attr("manager_fees", total_manager_fees.to_string()),
            attr("compound_amount", total_to_compound.to_string()),
            attr("affiliate_fees", affiliate_fees_total.to_string()),
            attr("user_amount", total_to_user.to_string()),
            attr("deposits_condensed", condensed_deposits.len().to_string()),
        ]))
}

/// Refresh lock on a deposit
pub fn refresh_lock(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    user: Option<String>,
    asset: String,
    ltv: Decimal,
    max_borrow_ltv: Decimal,
    deposit_id: Uint128,
    epoch_start_time: u64,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    
    // Use provided user or default to info.sender (permissionless)
    let user_addr = if let Some(user_str) = user {
        deps.api.addr_validate(&user_str)?
    } else {
        info.sender.clone()
    };
    
    let asset_str = asset.clone();
    let ltv_str = ltv.to_string();
    let max_borrow_ltv_str = max_borrow_ltv.to_string();
    let user_str = user_addr.to_string();
    
    // Construct deposit key using epoch_start_time
    let deposit_key = make_deposit_key(&asset_str, &ltv_str, &max_borrow_ltv_str, &user_str, &deposit_id, epoch_start_time);
    
    let mut deposit = BACKING_DEPOSITS.load(deps.storage, deposit_key.clone())?;

    // Calculate old locked_vault_tokens before refresh
    let old_locked_vault_tokens = deposit.locked_vault_tokens;
    let old_locked = deposit.locked.clone();
    
    // Save old deposit state for LVT tracking
    let old_deposit_for_tracking = deposit.clone();

    // Refresh lock
    refresh_deposit_lock(&mut deposit, &env, config.lock_duration_ceiling)?;
    
    // Recalculate locked_vault_tokens after refresh (in case perpetual lock extended)
    deposit.locked_vault_tokens = calculate_locked_vault_tokens(&deposit, &env);
    
    // Update group.total_locked_vault_tokens if changed
    if deposit.locked_vault_tokens != old_locked_vault_tokens {
        let mut queue = LTV_QUEUES.load(deps.storage, asset.clone())?;
        let slot_index = queue.slots.iter().position(|s| s.ltv == ltv)
            .ok_or_else(|| ContractError::CustomError { val: "Slot not found".to_string() })?;
        let mut slot = queue.slots[slot_index].clone();
        let group_index = find_or_create_borrow_group(&mut slot, max_borrow_ltv, false)?;
        let mut group = slot.deposit_groups[group_index].clone();
        
        // Update by delta
        if deposit.locked_vault_tokens > old_locked_vault_tokens {
            //Add delta to group.total_locked_vault_tokens
            let delta = deposit.locked_vault_tokens.checked_sub(old_locked_vault_tokens)
                .map_err(|e| ContractError::CustomError { 
                    val: format!("Overflow calculating locked_vault_tokens delta: {}", e) 
                })?;
            group.total_locked_vault_tokens = group.total_locked_vault_tokens.checked_add(delta)
                .map_err(|e| ContractError::CustomError { 
                    val: format!("Overflow updating total_locked_vault_tokens: {}", e) 
                })?;
        } else if deposit.locked_vault_tokens < old_locked_vault_tokens {
            //Subtract delta from group.total_locked_vault_tokens
            let delta = old_locked_vault_tokens.checked_sub(deposit.locked_vault_tokens)
                .map_err(|e| ContractError::CustomError { 
                    val: format!("Overflow calculating locked_vault_tokens delta: {}", e) 
                })?;
            group.total_locked_vault_tokens = group.total_locked_vault_tokens.checked_sub(delta)
                .map_err(|e| ContractError::CustomError { 
                    val: format!("Overflow updating total_locked_vault_tokens: {}", e) 
                })?;
        }
        
        // Update slot and queue
        slot.deposit_groups[group_index] = group;
        queue.slots[slot_index] = slot;
        LTV_QUEUES.save(deps.storage, asset.clone(), &queue)?;

        // Update effective total
        // NOTE: This must be called AFTER queue is saved, as it will load and save it again
        // Query epoch info to calculate effective vault tokens
        let epoch_info = if let Some(revenue_distributor_addr) = &config.revenue_distributor {
            deps.querier.query_wasm_smart::<EpochCountdownResponse>(
                revenue_distributor_addr,
                &RevenueDistributorQueryMsg::EpochCountdown {},
            )
            .ok()
            .map(|countdown| (countdown.epoch_start, countdown.epoch_end, env.block.time.seconds()))
        } else {
            None
        };

        // Calculate old and new effective vault tokens
        let mut old_deposit = deposit.clone();
        old_deposit.locked_vault_tokens = old_locked_vault_tokens;
        old_deposit.locked = old_locked.clone(); // Use old lock state for calculation
        let old_unused_vt = if let Some((epoch_start, epoch_end, current_time)) = epoch_info {
            calculate_unused_locked_vault_tokens(&old_deposit, &env, epoch_start, epoch_end, current_time)
        } else {
            old_locked_vault_tokens
        };

        let new_unused_vt = if let Some((epoch_start, epoch_end, current_time)) = epoch_info {
            calculate_unused_locked_vault_tokens(&deposit, &env, epoch_start, epoch_end, current_time)
        } else {
            deposit.locked_vault_tokens
        };

        // Update effective total
        update_group_unused_total(
            deps.storage,
            &deps.querier,
            &env,
            &config,
            &asset,
            ltv,
            max_borrow_ltv,
            old_unused_vt,
            new_unused_vt,
            deposit.deposit_time,
        )?;
        
        // Update deposit's LVT tracking after refresh
        update_deposit_lvt_tracking(&mut deposit, &env, config.lock_duration_ceiling)?;
        
        // Update group LVT tracking for modified deposit
        update_group_lvt_tracking_for_deposit(
            deps.storage,
            &env,
            &asset,
            ltv,
            max_borrow_ltv,
            Some(&old_deposit_for_tracking), // old_deposit: before refresh
            Some(&deposit), // new_deposit: after refresh
        )?;
    }

    BACKING_DEPOSITS.save(deps.storage, deposit_key, &deposit)?;
    
    // Update locked deposits tracking if still locked
    if deposit.locked.is_some() {
        update_locked_deposit(
            deps.storage,
            &user_addr,
            &asset,
            ltv,
            max_borrow_ltv,
            deposit_id,
            &deposit,
        )?;
    }
    
    Ok(Response::new()
        .add_attribute("method", "refresh_lock")
        .add_attribute("user", user_addr.to_string())
        .add_attribute("caller", info.sender.to_string())
        .add_attribute("asset", asset)
        .add_attribute("deposit_id", deposit_id.to_string()))
}

/// Lock a deposit for a specified duration
pub fn lock_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    asset: String,
    ltv: Decimal,
    max_borrow_ltv: Decimal,
    deposit_id: Uint128,
    locked: membrane::types::Locked,
    amount: Option<Uint128>,
    epoch_start_time: u64,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    
    // Validate lock duration doesn't exceed ceiling
    let lock_duration_days = (locked.locked_until - env.block.time.seconds()) / ONE_DAY_SECONDS;
    if lock_duration_days > config.lock_duration_ceiling {
        return Err(ContractError::CustomError {
            val: format!("Lock duration exceeds ceiling of {} days", config.lock_duration_ceiling),
        });
    }
    
    let asset_str = asset.clone();
    let ltv_str = ltv.to_string();
    let max_borrow_ltv_str = max_borrow_ltv.to_string();
    let user_str = info.sender.to_string();
    
    // Construct deposit key using epoch_start_time
    let deposit_key = make_deposit_key(&asset_str, &ltv_str, &max_borrow_ltv_str, &user_str, &deposit_id, epoch_start_time);
    
    let mut deposit = BACKING_DEPOSITS.load(deps.storage, deposit_key.clone())?;
    
    // Claim revenue before updating lock
    let _claimed = claim_revenue_for_deposit(
        deps.storage,
        &deps.querier,
        &env,
        &config,
        &mut deposit,
        asset.clone(),
        ltv,
        max_borrow_ltv,
        None,
    )?;
    
    // Calculate old locked_vault_tokens before updating lock
    let old_locked_vault_tokens = deposit.locked_vault_tokens;
    let old_locked = deposit.locked.clone();
    
    // Save old deposit state for LVT tracking
    let old_deposit_for_tracking = deposit.clone();

    // Check if deposit is already locked and still locked
    if let Some(ref existing_lock) = deposit.locked {
        if existing_lock.locked_until > env.block.time.seconds() {
            return Err(ContractError::CustomError {
                val: "Deposit is already locked".to_string(),
            });
        }
    }
    
    // Get group to update total_locked_vault_tokens
    let mut queue = LTV_QUEUES.load(deps.storage, asset.clone())?;
    let slot_index = queue.slots.iter().position(|s| s.ltv == ltv)
        .ok_or_else(|| ContractError::CustomError { val: "Slot not found".to_string() })?;
    let mut slot = queue.slots[slot_index].clone();
    let group_index = find_or_create_borrow_group(&mut slot, max_borrow_ltv, false)?;
    let mut group = slot.deposit_groups[group_index].clone();
    
    // If amount specified, split deposit
    let lock_amount_vt = if let Some(amount_vt) = amount {
        let amount_vt = std::cmp::min(amount_vt, deposit.vault_tokens);
        if amount_vt == deposit.vault_tokens {
            // Full lock - no split needed
            None
        } else {
            // Partial lock - split deposit
            Some(amount_vt)
        }
    } else {
        // Full lock
        None
    };
    
    if let Some(lock_amount_vt) = lock_amount_vt {
        // Partial lock: split the deposit
        let remaining_vt = deposit.vault_tokens - lock_amount_vt;
        
        // Update existing deposit to be the locked portion
        deposit.vault_tokens = lock_amount_vt;
        let intended_lock_days = (locked.locked_until - env.block.time.seconds()) / ONE_DAY_SECONDS;
        deposit.locked = Some(membrane::types::Locked {
            locked_until: locked.locked_until,
            perpetual_lock: locked.perpetual_lock,
            intended_lock_days: Some(intended_lock_days),
        });
        deposit.start_time = env.block.time.seconds(); // Reset start time for new lock
        
        // Recalculate locked_vault_tokens after lock update
        deposit.locked_vault_tokens = calculate_locked_vault_tokens(&deposit, &env);
        
        // Update group.total_locked_vault_tokens by subtracting the old and adding the new VTs
        group.total_locked_vault_tokens = group.total_locked_vault_tokens.checked_add(deposit.locked_vault_tokens)
            .map_err(|e| ContractError::CustomError { 
                val: format!("Overflow updating total_locked_vault_tokens: {}", e) 
            })?;
        group.total_locked_vault_tokens = group.total_locked_vault_tokens.checked_sub(old_locked_vault_tokens)
            .map_err(|e| ContractError::CustomError { 
                val: format!("Overflow updating total_locked_vault_tokens: {}", e) 
            })?;
    
        
        // Update slot and queue
        slot.deposit_groups[group_index] = group.clone();
        queue.slots[slot_index] = slot;
        LTV_QUEUES.save(deps.storage, asset.clone(), &queue)?;
        
        // Update effective total for locked portion
        // Query epoch info to calculate effective vault tokens
        let epoch_info = if let Some(revenue_distributor_addr) = &config.revenue_distributor {
            deps.querier.query_wasm_smart::<EpochCountdownResponse>(
                revenue_distributor_addr,
                &RevenueDistributorQueryMsg::EpochCountdown {},
            )
            .ok()
            .map(|countdown| (countdown.epoch_start, countdown.epoch_end, env.block.time.seconds()))
        } else {
            None
        };
        
        // Calculate old and new effective vault tokens for locked portion
        let mut old_deposit = deposit.clone();
        old_deposit.locked_vault_tokens = old_locked_vault_tokens;
        let old_unused_vt = if let Some((epoch_start, epoch_end, current_time)) = epoch_info {
            calculate_unused_locked_vault_tokens(&old_deposit, &env, epoch_start, epoch_end, current_time)
        } else {
            old_locked_vault_tokens
        };
        
        let new_unused_vt = if let Some((epoch_start, epoch_end, current_time)) = epoch_info {
            calculate_unused_locked_vault_tokens(&deposit, &env, epoch_start, epoch_end, current_time)
        } else {
            deposit.locked_vault_tokens
        };
        
        // Update deposit's LVT tracking after lock
        update_deposit_lvt_tracking(&mut deposit, &env, config.lock_duration_ceiling)?;
        
        BACKING_DEPOSITS.save(deps.storage, deposit_key.clone(), &deposit)?;

        // Add to locked deposits tracking
        add_locked_deposit(
            deps.storage,
            &info.sender,
            &asset,
            ltv,
            max_borrow_ltv,
            deposit_id,
            &deposit,
        )?;
        
        // Update group LVT tracking for modified locked deposit
        update_group_lvt_tracking_for_deposit(
            deps.storage,
            &env,
            &asset,
            ltv,
            max_borrow_ltv,
            Some(&old_deposit_for_tracking), // old_deposit: before lock
            Some(&deposit), // new_deposit: after lock
        )?;

        // Update effective total for locked portion
        // NOTE: Must be called before reloading queue, as it saves the queue
        update_group_unused_total(
            deps.storage,
            &deps.querier,
            &env,
            &config,
            &asset,
            ltv,
            max_borrow_ltv,
            old_unused_vt,
            new_unused_vt,
            deposit.deposit_time,
        )?;

        // Create new unlocked deposit with remaining amount
        // Reload queue after update_group_unused_total modified it
        let queue = LTV_QUEUES.load(deps.storage, asset.clone())?;
        let new_deposit_id = queue.current_deposit_id;
        LTV_QUEUES.update(deps.storage, asset.clone(), |q| -> Result<_, ContractError> {
            let mut queue = q.ok_or_else(|| ContractError::CustomError {
                val: "Queue not found".to_string(),
            })?;
            queue.current_deposit_id += Uint128::one();
            Ok(queue)
        })?;
        
        // Query epoch start time from revenue distributor for new deposit
        let epoch_start_time = if let Some((epoch_start, _, _)) = epoch_info {
            epoch_start
        } else {
            env.block.time.seconds()
        };
        
        let new_deposit_key = make_deposit_key(&asset_str, &ltv_str, &max_borrow_ltv_str, &user_str, &new_deposit_id, epoch_start_time);
        let temp_new_deposit = BackingDeposit {
            user: info.sender.clone(),
            vault_tokens: remaining_vt,
            locked_vault_tokens: Uint128::zero(), // Will be calculated below
            max_borrow_ltv,
            last_claimed: deposit.last_claimed, // Keep same last_claimed
            locked: None,
            start_time: deposit.start_time, // Keep original start time
            deposit_time: deposit.deposit_time, // Preserve deposit_time
            compound_claims: deposit.compound_claims, // Preserve compound_claims
            manager: deposit.manager.clone(), // Preserve manager
            depositor: deposit.depositor.clone(), // Preserve depositor
            withdrawals_enabled: deposit.withdrawals_enabled, // Preserve withdrawals_enabled
            lvt_tracking: DepositLVTTracking {
                base_lvt: Uint128::zero(),
                reference_time: env.block.time.seconds(),
                daily_delta: Int128::zero(),
                time_cliffs: vec![],
            },
            revenue_destination: deposit.revenue_destination.clone(),
        };
        let new_deposit_locked_vt = calculate_locked_vault_tokens(&temp_new_deposit, &env);
        let mut new_deposit = BackingDeposit {
            user: info.sender.clone(),
            vault_tokens: remaining_vt,
            locked_vault_tokens: new_deposit_locked_vt,
            max_borrow_ltv,
            last_claimed: deposit.last_claimed, // Keep same last_claimed
            locked: None,
            start_time: deposit.start_time, // Keep original start time
            deposit_time: deposit.deposit_time, // Preserve deposit_time
            compound_claims: deposit.compound_claims, // Preserve compound_claims
            manager: deposit.manager.clone(), // Preserve manager
            depositor: deposit.depositor.clone(), // Preserve depositor
            withdrawals_enabled: deposit.withdrawals_enabled, // Preserve withdrawals_enabled
            lvt_tracking: DepositLVTTracking {
                base_lvt: Uint128::zero(),
                reference_time: env.block.time.seconds(),
                daily_delta: Int128::zero(),
                time_cliffs: vec![],
            },
            revenue_destination: deposit.revenue_destination.clone(),
        };
        // Initialize LVT tracking for new deposit
        initialize_deposit_lvt_tracking(&mut new_deposit, &env, config.lock_duration_ceiling)?;
        // Add new deposit's locked_vault_tokens to group total.
        //We already subtracted the original deposit's VTs
        group.total_locked_vault_tokens = group.total_locked_vault_tokens.checked_add(new_deposit_locked_vt)
            .map_err(|e| ContractError::CustomError { 
                val: format!("Overflow adding new deposit locked_vault_tokens: {}", e) 
            })?;
        
        BACKING_DEPOSITS.save(deps.storage, new_deposit_key.clone(), &new_deposit)?;

        // Add to USER_DEPOSITS index
        let mut keys = USER_DEPOSITS
            .may_load(deps.storage, (info.sender.clone(), asset.clone()))?
            .unwrap_or_else(Vec::new);
        keys.push(new_deposit_key);
        USER_DEPOSITS.save(deps.storage, (info.sender.clone(), asset.clone()), &keys)?;
        
        // Update group LVT tracking for new unlocked deposit
        update_group_lvt_tracking_for_deposit(
            deps.storage,
            &env,
            &asset,
            ltv,
            max_borrow_ltv,
            None, // old_deposit: None for new deposit
            Some(&new_deposit), // new_deposit: the newly created unlocked deposit
        )?;

        // Update effective total for new unlocked deposit (old is 0)
        // NOTE: This must be called AFTER all other storage operations
        let new_deposit_effective_vt = if let Some((epoch_start, epoch_end, current_time)) = epoch_info {
            calculate_unused_locked_vault_tokens(&new_deposit, &env, epoch_start, epoch_end, current_time)
        } else {
            new_deposit_locked_vt
        };

        update_group_unused_total(
            deps.storage,
            &deps.querier,
            &env,
            &config,
            &asset,
            ltv,
            max_borrow_ltv,
            Uint128::zero(),
            new_deposit_effective_vt,
            new_deposit.deposit_time,
        )?;
    } else {
        // Full lock
        let intended_lock_days = (locked.locked_until - env.block.time.seconds()) / ONE_DAY_SECONDS;
        deposit.locked = Some(membrane::types::Locked {
            locked_until: locked.locked_until,
            perpetual_lock: locked.perpetual_lock,
            intended_lock_days: Some(intended_lock_days),
        });
        deposit.start_time = env.block.time.seconds(); // Reset start time for new lock
        
        // Recalculate locked_vault_tokens after lock update
        deposit.locked_vault_tokens = calculate_locked_vault_tokens(&deposit, &env);
        
        // Update group.total_locked_vault_tokens subtracting the old locked_vault_tokens
        group.total_locked_vault_tokens = group.total_locked_vault_tokens.checked_sub(old_locked_vault_tokens)
            .map_err(|e| ContractError::CustomError { 
                val: format!("Overflow subtracting old locked_vault_tokens from group: {}", e) 
            })?;
        // Add the new locked_vault_tokens to group total
        group.total_locked_vault_tokens = group.total_locked_vault_tokens.checked_add(deposit.locked_vault_tokens)
            .map_err(|e| ContractError::CustomError { 
                val: format!("Overflow adding new locked_vault_tokens to group: {}", e) 
            })?;
        
        
        // Update slot and queue
        slot.deposit_groups[group_index] = group.clone();
        queue.slots[slot_index] = slot;
        LTV_QUEUES.save(deps.storage, asset.clone(), &queue)?;
        
        // Update deposit's LVT tracking after full lock
        update_deposit_lvt_tracking(&mut deposit, &env, config.lock_duration_ceiling)?;

        BACKING_DEPOSITS.save(deps.storage, deposit_key.clone(), &deposit)?;

        // Add to locked deposits tracking
        add_locked_deposit(
            deps.storage,
            &info.sender,
            &asset,
            ltv,
            max_borrow_ltv,
            deposit_id,
            &deposit,
        )?;
        
        // Update group LVT tracking for full lock
        update_group_lvt_tracking_for_deposit(
            deps.storage,
            &env,
            &asset,
            ltv,
            max_borrow_ltv,
            Some(&old_deposit_for_tracking), // old_deposit: before lock
            Some(&deposit), // new_deposit: after lock
        )?;

        // Update effective total for full lock
        // NOTE: This must be called AFTER queue is saved, as it will load and save it again
        // Query epoch info to calculate effective vault tokens
        let epoch_info = if let Some(revenue_distributor_addr) = &config.revenue_distributor {
            deps.querier.query_wasm_smart::<EpochCountdownResponse>(
                revenue_distributor_addr,
                &RevenueDistributorQueryMsg::EpochCountdown {},
            )
            .ok()
            .map(|countdown| (countdown.epoch_start, countdown.epoch_end, env.block.time.seconds()))
        } else {
            None
        };

        // Calculate old and new effective vault tokens
        let mut old_deposit = deposit.clone();
        old_deposit.locked_vault_tokens = old_locked_vault_tokens;
        old_deposit.locked = old_locked.clone(); // Use old lock state for calculation
        let old_unused_vt = if let Some((epoch_start, epoch_end, current_time)) = epoch_info {
            calculate_unused_locked_vault_tokens(&old_deposit, &env, epoch_start, epoch_end, current_time)
        } else {
            old_locked_vault_tokens
        };

        let new_unused_vt = if let Some((epoch_start, epoch_end, current_time)) = epoch_info {
            calculate_unused_locked_vault_tokens(&deposit, &env, epoch_start, epoch_end, current_time)
        } else {
            deposit.locked_vault_tokens
        };

        // Update effective total
        update_group_unused_total(
            deps.storage,
            &deps.querier,
            &env,
            &config,
            &asset,
            ltv,
            max_borrow_ltv,
            old_unused_vt,
            new_unused_vt,
            deposit.deposit_time,
        )?;
    }
    
    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "lock_deposit"),
            attr("user", info.sender.to_string()),
            attr("asset", asset),
            attr("deposit_id", deposit_id.to_string()),
            attr("locked_until", locked.locked_until.to_string()),
        ]))
}

/// Move a deposit to a different slot/group
pub fn move_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    asset: String,
    ltv: Decimal,
    max_borrow_ltv: Decimal,
    deposit_id: Uint128,
    destination: BackingDepositInput,
    amount: Option<Uint128>,
    user: Option<String>,
    epoch_start_time: u64,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut msgs: Vec<CosmosMsg> = vec![];
    
    // Validate destination
    validate_deposit_input(deps.storage, destination.clone())?;
    
    // Clone early for attributes
    let source_asset = asset.clone();
    let dest_asset = destination.asset.clone();
    
    // Claim revenue for source deposit BEFORE moving
    let asset_str = asset.clone();
    let ltv_str = ltv.to_string();
    let max_borrow_ltv_str = max_borrow_ltv.to_string();
    
    // Determine user and deposit key: use provided user or default to sender
    let (actual_user, deposit_key) = if let Some(user_str) = user {
        let validated_user = deps.api.addr_validate(&user_str)?;
        
        // Construct deposit key using epoch_start_time
        let user_str_for_key = validated_user.to_string();
        let key = make_deposit_key(&asset_str, &ltv_str, &max_borrow_ltv_str, &user_str_for_key, &deposit_id, epoch_start_time);
        
        // If user is provided, check that sender is a manager for this user's deposit
        let deposit_check = BACKING_DEPOSITS.load(deps.storage, key.clone())?;
        // Verify that the sender is indeed the manager
        if deposit_check.manager.as_ref() == Some(&info.sender) {
            (validated_user, key)
        } else {
            return Err(ContractError::CustomError { 
                val: "Deposit not found or sender is not the manager for this deposit".to_string() 
            });
        }
    } else {
        // Default to sender - construct deposit key using epoch_start_time
        let user_str = info.sender.to_string();
        let key = make_deposit_key(&asset_str, &ltv_str, &max_borrow_ltv_str, &user_str, &deposit_id, epoch_start_time);
        (info.sender.clone(), key)
    };
    
    let mut deposit = BACKING_DEPOSITS.load(deps.storage, deposit_key.clone())?;
    
    // Refresh lock if needed
    refresh_deposit_lock(&mut deposit, &env, config.lock_duration_ceiling)?;
    
    // Claim revenue before move
    let claimed_revenue = claim_revenue_for_deposit(
        deps.storage,
        &deps.querier,
        &env,
        &config,
        &mut deposit,
        asset.clone(),
        ltv,
        max_borrow_ltv,
        None,
    )?;
    
    // Send claimed revenue to user (not manager)
    if !claimed_revenue.is_zero() {
        msgs.push(BankMsg::Send {
            to_address: actual_user.to_string(),
            amount: vec![Coin {
                denom: config.cdt_denom.clone(),
                amount: claimed_revenue,
            }],
        }.into());
    }
    
    // Calculate vault tokens to move
    let vault_tokens_to_move = if let Some(amount_vt) = amount {
        std::cmp::min(amount_vt, deposit.vault_tokens)
    } else {
        deposit.vault_tokens
    };
    
    // Load source queue
    let mut source_queue = LTV_QUEUES.load(deps.storage, asset.clone())?;
    let mut source_slot_index = source_queue.slots.iter().position(|s| s.ltv == ltv)
        .ok_or_else(|| ContractError::CustomError { val: "Source slot not found".to_string() })?;
    let mut source_slot = source_queue.slots[source_slot_index].clone();
    let mut source_group_index = find_or_create_borrow_group(&mut source_slot, max_borrow_ltv, false)?;
    let mut source_group = source_slot.deposit_groups[source_group_index].clone();
    
    // Calculate old locked_vault_tokens before moving
    let old_locked_vault_tokens = deposit.locked_vault_tokens;
    
    // Save old deposit state for LVT tracking
    let old_deposit_for_tracking = deposit.clone();
    
    // Calculate base tokens to move
    let base_tokens_to_move = calculate_base_tokens(
        vault_tokens_to_move,
        source_group.total_deposit_tokens,
        source_group.total_vault_tokens,
    )?;
    
    // Update source group
    source_group.total_deposit_tokens -= base_tokens_to_move;
    source_group.total_vault_tokens -= vault_tokens_to_move;
    
    // Subtract old locked_vault_tokens from source group total
    source_group.total_locked_vault_tokens = source_group.total_locked_vault_tokens.checked_sub(old_locked_vault_tokens)
        .map_err(|e| ContractError::CustomError { 
            val: format!("Underflow subtracting locked_vault_tokens from source group: {}", e) 
        })?;
    
    // Query epoch info for effective total updates
    let epoch_info = if let Some(revenue_distributor_addr) = &config.revenue_distributor {
        deps.querier.query_wasm_smart::<EpochCountdownResponse>(
            revenue_distributor_addr,
            &RevenueDistributorQueryMsg::EpochCountdown {},
        )
        .ok()
        .map(|countdown| (countdown.epoch_start, countdown.epoch_end, env.block.time.seconds()))
    } else {
        None
    };
    
    // Calculate old effective vault tokens for source deposit
    let old_source_effective_vt = if let Some((epoch_start, epoch_end, current_time)) = epoch_info {
        calculate_unused_locked_vault_tokens(&deposit, &env, epoch_start, epoch_end, current_time)
    } else {
        old_locked_vault_tokens
    };
    
    // Update or remove source deposit
    let is_full_move = vault_tokens_to_move == deposit.vault_tokens;
    let locked_info = deposit.locked.clone();
    
    if is_full_move {
        // Update effective total for source group (remove deposit - new effective is 0)
        update_group_unused_total(
            deps.storage,
            &deps.querier,
            &env,
            &config,
            &asset,
            ltv,
            max_borrow_ltv,
            old_source_effective_vt,
            Uint128::zero(),
            deposit.deposit_time,
        )?;
        
        // Remove source deposit
        BACKING_DEPOSITS.remove(deps.storage, deposit_key.clone());
        
        // Remove from USER_DEPOSITS index
        let mut user_keys = USER_DEPOSITS
            .may_load(deps.storage, (actual_user.clone(), asset.clone()))?
            .unwrap_or_else(Vec::new);
        user_keys.retain(|k| k != &deposit_key);
        if user_keys.is_empty() {
            USER_DEPOSITS.remove(deps.storage, (actual_user.clone(), asset.clone()));
        } else {
            USER_DEPOSITS.save(deps.storage, (actual_user.clone(), asset.clone()), &user_keys)?;
        }
        
        // Remove from MANAGED_DEPOSITS if manager exists
        if let Some(manager) = &deposit.manager {
            crate::state::remove_managed_deposit(deps.storage, manager, &deposit_key)?;
        }
        
        // Remove from locked deposits tracking if was locked
        if locked_info.is_some() {
            remove_locked_deposit(
                deps.storage,
                &actual_user,
                &asset,
                ltv,
                max_borrow_ltv,
                deposit_id,
            )?;
        }
        
        // Update source group LVT tracking for removed deposit
        update_group_lvt_tracking_for_deposit(
            deps.storage,
            &env,
            &asset,
            ltv,
            max_borrow_ltv,
            Some(&old_deposit_for_tracking), // old_deposit: deposit being moved
            None, // new_deposit: None since fully moved
        )?;

        // Reload source queue after update_group_unused_total modified it
        source_queue = LTV_QUEUES.load(deps.storage, asset.clone())?;
        source_slot_index = source_queue.slots.iter().position(|s| s.ltv == ltv)
            .ok_or_else(|| ContractError::CustomError { val: "Source slot not found after update".to_string() })?;
        source_slot = source_queue.slots[source_slot_index].clone();
        source_group_index = source_slot.deposit_groups.iter().position(|g| g.max_borrow_ltv == max_borrow_ltv)
            .ok_or_else(|| ContractError::CustomError { val: "Source group not found after update".to_string() })?;
        source_group = source_slot.deposit_groups[source_group_index].clone();
    } else {
        // Update source deposit
        deposit.vault_tokens -= vault_tokens_to_move;
        
        // Recalculate locked_vault_tokens for remaining deposit
        deposit.locked_vault_tokens = calculate_locked_vault_tokens(&deposit, &env);
        
        // Add back the new locked_vault_tokens to source group total
        source_group.total_locked_vault_tokens = source_group.total_locked_vault_tokens.checked_add(deposit.locked_vault_tokens)
            .map_err(|e| ContractError::CustomError { 
                val: format!("Overflow adding locked_vault_tokens to source group: {}", e) 
            })?;
        
        // Update deposit's LVT tracking after partial move
        update_deposit_lvt_tracking(&mut deposit, &env, config.lock_duration_ceiling)?;
        
        BACKING_DEPOSITS.save(deps.storage, deposit_key.clone(), &deposit)?;

        // Update locked deposits tracking if still locked
        if deposit.locked.is_some() {
            update_locked_deposit(
                deps.storage,
                &actual_user,
                &asset,
                ltv,
                max_borrow_ltv,
                deposit_id,
                &deposit,
            )?;
        }
        
        // Update source group LVT tracking for modified deposit
        update_group_lvt_tracking_for_deposit(
            deps.storage,
            &env,
            &asset,
            ltv,
            max_borrow_ltv,
            Some(&old_deposit_for_tracking), // old_deposit: before partial move
            Some(&deposit), // new_deposit: after partial move
        )?;

        // Calculate new effective vault tokens for remaining source deposit
        let new_source_effective_vt = if let Some((epoch_start, epoch_end, current_time)) = epoch_info {
            calculate_unused_locked_vault_tokens(&deposit, &env, epoch_start, epoch_end, current_time)
        } else {
            deposit.locked_vault_tokens
        };

        // Update effective total for source group
        // NOTE: Must be called before reloading source queue, as it saves the queue
        update_group_unused_total(
            deps.storage,
            &deps.querier,
            &env,
            &config,
            &asset,
            ltv,
            max_borrow_ltv,
            old_source_effective_vt,
            new_source_effective_vt,
            deposit.deposit_time,
        )?;

        // Reload source queue after update_group_unused_total modified it
        source_queue = LTV_QUEUES.load(deps.storage, asset.clone())?;
        source_slot_index = source_queue.slots.iter().position(|s| s.ltv == ltv)
            .ok_or_else(|| ContractError::CustomError { val: "Source slot not found after update".to_string() })?;
        source_slot = source_queue.slots[source_slot_index].clone();
        source_group_index = source_slot.deposit_groups.iter().position(|g| g.max_borrow_ltv == max_borrow_ltv)
            .ok_or_else(|| ContractError::CustomError { val: "Source group not found after update".to_string() })?;
        source_group = source_slot.deposit_groups[source_group_index].clone();
    }

    // Update source slot and queue
    source_slot.total_deposit_tokens -= base_tokens_to_move;
    source_slot.deposit_groups[source_group_index] = source_group;
    source_queue.slots[source_slot_index] = source_slot.clone();
    LTV_QUEUES.save(deps.storage, asset.clone(), &source_queue)?;
    
    // Load or create destination queue
    let mut dest_queue = if destination.asset == asset {
        source_queue
    } else {
        LTV_QUEUES.load(deps.storage, destination.asset.clone())?
    };
    
    // Find or create destination slot and group
    let dest_slot_index = find_or_create_ltv_slot(&mut dest_queue, destination.ltv)?;
    let mut dest_slot = dest_queue.slots[dest_slot_index].clone();
    let dest_group_index = find_or_create_borrow_group(&mut dest_slot, destination.max_borrow_ltv, true)?;
    let mut dest_group = dest_slot.deposit_groups[dest_group_index].clone();
    
    // Always create a new deposit (can't condense due to deposit_time requirements)
    let dest_deposit_id = {
        let new_id = dest_queue.current_deposit_id;
        dest_queue.current_deposit_id += Uint128::one();
        new_id
    };
    
    // Epoch start for the destination has to be the current epoch, we can't allow inputs for this
    let dest_epoch_start_time = 
        // Get epoch start time from epoch_info 
        epoch_info.map(|(epoch_start, _, _)| epoch_start).unwrap_or(env.block.time.seconds());
    
    // Create new destination deposit (always create new due to deposit_time requirements)
    let dest_deposit_key = make_deposit_key(
        &destination.asset,
        &destination.ltv.to_string(),
        &destination.max_borrow_ltv.to_string(),
        &actual_user.to_string(),
        &dest_deposit_id,
        dest_epoch_start_time,
    );

    //The start_time should only reset if its in a different asset
    
    // Determine start_time: reset if asset is changing, otherwise preserve
    let new_start_time = if destination.asset != asset {
        env.block.time.seconds() // Reset start_time when asset changes
    } else {
        deposit.start_time // Preserve start_time when asset is the same
    };
    
    // Create new deposit
    let temp_new_deposit = BackingDeposit {
        user: actual_user.clone(),
        vault_tokens: vault_tokens_to_move,
        locked_vault_tokens: Uint128::zero(), // Will be calculated below
        max_borrow_ltv: destination.max_borrow_ltv,
        last_claimed: env.block.time.seconds(),
        locked: locked_info.clone(), // Preserve lock from source
        start_time: new_start_time, // Reset only if asset is changing
        deposit_time: deposit.deposit_time, // Preserve original deposit_time for epoch isolation
        compound_claims: deposit.compound_claims, // Preserve compound_claims
        manager: deposit.manager.clone(), // Preserve manager
        depositor: deposit.depositor.clone(), // Preserve depositor
        withdrawals_enabled: deposit.withdrawals_enabled, // Preserve withdrawals_enabled
        lvt_tracking: DepositLVTTracking {
            base_lvt: Uint128::zero(),
            reference_time: env.block.time.seconds(),
            daily_delta: Int128::zero(),
            time_cliffs: vec![],
        },
        revenue_destination: deposit.revenue_destination.clone(),
    };
    let new_deposit_locked_vt = calculate_locked_vault_tokens(&temp_new_deposit, &env);
    let mut new_deposit = BackingDeposit {
        user: actual_user.clone(),
        vault_tokens: vault_tokens_to_move,
        locked_vault_tokens: new_deposit_locked_vt,
        max_borrow_ltv: destination.max_borrow_ltv,
        last_claimed: env.block.time.seconds(),
        locked: locked_info.clone(), // Preserve lock from source
        start_time: new_start_time, // Reset only if asset is changing
        deposit_time: deposit.deposit_time, // Preserve original deposit_time for epoch isolation
        compound_claims: deposit.compound_claims, // Preserve compound_claims
        manager: deposit.manager.clone(), // Preserve manager
        depositor: deposit.depositor.clone(), // Preserve depositor
        withdrawals_enabled: deposit.withdrawals_enabled, // Preserve withdrawals_enabled
        lvt_tracking: DepositLVTTracking {
            base_lvt: Uint128::zero(),
            reference_time: env.block.time.seconds(),
            daily_delta: Int128::zero(),
            time_cliffs: vec![],
        },
        revenue_destination: deposit.revenue_destination.clone(),
    };
    // Initialize LVT tracking for new deposit
    initialize_deposit_lvt_tracking(&mut new_deposit, &env, config.lock_duration_ceiling)?;
    
    // Add new deposit's locked_vault_tokens to dest group total
    dest_group.total_locked_vault_tokens = dest_group.total_locked_vault_tokens.checked_add(new_deposit_locked_vt)
        .map_err(|e| ContractError::CustomError {
            val: format!("Overflow adding new deposit locked_vault_tokens to dest group: {}", e)
        })?;

    // Save deposit (deposit_time is already set to preserve epoch isolation)
    BACKING_DEPOSITS.save(deps.storage, dest_deposit_key.clone(), &new_deposit)?;

    // Add to locked deposits tracking if locked
    let locked_for_tracking = new_deposit.locked.clone();

    // Add to USER_DEPOSITS index
    let mut dest_keys = USER_DEPOSITS
        .may_load(deps.storage, (actual_user.clone(), destination.asset.clone()))?
        .unwrap_or_else(Vec::new);
    dest_keys.push(dest_deposit_key.clone());
    USER_DEPOSITS.save(deps.storage, (actual_user.clone(), destination.asset.clone()), &dest_keys)?;

    // Add to MANAGED_DEPOSITS if manager exists
    if let Some(manager) = &new_deposit.manager {
        crate::state::add_managed_deposit(deps.storage, manager, dest_deposit_key.clone())?;
    }

    // Add to locked deposits tracking if locked
    if locked_for_tracking.is_some() {
        add_locked_deposit(
            deps.storage,
            &actual_user,
            &destination.asset,
            destination.ltv,
            destination.max_borrow_ltv,
            dest_deposit_id,
            &new_deposit,
        )?;
    }

    // Save dest_slot and dest_queue with the new group BEFORE calling update_group_unused_total
    // This ensures the group exists when update_group_unused_total reloads the queue
    // Note: dest_queue.current_deposit_id was already incremented earlier (line ~3529)
    dest_slot.deposit_groups[dest_group_index] = dest_group.clone();
    dest_queue.slots[dest_slot_index] = dest_slot.clone();
    LTV_QUEUES.save(deps.storage, destination.asset.clone(), &dest_queue)?;
    
    // Update destination group LVT tracking for new deposit
    update_group_lvt_tracking_for_deposit(
        deps.storage,
        &env,
        &destination.asset,
        destination.ltv,
        destination.max_borrow_ltv,
        None, // old_deposit: None for new deposit
        Some(&new_deposit), // new_deposit: the destination deposit
    )?;

    // Update effective total for destination group (new deposit case)
    // NOTE: Must be called before reloading dest queue, as it saves the queue
    let new_dest_effective_vt = if let Some((epoch_start, epoch_end, current_time)) = epoch_info {
        calculate_unused_locked_vault_tokens(&new_deposit, &env, epoch_start, epoch_end, current_time)
    } else {
        new_deposit_locked_vt
    };

    update_group_unused_total(
        deps.storage,
        &deps.querier,
        &env,
        &config,
        &destination.asset,
        destination.ltv,
        destination.max_borrow_ltv,
        Uint128::zero(),
        new_dest_effective_vt,
        new_deposit.deposit_time,
    )?;

    // Reload dest queue after update_group_unused_total modified it
    dest_queue = LTV_QUEUES.load(deps.storage, destination.asset.clone())?;
    let dest_slot_index = dest_queue.slots.iter().position(|s| s.ltv == destination.ltv)
        .ok_or_else(|| ContractError::CustomError { val: "Dest slot not found after update".to_string() })?;
    dest_slot = dest_queue.slots[dest_slot_index].clone();
    let dest_group_index_new = dest_slot.deposit_groups.iter().position(|g| g.max_borrow_ltv == destination.max_borrow_ltv)
        .ok_or_else(|| ContractError::CustomError { val: "Dest group not found after update".to_string() })?;
    dest_group = dest_slot.deposit_groups[dest_group_index_new].clone();

    // Update destination group and slot
    dest_group.total_deposit_tokens += base_tokens_to_move;
    dest_group.total_vault_tokens += vault_tokens_to_move;
    // Note: total_locked_vault_tokens already updated above for new deposit
    dest_slot.total_deposit_tokens += base_tokens_to_move;
    dest_slot.deposit_groups[dest_group_index_new] = dest_group;
    dest_queue.slots[dest_slot_index] = dest_slot.clone();
    LTV_QUEUES.save(deps.storage, destination.asset.clone(), &dest_queue)?;
    
    // Reload groups for rate assurance (they were moved above)
    let source_group_for_rate = source_slot.deposit_groups[source_group_index].clone();
    let dest_group_for_rate = dest_slot.deposit_groups[dest_group_index].clone();
    
    // Update rate assurance for both source and destination
    update_rate_assurance(deps.storage, asset.clone(), ltv, max_borrow_ltv, &source_group_for_rate)?;
    update_rate_assurance(deps.storage, destination.asset.clone(), destination.ltv, destination.max_borrow_ltv, &dest_group_for_rate)?;
    
    // Add rate assurance callbacks if needed
    if !source_group_for_rate.total_deposit_tokens.is_zero() && !source_group_for_rate.total_vault_tokens.is_zero() {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::RateAssurance {
                asset: asset.clone(),
                max_ltv: ltv,
                max_borrow_ltv,
            })?,
            funds: vec![],
        }));
    }
    
    if !dest_group_for_rate.total_deposit_tokens.is_zero() && !dest_group_for_rate.total_vault_tokens.is_zero() {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::RateAssurance {
                asset: destination.asset.clone(),
                max_ltv: destination.ltv,
                max_borrow_ltv: destination.max_borrow_ltv,
            })?,
            funds: vec![],
        }));
    }
    
    // Update daily TVL tracker
    update_daily_tvl_tracker(deps.storage, &env, &deps.querier)?;
    
    // Update daily LTV tracker
    update_daily_ltv_tracker(deps.storage, &env, asset.clone())?;

    // Update daily insurance tracker
    update_daily_insurance_tracker(deps.storage, &env, asset.clone())?;

    Ok(Response::new()
        .add_messages(msgs)
        .add_attributes(vec![
            attr("method", "move_deposit"),
            attr("user", info.sender.to_string()),
            attr("source_asset", source_asset),
            attr("source_ltv", ltv.to_string()),
            attr("source_max_borrow_ltv", max_borrow_ltv.to_string()),
            attr("dest_asset", dest_asset),
            attr("dest_ltv", destination.ltv.to_string()),
            attr("dest_max_borrow_ltv", destination.max_borrow_ltv.to_string()),
            attr("deposit_id", deposit_id.to_string()),
            attr("vault_tokens_moved", vault_tokens_to_move.to_string()),
            attr("base_tokens_moved", base_tokens_to_move.to_string()),
            attr("claimed_revenue", claimed_revenue.to_string()),
        ]))
}

/// Update deposit settings (owner, manager, revenue_destination)
/// Only the deposit owner can update these settings
/// Claims revenue before changing manager to ensure outgoing manager gets their fee
pub fn update_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    asset: String,
    ltv: Decimal,
    max_borrow_ltv: Decimal,
    deposit_id: Uint128,
    deposit_owner: Option<String>,
    manager: Option<String>,
    revenue_destination: Option<String>,
    epoch_start_time: u64,
) -> Result<Response, ContractError> {
    // Construct deposit key using epoch_start_time
    let asset_str = asset.clone();
    let ltv_str = ltv.to_string();
    let max_borrow_ltv_str = max_borrow_ltv.to_string();
    let user_str = info.sender.to_string(); //this gates the update_manager message to only the deposit owner
    let deposit_key = make_deposit_key(&asset_str, &ltv_str, &max_borrow_ltv_str, &user_str, &deposit_id, epoch_start_time);
    
    // Load deposit
    let mut deposit = BACKING_DEPOSITS.load(deps.storage, deposit_key.clone())?;
    
    // Only the deposit owner can update the manager (not the manager themselves)
    if deposit.user != info.sender {
        return Err(ContractError::Unauthorized {});
    }
    
    // Determine new manager
    let new_manager = if let Some(manager_str) = manager {
        let manager_addr = deps.api.addr_validate(&manager_str)?;
        Some(manager_addr)
    } else {
        None
    };
    
    let mut msgs: Vec<CosmosMsg> = vec![];
    
    // Only update if manager actually changed
    if deposit.manager != new_manager {
        let config = CONFIG.load(deps.storage)?;
        
        // If there's an old manager, claim revenue and pay manager fee before changing
        if let Some(old_manager) = deposit.manager.clone() {
            // Refresh lock before claiming
            refresh_deposit_lock(&mut deposit, &env, config.lock_duration_ceiling)?;
            
            // Claim revenue for this deposit
            let mut claimed = claim_revenue_for_deposit(
                deps.storage,
                &deps.querier,
                &env,
                &config,
                &mut deposit,
                asset.clone(),
                ltv,
                max_borrow_ltv,
                None,
            )?;
            
            // Calculate and send manager fee if manager has a fee set
            if !claimed.is_zero() {
                let manager_fee_rate = MANAGER_FEE
                    .may_load(deps.storage, old_manager.clone())?
                    .unwrap_or(Decimal::zero());
                
                if !manager_fee_rate.is_zero() {
                    let manager_fee_amount = decimal_multiplication(
                        Decimal::from_ratio(claimed, Uint128::one()),
                        manager_fee_rate
                    )?.to_uint_floor();
                    
                    if !manager_fee_amount.is_zero() {
                        // Deduct manager fee from claimed
                        claimed = claimed.checked_sub(manager_fee_amount)
                            .map_err(|e| ContractError::CustomError {
                                val: format!("Overflow subtracting manager fee: {}", e)
                            })?;
                        
                        // Send manager fee to old manager
                        msgs.push(BankMsg::Send {
                            to_address: old_manager.to_string(),
                            amount: vec![Coin {
                                denom: config.cdt_denom.clone(),
                                amount: manager_fee_amount,
                            }],
                        }.into());
                    }
                }
                
                // Send remaining claimed amount to revenue_destination if set, otherwise to user (after manager fee deduction)
                if !claimed.is_zero() {
                    let recipient = deposit.revenue_destination.as_ref().unwrap_or(&deposit.user);
                    msgs.push(BankMsg::Send {
                        to_address: recipient.to_string(),
                        amount: vec![Coin {
                            denom: config.cdt_denom.clone(),
                            amount: claimed,
                        }],
                    }.into());
                }
            } else {
                // No manager fee, send all claimed to revenue_destination if set, otherwise to user
                if !claimed.is_zero() {
                    let recipient = deposit.revenue_destination.as_ref().unwrap_or(&deposit.user);
                    msgs.push(BankMsg::Send {
                        to_address: recipient.to_string(),
                        amount: vec![Coin {
                            denom: config.cdt_denom.clone(),
                            amount: claimed,
                        }],
                    }.into());
                }
            }
            
            // Save deposit with updated last_claimed timestamp
            BACKING_DEPOSITS.save(deps.storage, deposit_key.clone(), &deposit)?;
            
            // Remove old manager from MANAGED_DEPOSITS
            crate::state::remove_managed_deposit(deps.storage, &old_manager, &deposit_key)?;
        }
        
        // Add new manager to MANAGED_DEPOSITS if provided
        if let Some(ref manager_addr) = new_manager {
            crate::state::add_managed_deposit(deps.storage, manager_addr, deposit_key.clone())?;
        }
        
        // Update deposit manager
        deposit.manager = new_manager.clone();
    }
    
    // Update deposit_owner if provided
    if let Some(ref new_owner_str) = deposit_owner {
        let new_owner_addr = deps.api.addr_validate(new_owner_str)?;
        // Only allow updating owner if current owner is the sender
        if deposit.user != info.sender {
            return Err(ContractError::Unauthorized {});
        }
        deposit.user = new_owner_addr;
    }
    
    // Update revenue_destination if provided
    if let Some(ref revenue_dest_str) = revenue_destination {
        let revenue_dest_addr = deps.api.addr_validate(revenue_dest_str)?;
        deposit.revenue_destination = Some(revenue_dest_addr);
    }
    // If None, don't change existing revenue_destination
    
    BACKING_DEPOSITS.save(deps.storage, deposit_key.clone(), &deposit)?;
    
    Ok(Response::new()
        .add_messages(msgs)
        .add_attributes(vec![
            attr("method", "update_deposit"),
            attr("user", info.sender.to_string()),
            attr("asset", asset),
            attr("ltv", ltv.to_string()),
            attr("max_borrow_ltv", max_borrow_ltv.to_string()),
            attr("deposit_id", deposit_id.to_string()),
            attr("manager", new_manager.map(|m| m.to_string()).unwrap_or_else(|| "unchanged".to_string())),
            attr("deposit_owner", deposit_owner.as_ref().map(|o| o.clone()).unwrap_or_else(|| "unchanged".to_string())),
            attr("revenue_destination", revenue_destination.as_ref().map(|r| r.clone()).unwrap_or_else(|| "unchanged".to_string())),
        ]))
}

/// Toggle withdrawals for a deposit
/// Only the depositor (the address that made the deposit) can call this
pub fn toggle_withdrawals(
    deps: DepsMut,
    info: MessageInfo,
    user: String,
    asset: String,
    ltv: Decimal,
    max_borrow_ltv: Decimal,
    deposit_id: Uint128,
    enabled: bool,
    epoch_start_time: u64,
) -> Result<Response, ContractError> {
    // Validate user address
    let user_addr = deps.api.addr_validate(&user)?;
    
    // Find the actual deposit key by searching user's deposits
    let asset_str = asset.clone();
    let ltv_str = ltv.to_string();
    let max_borrow_ltv_str = max_borrow_ltv.to_string();
    let user_str = user_addr.to_string();
    let deposit_key = make_deposit_key(&asset_str, &ltv_str, &max_borrow_ltv_str, &user_str, &deposit_id, epoch_start_time);
    
    // Load deposit
    let mut deposit = BACKING_DEPOSITS.load(deps.storage, deposit_key.clone())?;
    
    // Verify that info.sender is the depositor for this deposit
    match deposit.depositor.clone() {
        Some(depositor_addr) => {
            if depositor_addr != info.sender {
                return Err(ContractError::Unauthorized {});
            }
        }
        None => {
            // If depositor is None, this is a self-deposit and withdrawals cannot be toggled
            // (they're always enabled for self-deposits)
            return Err(ContractError::CustomError {
                val: "Cannot toggle withdrawals for self-deposits".to_string(),
            });
        }
    }
    
    // Update withdrawals_enabled
    deposit.withdrawals_enabled = enabled;
    
    // Save updated deposit
    BACKING_DEPOSITS.save(deps.storage, deposit_key, &deposit)?;
    
    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "toggle_withdrawals"),
            attr("depositor", info.sender.to_string()),
            attr("user", user_addr.to_string()),
            attr("asset", asset),
            attr("ltv", ltv.to_string()),
            attr("max_borrow_ltv", max_borrow_ltv.to_string()),
            attr("deposit_id", deposit_id.to_string()),
            attr("withdrawals_enabled", enabled.to_string()),
        ]))
}

/// Update contract configuration
pub fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    cdp_contract: Option<String>,
    deposit_denom: Option<DepositDenom>,
    cdt_denom: Option<String>,
    minimum_deposit: Option<Uint128>,
    percent_to_disperse: Option<Decimal>,
    dispersal_window: Option<u64>,
    activation_window: Option<u64>,
    oracle_contract: Option<String>,
    chain_proxy_contract: Option<String>,
    emissions_voting_contract: Option<String>,
    lock_duration_ceiling: Option<u64>,
    affiliate_fee: Option<Decimal>,
    max_management_fee: Option<Decimal>,
    ltv_delta_minimum: Option<Decimal>,
    points_system_contract: Option<String>,
    revenue_distributor: Option<String>,
    auction_contract: Option<String>,
    mbrn_denom: Option<String>,
) -> Result<Response, ContractError> {
    let mut config: Config = CONFIG.load(deps.storage)?;

    // Only owner can update config
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    if let Some(owner) = owner {
        config.owner = deps.api.addr_validate(&owner)?;
    }
    if let Some(cdp_contract) = cdp_contract {
        config.cdp_contract = deps.api.addr_validate(&cdp_contract)?;
    }
    if let Some(deposit_denom) = deposit_denom {
        config.deposit_denom = deposit_denom.clone();
    }
    if let Some(cdt_denom) = cdt_denom {
        config.cdt_denom = cdt_denom;
    }
    if let Some(minimum_deposit) = minimum_deposit {
        config.minimum_deposit = minimum_deposit;
    }
    // waiting_period removed - no longer part of config
    if let Some(percent_to_disperse) = percent_to_disperse {
        config.percent_to_disperse = percent_to_disperse;
    }
    if let Some(dispersal_window) = dispersal_window {
        config.dispersal_window = dispersal_window;
    }
    if let Some(activation_window) = activation_window {
        config.activation_window = activation_window;
    }
    if let Some(oracle_contract) = oracle_contract {
        config.oracle_contract = deps.api.addr_validate(&oracle_contract)?;
    }
    if let Some(chain_proxy_contract) = chain_proxy_contract {
        config.chain_proxy_contract = deps.api.addr_validate(&chain_proxy_contract)?;
    }
    
    if let Some(emissions_voting_contract) = emissions_voting_contract {
        config.emissions_voting_contract = Some(deps.api.addr_validate(&emissions_voting_contract)?);
    }
    
    if let Some(lock_duration_ceiling) = lock_duration_ceiling {
        config.lock_duration_ceiling = lock_duration_ceiling;
    }
    
    if let Some(fee) = affiliate_fee {
        if fee > Decimal::one() {
            return Err(ContractError::CustomError {
                val: "affiliate_fee must be less than or equal to 1".to_string(),
            });
        }
        config.affiliate_fee = fee;
    }
    
    if let Some(fee) = max_management_fee {
        if fee > Decimal::one() {
            return Err(ContractError::CustomError {
                val: "max_management_fee must be less than or equal to 1".to_string(),
            });
        }
        config.max_management_fee = fee;
    }
    
    if let Some(delta) = ltv_delta_minimum {
        config.ltv_delta_minimum = delta;
    }
    
    if let Some(pts) = points_system_contract {
        config.points_system_contract = Some(deps.api.addr_validate(&pts)?);
    }
    
    if let Some(rd) = revenue_distributor {
        config.revenue_distributor = Some(deps.api.addr_validate(&rd)?);
    }
    
    if let Some(auction) = auction_contract {
        config.auction_contract = Some(deps.api.addr_validate(&auction)?);
    }
    
    if let Some(mbrn) = mbrn_denom {
        config.mbrn_denom = Some(mbrn);
    }
    
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "update_config"),
            attr("config", format!("{:?}", config)),
        ]))
}

/// Set manager fee (only callable by managers with active deposits)
pub fn set_manager_fee(
    deps: DepsMut,
    info: MessageInfo,
    fee: Decimal,
) -> Result<Response, ContractError> {
    // Check if sender has managed deposits
    let managed_deposits = MANAGED_DEPOSITS
        .may_load(deps.storage, info.sender.clone())?
        .unwrap_or_default();
    
    if managed_deposits.is_empty() {
        return Err(ContractError::CustomError {
            val: "Manager must have active deposits to set fee".to_string(),
        });
    }
    
    // Load config to check max_management_fee
    let config = CONFIG.load(deps.storage)?;
    
    // Validate fee <= max_management_fee
    if fee > config.max_management_fee {
        return Err(ContractError::CustomError {
            val: format!("Fee {} exceeds max_management_fee {}", fee, config.max_management_fee),
        });
    }
    
    // Validate fee <= 1 (100%)
    if fee > Decimal::one() {
        return Err(ContractError::CustomError {
            val: "Fee must be less than or equal to 1".to_string(),
        });
    }
    
    // Save manager fee
    MANAGER_FEE.save(deps.storage, info.sender.clone(), &fee)?;
    
    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "set_manager_fee"),
            attr("manager", info.sender.to_string()),
            attr("fee", fee.to_string()),
        ]))
}

/// Clean manager fee state for a manager with no deposits (permissionless)
pub fn clean_manager_fee(
    deps: DepsMut,
    _info: MessageInfo,
    manager: String,
) -> Result<Response, ContractError> {
    let manager_addr = deps.api.addr_validate(&manager)?;
    
    // Check if manager has any deposits
    let managed_deposits = MANAGED_DEPOSITS
        .may_load(deps.storage, manager_addr.clone())?
        .unwrap_or_default();
    
    if !managed_deposits.is_empty() {
        return Err(ContractError::CustomError {
            val: "Manager still has active deposits".to_string(),
        });
    }
    
    // Remove manager fee entry if it exists
    MANAGER_FEE.remove(deps.storage, manager_addr.clone());
    
    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "clean_manager_fee"),
            attr("manager", manager),
        ]))
}

/// Disperse revenue linearly over the active window
pub fn disperse_revenue(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    asset: String,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    // Only CDP contract can disperse revenue
    if info.sender != config.cdp_contract {
        return Err(ContractError::Unauthorized {});
    }

    // Load dispersal
    let mut dispersal = DISPERSAL.load(deps.storage, asset.clone())?;
    let current_time = env.block.time.seconds();

    // Check if dispersal is active (has active_dispersal)
    if dispersal.active_dispersal.dispersal_start == 0 {
        /////Check if this dispersal is activatible if not, error////

        //Calc block time of the start of the activation window
        let activation_window_in_seconds = config.clone().activation_window * 60;
        let start_of_activation_window = current_time.checked_sub(activation_window_in_seconds).ok_or(ContractError::CustomError { val: "Activation window subtraction underflow (should be impossible here)".to_string() })?;
        //Query the CDP's Liquidation history
        let liquidation_history: Vec<LiquidationStatResponse> = match deps.querier.query_wasm_smart::<Vec<LiquidationStatResponse>>(
            config.cdp_contract.to_string(),
            &CDP_QueryMsg::GetLiquidationStats { 
                start_after: Some(start_of_activation_window - 1u64), 
                limit: Some(100u32) 
            },
        ){
            Ok(liquidation_history) => liquidation_history,
            Err(_) => return Err(ContractError::CustomError { val: format!("Failed to query the CDP liquidation history starting after {}", start_of_activation_window) }),
        };

        //If the history isn't empty, it means there is an event that started within the activation window
        if !liquidation_history.is_empty(){
            //If any of the found events have the asset we're looking to disperse for
            if let Some(entry) = liquidation_history.into_iter()
            .find(|entry | entry.clone().collateral_assets.into_iter().map(|asset| asset.info.to_string()).collect::<Vec<String>>().contains(&asset.clone())){
                //Set the disperal start time to the time of the found liquidation history entry for the asset
                dispersal.active_dispersal.dispersal_start = entry.block_time;
            } else { return Err(ContractError::DispersalNotActive {}) }
        } else { return Err(ContractError::DispersalNotActive {}) }
    }

    // Calculate elapsed time since dispersal started
    let elapsed_seconds = current_time - dispersal.active_dispersal.dispersal_start;
    let elapsed_hours = elapsed_seconds / 3600; // Convert seconds to hours

    // Calculate how much should have been dispersed by now
    let total_hours = dispersal.dispersal_window;
    let dispersal_rate = decimal_division(
        Decimal::from_ratio(dispersal.total_to_disperse, Uint128::one()),
        Decimal::from_ratio(total_hours, Uint128::one()),
    )?;
    
    let total_should_disperse = decimal_multiplication(
        dispersal_rate,
        Decimal::from_ratio(elapsed_hours, Uint128::one()),
    )?.to_uint_floor();

    // Calculate how much to disperse this time (difference between what should be dispersed and what has been dispersed)
    let current_dispersed = dispersal.active_dispersal.amount_dispersed;
    let disperse_amount = if total_should_disperse > current_dispersed {
        total_should_disperse - current_dispersed
    } else {
        Uint128::zero()
    };

    // Ensure we don't disperse more than total
    let final_disperse_amount = disperse_amount.min(dispersal.total_to_disperse - current_dispersed);

    // Update dispersal tracking
    dispersal.active_dispersal.amount_dispersed += final_disperse_amount;
    if dispersal.active_dispersal.amount_dispersed > dispersal.total_to_disperse {
        return Err(ContractError::CustomError {
            val: "Dispersal amount exceeds total to disperse".to_string(),
        });
    } else if dispersal.active_dispersal.amount_dispersed == dispersal.total_to_disperse {
        let _ = DISPERSAL.save(deps.storage, asset.clone(), &Dispersal {
            total_to_disperse:  dispersal.clone().pending_dispersal,
            dispersal_window: config.dispersal_window,
            active_dispersal: ActiveDispersal {
                dispersal_start: 0,
                amount_dispersed: Uint128::zero(),
            },
            pending_dispersal: Uint128::zero()
        });
    } else {
        DISPERSAL.save(deps.storage, asset.clone(), &dispersal)?;
    }

    // Load queue and distribute the dispersed amount to claimable revenue
    let config = CONFIG.load(deps.storage)?;
    let queue: LTVQueue = LTV_QUEUES.load(deps.storage, asset.clone())?;
    distribute_revenue_to_users(deps.storage, &deps.querier, &env, &config, &queue, asset.clone(), final_disperse_amount)?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "disperse_revenue"),
            attr("asset", asset),
            attr("elapsed_hours", elapsed_hours.to_string()),
            attr("disperse_amount", final_disperse_amount.to_string()),
        ]))
}

//DONT DELETE.
/// Retry failed bad debt, only necessary for the Transmuter's exit failures.
/// Pull from the pending bad debt map & attempt to withdraw it from the Transmuter & send to the CDP contract as a FulfillBadDebt Msg.
/// If the withdrawal errors, we don't update the pending bad debt map.
/// If it succeeds, we update the pending bad debt map.
// pub fn retry_failed_bad_debt(
//     deps: DepsMut,
//     _env: Env,
//     _info: MessageInfo,
//     asset: String,
// ) -> Result<Response, ContractError> {
//     let mut msgs: Vec<SubMsg> = vec![];
//     let config = CONFIG.load(deps.storage)?;
//     //Load the pending bad debt
//     let pending_bad_debt = PENDING_BAD_DEBT.load(deps.storage, asset.clone())?;
//     //Save the pending asset to the BAD_DEBT_PROPAGATION
//     BAD_DEBT_PROPAGATION.save(deps.storage, &BadDebtPropagation {
//         asset: asset.clone(),
//         amount: pending_bad_debt,
//     })?;

//     //Set the denom
//     let withdraw_as_denom = config.deposit_denom.vault_info.clone().unwrap().underlying_token.clone();

//     //Attempt to withdraw the pending bad debt from the Transmuter
//     msgs.push(SubMsg::reply_on_success(CosmosMsg::Wasm(WasmMsg::Execute {
//         contract_addr: config.deposit_denom.vault_info.clone().unwrap().vault_contract.clone(),
//         msg: to_json_binary(&Transmuter_ExecuteMsg::ExitVault { 
//             recipient: None, //set to none so we don't have to conditionally add the CDP_ExecuteMsg::FulfillBadDebt Msg at the end of this fn
//             withdraw_as: Some(withdraw_as_denom.clone()),
//          })?,
//         funds: vec![
//             Coin {
//                 denom: config.deposit_denom.denom.clone(),
//                 amount: pending_bad_debt,
//             }
//         ],
//     }), TRANSMUTER_REPLY_ID));

//     //convert the pending bad debt to the underlying token
//     let pending_bad_debt_as_underlying = deps.querier.query_wasm_smart::<Uint128>(
//         config.deposit_denom.vault_info.clone().unwrap().vault_contract.clone(),
//         &Transmuter_QueryMsg::VaultTokenUnderlying { vault_token_amount: pending_bad_debt },
//     )?;

//     //Add the CDP_ExecuteMsg::FulfillBadDebt Msg 
//     msgs.push(SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
//         contract_addr: config.cdp_contract.to_string(),
//         msg: to_json_binary(&CDP_ExecuteMsg::FulfillBadDebt {})?,
//         funds: vec![
//             Coin {
//                 denom: withdraw_as_denom,
//                 amount: pending_bad_debt_as_underlying,
//             }
//         ],
//     })));

//     Ok(Response::new()
//         .add_submessages(msgs)
//         .add_attributes(vec![
//             attr("method", "retry_failed_bad_debt"),
//             attr("asset", asset),
//         ]))
// }


/// Find or create LTV slot for a given LTV (1% increments)
fn find_or_create_ltv_slot(queue: &mut LTVQueue, ltv: Decimal) -> Result<usize, ContractError> {
    // Round LTV to nearest 1% increment
    // Convert ltv to Uint128 first, then back to Decimal
    let ltv_as_uint = decimal_multiplication(
        ltv,
        Decimal::from_ratio(Uint128::new(100), Uint128::one()),
    )?.to_uint_floor();
    let rounded_ltv = Decimal::from_ratio(ltv_as_uint, Uint128::new(100));

    // Check if slot exists
    if let Some(index) = queue.slots.iter().position(|slot| slot.ltv == rounded_ltv) {
        return Ok(index);
    }

    // Create new slot
    let new_slot = MaxLTVSlot {
        ltv: rounded_ltv,
        deposit_groups: Vec::new(),
        bad_debt: Uint128::zero(),
        total_deposit_tokens: Uint128::zero(),
    };

    queue.slots.push(new_slot);
    queue.slots.sort_by(|a, b| a.ltv.cmp(&b.ltv));

    Ok(queue.slots.len() - 1)
}

/// Find or create maxBorrowLTV group within a slot
pub fn find_or_create_borrow_group(slot: &mut MaxLTVSlot, max_borrow_ltv: Decimal, create_if_not_exists: bool) -> Result<usize, ContractError> {
    // Check if group exists
    if let Some(index) = slot.deposit_groups.iter().position(|group| group.max_borrow_ltv == max_borrow_ltv) {
        return Ok(index);
    }

    if create_if_not_exists {
        // Create new group
        let new_group = MaxBorrowLTVGroup {
            max_borrow_ltv,
            total_deposit_tokens: Uint128::zero(),
            total_vault_tokens: Uint128::zero(),
            total_locked_vault_tokens: Uint128::zero(),
            total_unused_locked_vault_tokens: Uint128::zero(),
            effective_epoch_start: None,
            lvt_tracking: GroupLVTTracking {
                base_total: Uint128::zero(),
                reference_time: 0, // Will be set when first deposit is added
                base_daily_delta: Int128::zero(),
                time_cliffs: vec![],
            },
        };

        slot.deposit_groups.push(new_group);
        slot.deposit_groups.sort_by(|a, b| a.max_borrow_ltv.cmp(&b.max_borrow_ltv));

        Ok(slot.deposit_groups.len() - 1)
    } else {
        Err(ContractError::CustomError {
            val: "Group not found".to_string(),
        })
    }
}

/// Validate deposit input
fn validate_deposit_input(deps: &dyn Storage, deposit_input: BackingDepositInput) -> Result<(), ContractError> {
    match LTV_QUEUES.load(deps, deposit_input.asset.clone()) {
        Ok(queue) => {
            // Validate LTV ranges
            if deposit_input.ltv >= queue.liquidation_ltv.min && deposit_input.ltv <= queue.liquidation_ltv.max 
            && deposit_input.max_borrow_ltv >= queue.borrow_ltv.min && deposit_input.max_borrow_ltv <= queue.borrow_ltv.max {
                // Ensure max_borrow_ltv is not greater than ltv
                if deposit_input.max_borrow_ltv > deposit_input.ltv {
                    return Err(ContractError::CustomError {
                        val: format!("max_borrow_ltv ({}) cannot be greater than ltv ({})", 
                            deposit_input.max_borrow_ltv, deposit_input.ltv),
                    });
                }
                Ok(())
            } else {
                Err(ContractError::InvalidLTV {})
            }
        }
        Err(_) => Err(ContractError::InvalidAsset {}),
    }
}

/// Validate deposit owner
fn validate_deposit_owner(
    api: &dyn cosmwasm_std::Api,
    info: MessageInfo,
    deposit_owner: Option<String>,
) -> Result<Addr, ContractError> {
    match deposit_owner {
        Some(owner) => api.addr_validate(&owner).map_err(|_| ContractError::CustomError {
            val: "Invalid deposit owner address".to_string(),
        }),
        None => Ok(info.sender),
    }
}

/// Update rate assurance for a specific slot/group combination
fn update_rate_assurance(
    storage: &mut dyn Storage,
    asset: String,
    max_ltv: Decimal,
    max_borrow_ltv: Decimal,
    group: &MaxBorrowLTVGroup,
) -> Result<(), ContractError> {
    if !group.total_vault_tokens.is_zero() {
        let base_tokens_for_trillion = calculate_base_tokens(
            Uint128::new(1_000_000_000_000),
            group.total_deposit_tokens,
            group.total_vault_tokens,
        )?;
        
        RATE_ASSURANCE.save(
            storage,
            (asset, max_ltv.to_string(), max_borrow_ltv.to_string()),
            &base_tokens_for_trillion
        )?;
    }
    Ok(())
}

/// Post a deposit tracker entry for base token tracking
// pub fn post_deposit_tracker_entry(
//     deps: DepsMut,
//     _env: Env,
//     _info: MessageInfo,
//     asset: String,
//     max_ltv: Decimal,
//     max_borrow_ltv: Decimal,
// ) -> Result<Response, ContractError> {
//     // Only owner or CDP contract can post tracker entries
//     // if info.sender != config.owner && info.sender != config.cdp_contract {
//     //     return Err(ContractError::Unauthorized {});
//     // }

//     // Call the base token tracking function
//     // Load the group first
//     let queue: LTVQueue = LTV_QUEUES.load(deps.storage, asset.clone())?;
//     if let Some(slot) = queue.slots.iter().find(|s| s.ltv == max_ltv) {
//         if let Some(group) = slot.deposit_groups.iter().find(|g| g.max_borrow_ltv == max_borrow_ltv) {
//             update_rate_assurance(deps.storage, asset.clone(), max_ltv, max_borrow_ltv, group)?;
//         }
//     }

//     Ok(Response::new()
//         .add_attributes(vec![
//             attr("method", "post_deposit_tracker_entry"),
//             attr("asset", asset),
//             attr("max_ltv", max_ltv.to_string()),
//             attr("max_borrow_ltv", max_borrow_ltv.to_string()),
//         ]))
// }

/// Rate assurance
/// Ensures that the conversion rate is static for deposits & withdrawals
pub fn execute_rate_assurance(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    asset: String,
    max_ltv: Decimal,
    max_borrow_ltv: Decimal,
) -> Result<Response, ContractError> {
    //Error if not the contract calling
    if _info.sender != env.contract.address {
        return Err(ContractError::Unauthorized {});
    }

    //Load queue for the asset
    let queue: LTVQueue = LTV_QUEUES.load(deps.storage, asset.clone())?;
    
    //Find the specific slot (by max_ltv) and group (by max_borrow_ltv)
    let slot = queue.slots.iter()
        .find(|s| s.ltv == max_ltv)
        .ok_or_else(|| ContractError::CustomError {
            val: "Slot not found".to_string(),
        })?;
    
    let group = slot.deposit_groups.iter()
        .find(|g| g.max_borrow_ltv == max_borrow_ltv)
        .ok_or_else(|| ContractError::CustomError {
            val: "Group not found".to_string(),
        })?;

    // Load last rate
    let last_rate = RATE_ASSURANCE
        .may_load(deps.storage, (asset.clone(), max_ltv.to_string(), max_borrow_ltv.to_string()))?;

    if let Some(last_rate) = last_rate {
        let current_rate = calculate_base_tokens(
            Uint128::new(1_000_000_000_000),
            group.total_deposit_tokens,
            group.total_vault_tokens,
        )?;

        if !(current_rate + Uint128::one() >= last_rate) {
            return Err(ContractError::CustomError { 
                val: format!("Rate assurance failed for asset {} (max_ltv: {}, max_borrow_ltv: {}). Previous rate: {:?}, current rate: {:?}", 
                    asset, max_ltv, max_borrow_ltv, last_rate, current_rate) 
            });
        }
    }

    Ok(Response::new())
}

/// Reduce dispersals (active and pending) to fulfill bad debt
fn reduce_dispersals(
    storage: &mut dyn Storage,
    asset: String,
    mut needed_amount: Uint128,
) -> Result<Uint128, ContractError> {
    let mut dispersal = match DISPERSAL.may_load(storage, asset.clone())? {
        Some(d) => d,
        None => return Ok(Uint128::zero()),
    };

    let mut fulfilled_amount = Uint128::zero();

    // First take from active dispersal (remaining allocation not yet dispersed)
    if dispersal.active_dispersal.dispersal_start != 0 && !needed_amount.is_zero() {
        let available_in_active = dispersal.total_to_disperse
            .checked_sub(dispersal.active_dispersal.amount_dispersed)
            .unwrap_or(Uint128::zero());
        
        let take_from_active = std::cmp::min(needed_amount, available_in_active);
        
        if !take_from_active.is_zero() {
            dispersal.total_to_disperse -= take_from_active;
            fulfilled_amount += take_from_active;
            needed_amount -= take_from_active;
        }
    }

    // Then take from pending dispersal
    if !dispersal.pending_dispersal.is_zero() && !needed_amount.is_zero() {
        let take_from_pending = std::cmp::min(needed_amount, dispersal.pending_dispersal);
        
        dispersal.pending_dispersal -= take_from_pending;
        fulfilled_amount += take_from_pending;
        needed_amount -= take_from_pending;
    }

    // Save updated dispersal
    DISPERSAL.save(storage, asset, &dispersal)?;
    
    Ok(fulfilled_amount)
}

/// Reduce revenue events to fulfill bad debt, following waterfall order
// fn reduce_revenue_events(
//     storage: &mut dyn Storage,
//     queue: &LTVQueue,
//     asset: String,
//     mut needed_amount: Uint128,
// ) -> Result<Uint128, ContractError> {
//     let mut fulfilled_amount = Uint128::zero();

//     // Sort slots by LTV in descending order (highest first)
//     let mut sorted_slots: Vec<&MaxLTVSlot> = queue.slots.iter().collect();
//     sorted_slots.sort_by(|a, b| b.ltv.cmp(&a.ltv));

//     for slot in sorted_slots {
//         if needed_amount.is_zero() {
//             break;
//         }

//         // Sort groups by max_borrow_ltv in descending order
//         let mut sorted_groups: Vec<&MaxBorrowLTVGroup> = slot.deposit_groups.iter().collect();
//         sorted_groups.sort_by(|a, b| b.max_borrow_ltv.cmp(&a.max_borrow_ltv));

//         for group in sorted_groups {
//             if needed_amount.is_zero() {
//                 break;
//             }

//             let key = (asset.clone(), slot.ltv.to_string(), group.max_borrow_ltv.to_string());
//             let mut events = REVENUE_EVENTS
//                 .may_load(storage, key.clone())?
//                 .unwrap_or_else(Vec::new);

//             for event in events.iter_mut() {
//                 if needed_amount.is_zero() {
//                     break;
//                 }

//                 let take_from_event = std::cmp::min(needed_amount, event.amount_to_be_claimed);
                
//                 if !take_from_event.is_zero() {
//                     event.amount_to_be_claimed -= take_from_event;
//                     fulfilled_amount += take_from_event;
//                     needed_amount -= take_from_event;
//                 }
//             }

//             // Remove fully depleted events
//             events.retain(|e| !e.amount_to_be_claimed.is_zero());
//             REVENUE_EVENTS.save(storage, key, &events)?;
//         }
//     }

//     Ok(fulfilled_amount)
// }

/// Handle bad debt waterfall: dispersals → revenue events
/// Returns (amount_fulfilled_from_revenue, remaining_bad_debt)
fn handle_bad_debt_waterfall(
    storage: &mut dyn Storage,
    _queue: &LTVQueue,
    asset: String,
    bad_debt_amount: Uint128,
) -> Result<(Uint128, Uint128), ContractError> {
    let mut remaining = bad_debt_amount;
    let mut total_fulfilled = Uint128::zero();

    // 1. Take from dispersals first
    let from_dispersals = reduce_dispersals(storage, asset.clone(), remaining)?;
    total_fulfilled += from_dispersals;
    remaining = remaining.checked_sub(from_dispersals).unwrap_or(Uint128::zero());

    // 2. Take from revenue events
    // if !remaining.is_zero() {
    //     let from_events = reduce_revenue_events(storage, queue, asset, remaining)?;
    //     total_fulfilled += from_events;
    //     remaining = remaining.checked_sub(from_events).unwrap_or(Uint128::zero());
    // }
    // We won't do this because:
    // - This is revenue that should've been claimed already in the best case UX
    // - This increases runtime for a small benefit. We need liquidations to be gas efficient & bad debt flow is added to the end of liquidations.

    Ok((total_fulfilled, remaining))
}

/// Query asset price from oracle and convert to CDT amount
fn query_asset_price(
    querier: &QuerierWrapper,
    oracle_contract: Addr,
    asset_info: AssetInfo,
    cdt_denom: String,
    amount: Uint128,
) -> Result<Uint128, ContractError> {
 
    let cdt_info = AssetInfo::NativeToken { 
        denom: cdt_denom.clone()
    };
    let asset_infos = vec![asset_info.clone(), cdt_info.clone()];
    let price_response: Vec<PriceResponse> = querier.query_wasm_smart(
        oracle_contract.to_string(),
        &Oracle_QueryMsg::Prices {
            asset_infos,
            twap_timeframe: 0, // (No TWAPs)
            oracle_time_limit: 0,
        },
    )?;

    // Use PriceResponse helper functions to convert amount to value
    let value = price_response[0].get_value(amount)?; //Base token value

    // Value is already in CDT terms (USD), now we need to get CDT amount
    // Since CDT is also USD par, 1 USD value = 1 CDT
    // We need to query CDT's decimals to convert properly

    // Convert value back to CDT amount using CDT's price and decimals
    let cdt_amount = price_response[1].get_amount(value)?;

    Ok(cdt_amount)
}

/// Create liquidation swap message for slashed deposits
fn create_liquidation_swap_msg(
    storage: &mut dyn Storage,
    querier: &QuerierWrapper,
    env: &Env,
    config: &Config,
    asset_denom: String,
    amount: Uint128,
) -> Result<SubMsg, ContractError> {
    // Query oracle for CDT equivalent (for validation)
    let asset_info = AssetInfo::NativeToken { denom: asset_denom.clone() };
    let _cdt_equivalent = query_asset_price(
        querier,
        config.oracle_contract.clone(),
        asset_info.clone(),
        config.cdt_denom.clone(),
        amount,
    )?;

    // Query current CDT balance and save to SWAP_PROPAGATION
    let cdt_balance: Coin = querier.query_balance(
        env.contract.address.clone(),
        config.cdt_denom.clone(),
    )?;

    SWAP_PROPAGATION.save(storage, &SwapPropagation {
        cdt_balance_before: cdt_balance.amount,
    })?;

    // Create swap message via chain proxy using ExecuteSwaps
    let swap_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.chain_proxy_contract.to_string(),
        msg: to_json_binary(&OsmosisProxy_ExecuteMsg::ExecuteSwaps {
            token_out: config.cdt_denom.clone(),
            max_slippage: Decimal::percent(90), // 90% max slippage
        })?,
        funds: vec![Coin {
            denom: asset_denom,
            amount,
        }],
    });

    Ok(SubMsg::reply_on_success(swap_msg, crate::contract::LIQUIDATION_SWAP_REPLY_ID))
}

/// Update daily TVL tracker with current total deposit tokens
/// Only updates if it's been at least 1 day since last entry AND total has changed
pub fn update_daily_tvl_tracker(
    storage: &mut dyn Storage,
    env: &Env,
    querier: &QuerierWrapper,
) -> Result<(), ContractError> {
    // Load config to get deposit denomination
    let config = CONFIG.load(storage)?;
    
    // Query contract balance of deposit token
    let balance: Coin = querier.query_balance(
        env.contract.address.clone(),
        config.deposit_denom.denom,
    )?;
    
    let global_total = balance.amount;
    
    // Load existing entries
    let mut entries = DAILY_TVL_TRACKER.may_load(storage)?.unwrap_or_else(Vec::new);
    
    // Check if we should add a new entry
    let should_add = if let Some(last_entry) = entries.last() {
        // Check if at least 1 day has passed
        let time_elapsed = env.block.time.seconds().saturating_sub(last_entry.timestamp);
        time_elapsed >= ONE_DAY_SECONDS && last_entry.total_deposit_tokens != global_total
    } else {
        // First entry
        true
    };
    
    if should_add {
        let new_entry = TVLEntry {
            timestamp: env.block.time.seconds(),
            total_deposit_tokens: global_total,
        };
        
        entries.push(new_entry);
        
        // Apply limit
        if entries.len() > TVL_TRACKER_LIMIT {
            entries.drain(0..entries.len() - TVL_TRACKER_LIMIT);
        }
        
        DAILY_TVL_TRACKER.save(storage, &entries)?;
    }
    
    Ok(())
}

/// Update daily LTV tracker for an asset with current average LTV values
/// Updates if it's been at least 1 day since last entry OR if absolute change exceeds delta_minimum
pub fn update_daily_ltv_tracker(
    storage: &mut dyn Storage,
    env: &Env,
    asset: String,
) -> Result<(), ContractError> {
    let config = CONFIG.load(storage)?;
    
    // Load LTV queue for the asset
    let queue = match LTV_QUEUES.load(storage, asset.clone()) {
        Ok(q) => q,
        Err(_) => {
            // Asset doesn't have a queue yet, skip tracking
            return Ok(());
        }
    };
    
    // Calculate current average LTVs for this asset (similar to query_average_ltvs but for single asset)
    let mut total_weighted_ltv = Decimal::zero();
    let mut total_weighted_borrow_ltv = Decimal::zero();
    let mut total_weight = Decimal::zero();
    let mut total_borrow_weight = Decimal::zero();
    
    for slot in queue.slots {
        if !slot.total_deposit_tokens.is_zero() {
            // Use total_deposit_tokens as weight for LTV
            let weight = Decimal::from_ratio(slot.total_deposit_tokens.u128(), 1u128);
            total_weighted_ltv += slot.ltv * weight;
            total_weight += weight;
        }
        
        for group in slot.deposit_groups {
            if !group.total_vault_tokens.is_zero() {
                let borrow_weight = Decimal::from_ratio(group.total_vault_tokens.u128(), 1u128);
                total_weighted_borrow_ltv += group.max_borrow_ltv * borrow_weight;
                total_borrow_weight += borrow_weight;
            }
        }
    }
    
    // Calculate averages
    let current_avg_max_ltv = if total_weight.is_zero() {
        Decimal::zero()
    } else {
        total_weighted_ltv / total_weight
    };
    
    let current_avg_max_borrow_ltv = if total_borrow_weight.is_zero() {
        Decimal::zero()
    } else {
        total_weighted_borrow_ltv / total_borrow_weight
    };
    
    // Load existing entries for this asset
    let mut entries = DAILY_LTV_TRACKER.may_load(storage, asset.clone())?.unwrap_or_else(Vec::new);
    
    // Check if we should add a new entry
    let should_add = if let Some(last_entry) = entries.last() {
        // Check if at least 1 day has passed
        let time_elapsed = env.block.time.seconds().saturating_sub(last_entry.timestamp);
        let day_passed = time_elapsed >= ONE_DAY_SECONDS;
        
        // Check if absolute change exceeds delta_minimum
        let max_ltv_delta = if current_avg_max_ltv > last_entry.average_max_ltv {
            current_avg_max_ltv - last_entry.average_max_ltv
        } else {
            last_entry.average_max_ltv - current_avg_max_ltv
        };
        
        let max_borrow_ltv_delta = if current_avg_max_borrow_ltv > last_entry.average_max_borrow_ltv {
            current_avg_max_borrow_ltv - last_entry.average_max_borrow_ltv
        } else {
            last_entry.average_max_borrow_ltv - current_avg_max_borrow_ltv
        };
        
        let delta_exceeded = max_ltv_delta >= config.ltv_delta_minimum || max_borrow_ltv_delta >= config.ltv_delta_minimum;
        
        // Add entry if day passed OR delta exceeded
        day_passed || delta_exceeded
    } else {
        // First entry
        true
    };
    
    if should_add {
        let new_entry = LTVEntry {
            timestamp: env.block.time.seconds(),
            average_max_ltv: current_avg_max_ltv,
            average_max_borrow_ltv: current_avg_max_borrow_ltv,
        };
        
        entries.push(new_entry);
        
        // Apply limit
        if entries.len() > LTV_TRACKER_LIMIT {
            entries.drain(0..entries.len() - LTV_TRACKER_LIMIT);
        }
        
        DAILY_LTV_TRACKER.save(storage, asset, &entries)?;
    }
    
    Ok(())
}

/// Update daily insurance tracker for an asset with current insurance components
/// Only updates if at least 1 day since last entry AND values have changed
pub fn update_daily_insurance_tracker(
    storage: &mut dyn Storage,
    env: &Env,
    asset: String,
) -> Result<(), ContractError> {
    // 1. Calculate pending CDT from this asset's dispersal
    let mut pending_cdt = Uint128::zero();
    if let Some(dispersal) = DISPERSAL.may_load(storage, asset.clone())? {
        if dispersal.active_dispersal.dispersal_start != 0 {
            let available_in_active = dispersal.total_to_disperse
                .checked_sub(dispersal.active_dispersal.amount_dispersed)
                .unwrap_or(Uint128::zero());
            pending_cdt += available_in_active;
        }
        pending_cdt += dispersal.pending_dispersal;
    }

    // 2. Calculate total deposit tokens for this asset
    let deposit_tokens = match LTV_QUEUES.load(storage, asset.clone()) {
        Ok(queue) => queue.slots
            .iter()
            .flat_map(|slot| &slot.deposit_groups)
            .map(|group| group.total_deposit_tokens)
            .sum(),
        Err(_) => Uint128::zero(),
    };

    // 3. Load existing entries for this asset
    let mut entries = DAILY_INSURANCE_TRACKER.may_load(storage, asset.clone())?.unwrap_or_else(Vec::new);

    // 4. Check if we should add a new entry
    let should_add = if let Some(last_entry) = entries.last() {
        let time_elapsed = env.block.time.seconds().saturating_sub(last_entry.timestamp);
        time_elapsed >= ONE_DAY_SECONDS && (
            last_entry.pending_cdt != pending_cdt ||
            last_entry.deposit_tokens != deposit_tokens
        )
    } else {
        true // First entry
    };

    if should_add {
        let new_entry = InsuranceEntry {
            timestamp: env.block.time.seconds(),
            pending_cdt,
            deposit_tokens,
        };
        entries.push(new_entry);

        // Apply FIFO limit
        if entries.len() > INSURANCE_TRACKER_LIMIT {
            entries.drain(0..entries.len() - INSURANCE_TRACKER_LIMIT);
        }

        DAILY_INSURANCE_TRACKER.save(storage, asset, &entries)?;
    }

    Ok(())
}

// ================= Affiliate Helper Functions =================

/// Add affiliate from deposit
fn add_affiliate_from_deposit(
    storage: &mut dyn Storage,
    api: &dyn cosmwasm_std::Api,
    user: String,
    affiliate_address: String,
    affiliate_fee: Decimal,
    current_time: u64,
) -> Result<(), ContractError> {
    // Validate affiliate address
    let _valid_addr = api.addr_validate(&affiliate_address)?;
    
    // Load existing affiliates
    let mut affiliations = crate::state::AFFILIATES.load(storage, user.clone()).unwrap_or_else(|_| vec![]);
    
    // Check if affiliate already exists
    if affiliations.iter().any(|a| a.affiliate_address == affiliate_address) {
        // Affiliate already exists, no need to add
        return Ok(());
    }
    
    // Check limit
    if affiliations.len() >= crate::state::AFFILIATE_LIMIT {
        return Err(ContractError::CustomError {
            val: format!("Can't add more than {} affiliations", crate::state::AFFILIATE_LIMIT),
        });
    }
    
    // Add new affiliate
    affiliations.push(membrane::types::AffiliateData {
        affiliate_address: affiliate_address.clone(),
        affiliate_fee,
        time_affiliated: current_time,
        label: None,
    });
    
    // Save
    crate::state::AFFILIATES.save(storage, user, &affiliations)?;
    
    Ok(())
}

/// Set affiliate for a user
pub fn execute_set_affiliate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    user: String,
    affiliate_address: String,
    label: Option<String>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    
    // Validate affiliate address
    let _valid_addr = deps.api.addr_validate(&affiliate_address)?;
    
    // Load existing affiliates
    let mut affiliations = crate::state::AFFILIATES.load(deps.storage, user.clone()).unwrap_or_else(|_| vec![]);
    
    // Check if affiliate already exists
    if let Some(existing) = affiliations.iter().find(|a| a.affiliate_address == affiliate_address) {
        // Can only update if caller is the affiliate themselves
        if info.sender.to_string() != existing.affiliate_address {
            return Err(ContractError::Unauthorized {});
        }
        // Update existing affiliate (though fee is from config, so no change needed)
        // Just update label if provided
        for aff in affiliations.iter_mut() {
            if aff.affiliate_address == affiliate_address {
                if let Some(ref l) = label {
                    aff.label = Some(l.clone());
                }
            }
        }
    } else {
        // Check limit
        if affiliations.len() >= crate::state::AFFILIATE_LIMIT {
            return Err(ContractError::CustomError {
                val: format!("Can't add more than {} affiliations", crate::state::AFFILIATE_LIMIT),
            });
        }
        
        // Add new affiliate
        affiliations.push(membrane::types::AffiliateData {
            affiliate_address: affiliate_address.clone(),
            affiliate_fee: config.affiliate_fee,
            time_affiliated: env.block.time.seconds(),
            label,
        });
    }
    
    // Save
    crate::state::AFFILIATES.save(deps.storage, user.clone(), &affiliations)?;
    
    Ok(Response::new()
        .add_attribute("method", "set_affiliate")
        .add_attribute("user", user)
        .add_attribute("affiliate_address", affiliate_address))
}

/// Split affiliate fee % between affiliates based on time affiliated
fn split_affiliate_fee(
    affiliates: Vec<membrane::types::AffiliateData>,
    affiliate_fee: Decimal,
    current_time: u64,
) -> StdResult<Vec<Decimal>> {
    if affiliates.is_empty() {
        return Ok(vec![]);
    }
    
    // Calculate total time affiliated since last claim/repayment
    let time_since_last_claim = current_time - affiliates[0].time_affiliated;
    
    if time_since_last_claim == 0 {
        // If no time has passed, split equally
        let fee_per_affiliate = decimal_multiplication(
            affiliate_fee,
            Decimal::from_ratio(1u128, affiliates.len() as u128)
        )?;
        return Ok(vec![fee_per_affiliate; affiliates.len()]);
    }
    
    let mut affiliate_fees = vec![];
    
    // Calculate time affiliated for each affiliate
    for i in 0..affiliates.len() {
        let time_affiliated = if i == affiliates.len() - 1 {
            // Last affiliate: time from their affiliation to now
            current_time - affiliates[i].time_affiliated
        } else {
            // Other affiliates: time from their affiliation to next affiliate's affiliation
            affiliates[i + 1].time_affiliated - affiliates[i].time_affiliated
        };
        
        let ratio_affiliated = Decimal::from_ratio(time_affiliated, time_since_last_claim);
        // All affiliates use the same fee from config
        let per_affiliate_fee = decimal_multiplication(affiliate_fee, ratio_affiliated)?;
        affiliate_fees.push(per_affiliate_fee);
    }
    
    // Assert that the sum of the affiliate fees is equal or less than the affiliate fee
    let sum_of_affiliate_fees = affiliate_fees.iter().sum::<Decimal>();
    if sum_of_affiliate_fees > affiliate_fee {
        return Err(StdError::generic_err(format!("Sum of affiliate fees is greater than the affiliate fee: {} > {}", sum_of_affiliate_fees, affiliate_fee)));
    }
    
    Ok(affiliate_fees)
}

/// Updates the affiliates for a user.
/// Used during claim to reset the time affiliated & preserve historic affiliate flows (up to 10).
fn update_affiliates(
    storage: &mut dyn Storage,
    affiliates: Vec<membrane::types::AffiliateData>,
    user: String,
    current_time: u64,
) -> StdResult<()> {
    if affiliates.is_empty() {
        return Ok(());
    }
    // Keep all affiliates (up to 10), limit to last 10 if more exist
    let mut updated_affiliates = affiliates;
    if updated_affiliates.len() > 10 {
        // Remove from the front, keep last 10
        let start_idx = updated_affiliates.len() - 10;
        updated_affiliates = updated_affiliates.into_iter().skip(start_idx).collect();
    }
    
    // Reset time_affiliated: set to 0 for all except the last one, set to current_time for the last one
    let len = updated_affiliates.len();
    for (i, aff) in updated_affiliates.iter_mut().enumerate() {
        if i == len - 1 {
            // Last affiliate: set to current time
            aff.time_affiliated = current_time;
        } else {
            // All others: reset to 0 (wipe time spent)
            aff.time_affiliated = 0;
        }
    }
    
    // Update affiliates
    crate::state::AFFILIATES.save(storage, user, &updated_affiliates)?;
    
    Ok(())
}

#[cfg(test)]
mod lvt_tests;
