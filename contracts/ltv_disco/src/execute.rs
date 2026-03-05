use cosmwasm_std::{
    attr, to_json_binary, Addr, BankMsg, Coin, CosmosMsg, Decimal, DepsMut, Env, MessageInfo, Response, StdError, StdResult, Storage, SubMsg, Uint128, WasmMsg, QuerierWrapper
};
use std::collections::HashMap;
use membrane::emissions_voting::{HasAnyVotesResponse, QueryMsg as EmissionsVotingQueryMsg};
use membrane::ltv_disco::{
    BackingDeposit, BackingDepositInput, RevenueTrackingEntry, RevenueEvent, UserLifetimeRevenueEntry, Config, ExecuteMsg, TVLEntry, DepositEntry, CompoundAction, Slot, AssetQueue, UnstakeRequest
};
use membrane::math::decimal_multiplication;
use membrane::types::{Asset, DepositDenom, AssetInfo};
use membrane::stability_pool_vault::{calculate_base_tokens, calculate_vault_tokens};
use membrane::oracle::{QueryMsg as Oracle_QueryMsg, PriceResponse};
use membrane::neutron_proxy::ExecuteMsg as NeutronProxy_ExecuteMsg;

use crate::error::ContractError;
use crate::state::{CompoundPropagation, COMPOUND_PROPAGATION, REVENUE_TRACKING, RATE_ASSURANCE, REVENUE_EVENTS, USER_LIFETIME_REVENUE, BACKING_DEPOSITS, USER_DEPOSITS, CONFIG, ASSET_QUEUES, DAILY_TVL_TRACKER, DAILY_DEPOSIT_TRACKER, USER_TOTAL_DEPOSITS, MANAGER_FEE, MANAGED_DEPOSITS, UNSTAKE_REQUESTS, USER_UNSTAKE_REQUESTS};

const REVENUE_TRACKING_LIMIT: usize = 100;
const LIFETIME_REVENUE_LIMIT: usize = 100;
const TVL_TRACKER_LIMIT: usize = 100;
const DEPOSIT_TRACKER_LIMIT: usize = 100;
const ONE_DAY_SECONDS: u64 = 86400;

// =================== Key Helpers ===================

/// Build deposit key: "asset:slot:user:deposit_id"
pub fn make_deposit_key(asset: &str, slot: u8, user: &str, deposit_id: &Uint128) -> String {
    format!("{}:{}:{}:{}", asset, slot, user, deposit_id)
}

/// Build unstake request key (same format as deposit key)
fn make_unstake_key(asset: &str, slot: u8, user: &str, deposit_id: &Uint128) -> String {
    make_deposit_key(asset, slot, user, deposit_id)
}

/// Convert a Decimal LTV (e.g. 0.80) to its integer percentage (e.g. 80)
pub fn ltv_to_pct(ltv: Decimal) -> u8 {
    // Decimal::percent(80) = 0.80, so multiply by 100
    (ltv.atomics().u128() * 100 / Decimal::one().atomics().u128()) as u8
}

/// Convert a slot's max_ltv to the u8 key used in deposit keys and storage
fn slot_ltv_pct(slot: &Slot) -> u8 {
    ltv_to_pct(slot.max_ltv)
}

/// Check if a slot is active (within the queue's current min/max LTV range)
pub fn is_active_slot(slot: &Slot, queue: &AssetQueue) -> bool {
    slot.max_ltv >= queue.min_ltv && slot.max_ltv <= queue.max_ltv
}

/// Validate slot LTV percentage exists in the queue AND is within the active range.
/// Use for new deposits and moves (only allow into active slots).
/// `slot_ltv` is an LTV percentage (e.g. 80 for 80%).
fn validate_slot_ltv(queue: &AssetQueue, slot_ltv: u8) -> Result<(), ContractError> {
    let target = Decimal::percent(slot_ltv as u64);
    let slot = queue.slots.iter().find(|s| s.max_ltv == target)
        .ok_or(ContractError::SlotNotFound {})?;
    if !is_active_slot(slot, queue) {
        return Err(ContractError::InvalidSlot {});
    }
    Ok(())
}

/// Validate slot LTV percentage exists in the queue (active or inactive).
/// Use for withdrawals, unstakes, and claims — these must work on inactive slots.
fn validate_slot_exists(queue: &AssetQueue, slot_ltv: u8) -> Result<(), ContractError> {
    let target = Decimal::percent(slot_ltv as u64);
    queue.slots.iter().find(|s| s.max_ltv == target)
        .ok_or(ContractError::SlotNotFound {})?;
    Ok(())
}

/// Get mutable reference to slot from queue by LTV percentage (e.g. 80 for 80%)
fn get_slot_mut(queue: &mut AssetQueue, slot_ltv: u8) -> Result<&mut Slot, ContractError> {
    let target = Decimal::percent(slot_ltv as u64);
    queue.slots.iter_mut()
        .find(|s| s.max_ltv == target)
        .ok_or(ContractError::SlotNotFound {})
}

/// Get reference to slot from queue by LTV percentage (e.g. 80 for 80%)
fn get_slot(queue: &AssetQueue, slot_ltv: u8) -> Result<&Slot, ContractError> {
    let target = Decimal::percent(slot_ltv as u64);
    queue.slots.iter()
        .find(|s| s.max_ltv == target)
        .ok_or(ContractError::SlotNotFound {})
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

// =================== Integer Square Root ===================

/// Integer square root using Newton's method
/// Used for revenue distribution weight calculation
fn isqrt(n: Uint128) -> Uint128 {
    if n.is_zero() {
        return Uint128::zero();
    }
    let n_val = n.u128();
    if n_val == 1 {
        return Uint128::one();
    }
    let mut x = n_val;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n_val / x) / 2;
    }
    Uint128::new(x)
}

// =================== Revenue Weight Calculation ===================

/// Calculate revenue weights for all active slots using the formula:
/// weight = 1 / sqrt(deposits_above + avg_slot_size)
/// where deposits_above = cumulative deposits in higher-LTV (riskier) slots
/// that buffer this slot. Slots are sorted descending by LTV, so forward
/// iteration goes from riskiest to safest.
/// Inactive (out-of-range) slots get zero weight and no revenue.
pub fn calculate_slot_weights(queue: &AssetQueue) -> StdResult<Vec<(u8, Decimal)>> {
    // Only consider active slots for total deposits and weights
    let active_slots: Vec<&Slot> = queue.slots.iter()
        .filter(|s| is_active_slot(s, queue))
        .collect();

    let total_deposits: Uint128 = active_slots.iter().map(|s| s.total_deposit_tokens).sum();

    if total_deposits.is_zero() {
        return Ok(vec![]);
    }

    // Count non-empty active slots for average calculation
    let non_empty_count = active_slots.iter().filter(|s| !s.total_deposit_tokens.is_zero()).count() as u128;
    let slot_count = active_slots.len().max(1) as u128;
    let avg_slot_size = if non_empty_count > 0 {
        total_deposits.u128() / non_empty_count
    } else {
        total_deposits.u128() / slot_count
    };
    let avg_slot_uint = Uint128::new(avg_slot_size.max(1)); // Minimum 1 to avoid division by zero

    let mut raw_weights: Vec<(u8, Uint128)> = Vec::new();
    let mut deposits_above = Uint128::zero(); // cumulative deposits in riskier (higher-LTV) slots
    let mut total_weight_inv = Uint128::zero();

    // Scale factor for precision (10^18)
    let scale = Uint128::new(1_000_000_000_000_000_000u128);

    for slot in &active_slots {
        let ltv_pct = slot_ltv_pct(slot);

        if slot.total_vault_tokens.is_zero() {
            // Skip empty slots (no deposits = no weight)
            raw_weights.push((ltv_pct, Uint128::zero()));
            deposits_above += slot.total_deposit_tokens;
            continue;
        }

        // weight = 1 / sqrt(deposits_above + avg_slot_size)
        let denominator = deposits_above + avg_slot_uint;
        let sqrt_denom = isqrt(denominator);

        // Scaled weight = scale / sqrt_denom
        let weight = if sqrt_denom.is_zero() {
            scale // If sqrt is zero, max weight
        } else {
            scale.checked_div(sqrt_denom).unwrap_or(scale)
        };

        raw_weights.push((ltv_pct, weight));
        total_weight_inv += weight;
        deposits_above += slot.total_deposit_tokens;
    }

    if total_weight_inv.is_zero() {
        return Ok(vec![]);
    }

    // Normalize weights to sum to 1.0
    let mut weights: Vec<(u8, Decimal)> = Vec::new();
    for (ltv_pct, raw_weight) in raw_weights {
        if raw_weight.is_zero() {
            weights.push((ltv_pct, Decimal::zero()));
        } else {
            let normalized = Decimal::from_ratio(raw_weight, total_weight_inv);
            weights.push((ltv_pct, normalized));
        }
    }

    Ok(weights)
}

// =================== Queue Management ===================

/// Create a new asset queue with LTV-designated slots.
/// Slots are created at 1% intervals from max_ltv down to min_ltv (descending order).
/// For example, min_ltv=50%, max_ltv=90% creates 41 slots: 90%, 89%, ..., 50%.
pub fn create_queue(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    asset: String,
    min_ltv: Decimal,
    max_ltv: Decimal,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only owner can create queues
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    // Check if queue already exists
    if ASSET_QUEUES.has(deps.storage, asset.clone()) {
        return Err(ContractError::CustomError {
            val: "Queue already exists for this asset".to_string(),
        });
    }

    // Validate LTV bounds
    if min_ltv >= max_ltv {
        return Err(ContractError::CustomError {
            val: "min_ltv must be less than max_ltv".to_string(),
        });
    }
    if max_ltv > Decimal::one() {
        return Err(ContractError::CustomError {
            val: "max_ltv must be <= 100%".to_string(),
        });
    }
    if min_ltv.is_zero() {
        return Err(ContractError::CustomError {
            val: "min_ltv must be > 0%".to_string(),
        });
    }

    // Convert to integer percentages
    let min_pct = ltv_to_pct(min_ltv);
    let max_pct = ltv_to_pct(max_ltv);

    if min_pct >= max_pct {
        return Err(ContractError::CustomError {
            val: "min_ltv and max_ltv must differ by at least 1%".to_string(),
        });
    }

    // Create slots from max_ltv down to min_ltv in 1% steps (descending = riskiest first)
    let slots: Vec<Slot> = (min_pct..=max_pct).rev().map(|pct| Slot {
        max_ltv: Decimal::percent(pct as u64),
        total_deposit_tokens: Uint128::zero(),
        total_vault_tokens: Uint128::zero(),
        bad_debt: Uint128::zero(),
    }).collect();

    let slot_count = slots.len();

    let queue = AssetQueue {
        slots,
        current_deposit_id: Uint128::zero(),
        min_ltv,
        max_ltv,
    };

    ASSET_QUEUES.save(deps.storage, asset.clone(), &queue)?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "create_queue"),
            attr("asset", asset),
            attr("slots", slot_count.to_string()),
            attr("min_ltv", min_ltv.to_string()),
            attr("max_ltv", max_ltv.to_string()),
        ]))
}

/// Update an existing asset queue's LTV range (admin only).
/// Expansion: creates new slots for LTV values not yet in the queue.
/// Contraction: existing out-of-range slots become inactive (no deposits, no revenue).
pub fn update_queue(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    asset: String,
    min_ltv: Option<Decimal>,
    max_ltv: Option<Decimal>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let mut queue = ASSET_QUEUES.load(deps.storage, asset.clone())?;

    let new_min = min_ltv.unwrap_or(queue.min_ltv);
    let new_max = max_ltv.unwrap_or(queue.max_ltv);

    // Validate new bounds
    if new_min >= new_max {
        return Err(ContractError::CustomError {
            val: "min_ltv must be less than max_ltv".to_string(),
        });
    }
    if new_max > Decimal::one() {
        return Err(ContractError::CustomError {
            val: "max_ltv must be <= 100%".to_string(),
        });
    }
    if new_min.is_zero() {
        return Err(ContractError::CustomError {
            val: "min_ltv must be > 0%".to_string(),
        });
    }

    let new_min_pct = ltv_to_pct(new_min);
    let new_max_pct = ltv_to_pct(new_max);

    // For expansion: add any missing slot values
    for pct in new_min_pct..=new_max_pct {
        let target = Decimal::percent(pct as u64);
        if !queue.slots.iter().any(|s| s.max_ltv == target) {
            let new_slot = Slot {
                max_ltv: target,
                total_deposit_tokens: Uint128::zero(),
                total_vault_tokens: Uint128::zero(),
                bad_debt: Uint128::zero(),
            };
            // Insert in sorted descending position by max_ltv
            let pos = queue.slots.iter().position(|s| s.max_ltv < target)
                .unwrap_or(queue.slots.len());
            queue.slots.insert(pos, new_slot);
        }
    }

    queue.min_ltv = new_min;
    queue.max_ltv = new_max;

    ASSET_QUEUES.save(deps.storage, asset.clone(), &queue)?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "update_queue"),
            attr("asset", asset),
            attr("min_ltv", new_min.to_string()),
            attr("max_ltv", new_max.to_string()),
            attr("total_slots", queue.slots.len().to_string()),
        ]))
}

// =================== Deposit Logic ===================

/// Submit a backing deposit into a slot
pub fn submit_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    deposit_input: BackingDepositInput,
    deposit_owner: Option<String>,
    deposit_id: Option<Uint128>,
    manager: Option<String>,
    affiliate_address: Option<String>,
    revenue_destination: Option<String>,
) -> Result<Response, ContractError> {
    let mut msgs: Vec<CosmosMsg> = vec![];
    let config = CONFIG.load(deps.storage)?;

    let valid_owner_addr = validate_deposit_owner(deps.api, info.clone(), deposit_owner)?;

    // Validate deposit amount
    if info.funds.len() != 1 {
        return Err(ContractError::TooManyAssets { valid: config.deposit_denom.denom.clone() });
    }
    if info.funds[0].denom != deposit_input.asset {
        return Err(ContractError::CustomError {
            val: "Asset denomination mismatch".to_string(),
        });
    }
    let deposit_amount = info.funds[0].amount;
    if deposit_amount < config.minimum_deposit {
        return Err(ContractError::InvalidDepositAmount {});
    }

    // Validate queue exists and slot is active
    let mut queue = ASSET_QUEUES.load(deps.storage, deposit_input.asset.clone())
        .map_err(|_| ContractError::InvalidAsset {})?;
    validate_slot_ltv(&queue, deposit_input.slot)?;
    let slot = get_slot(&queue, deposit_input.slot)?;

    // Calculate vault tokens
    let vault_tokens = calculate_vault_tokens(
        deposit_amount,
        slot.total_deposit_tokens,
        slot.total_vault_tokens,
    )?;

    let user_str = valid_owner_addr.to_string();
    let slot_num = deposit_input.slot;

    // Determine deposit_id
    let deposit_id = if let Some(id) = deposit_id {
        // Top-up existing deposit
        let check_key = make_deposit_key(&deposit_input.asset, slot_num, &user_str, &id);
        if !BACKING_DEPOSITS.has(deps.storage, check_key.clone()) {
            return Err(ContractError::CustomError {
                val: format!("Deposit with id {} not found", id),
            });
        }
        id
    } else {
        // New deposit
        let new_id = queue.current_deposit_id;
        queue.current_deposit_id += Uint128::one();
        new_id
    };

    let deposit_key = make_deposit_key(&deposit_input.asset, slot_num, &user_str, &deposit_id);
    let is_new_deposit = !BACKING_DEPOSITS.has(deps.storage, deposit_key.clone());

    if let Some(mut existing) = BACKING_DEPOSITS.may_load(deps.storage, deposit_key.clone())? {
        // Auto-claim revenue before top-up
        let claimed = claim_revenue_for_deposit(
            deps.storage,
            &env,
            &existing,
            deposit_input.asset.clone(),
            slot_num,
            None,
        )?;
        if !claimed.is_zero() {
            let recipient = existing.revenue_destination.as_ref().unwrap_or(&valid_owner_addr);
            msgs.push(BankMsg::Send {
                to_address: recipient.to_string(),
                amount: vec![Coin { denom: config.cdt_denom.clone(), amount: claimed }],
            }.into());
        }

        // Update last_claimed after auto-claim
        existing.last_claimed = env.block.time.seconds();
        existing.vault_tokens += vault_tokens;

        // Update manager if provided
        if let Some(manager_str) = manager {
            let manager_addr = deps.api.addr_validate(&manager_str)?;
            existing.manager = Some(manager_addr.clone());
            crate::state::add_managed_deposit(deps.storage, &manager_addr, deposit_key.clone())?;
        }

        // Update revenue_destination if provided
        if let Some(revenue_dest_str) = revenue_destination {
            let revenue_dest_addr = deps.api.addr_validate(&revenue_dest_str)?;
            existing.revenue_destination = Some(revenue_dest_addr);
        }

        BACKING_DEPOSITS.save(deps.storage, deposit_key.clone(), &existing)?;
    } else {
        // New deposit
        let manager_addr = if let Some(manager_str) = manager {
            Some(deps.api.addr_validate(&manager_str)?)
        } else {
            None
        };

        let revenue_destination_addr = if let Some(revenue_dest_str) = revenue_destination {
            Some(deps.api.addr_validate(&revenue_dest_str)?)
        } else {
            None
        };

        let depositor = if valid_owner_addr != info.sender {
            Some(info.sender.clone())
        } else {
            None
        };

        let deposit = BackingDeposit {
            user: valid_owner_addr.clone(),
            vault_tokens,
            last_claimed: env.block.time.seconds(),
            start_time: env.block.time.seconds(),
            deposit_time: Some(env.block.time.seconds()),
            compound_claims: false,
            manager: manager_addr.clone(),
            depositor,
            withdrawals_enabled: true,
            revenue_destination: revenue_destination_addr,
        };

        BACKING_DEPOSITS.save(deps.storage, deposit_key.clone(), &deposit)?;

        // Add to USER_DEPOSITS index
        let mut keys = USER_DEPOSITS
            .may_load(deps.storage, (valid_owner_addr.clone(), deposit_input.asset.clone()))?
            .unwrap_or_default();
        keys.push(deposit_key.clone());
        USER_DEPOSITS.save(deps.storage, (valid_owner_addr.clone(), deposit_input.asset.clone()), &keys)?;

        // Add to MANAGED_DEPOSITS if manager
        if let Some(mgr) = manager_addr {
            crate::state::add_managed_deposit(deps.storage, &mgr, deposit_key.clone())?;
        }
    }

    // Save rate assurance BEFORE updating slot totals
    let slot_ref = get_slot(&queue, slot_num)?;
    update_rate_assurance(deps.storage, deposit_input.asset.clone(), slot_num, slot_ref)?;

    // Update slot totals
    {
        let slot_mut = get_slot_mut(&mut queue, slot_num)?;
        slot_mut.total_deposit_tokens += deposit_amount;
        slot_mut.total_vault_tokens += vault_tokens;
    }
    ASSET_QUEUES.save(deps.storage, deposit_input.asset.clone(), &queue)?;

    // Rate assurance callback
    {
        let slot_ref = get_slot(&queue, slot_num)?;
        if !slot_ref.total_deposit_tokens.is_zero() && !slot_ref.total_vault_tokens.is_zero() {
            msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: env.contract.address.to_string(),
                msg: to_json_binary(&ExecuteMsg::RateAssurance {
                    asset: deposit_input.asset.clone(),
                    slot: slot_num,
                })?,
                funds: vec![],
            }));
        }
    }

    // Handle affiliate
    if let Some(affiliate_addr) = affiliate_address {
        add_affiliate_from_deposit(
            deps.storage,
            deps.api,
            valid_owner_addr.to_string(),
            affiliate_addr,
            config.affiliate_fee,
            env.block.time.seconds(),
        )?;
    }

    // Update daily trackers
    update_daily_tvl_tracker(deps.storage, &env, &deps.querier)?;
    update_daily_deposit_tracker(deps.storage, &env, deposit_input.asset.clone())?;

    // Update user total deposits
    let user_key = valid_owner_addr.to_string();
    let current_total = USER_TOTAL_DEPOSITS
        .may_load(deps.storage, user_key.clone())?
        .unwrap_or(Uint128::zero());
    USER_TOTAL_DEPOSITS.save(deps.storage, user_key, &(current_total + deposit_amount))?;

    Ok(Response::new()
        .add_messages(msgs)
        .add_attributes(vec![
            attr("method", "submit_deposit"),
            attr("user", valid_owner_addr.to_string()),
            attr("asset", deposit_input.asset),
            attr("slot", slot_num.to_string()),
            attr("deposit_id", deposit_id.to_string()),
            attr("deposit_amount", deposit_amount.to_string()),
            attr("vault_tokens", vault_tokens.to_string()),
            attr("is_new", is_new_deposit.to_string()),
        ]))
}

// =================== Unstake Logic ===================

/// Request unstaking - starts cooldown, deposit keeps earning revenue
pub fn request_unstake(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    asset: String,
    slot: u8,
    deposit_id: Uint128,
    amount: Option<Uint128>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let queue = ASSET_QUEUES.load(deps.storage, asset.clone())?;
    validate_slot_exists(&queue, slot)?;

    let deposit_key = make_deposit_key(&asset, slot, &info.sender.to_string(), &deposit_id);
    let mut deposit = BACKING_DEPOSITS.load(deps.storage, deposit_key.clone())
        .map_err(|_| ContractError::DepositNotFound {})?;

    // Verify sender is the user
    if deposit.user != info.sender {
        return Err(ContractError::Unauthorized {});
    }

    // Check withdrawals are enabled
    if !deposit.withdrawals_enabled {
        return Err(ContractError::WithdrawalsDisabled {});
    }

    // Block if user has active emissions votes
    if let Some(ref emissions_contract) = config.emissions_voting_contract {
        let has_votes: HasAnyVotesResponse = deps.querier.query_wasm_smart(
            emissions_contract.to_string(),
            &EmissionsVotingQueryMsg::HasAnyVotes { user: info.sender.to_string() },
        )?;
        if has_votes.has_votes {
            return Err(ContractError::CustomError {
                val: "Cannot unstake while you have active emissions votes".to_string(),
            });
        }
    }

    // Check no existing unstake for this deposit
    let unstake_key = make_unstake_key(&asset, slot, &info.sender.to_string(), &deposit_id);
    if UNSTAKE_REQUESTS.has(deps.storage, unstake_key.clone()) {
        return Err(ContractError::UnstakeAlreadyPending {});
    }

    // Determine vault tokens to unstake
    let vault_tokens_to_unstake = match amount {
        Some(amt) => {
            if amt > deposit.vault_tokens {
                return Err(ContractError::InsufficientDeposits {});
            }
            amt
        }
        None => deposit.vault_tokens,
    };

    if vault_tokens_to_unstake.is_zero() {
        return Err(ContractError::InvalidDepositAmount {});
    }

    // Auto-claim revenue before unstake
    let mut msgs: Vec<CosmosMsg> = vec![];
    let claimed = claim_revenue_for_deposit(
        deps.storage,
        &env,
        &deposit,
        asset.clone(),
        slot,
        None,
    )?;
    if !claimed.is_zero() {
        let recipient = deposit.revenue_destination.as_ref().unwrap_or(&deposit.user);
        msgs.push(BankMsg::Send {
            to_address: recipient.to_string(),
            amount: vec![Coin { denom: config.cdt_denom.clone(), amount: claimed }],
        }.into());
    }
    deposit.last_claimed = env.block.time.seconds();
    BACKING_DEPOSITS.save(deps.storage, deposit_key, &deposit)?;

    // Create unstake request
    let unlock_time = env.block.time.seconds() + config.unstaking_period;
    let request = UnstakeRequest {
        user: info.sender.clone(),
        asset: asset.clone(),
        slot,
        deposit_id,
        vault_tokens: vault_tokens_to_unstake,
        request_time: env.block.time.seconds(),
        unlock_time,
    };
    UNSTAKE_REQUESTS.save(deps.storage, unstake_key.clone(), &request)?;

    // Add to user unstake requests index
    let mut user_unstake_keys = USER_UNSTAKE_REQUESTS
        .may_load(deps.storage, (info.sender.clone(), asset.clone()))?
        .unwrap_or_default();
    user_unstake_keys.push(unstake_key);
    USER_UNSTAKE_REQUESTS.save(deps.storage, (info.sender.clone(), asset.clone()), &user_unstake_keys)?;

    Ok(Response::new()
        .add_messages(msgs)
        .add_attributes(vec![
            attr("method", "request_unstake"),
            attr("user", info.sender.to_string()),
            attr("asset", asset),
            attr("slot", slot.to_string()),
            attr("deposit_id", deposit_id.to_string()),
            attr("vault_tokens", vault_tokens_to_unstake.to_string()),
            attr("unlock_time", unlock_time.to_string()),
            attr("claimed_revenue", claimed.to_string()),
        ]))
}

/// Complete unstaking after cooldown period has passed
pub fn complete_unstake(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    asset: String,
    slot: u8,
    deposit_id: Uint128,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let queue_check = ASSET_QUEUES.load(deps.storage, asset.clone())?;
    validate_slot_exists(&queue_check, slot)?;

    let unstake_key = make_unstake_key(&asset, slot, &info.sender.to_string(), &deposit_id);
    let request = UNSTAKE_REQUESTS.load(deps.storage, unstake_key.clone())
        .map_err(|_| ContractError::UnstakeNotFound {})?;

    // Verify sender
    if request.user != info.sender {
        return Err(ContractError::Unauthorized {});
    }

    // Verify cooldown passed
    if env.block.time.seconds() < request.unlock_time {
        return Err(ContractError::UnstakeNotReady {});
    }

    let deposit_key = make_deposit_key(&asset, slot, &info.sender.to_string(), &deposit_id);
    let mut deposit = BACKING_DEPOSITS.load(deps.storage, deposit_key.clone())
        .map_err(|_| ContractError::DepositNotFound {})?;

    // Auto-claim revenue before completing unstake
    let mut msgs: Vec<CosmosMsg> = vec![];
    let claimed = claim_revenue_for_deposit(
        deps.storage,
        &env,
        &deposit,
        asset.clone(),
        slot,
        None,
    )?;
    deposit.last_claimed = env.block.time.seconds();

    // The unstake request might be for less than current vault_tokens (partial unstake)
    let vault_tokens_to_withdraw = std::cmp::min(request.vault_tokens, deposit.vault_tokens);

    // Load queue and calculate base tokens
    let mut queue = ASSET_QUEUES.load(deps.storage, asset.clone())?;
    let slot_ref = get_slot(&queue, slot)?;
    let base_tokens_to_withdraw = calculate_base_tokens(
        vault_tokens_to_withdraw,
        slot_ref.total_deposit_tokens,
        slot_ref.total_vault_tokens,
    )?;

    // Save rate assurance before updating totals
    update_rate_assurance(deps.storage, asset.clone(), slot, slot_ref)?;

    let is_full_withdrawal = vault_tokens_to_withdraw >= deposit.vault_tokens;

    if is_full_withdrawal {
        // Remove deposit entirely
        BACKING_DEPOSITS.remove(deps.storage, deposit_key.clone());

        // Remove from USER_DEPOSITS index
        let mut user_keys = USER_DEPOSITS
            .may_load(deps.storage, (info.sender.clone(), asset.clone()))?
            .unwrap_or_default();
        user_keys.retain(|k| k != &deposit_key);
        if user_keys.is_empty() {
            USER_DEPOSITS.remove(deps.storage, (info.sender.clone(), asset.clone()));
        } else {
            USER_DEPOSITS.save(deps.storage, (info.sender.clone(), asset.clone()), &user_keys)?;
        }

        // Remove from MANAGED_DEPOSITS
        if let Some(ref mgr) = deposit.manager {
            crate::state::remove_managed_deposit(deps.storage, mgr, &deposit_key)?;
        }
    } else {
        // Partial withdrawal
        deposit.vault_tokens -= vault_tokens_to_withdraw;

        // Validate remaining deposit meets minimum
        let remaining_base = calculate_base_tokens(
            deposit.vault_tokens,
            slot_ref.total_deposit_tokens - base_tokens_to_withdraw,
            slot_ref.total_vault_tokens - vault_tokens_to_withdraw,
        )?;
        if remaining_base < config.minimum_deposit {
            return Err(ContractError::InvalidWithdrawal {
                minimum: config.minimum_deposit,
            });
        }

        BACKING_DEPOSITS.save(deps.storage, deposit_key, &deposit)?;
    }

    // Update slot totals
    {
        let slot_mut = get_slot_mut(&mut queue, slot)?;
        slot_mut.total_deposit_tokens -= base_tokens_to_withdraw;
        slot_mut.total_vault_tokens -= vault_tokens_to_withdraw;
    }
    ASSET_QUEUES.save(deps.storage, asset.clone(), &queue)?;

    // Clean up unstake request
    UNSTAKE_REQUESTS.remove(deps.storage, unstake_key.clone());
    let mut user_unstake_keys = USER_UNSTAKE_REQUESTS
        .may_load(deps.storage, (info.sender.clone(), asset.clone()))?
        .unwrap_or_default();
    user_unstake_keys.retain(|k| k != &unstake_key);
    if user_unstake_keys.is_empty() {
        USER_UNSTAKE_REQUESTS.remove(deps.storage, (info.sender.clone(), asset.clone()));
    } else {
        USER_UNSTAKE_REQUESTS.save(deps.storage, (info.sender.clone(), asset.clone()), &user_unstake_keys)?;
    }

    // Send base tokens back to user
    if !base_tokens_to_withdraw.is_zero() {
        msgs.push(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![Coin { denom: asset.clone(), amount: base_tokens_to_withdraw }],
        }.into());
    }

    // Send claimed revenue
    if !claimed.is_zero() {
        let recipient = deposit.revenue_destination.as_ref().unwrap_or(&deposit.user);
        msgs.push(BankMsg::Send {
            to_address: recipient.to_string(),
            amount: vec![Coin { denom: config.cdt_denom.clone(), amount: claimed }],
        }.into());
    }

    // Rate assurance callback
    {
        let slot_ref = get_slot(&queue, slot)?;
        if !slot_ref.total_deposit_tokens.is_zero() && !slot_ref.total_vault_tokens.is_zero() {
            msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: env.contract.address.to_string(),
                msg: to_json_binary(&ExecuteMsg::RateAssurance {
                    asset: asset.clone(),
                    slot,
                })?,
                funds: vec![],
            }));
        }
    }

    // Update daily trackers
    update_daily_tvl_tracker(deps.storage, &env, &deps.querier)?;
    update_daily_deposit_tracker(deps.storage, &env, asset.clone())?;

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

    Ok(Response::new()
        .add_messages(msgs)
        .add_attributes(vec![
            attr("method", "complete_unstake"),
            attr("user", info.sender.to_string()),
            attr("asset", asset),
            attr("slot", slot.to_string()),
            attr("deposit_id", deposit_id.to_string()),
            attr("vault_tokens_withdrawn", vault_tokens_to_withdraw.to_string()),
            attr("base_tokens", base_tokens_to_withdraw.to_string()),
            attr("claimed_revenue", claimed.to_string()),
        ]))
}

/// Cancel a pending unstake request
pub fn cancel_unstake(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    asset: String,
    slot: u8,
    deposit_id: Uint128,
) -> Result<Response, ContractError> {
    let queue = ASSET_QUEUES.load(deps.storage, asset.clone())?;
    validate_slot_exists(&queue, slot)?;

    let unstake_key = make_unstake_key(&asset, slot, &info.sender.to_string(), &deposit_id);
    let request = UNSTAKE_REQUESTS.load(deps.storage, unstake_key.clone())
        .map_err(|_| ContractError::UnstakeNotFound {})?;

    if request.user != info.sender {
        return Err(ContractError::Unauthorized {});
    }

    // Remove request
    UNSTAKE_REQUESTS.remove(deps.storage, unstake_key.clone());
    let mut user_unstake_keys = USER_UNSTAKE_REQUESTS
        .may_load(deps.storage, (info.sender.clone(), asset.clone()))?
        .unwrap_or_default();
    user_unstake_keys.retain(|k| k != &unstake_key);
    if user_unstake_keys.is_empty() {
        USER_UNSTAKE_REQUESTS.remove(deps.storage, (info.sender.clone(), asset.clone()));
    } else {
        USER_UNSTAKE_REQUESTS.save(deps.storage, (info.sender.clone(), asset.clone()), &user_unstake_keys)?;
    }

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "cancel_unstake"),
            attr("user", info.sender.to_string()),
            attr("asset", asset),
            attr("slot", slot.to_string()),
            attr("deposit_id", deposit_id.to_string()),
        ]))
}

// =================== Move Deposit ===================

/// Move a deposit between slots (auto-claims first)
pub fn move_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    asset: String,
    slot: u8,
    deposit_id: Uint128,
    destination: BackingDepositInput,
    amount: Option<Uint128>,
    user: Option<String>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut msgs: Vec<CosmosMsg> = vec![];

    let queue_for_validation = ASSET_QUEUES.load(deps.storage, asset.clone())?;
    validate_slot_exists(&queue_for_validation, slot)?;
    validate_slot_ltv(&queue_for_validation, destination.slot)?; // destination must be active

    // Determine user and verify authorization
    let (actual_user, deposit_key) = if let Some(user_str) = user {
        let validated_user = deps.api.addr_validate(&user_str)?;
        let key = make_deposit_key(&asset, slot, &validated_user.to_string(), &deposit_id);
        let deposit_check = BACKING_DEPOSITS.load(deps.storage, key.clone())?;
        if deposit_check.manager.as_ref() != Some(&info.sender) {
            return Err(ContractError::CustomError {
                val: "Sender is not the manager for this deposit".to_string(),
            });
        }
        (validated_user, key)
    } else {
        let key = make_deposit_key(&asset, slot, &info.sender.to_string(), &deposit_id);
        (info.sender.clone(), key)
    };

    let mut deposit = BACKING_DEPOSITS.load(deps.storage, deposit_key.clone())?;

    // Auto-claim revenue before move
    let claimed_revenue = claim_revenue_for_deposit(
        deps.storage,
        &env,
        &deposit,
        asset.clone(),
        slot,
        None,
    )?;
    if !claimed_revenue.is_zero() {
        let recipient = deposit.revenue_destination.as_ref().unwrap_or(&actual_user);
        msgs.push(BankMsg::Send {
            to_address: recipient.to_string(),
            amount: vec![Coin { denom: config.cdt_denom.clone(), amount: claimed_revenue }],
        }.into());
    }
    deposit.last_claimed = env.block.time.seconds();

    // Calculate vault tokens to move
    let vault_tokens_to_move = match amount {
        Some(amt) => std::cmp::min(amt, deposit.vault_tokens),
        None => deposit.vault_tokens,
    };

    // Load queue
    let mut queue = ASSET_QUEUES.load(deps.storage, asset.clone())?;

    // Calculate base tokens from source slot
    let source_slot = get_slot(&queue, slot)?;
    let base_tokens_to_move = calculate_base_tokens(
        vault_tokens_to_move,
        source_slot.total_deposit_tokens,
        source_slot.total_vault_tokens,
    )?;

    // Save rate assurance for source slot
    update_rate_assurance(deps.storage, asset.clone(), slot, source_slot)?;

    // Calculate vault tokens in destination slot
    let dest_slot = get_slot(&queue, destination.slot)?;
    let new_vault_tokens = calculate_vault_tokens(
        base_tokens_to_move,
        dest_slot.total_deposit_tokens,
        dest_slot.total_vault_tokens,
    )?;

    // Save rate assurance for destination slot
    update_rate_assurance(deps.storage, destination.asset.clone(), destination.slot, dest_slot)?;

    let is_full_move = vault_tokens_to_move >= deposit.vault_tokens;

    if is_full_move {
        // Remove source deposit
        BACKING_DEPOSITS.remove(deps.storage, deposit_key.clone());
        let mut user_keys = USER_DEPOSITS
            .may_load(deps.storage, (actual_user.clone(), asset.clone()))?
            .unwrap_or_default();
        user_keys.retain(|k| k != &deposit_key);
        if user_keys.is_empty() {
            USER_DEPOSITS.remove(deps.storage, (actual_user.clone(), asset.clone()));
        } else {
            USER_DEPOSITS.save(deps.storage, (actual_user.clone(), asset.clone()), &user_keys)?;
        }
        if let Some(ref mgr) = deposit.manager {
            crate::state::remove_managed_deposit(deps.storage, mgr, &deposit_key)?;
        }
    } else {
        // Partial move - reduce source deposit
        deposit.vault_tokens -= vault_tokens_to_move;
        BACKING_DEPOSITS.save(deps.storage, deposit_key.clone(), &deposit)?;
    }

    // Create destination deposit with a new ID
    let new_deposit_id = queue.current_deposit_id;
    queue.current_deposit_id += Uint128::one();

    let dest_deposit_key = make_deposit_key(&destination.asset, destination.slot, &actual_user.to_string(), &new_deposit_id);
    let dest_deposit = BackingDeposit {
        user: actual_user.clone(),
        vault_tokens: new_vault_tokens,
        last_claimed: env.block.time.seconds(),
        start_time: deposit.start_time,
        deposit_time: Some(env.block.time.seconds()),
        compound_claims: deposit.compound_claims,
        manager: deposit.manager.clone(),
        depositor: deposit.depositor.clone(),
        withdrawals_enabled: deposit.withdrawals_enabled,
        revenue_destination: deposit.revenue_destination.clone(),
    };
    BACKING_DEPOSITS.save(deps.storage, dest_deposit_key.clone(), &dest_deposit)?;

    // Add destination to USER_DEPOSITS
    let mut dest_user_keys = USER_DEPOSITS
        .may_load(deps.storage, (actual_user.clone(), destination.asset.clone()))?
        .unwrap_or_default();
    dest_user_keys.push(dest_deposit_key.clone());
    USER_DEPOSITS.save(deps.storage, (actual_user.clone(), destination.asset.clone()), &dest_user_keys)?;

    // Add to MANAGED_DEPOSITS if manager
    if let Some(ref mgr) = deposit.manager {
        crate::state::add_managed_deposit(deps.storage, mgr, dest_deposit_key)?;
    }

    // Update source slot totals
    {
        let src = get_slot_mut(&mut queue, slot)?;
        src.total_deposit_tokens -= base_tokens_to_move;
        src.total_vault_tokens -= vault_tokens_to_move;
    }

    // Update destination slot totals
    {
        let dst = get_slot_mut(&mut queue, destination.slot)?;
        dst.total_deposit_tokens += base_tokens_to_move;
        dst.total_vault_tokens += new_vault_tokens;
    }

    ASSET_QUEUES.save(deps.storage, asset.clone(), &queue)?;

    // Rate assurance callbacks
    let source_slot_ref = get_slot(&queue, slot)?;
    if !source_slot_ref.total_deposit_tokens.is_zero() && !source_slot_ref.total_vault_tokens.is_zero() {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::RateAssurance { asset: asset.clone(), slot })?,
            funds: vec![],
        }));
    }
    let dest_slot_ref = get_slot(&queue, destination.slot)?;
    if !dest_slot_ref.total_deposit_tokens.is_zero() && !dest_slot_ref.total_vault_tokens.is_zero() {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::RateAssurance { asset: asset.clone(), slot: destination.slot })?,
            funds: vec![],
        }));
    }

    Ok(Response::new()
        .add_messages(msgs)
        .add_attributes(vec![
            attr("method", "move_deposit"),
            attr("user", actual_user.to_string()),
            attr("source_slot", slot.to_string()),
            attr("dest_slot", destination.slot.to_string()),
            attr("vault_tokens_moved", vault_tokens_to_move.to_string()),
            attr("base_tokens_moved", base_tokens_to_move.to_string()),
            attr("new_vault_tokens", new_vault_tokens.to_string()),
            attr("claimed_revenue", claimed_revenue.to_string()),
        ]))
}

// =================== Update Deposit ===================

/// Update deposit settings
pub fn update_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    asset: String,
    slot: u8,
    deposit_id: Uint128,
    deposit_owner: Option<String>,
    manager: Option<String>,
    revenue_destination: Option<String>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let queue = ASSET_QUEUES.load(deps.storage, asset.clone())?;
    validate_slot_exists(&queue, slot)?;
    let mut msgs: Vec<CosmosMsg> = vec![];

    let deposit_key = make_deposit_key(&asset, slot, &info.sender.to_string(), &deposit_id);
    let mut deposit = BACKING_DEPOSITS.load(deps.storage, deposit_key.clone())
        .map_err(|_| ContractError::DepositNotFound {})?;

    // Only user or manager can update
    if deposit.user != info.sender && deposit.manager.as_ref() != Some(&info.sender) {
        return Err(ContractError::Unauthorized {});
    }

    // Update manager if provided (auto-claim and update managed deposits)
    if let Some(ref new_manager_str) = manager {
        let new_manager = deps.api.addr_validate(new_manager_str)?;

        // Auto-claim before manager change
        let claimed = claim_revenue_for_deposit(
            deps.storage,
            &env,
            &deposit,
            asset.clone(),
            slot,
            None,
        )?;
        if !claimed.is_zero() {
            let recipient = deposit.revenue_destination.as_ref().unwrap_or(&deposit.user);
            msgs.push(BankMsg::Send {
                to_address: recipient.to_string(),
                amount: vec![Coin { denom: config.cdt_denom.clone(), amount: claimed }],
            }.into());
        }
        deposit.last_claimed = env.block.time.seconds();

        // Remove old manager from MANAGED_DEPOSITS
        if let Some(ref old_manager) = deposit.manager {
            crate::state::remove_managed_deposit(deps.storage, old_manager, &deposit_key)?;
        }

        // Add new manager
        crate::state::add_managed_deposit(deps.storage, &new_manager, deposit_key.clone())?;
        deposit.manager = Some(new_manager);
    }

    // Update owner if provided
    if let Some(ref new_owner_str) = deposit_owner {
        let new_owner = deps.api.addr_validate(new_owner_str)?;
        if deposit.user != info.sender {
            return Err(ContractError::Unauthorized {});
        }
        deposit.user = new_owner;
    }

    // Update revenue_destination if provided
    if let Some(ref revenue_dest_str) = revenue_destination {
        let revenue_dest = deps.api.addr_validate(revenue_dest_str)?;
        deposit.revenue_destination = Some(revenue_dest);
    }

    BACKING_DEPOSITS.save(deps.storage, deposit_key, &deposit)?;

    Ok(Response::new()
        .add_messages(msgs)
        .add_attributes(vec![
            attr("method", "update_deposit"),
            attr("user", info.sender.to_string()),
            attr("asset", asset),
            attr("slot", slot.to_string()),
            attr("deposit_id", deposit_id.to_string()),
        ]))
}

// =================== Revenue Distribution ===================

/// Add CDT revenue to an asset's queue, distributing immediately to slots
pub fn add_revenue(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    asset: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Validate CDT sent
    if info.funds.len() != 1 || info.funds[0].denom != config.cdt_denom {
        return Err(ContractError::CustomError {
            val: "Invalid CDT token denomination".to_string(),
        });
    }

    let revenue_amount = info.funds[0].amount;
    let queue = ASSET_QUEUES.load(deps.storage, asset.clone())?;

    // Calculate slot weights
    let weights = calculate_slot_weights(&queue)?;
    if weights.is_empty() {
        return Err(ContractError::CustomError {
            val: "No deposits in queue to distribute revenue to".to_string(),
        });
    }

    let timestamp = env.block.time.seconds();
    let mut total_distributed = Uint128::zero();

    // Distribute revenue to each slot based on weight
    for (slot_index, weight) in &weights {
        if weight.is_zero() {
            continue;
        }

        let slot = get_slot(&queue, *slot_index)?;
        if slot.total_vault_tokens.is_zero() {
            continue;
        }

        // Calculate slot's share
        let slot_revenue = decimal_multiplication(
            Decimal::from_ratio(revenue_amount, Uint128::one()),
            *weight,
        )?.to_uint_floor();

        if slot_revenue.is_zero() {
            continue;
        }

        // Calculate amount_per_vt
        let amount_per_vt = Decimal::from_ratio(slot_revenue, slot.total_vault_tokens);

        // Create revenue event
        let event = RevenueEvent {
            timestamp,
            amount_per_vt,
            amount_to_be_claimed: slot_revenue,
        };

        let slot_str = slot_index.to_string();
        let mut events = REVENUE_EVENTS
            .may_load(deps.storage, (asset.clone(), slot_str.clone()))?
            .unwrap_or_default();
        events.push(event);
        REVENUE_EVENTS.save(deps.storage, (asset.clone(), slot_str.clone()), &events)?;

        // Update revenue tracking
        let mut tracking = REVENUE_TRACKING
            .may_load(deps.storage, (asset.clone(), slot_str.clone()))?
            .unwrap_or_default();
        let cumulative = tracking.last().map(|t| t.total_revenue).unwrap_or(Uint128::zero()) + slot_revenue;
        tracking.push(RevenueTrackingEntry {
            timestamp,
            total_revenue: cumulative,
        });
        if tracking.len() > REVENUE_TRACKING_LIMIT {
            tracking.drain(0..tracking.len() - REVENUE_TRACKING_LIMIT);
        }
        REVENUE_TRACKING.save(deps.storage, (asset.clone(), slot_str), &tracking)?;

        total_distributed += slot_revenue;
    }

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "add_revenue"),
            attr("asset", asset),
            attr("total_revenue", revenue_amount.to_string()),
            attr("total_distributed", total_distributed.to_string()),
        ]))
}

/// Add deposit token revenue (from auction) with per-asset distribution
pub fn add_deposit_token_revenue(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    per_asset_distribution: Vec<Asset>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only auction contract can call
    if let Some(ref auction) = config.auction_contract {
        if info.sender != *auction {
            return Err(ContractError::Unauthorized {});
        }
    } else {
        return Err(ContractError::Unauthorized {});
    }

    let mut attrs = vec![attr("method", "add_deposit_token_revenue")];

    for asset_entry in per_asset_distribution {
        let asset_denom = asset_entry.info.to_string();
        let asset_amount = asset_entry.amount;

        if asset_amount.is_zero() {
            continue;
        }

        let queue = match ASSET_QUEUES.may_load(deps.storage, asset_denom.clone())? {
            Some(q) => q,
            None => continue,
        };

        // Distribute proportionally to each active slot based on deposit tokens
        let total_deposits: Uint128 = queue.slots.iter()
            .filter(|s| is_active_slot(s, &queue))
            .map(|s| s.total_deposit_tokens)
            .sum();
        if total_deposits.is_zero() {
            continue;
        }

        let mut queue_mut = queue;
        let min_ltv = queue_mut.min_ltv;
        let max_ltv = queue_mut.max_ltv;
        for slot in &mut queue_mut.slots {
            // Skip inactive slots — no yield for out-of-range
            if slot.max_ltv < min_ltv || slot.max_ltv > max_ltv {
                continue;
            }
            if slot.total_deposit_tokens.is_zero() {
                continue;
            }

            let slot_share = asset_amount.multiply_ratio(slot.total_deposit_tokens, total_deposits);
            if !slot_share.is_zero() {
                slot.total_deposit_tokens += slot_share;
            }
        }

        ASSET_QUEUES.save(deps.storage, asset_denom.clone(), &queue_mut)?;
        attrs.push(attr(format!("distributed_{}", asset_denom), asset_amount.to_string()));
    }

    Ok(Response::new().add_attributes(attrs))
}

/// Send MBRN for bad debt auction sale (callable by auction contract only).
/// Auction pulls MBRN from disco to send to buyers.
pub fn send_mbrn_for_sale(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    amount: Uint128,
    recipient: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only auction contract can pull MBRN
    if let Some(ref auction) = config.auction_contract {
        if info.sender != *auction {
            return Err(ContractError::Unauthorized {});
        }
    } else {
        return Err(ContractError::Unauthorized {});
    }

    let mbrn_denom = config.mbrn_denom.clone()
        .ok_or(ContractError::CustomError {
            val: String::from("mbrn_denom not configured"),
        })?;

    if amount.is_zero() {
        return Err(ContractError::CustomError {
            val: String::from("Amount must be greater than zero"),
        });
    }

    let valid_recipient = deps.api.addr_validate(&recipient)?;

    // Verify we have enough MBRN
    let mbrn_balance: Coin = deps.querier.query_balance(
        env.contract.address,
        mbrn_denom.clone(),
    )?;
    if mbrn_balance.amount < amount {
        return Err(ContractError::CustomError {
            val: format!("Insufficient MBRN: have {}, need {}", mbrn_balance.amount, amount),
        });
    }

    // Send MBRN to recipient (the buyer)
    let send_msg = CosmosMsg::Bank(BankMsg::Send {
        to_address: valid_recipient.to_string(),
        amount: vec![Coin { denom: mbrn_denom, amount }],
    });

    Ok(Response::new()
        .add_message(send_msg)
        .add_attribute("method", "send_mbrn_for_sale")
        .add_attribute("amount", amount)
        .add_attribute("recipient", valid_recipient))
}

// =================== Claiming ===================

/// Claim revenue for a single deposit (internal helper)
/// Returns the amount of CDT claimed. Updates REVENUE_EVENTS in storage.
fn claim_revenue_for_deposit(
    storage: &mut dyn Storage,
    _env: &Env,
    deposit: &BackingDeposit,
    asset: String,
    slot: u8,
    limit: Option<u32>,
) -> Result<Uint128, ContractError> {
    let slot_str = slot.to_string();
    let mut events = REVENUE_EVENTS
        .may_load(storage, (asset.clone(), slot_str.clone()))?
        .unwrap_or_default();

    if events.is_empty() || deposit.vault_tokens.is_zero() {
        return Ok(Uint128::zero());
    }

    let mut total_claimed = Uint128::zero();
    let mut events_processed = 0u32;

    for event in events.iter_mut() {
        // Only claim events created after deposit's last_claimed
        if event.timestamp <= deposit.last_claimed {
            continue;
        }

        if let Some(lim) = limit {
            if events_processed >= lim {
                break;
            }
        }

        // user_share = deposit.vault_tokens * event.amount_per_vt
        let user_share = decimal_multiplication(
            Decimal::from_ratio(deposit.vault_tokens, Uint128::one()),
            event.amount_per_vt,
        )?.to_uint_floor();

        if user_share.is_zero() {
            events_processed += 1;
            continue;
        }

        // Safety check: don't drain more than available
        let actual_share = std::cmp::min(user_share, event.amount_to_be_claimed);
        event.amount_to_be_claimed -= actual_share;
        total_claimed += actual_share;
        events_processed += 1;
    }

    // Remove fully depleted events
    events.retain(|e| !e.amount_to_be_claimed.is_zero());
    REVENUE_EVENTS.save(storage, (asset, slot_str), &events)?;

    Ok(total_claimed)
}

/// Update user lifetime revenue tracking
fn update_user_lifetime_revenue(
    storage: &mut dyn Storage,
    env: &Env,
    user: &Addr,
    asset: &str,
    claimed_amount: Uint128,
) -> Result<(), ContractError> {
    let mut entries = USER_LIFETIME_REVENUE
        .may_load(storage, (user.clone(), asset.to_string()))?
        .unwrap_or_default();

    let cumulative = entries.last().map(|e| e.total_claimed).unwrap_or(Uint128::zero()) + claimed_amount;
    entries.push(UserLifetimeRevenueEntry {
        timestamp: env.block.time.seconds(),
        total_claimed: cumulative,
    });

    if entries.len() > LIFETIME_REVENUE_LIMIT {
        entries.drain(0..entries.len() - LIFETIME_REVENUE_LIMIT);
    }

    USER_LIFETIME_REVENUE.save(storage, (user.clone(), asset.to_string()), &entries)?;
    Ok(())
}

/// Claim accumulated revenue for a user (public entry point)
pub fn claim_revenue_for_user(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    user: String,
    asset: String,
    limit: Option<u32>,
    mut compound_action: Option<CompoundAction>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let user_addr = deps.api.addr_validate(&user)?;

    // If caller isn't user, disable compound_action
    if info.sender != user_addr {
        compound_action = None;
    }

    let deposit_keys = USER_DEPOSITS
        .may_load(deps.storage, (user_addr.clone(), asset.clone()))?
        .unwrap_or_default();

    if deposit_keys.is_empty() {
        return Err(ContractError::CustomError {
            val: "No deposits found for user".to_string(),
        });
    }

    let mut total_claimed = Uint128::zero();
    let mut compound_contributions: Vec<(String, Uint128)> = Vec::new();
    let mut total_to_compound = Uint128::zero();
    let mut manager_fees: HashMap<Addr, Uint128> = HashMap::new();

    for deposit_key_str in deposit_keys.clone() {
        if let Ok(mut deposit) = BACKING_DEPOSITS.load(deps.storage, deposit_key_str.clone()) {
            // Parse slot from key: "asset:slot:user:deposit_id"
            let parts: Vec<&str> = deposit_key_str.split(':').collect();
            if parts.len() != 4 {
                continue;
            }
            let slot: u8 = parts[1].parse().unwrap_or(0);
            if slot == 0 || slot > 100 {
                continue;
            }

            let mut claimed = claim_revenue_for_deposit(
                deps.storage,
                &env,
                &deposit,
                asset.clone(),
                slot,
                limit,
            )?;

            // Deduct manager fee
            if let Some(ref manager_addr) = deposit.manager {
                let manager_fee_rate = MANAGER_FEE
                    .may_load(deps.storage, manager_addr.clone())?
                    .unwrap_or(Decimal::zero());

                if !manager_fee_rate.is_zero() && !claimed.is_zero() {
                    let manager_fee_amount = decimal_multiplication(
                        Decimal::from_ratio(claimed, Uint128::one()),
                        manager_fee_rate,
                    )?.to_uint_floor();

                    if !manager_fee_amount.is_zero() {
                        claimed = claimed.checked_sub(manager_fee_amount)
                            .map_err(|e| ContractError::CustomError {
                                val: format!("Overflow subtracting manager fee: {}", e),
                            })?;
                        let current_fee = manager_fees.get(manager_addr).copied().unwrap_or(Uint128::zero());
                        manager_fees.insert(manager_addr.clone(), current_fee + manager_fee_amount);
                    }
                }
            }

            // Determine if should compound
            let should_compound = if let Some(ref action) = compound_action {
                action.compound_now || deposit.compound_claims
            } else {
                deposit.compound_claims
            };

            // Update compound_claims if set_ongoing
            if let Some(ref action) = compound_action {
                if action.set_ongoing {
                    deposit.compound_claims = true;
                }
            }

            total_claimed += claimed;

            if should_compound && !claimed.is_zero() {
                compound_contributions.push((deposit_key_str.clone(), claimed));
                total_to_compound += claimed;
            }

            // Update last_claimed
            deposit.last_claimed = env.block.time.seconds();
            BACKING_DEPOSITS.save(deps.storage, deposit_key_str, &deposit)?;
        }
    }

    // Update lifetime revenue
    if !total_claimed.is_zero() {
        update_user_lifetime_revenue(deps.storage, &env, &user_addr, &asset, total_claimed)?;
    }

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

    // Send manager fees
    let mut total_manager_fees = Uint128::zero();
    for (manager_addr, fee_amount) in manager_fees.iter() {
        if !fee_amount.is_zero() {
            total_manager_fees += *fee_amount;
            msgs.push(BankMsg::Send {
                to_address: manager_addr.to_string(),
                amount: vec![Coin { denom: config.cdt_denom.clone(), amount: *fee_amount }],
            }.into());

            // Award points to manager
            if let Some(ref points_system) = config.points_system_contract {
                let points_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: points_system.to_string(),
                    msg: to_json_binary(&membrane::points_system::ExecuteMsg::GivePointsForManagerFee {
                        manager: manager_addr.to_string(),
                        fee_amount: *fee_amount,
                    })?,
                    funds: vec![],
                });
                submsgs.push(SubMsg::reply_on_error(points_msg, 0));
            }
        }
    }

    // Handle compound swap
    if !total_to_compound.is_zero() && !compound_contributions.is_empty() {
        let deposit_token_balance_before: Coin = deps.querier.query_balance(
            env.contract.address.clone(),
            config.deposit_denom.denom.clone(),
        )?;

        COMPOUND_PROPAGATION.save(deps.storage, &CompoundPropagation {
            deposit_contributions: compound_contributions,
            deposit_token_balance_before: deposit_token_balance_before.amount,
            asset: asset.clone(),
        })?;

        let swap_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.chain_proxy_contract.to_string(),
            msg: to_json_binary(&NeutronProxy_ExecuteMsg::ExecuteSwaps {
                token_out: config.deposit_denom.denom.clone(),
                max_slippage: Decimal::percent(90),
            })?,
            funds: vec![Coin { denom: config.cdt_denom.clone(), amount: total_to_compound }],
        });
        submsgs.push(SubMsg::reply_on_success(swap_msg, crate::contract::COMPOUND_SWAP_REPLY_ID));
    }

    // Handle affiliate fees
    let mut affiliate_fees_total = Uint128::zero();
    if !total_claimed.is_zero() {
        let mut affiliates = crate::state::AFFILIATES.load(deps.storage, user.clone()).unwrap_or_default();
        affiliates.retain(|a| !a.affiliate_fee.is_zero() || a.time_affiliated != 0);

        if !affiliates.is_empty() {
            let affiliate_fees = split_affiliate_fee(affiliates.clone(), config.affiliate_fee, env.block.time.seconds())?;
            let total_affiliate_fee_ratio: Decimal = affiliate_fees.iter().sum();
            affiliate_fees_total = decimal_multiplication(
                Decimal::from_ratio(total_claimed, Uint128::one()),
                total_affiliate_fee_ratio,
            )?.to_uint_floor();

            for (i, affiliate_fee_ratio) in affiliate_fees.into_iter().enumerate() {
                let affiliate_amount = decimal_multiplication(
                    Decimal::from_ratio(affiliate_fees_total, Uint128::one()),
                    affiliate_fee_ratio,
                )?.to_uint_floor();

                if !affiliate_amount.is_zero() {
                    msgs.push(BankMsg::Send {
                        to_address: affiliates[i].affiliate_address.clone(),
                        amount: vec![Coin { denom: config.cdt_denom.clone(), amount: affiliate_amount }],
                    }.into());

                    // Award points to affiliate
                    if let Some(ref points_system) = config.points_system_contract {
                        let points_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                            contract_addr: points_system.to_string(),
                            msg: to_json_binary(&membrane::points_system::ExecuteMsg::GivePointsForAffiliateFee {
                                affiliate: affiliates[i].affiliate_address.clone(),
                                fee_amount: affiliate_amount,
                            })?,
                            funds: vec![],
                        });
                        submsgs.push(SubMsg::reply_on_error(points_msg, 0));
                    }
                }
            }

            update_affiliates(deps.storage, affiliates, user.clone(), env.block.time.seconds())?;
        }
    }

    // Calculate user amount
    let user_amount = total_claimed
        .saturating_sub(total_to_compound)
        .saturating_sub(affiliate_fees_total);

    if !user_amount.is_zero() {
        msgs.push(BankMsg::Send {
            to_address: recipient_addr.to_string(),
            amount: vec![Coin { denom: config.cdt_denom.clone(), amount: user_amount }],
        }.into());
    }

    Ok(Response::new()
        .add_messages(msgs)
        .add_submessages(submsgs)
        .add_attributes(vec![
            attr("method", "claim_revenue_for_user"),
            attr("user", user_addr.to_string()),
            attr("asset", asset),
            attr("revenue_claimed", total_claimed + total_manager_fees),
            attr("manager_fees", total_manager_fees.to_string()),
            attr("compound_amount", total_to_compound.to_string()),
            attr("affiliate_fees", affiliate_fees_total.to_string()),
            attr("user_amount", user_amount.to_string()),
        ]))
}

// =================== Bad Debt ===================

/// Add bad debt to an asset queue (CDP contract only)
/// Slashes deposits from slot 1 (riskiest) first
pub fn add_bad_debt(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    asset: String,
    amount: Uint128,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut msgs: Vec<CosmosMsg> = vec![];

    if info.sender != config.cdp_contract {
        return Err(ContractError::Unauthorized {});
    }

    let mut queue = ASSET_QUEUES.load(deps.storage, asset.clone())?;

    // Convert CDT bad debt to collateral amount using oracle
    let asset_info = AssetInfo::NativeToken { denom: asset.clone() };
    let cdt_info = AssetInfo::NativeToken { denom: config.cdt_denom.clone() };
    let price_response: Vec<PriceResponse> = deps.querier.query_wasm_smart(
        config.oracle_contract.to_string(),
        &Oracle_QueryMsg::Prices {
            asset_infos: vec![asset_info.clone(), cdt_info.clone()],
            twap_timeframe: 0,
            oracle_time_limit: 0,
        },
    )?;

    let cdt_value = price_response[1].get_value(amount)?;
    let collateral_amount_needed = price_response[0].get_amount(cdt_value)?;

    let mut remaining_to_slash = collateral_amount_needed;
    let mut total_slashed = Uint128::zero();

    // Slash from slot 1 (riskiest) first, ascending order
    for slot in &mut queue.slots {
        if remaining_to_slash.is_zero() {
            break;
        }
        if slot.total_deposit_tokens.is_zero() {
            continue;
        }

        let slash_amount = std::cmp::min(remaining_to_slash, slot.total_deposit_tokens);
        slot.total_deposit_tokens -= slash_amount;

        // Track bad debt in CDT terms
        let slashed_cdt = price_response[1].get_amount(price_response[0].get_value(slash_amount)?)?;
        slot.bad_debt += slashed_cdt;

        total_slashed += slash_amount;
        remaining_to_slash -= slash_amount;
    }

    // If deposits were slashed, start MBRN sale at auction for bad debt coverage
    if !total_slashed.is_zero() {
        // Calculate CDT value of slashed amount (this is the max CDT to collect)
        let slashed_cdt_value = price_response[1].get_amount(
            price_response[0].get_value(total_slashed)?
        )?;

        let sale_msg = create_start_mbrn_sale_msg(
            &config,
            slashed_cdt_value,
        )?;
        msgs.push(sale_msg);
    }

    ASSET_QUEUES.save(deps.storage, asset.clone(), &queue)?;

    let remaining_bad_debt = amount.saturating_sub(
        price_response[1].get_amount(price_response[0].get_value(total_slashed).unwrap_or(cosmwasm_std::Decimal::zero())).unwrap_or(Uint128::zero())
    );

    Ok(Response::new()
        .add_messages(msgs)
        .add_attributes(vec![
            attr("method", "add_bad_debt"),
            attr("asset", asset),
            attr("total_bad_debt_cdt", amount.to_string()),
            attr("slashed_collateral_amount", total_slashed.to_string()),
            attr("remaining_bad_debt_cdt", remaining_bad_debt.to_string()),
        ]))
}

// =================== Config ===================

/// Update contract configuration
pub fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    cdp_contract: Option<String>,
    deposit_denom: Option<DepositDenom>,
    cdt_denom: Option<String>,
    minimum_deposit: Option<Uint128>,
    unstaking_period: Option<u64>,
    oracle_contract: Option<String>,
    chain_proxy_contract: Option<String>,
    emissions_voting_contract: Option<String>,
    affiliate_fee: Option<Decimal>,
    max_management_fee: Option<Decimal>,
    points_system_contract: Option<String>,
    revenue_distributor: Option<String>,
    auction_contract: Option<String>,
    mbrn_denom: Option<String>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;

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
        config.deposit_denom = deposit_denom;
    }
    if let Some(cdt_denom) = cdt_denom {
        config.cdt_denom = cdt_denom;
    }
    if let Some(minimum_deposit) = minimum_deposit {
        config.minimum_deposit = minimum_deposit;
    }
    if let Some(unstaking_period) = unstaking_period {
        config.unstaking_period = unstaking_period;
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

// =================== Rate Assurance ===================

/// Update rate assurance for a slot
fn update_rate_assurance(
    storage: &mut dyn Storage,
    asset: String,
    slot: u8,
    slot_data: &Slot,
) -> Result<(), ContractError> {
    if !slot_data.total_vault_tokens.is_zero() {
        let base_tokens_for_trillion = calculate_base_tokens(
            Uint128::new(1_000_000_000_000),
            slot_data.total_deposit_tokens,
            slot_data.total_vault_tokens,
        )?;
        RATE_ASSURANCE.save(storage, (asset, slot.to_string()), &base_tokens_for_trillion)?;
    }
    Ok(())
}

/// Rate assurance execution (self-callback)
pub fn execute_rate_assurance(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    asset: String,
    slot: u8,
) -> Result<Response, ContractError> {
    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized {});
    }

    let queue = ASSET_QUEUES.load(deps.storage, asset.clone())?;
    validate_slot_exists(&queue, slot)?;
    let slot_data = get_slot(&queue, slot)?;

    let last_rate = RATE_ASSURANCE
        .may_load(deps.storage, (asset.clone(), slot.to_string()))?;

    if let Some(last_rate) = last_rate {
        let current_rate = calculate_base_tokens(
            Uint128::new(1_000_000_000_000),
            slot_data.total_deposit_tokens,
            slot_data.total_vault_tokens,
        )?;

        if !(current_rate + Uint128::one() >= last_rate) {
            return Err(ContractError::CustomError {
                val: format!("Rate assurance failed for asset {} slot {}. Previous: {:?}, current: {:?}",
                    asset, slot, last_rate, current_rate),
            });
        }
    }

    Ok(Response::new())
}

// =================== Manager Fee ===================

/// Set manager fee
pub fn set_manager_fee(
    deps: DepsMut,
    info: MessageInfo,
    fee: Decimal,
) -> Result<Response, ContractError> {
    let managed_deposits = MANAGED_DEPOSITS
        .may_load(deps.storage, info.sender.clone())?
        .unwrap_or_default();

    if managed_deposits.is_empty() {
        return Err(ContractError::CustomError {
            val: "Manager must have active deposits to set fee".to_string(),
        });
    }

    let config = CONFIG.load(deps.storage)?;
    if fee > config.max_management_fee {
        return Err(ContractError::CustomError {
            val: format!("Fee {} exceeds max_management_fee {}", fee, config.max_management_fee),
        });
    }
    if fee > Decimal::one() {
        return Err(ContractError::CustomError {
            val: "Fee must be less than or equal to 1".to_string(),
        });
    }

    MANAGER_FEE.save(deps.storage, info.sender.clone(), &fee)?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "set_manager_fee"),
            attr("manager", info.sender.to_string()),
            attr("fee", fee.to_string()),
        ]))
}

/// Clean manager fee state
pub fn clean_manager_fee(
    deps: DepsMut,
    _info: MessageInfo,
    manager: String,
) -> Result<Response, ContractError> {
    let manager_addr = deps.api.addr_validate(&manager)?;

    let managed_deposits = MANAGED_DEPOSITS
        .may_load(deps.storage, manager_addr.clone())?
        .unwrap_or_default();

    if !managed_deposits.is_empty() {
        return Err(ContractError::CustomError {
            val: "Manager still has active deposits".to_string(),
        });
    }

    MANAGER_FEE.remove(deps.storage, manager_addr);

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "clean_manager_fee"),
            attr("manager", manager),
        ]))
}

// =================== Daily Trackers ===================

/// Update daily TVL tracker
pub fn update_daily_tvl_tracker(
    storage: &mut dyn Storage,
    env: &Env,
    querier: &QuerierWrapper,
) -> Result<(), ContractError> {
    let config = CONFIG.load(storage)?;
    let balance: Coin = querier.query_balance(
        env.contract.address.clone(),
        config.deposit_denom.denom,
    )?;
    let global_total = balance.amount;

    let mut entries = DAILY_TVL_TRACKER.may_load(storage)?.unwrap_or_default();

    let should_add = if let Some(last_entry) = entries.last() {
        let time_elapsed = env.block.time.seconds().saturating_sub(last_entry.timestamp);
        time_elapsed >= ONE_DAY_SECONDS && last_entry.total_deposit_tokens != global_total
    } else {
        true
    };

    if should_add {
        entries.push(TVLEntry {
            timestamp: env.block.time.seconds(),
            total_deposit_tokens: global_total,
        });
        if entries.len() > TVL_TRACKER_LIMIT {
            entries.drain(0..entries.len() - TVL_TRACKER_LIMIT);
        }
        DAILY_TVL_TRACKER.save(storage, &entries)?;
    }

    Ok(())
}

/// Update daily deposit tracker for an asset
pub fn update_daily_deposit_tracker(
    storage: &mut dyn Storage,
    env: &Env,
    asset: String,
) -> Result<(), ContractError> {
    let deposit_tokens = match ASSET_QUEUES.load(storage, asset.clone()) {
        Ok(queue) => queue.slots.iter().map(|s| s.total_deposit_tokens).sum(),
        Err(_) => Uint128::zero(),
    };

    let mut entries = DAILY_DEPOSIT_TRACKER.may_load(storage, asset.clone())?.unwrap_or_default();

    let should_add = if let Some(last_entry) = entries.last() {
        let time_elapsed = env.block.time.seconds().saturating_sub(last_entry.timestamp);
        time_elapsed >= ONE_DAY_SECONDS && last_entry.deposit_tokens != deposit_tokens
    } else {
        true
    };

    if should_add {
        entries.push(DepositEntry {
            timestamp: env.block.time.seconds(),
            deposit_tokens,
        });
        if entries.len() > DEPOSIT_TRACKER_LIMIT {
            entries.drain(0..entries.len() - DEPOSIT_TRACKER_LIMIT);
        }
        DAILY_DEPOSIT_TRACKER.save(storage, asset, &entries)?;
    }

    Ok(())
}

// =================== MBRN Sale Helper ===================

/// Create message to start MBRN sale at auction contract for bad debt coverage.
/// No funds sent — auction pulls MBRN from disco on demand via SendMBRNForSale.
fn create_start_mbrn_sale_msg(
    config: &Config,
    bad_debt_cdt: Uint128,
) -> Result<CosmosMsg, ContractError> {
    let auction_contract = config.auction_contract.clone()
        .ok_or(ContractError::CustomError {
            val: String::from("auction_contract not configured"),
        })?;

    if bad_debt_cdt.is_zero() {
        return Err(ContractError::CustomError {
            val: String::from("bad_debt_cdt must be greater than zero"),
        });
    }

    let msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: auction_contract.to_string(),
        msg: to_json_binary(&membrane::auction::ExecuteMsg::StartMBRNSale {
            max_cdt: bad_debt_cdt,
        })?,
        funds: vec![],
    });

    Ok(msg)
}

// =================== Affiliate Functions ===================

/// Add affiliate from deposit
fn add_affiliate_from_deposit(
    storage: &mut dyn Storage,
    api: &dyn cosmwasm_std::Api,
    user: String,
    affiliate_address: String,
    affiliate_fee: Decimal,
    current_time: u64,
) -> Result<(), ContractError> {
    let _valid_addr = api.addr_validate(&affiliate_address)?;

    let mut affiliations = crate::state::AFFILIATES.load(storage, user.clone()).unwrap_or_default();

    if affiliations.iter().any(|a| a.affiliate_address == affiliate_address) {
        return Ok(());
    }

    if affiliations.len() >= crate::state::AFFILIATE_LIMIT {
        return Err(ContractError::CustomError {
            val: format!("Can't add more than {} affiliations", crate::state::AFFILIATE_LIMIT),
        });
    }

    affiliations.push(membrane::types::AffiliateData {
        affiliate_address,
        affiliate_fee,
        time_affiliated: current_time,
        label: None,
    });

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
    let _valid_addr = deps.api.addr_validate(&affiliate_address)?;

    let mut affiliations = crate::state::AFFILIATES.load(deps.storage, user.clone()).unwrap_or_default();

    if let Some(_existing) = affiliations.iter().find(|a| a.affiliate_address == affiliate_address) {
        if info.sender.to_string() != affiliate_address {
            return Err(ContractError::Unauthorized {});
        }
        for aff in affiliations.iter_mut() {
            if aff.affiliate_address == affiliate_address {
                if let Some(ref l) = label {
                    aff.label = Some(l.clone());
                }
            }
        }
    } else {
        if affiliations.len() >= crate::state::AFFILIATE_LIMIT {
            return Err(ContractError::CustomError {
                val: format!("Can't add more than {} affiliations", crate::state::AFFILIATE_LIMIT),
            });
        }
        affiliations.push(membrane::types::AffiliateData {
            affiliate_address: affiliate_address.clone(),
            affiliate_fee: config.affiliate_fee,
            time_affiliated: env.block.time.seconds(),
            label,
        });
    }

    crate::state::AFFILIATES.save(deps.storage, user.clone(), &affiliations)?;

    Ok(Response::new()
        .add_attribute("method", "set_affiliate")
        .add_attribute("user", user)
        .add_attribute("affiliate_address", affiliate_address))
}

/// Split affiliate fee between affiliates based on time affiliated
fn split_affiliate_fee(
    affiliates: Vec<membrane::types::AffiliateData>,
    affiliate_fee: Decimal,
    current_time: u64,
) -> StdResult<Vec<Decimal>> {
    if affiliates.is_empty() {
        return Ok(vec![]);
    }

    let time_since_last_claim = current_time - affiliates[0].time_affiliated;

    if time_since_last_claim == 0 {
        let fee_per_affiliate = decimal_multiplication(
            affiliate_fee,
            Decimal::from_ratio(1u128, affiliates.len() as u128),
        )?;
        return Ok(vec![fee_per_affiliate; affiliates.len()]);
    }

    let mut affiliate_fees = vec![];

    for i in 0..affiliates.len() {
        let time_affiliated = if i == affiliates.len() - 1 {
            current_time - affiliates[i].time_affiliated
        } else {
            affiliates[i + 1].time_affiliated - affiliates[i].time_affiliated
        };

        let ratio_affiliated = Decimal::from_ratio(time_affiliated, time_since_last_claim);
        let per_affiliate_fee = decimal_multiplication(affiliate_fee, ratio_affiliated)?;
        affiliate_fees.push(per_affiliate_fee);
    }

    let sum_of_affiliate_fees: Decimal = affiliate_fees.iter().sum();
    if sum_of_affiliate_fees > affiliate_fee {
        return Err(StdError::generic_err(format!(
            "Sum of affiliate fees is greater than the affiliate fee: {} > {}",
            sum_of_affiliate_fees, affiliate_fee
        )));
    }

    Ok(affiliate_fees)
}

/// Update affiliates after claim
fn update_affiliates(
    storage: &mut dyn Storage,
    affiliates: Vec<membrane::types::AffiliateData>,
    user: String,
    current_time: u64,
) -> StdResult<()> {
    if affiliates.is_empty() {
        return Ok(());
    }

    let mut updated_affiliates = affiliates;
    if updated_affiliates.len() > 10 {
        let start_idx = updated_affiliates.len() - 10;
        updated_affiliates = updated_affiliates.into_iter().skip(start_idx).collect();
    }

    let len = updated_affiliates.len();
    for (i, aff) in updated_affiliates.iter_mut().enumerate() {
        if i == len - 1 {
            aff.time_affiliated = current_time;
        } else {
            aff.time_affiliated = 0;
        }
    }

    crate::state::AFFILIATES.save(storage, user, &updated_affiliates)?;
    Ok(())
}
