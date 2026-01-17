use cosmwasm_std::{
    attr, coin, entry_point, to_json_binary, Addr, BankMsg, Binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, Response, StdError, StdResult, Storage, Timestamp, Uint128, WasmMsg,
};
use cw2::set_contract_version;

use membrane::transmuter_lockdrop::{
    Config, ExecuteMsg, InstantiateMsg, QueryMsg, LockdropState, UserDeposit,
    ConfigResponse, CurrentLockdropResponse, UserDepositsResponse, PendingLocksResponse,
    UserClaimsResponse, UserClaim, MbrnClaimIntent, MbrnIntentOption, MbrnIntentType,
    UserLockdropHistory, UserHistoryResponse,
};
use membrane::transmuter::ExecuteMsg as TransmuterExecuteMsg;
use membrane::neutron_proxy::ExecuteMsg as NeutronProxyExecuteMsg;
use membrane::math::decimal_multiplication;
use membrane::staking::ExecuteMsg as StakingExecuteMsg;
use membrane::ltv_disco::{ExecuteMsg as LtvDiscoExecuteMsg, BackingDepositInput};
use membrane::system_discounts::{QueryMsg as DiscountQueryMsg, IntentBoostsResponse};

use crate::error::ContractError;
use crate::state::{CONFIG, CURRENT_LOCKDROP, USER_DEPOSITS, PENDING_LOCKS, USER_INTENTS, LOCKDROP_HISTORY, USER_LOCKDROP_HISTORY, MAX_HISTORY_LIMIT};

const CONTRACT_NAME: &str = "membrane-transmuter-lockdrop";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");
const SECONDS_PER_DAY: u64 = 86_400;

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let owner = deps.api.addr_validate(&msg.owner)?;
    
    // Validate addresses
    let _ = deps.api.addr_validate(&msg.transmuter_contract)?;
    let _ = deps.api.addr_validate(&msg.neutron_proxy)?;
    let _ = deps.api.addr_validate(&msg.discounts_contract)?;
    
    // Validate optional addresses
    if let Some(ref staking) = msg.staking_contract {
        let _ = deps.api.addr_validate(staking)?;
    }
    if let Some(ref mirror) = msg.mars_mirror_contract {
        let _ = deps.api.addr_validate(mirror)?;
    }
    if let Some(ref ltv_disco) = msg.ltv_disco_contract {
        let _ = deps.api.addr_validate(ltv_disco)?;
    }
    if let Some(ref emissions_voting) = msg.emissions_voting_contract {
        let _ = deps.api.addr_validate(emissions_voting)?;
    }
    
    // Validate periods
    if msg.deposit_period_days == 0 {
        return Err(ContractError::Validation(
            "deposit_period_days must be greater than zero".into(),
        ));
    }
    if msg.withdrawal_period_days == 0 {
        return Err(ContractError::Validation(
            "withdrawal_period_days must be greater than zero".into(),
        ));
    }
    
    // Validate minimum deposit
    if msg.minimum_deposit.is_zero() {
        return Err(ContractError::Validation(
            "minimum_deposit must be greater than zero".into(),
        ));
    }
    
    // Validate maximum_boost
    if msg.maximum_boost < Decimal::zero() {
        return Err(ContractError::Validation(
            "maximum_boost must be greater than or equal to zero".into(),
        ));
    }
    
    // Validate minimum_lock_days
    if msg.minimum_lock_days == 0 {
        return Err(ContractError::Validation(
            "minimum_lock_days must be greater than zero".into(),
        ));
    }
    
    let config = Config {
        owner,
        transmuter_contract: msg.transmuter_contract,
        neutron_proxy: msg.neutron_proxy,
        lockdrop_incentive_size: msg.lockdrop_incentive_size,
        deposit_period_days: msg.deposit_period_days,
        withdrawal_period_days: msg.withdrawal_period_days,
        deposit_token: msg.deposit_token,
        minimum_deposit: msg.minimum_deposit,
        mbrn_denom: msg.mbrn_denom,
        staking_contract: msg.staking_contract,
        mars_mirror_contract: msg.mars_mirror_contract,
        ltv_disco_contract: msg.ltv_disco_contract,
        discounts_contract: msg.discounts_contract,
        maximum_boost: msg.maximum_boost,
        minimum_lock_days: msg.minimum_lock_days,
        emissions_voting_contract: msg.emissions_voting_contract,
    };
    
    CONFIG.save(deps.storage, &config)?;
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    
    Ok(Response::new()
        .add_attributes(vec![
            attr("action", "instantiate"),
            attr("owner", config.owner.as_str()),
        ]))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::StartLockdrop { deposit_period_days, withdrawal_period_days } => {
            execute_start_lockdrop(deps, env, info, deposit_period_days, withdrawal_period_days)
        }
        ExecuteMsg::Deposit { lock_days, intents } => execute_deposit(deps, env, info, lock_days, intents),
        ExecuteMsg::Withdraw { amount, lock_days } => execute_withdraw(deps, env, info, amount, lock_days),
        ExecuteMsg::EditLock { lock_days, new_lock_days } => execute_edit_lock(deps, env, info, lock_days, new_lock_days),
        ExecuteMsg::CompleteLocks { limit } => execute_complete_locks(deps, env, limit),
        ExecuteMsg::Claim { users, mbrn_intent } => execute_claim(deps, env, users, mbrn_intent),
        ExecuteMsg::UpdateConfig {
            owner,
            transmuter_contract,
            neutron_proxy,
            lockdrop_incentive_size,
            deposit_period_days,
            withdrawal_period_days,
            deposit_token,
            minimum_deposit,
            mbrn_denom,
            staking_contract,
            mars_mirror_contract,
            ltv_disco_contract,
            discounts_contract,
            maximum_boost,
            minimum_lock_days,
            emissions_voting_contract,
        } => execute_update_config(
            deps,
            info,
            owner,
            transmuter_contract,
            neutron_proxy,
            lockdrop_incentive_size,
            deposit_period_days,
            withdrawal_period_days,
            deposit_token,
            minimum_deposit,
            mbrn_denom,
            staking_contract,
            mars_mirror_contract,
            ltv_disco_contract,
            discounts_contract,
            maximum_boost,
            minimum_lock_days,
            emissions_voting_contract,
        ),
        ExecuteMsg::ReceiveVotingResult {
            label,
            result_uint128,
            result_decimal,
        } => execute_receive_voting_result(deps, info, label, result_uint128, result_decimal),
    }
}

fn execute_start_lockdrop(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    deposit_period_days: Option<u64>,
    withdrawal_period_days: Option<u64>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    ensure_owner(&config, &info.sender)?;
    
    // Check if all users have claimed (USER_DEPOSITS should be empty)
    let user_deposits_count: usize = USER_DEPOSITS
        .keys(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .count();
    
    if user_deposits_count > 0 {
        return Err(ContractError::Validation(
            "Not all users have claimed from the previous lockdrop".into(),
        ));
    }
    
    // Check if there's an active lockdrop and save to history
    if let Ok(previous_lockdrop) = CURRENT_LOCKDROP.load(deps.storage) {
        // Save previous lockdrop to history
        save_lockdrop_to_history(deps.storage, previous_lockdrop)?;
    }
    
    let deposit_period = deposit_period_days.unwrap_or(config.deposit_period_days);
    let withdrawal_period = withdrawal_period_days.unwrap_or(config.withdrawal_period_days);
    
    if deposit_period == 0 {
        return Err(ContractError::Validation(
            "deposit_period_days must be greater than zero".into(),
        ));
    }
    if withdrawal_period == 0 {
        return Err(ContractError::Validation(
            "withdrawal_period_days must be greater than zero".into(),
        ));
    }
    
    let start_time = env.block.time.seconds();
    let deposit_end = start_time + (deposit_period * SECONDS_PER_DAY);
    let withdrawal_end = deposit_end + (withdrawal_period * SECONDS_PER_DAY);
    
    let lockdrop = LockdropState {
        start_time,
        deposit_end,
        withdrawal_end,
        total_deposit_points: Some(Uint128::zero()),
    };
    
    CURRENT_LOCKDROP.save(deps.storage, &lockdrop)?;
    
    
    // Note: We keep PENDING_LOCKS from previous lockdrop until they're processed
    
    Ok(Response::new()
        .add_attributes(vec![
            attr("action", "start_lockdrop"),
            attr("start_time", start_time.to_string()),
            attr("deposit_end", deposit_end.to_string()),
            attr("withdrawal_end", withdrawal_end.to_string()),
        ]))
}

fn execute_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    lock_days: u64,
    intents: Option<Vec<MbrnIntentOption>>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let lockdrop = CURRENT_LOCKDROP.load(deps.storage)?;
    
    // Check if in deposit period
    let current_time = env.block.time.seconds();
    if current_time > lockdrop.deposit_end {
        return Err(ContractError::DepositPeriodEnded {});
    }
    
    // Get deposit amount from funds
    let deposit_amount = info.funds
        .iter()
        .find(|c| c.denom == config.deposit_token)
        .map(|c| c.amount)
        .ok_or_else(|| ContractError::InvalidFunds {
            reason: format!("Expected {} deposit", config.deposit_token),
        })?;
    
    // Validate minimum deposit
    if deposit_amount < config.minimum_deposit {
        return Err(ContractError::Validation(
            format!("Deposit amount {} is below minimum {}", deposit_amount, config.minimum_deposit),
        ));
    }
    
    // Query transmuter for lock_ceiling
    let transmuter_config: membrane::transmuter::Config = deps.querier.query_wasm_smart(
        config.transmuter_contract.clone(),
        &membrane::transmuter::QueryMsg::Config {},
    )?;
    
    // Validate lock_days
    if lock_days == 0 {
        return Err(ContractError::Validation(
            "lock_days must be greater than zero".into(),
        ));
    }
    if lock_days < config.minimum_lock_days {
        return Err(ContractError::Validation(
            format!("lock_days ({}) is below minimum ({})", lock_days, config.minimum_lock_days),
        ));
    }
    if lock_days > transmuter_config.lock_ceiling {
        return Err(ContractError::Validation(
            format!("lock_days ({}) exceeds transmuter lock_ceiling ({})", lock_days, transmuter_config.lock_ceiling),
        ));
    }
    
    // Validate intents if provided
    if let Some(ref intents) = intents {
        // Validate ratios sum to ~1.0
        let total_ratio: Decimal = intents.iter()
            .map(|i| i.ratio)
            .fold(Decimal::zero(), |acc, x| acc + x);
        let diff = if total_ratio > Decimal::one() {
            total_ratio - Decimal::one()
        } else {
            Decimal::one() - total_ratio
        };
        if diff > Decimal::percent(1) {
            return Err(ContractError::Validation(
                "Intent ratios must sum to approximately 1.0".into(),
            ));
        }
        // Validate each ratio is between 0 and 1
        for intent in intents {
            if intent.ratio > Decimal::one() || intent.ratio < Decimal::zero() {
                return Err(ContractError::Validation(
                    "Intent ratios must be between 0 and 1".into(),
                ));
            }
        }
    }
    
    // Load or create user deposits
    let mut deposits = USER_DEPOSITS
        .may_load(deps.storage, info.sender.to_string())?
        .unwrap_or_default();
    
    // Check if user already has a deposit with this lock_days
    if let Some(existing) = deposits.iter_mut().find(|d| d.intended_lock_days == lock_days) {
        existing.amount = existing.amount.checked_add(deposit_amount)
            .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
        // Update intents if provided (replace existing)
        if intents.is_some() {
            existing.intents = intents.clone();
        }
    } else {
        deposits.push(UserDeposit {
            amount: deposit_amount,
            intended_lock_days: lock_days,
            deposit_time: current_time,
            intents: intents.clone(),
        });
    }
    
    USER_DEPOSITS.save(deps.storage, info.sender.to_string(), &deposits)?;
    
    // Update PENDING_LOCKS to keep in sync
    PENDING_LOCKS.save(deps.storage, info.sender.to_string(), &deposits)?;
    
    // Update total_deposit_points
    let mut lockdrop = CURRENT_LOCKDROP.load(deps.storage)?;
    let transmuter_config: membrane::transmuter::Config = deps.querier.query_wasm_smart(
        config.transmuter_contract.clone(),
        &membrane::transmuter::QueryMsg::Config {},
    )?;
    
    let deposit_points = calculate_deposit_points(
        deposit_amount,
        lock_days,
        transmuter_config.lock_ceiling,
        config.maximum_boost,
    )?;
    
    update_total_deposit_points(deps.storage, &config, &mut lockdrop, deposit_points, false)?;
    
    Ok(Response::new()
        .add_attributes(vec![
            attr("action", "deposit"),
            attr("user", info.sender.to_string()),
            attr("amount", deposit_amount.to_string()),
            attr("lock_days", lock_days.to_string()),
        ]))
}

fn execute_withdraw(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    amount: Uint128,
    lock_days: u64,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let lockdrop = CURRENT_LOCKDROP.load(deps.storage)?;
    
    // Check if in withdrawal period
    let current_time = env.block.time.seconds();
    if current_time < lockdrop.deposit_end || current_time > lockdrop.withdrawal_end {
        return Err(ContractError::NotInWithdrawalPeriod {});
    }
    
    
    // Load user deposits
    let mut deposits = USER_DEPOSITS
        .may_load(deps.storage, info.sender.to_string())?
        .ok_or_else(|| ContractError::InvalidFunds {
            reason: "User has no deposits".into(),
        })?;
    
    // Find deposit with matching lock_days
    let deposit = deposits.iter_mut()
        .find(|d| d.intended_lock_days == lock_days)
        .ok_or_else(|| ContractError::InvalidFunds {
            reason: format!("No deposit found with lock_days {}", lock_days),
        })?;
    
    // Validate withdrawal amount
    if amount > deposit.amount {
        return Err(ContractError::InvalidFunds {
            reason: format!("Withdrawal amount {} exceeds deposit amount {}", amount, deposit.amount),
        });
    }
    
    // Update deposit
    deposit.amount = deposit.amount.checked_sub(amount)
        .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;


    // Validate minimum deposit (for remaining balance)
    if deposit.amount < config.minimum_deposit && deposit.amount > Uint128::zero() {
        return Err(ContractError::Validation(
            format!("Withdrawal would leave balance below minimum {}", config.minimum_deposit),
        ));
    }
    
    // Remove deposit if zero
    if deposit.amount.is_zero() {
        deposits.retain(|d| d.intended_lock_days != lock_days);
    }
    
    // Save deposits (or remove if empty)
    if deposits.is_empty() {
        USER_DEPOSITS.remove(deps.storage, info.sender.to_string());
        PENDING_LOCKS.remove(deps.storage, info.sender.to_string());
    } else {
        USER_DEPOSITS.save(deps.storage, info.sender.to_string(), &deposits)?;
        // Update PENDING_LOCKS to keep in sync
        PENDING_LOCKS.save(deps.storage, info.sender.to_string(), &deposits)?;
    }
    
    // Update total_deposit_points
    let mut lockdrop = CURRENT_LOCKDROP.load(deps.storage)?;
    let transmuter_config: membrane::transmuter::Config = deps.querier.query_wasm_smart(
        config.transmuter_contract.clone(),
        &membrane::transmuter::QueryMsg::Config {},
    )?;
    
    let withdrawn_points = calculate_deposit_points(
        amount,
        lock_days,
        transmuter_config.lock_ceiling,
        config.maximum_boost,
    )?;
    
    update_total_deposit_points(deps.storage, &config, &mut lockdrop, withdrawn_points, true)?;
    
    // Send funds back to user
    let mut response = Response::new()
        .add_attributes(vec![
            attr("action", "withdraw"),
            attr("user", info.sender.to_string()),
            attr("amount", amount.to_string()),
            attr("lock_days", lock_days.to_string()),
        ]);
    
    if !amount.is_zero() {
        response = response.add_message(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![coin(amount.u128(), config.deposit_token.clone())],
        });
    }
    
    Ok(response)
}

fn execute_edit_lock(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    lock_days: u64,
    new_lock_days: u64,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let lockdrop = CURRENT_LOCKDROP.load(deps.storage)?;
    
    // Check if in deposit period only
    let current_time = env.block.time.seconds();
    if current_time > lockdrop.deposit_end {
        return Err(ContractError::DepositPeriodEnded {});
    }
    
    // Validate new_lock_days
    if new_lock_days == 0 {
        return Err(ContractError::Validation(
            "new_lock_days must be greater than zero".into(),
        ));
    }
    if new_lock_days < config.minimum_lock_days {
        return Err(ContractError::Validation(
            format!("new_lock_days ({}) is below minimum ({})", new_lock_days, config.minimum_lock_days),
        ));
    }
    
    // Query transmuter for lock_ceiling
    let transmuter_config: membrane::transmuter::Config = deps.querier.query_wasm_smart(
        config.transmuter_contract.clone(),
        &membrane::transmuter::QueryMsg::Config {},
    )?;
    
    if new_lock_days > transmuter_config.lock_ceiling {
        return Err(ContractError::Validation(
            format!("new_lock_days ({}) exceeds transmuter lock_ceiling ({})", new_lock_days, transmuter_config.lock_ceiling),
        ));
    }
    
    // Load user deposits
    let mut deposits = USER_DEPOSITS
        .may_load(deps.storage, info.sender.to_string())?
        .ok_or_else(|| ContractError::InvalidFunds {
            reason: "User has no deposits".into(),
        })?;
    
    // Find deposit with matching lock_days
    let mut deposit = deposits.clone().into_iter()
        .find(|d| d.intended_lock_days == lock_days)
        .ok_or_else(|| ContractError::InvalidFunds {
            reason: format!("No deposit found with lock_days {}", lock_days),
        })?;
    
    // Check if new_lock_days already exists (can't have duplicate lock_days)
    if new_lock_days != lock_days && deposits.clone().iter().any(|d| d.intended_lock_days == new_lock_days) {
        return Err(ContractError::Validation(
            format!("Deposit with lock_days {} already exists", new_lock_days),
        ));
    }
    
    // Calculate old and new deposit points
    let old_points = calculate_deposit_points(
        deposit.amount,
        lock_days,
        transmuter_config.lock_ceiling,
        config.maximum_boost,
    )?;
    
    let new_points = calculate_deposit_points(
        deposit.amount,
        new_lock_days,
        transmuter_config.lock_ceiling,
        config.maximum_boost,
    )?;
    
    // Update total_deposit_points: subtract old, add new
    let mut lockdrop = CURRENT_LOCKDROP.load(deps.storage)?;
    update_total_deposit_points(deps.storage, &config, &mut lockdrop, old_points, true)?;
    update_total_deposit_points(deps.storage, &config, &mut lockdrop, new_points, false)?;
    
    // Update deposit's intended_lock_days
    deposit.intended_lock_days = new_lock_days;
    
    // Replace deposit in deposits
    deposits.retain(|d| d.intended_lock_days != lock_days);
    deposits.push(deposit.clone());
    
    // Save deposits
    USER_DEPOSITS.save(deps.storage, info.sender.to_string(), &deposits)?;
    // Update PENDING_LOCKS to keep in sync
    PENDING_LOCKS.save(deps.storage, info.sender.to_string(), &deposits)?;
    
    Ok(Response::new()
        .add_attributes(vec![
            attr("action", "edit_lock"),
            attr("user", info.sender.to_string()),
            attr("old_lock_days", lock_days.to_string()),
            attr("new_lock_days", new_lock_days.to_string()),
        ]))
}

fn execute_complete_locks(
    deps: DepsMut,
    env: Env,
    limit: Option<u32>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let lockdrop = CURRENT_LOCKDROP.load(deps.storage)?;
    
    // Check if withdrawal period has ended
    let current_time = env.block.time.seconds();
    if current_time <= lockdrop.withdrawal_end {
        return Err(ContractError::WithdrawalPeriodEnded {});
    }
    
    // Calculate days since withdrawal end
    let days_since_withdrawal_end = (current_time - lockdrop.withdrawal_end) / SECONDS_PER_DAY;
    
    // Load pending locks (should already be populated from deposits/withdrawals)
    let pending_locks: Vec<(String, Vec<UserDeposit>)> = PENDING_LOCKS
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .take(limit.unwrap_or(50) as usize)
        .collect::<StdResult<Vec<_>>>()?;
    
    // Verify total_deposit_points is set (should be calculated incrementally)
    if lockdrop.total_deposit_points.is_none() {
        let mut updated_lockdrop = lockdrop.clone();
        updated_lockdrop.total_deposit_points = Some(Uint128::zero());
        CURRENT_LOCKDROP.save(deps.storage, &updated_lockdrop)?;
    }
    
    execute_complete_locks_internal(deps, env, config, lockdrop, days_since_withdrawal_end, pending_locks)
}

fn update_total_deposit_points(
    storage: &mut dyn Storage,
    config: &Config,
    lockdrop: &mut LockdropState,
    deposit_points_delta: Uint128,
    withdrawal: bool,
) -> Result<(), ContractError> {
    let current_total = lockdrop.total_deposit_points.unwrap_or(Uint128::zero());
    let new_total = if withdrawal {
        if deposit_points_delta > current_total {
            // Handle underflow for withdrawals
            Uint128::zero()
        } else {
            current_total.checked_sub(deposit_points_delta)
                .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?
        }
    } else {
        current_total.checked_add(deposit_points_delta)
            .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?
    };
    
    lockdrop.total_deposit_points = Some(new_total);
    CURRENT_LOCKDROP.save(storage, lockdrop)?;
    Ok(())
}

fn save_lockdrop_to_history(
    storage: &mut dyn Storage,
    lockdrop: LockdropState,
) -> Result<(), ContractError> {
    let mut history = LOCKDROP_HISTORY.may_load(storage)?
        .unwrap_or_default();
    
    // Add new lockdrop to front of history
    history.insert(0, lockdrop);
    
    // Trim to max limit
    if history.len() > MAX_HISTORY_LIMIT {
        history.truncate(MAX_HISTORY_LIMIT);
    }
    
    LOCKDROP_HISTORY.save(storage, &history)?;
    Ok(())
}

fn calculate_deposit_points(
    amount: Uint128,
    lock_days: u64,
    lock_ceiling: u64,
    maximum_boost: Decimal,
) -> Result<Uint128, ContractError> {
    // Calculate base points: amount * lock_days
    let base_points = amount;
    
    // Calculate lock percentage: lock_days / lock_ceiling
    let lock_percentage = if lock_ceiling == 0 {
        Decimal::zero()
    } else {
        Decimal::from_ratio(lock_days, lock_ceiling)
    };
    
    // Calculate boost multiplier: 1 + maximum_boost * lock_percentage
    let boost_multiplier = Decimal::one() + decimal_multiplication(
        maximum_boost,
        lock_percentage,
    )?;
    
    // Calculate final points: base_points * boost_multiplier
    Ok(decimal_multiplication(
        Decimal::from_ratio(base_points, Uint128::one()),
        boost_multiplier,
    )?.to_uint_floor())
}

fn execute_complete_locks_internal(
    mut deps: DepsMut,
    env: Env,
    config: Config,
    lockdrop: LockdropState,
    days_since_withdrawal_end: u64,
    pending_locks: Vec<(String, Vec<UserDeposit>)>,
) -> Result<Response, ContractError> {
    let mut messages: Vec<CosmosMsg> = vec![];
    let mut processed_users = Vec::new();
    
    // Get total_deposit_points for share calculation
    let total_deposit_points = lockdrop.total_deposit_points
        .ok_or_else(|| ContractError::Validation(
            "total_deposit_points not calculated".into(),
        ))?;
    
    // Query transmuter for lock_ceiling (needed for points calculation)
    let transmuter_config: membrane::transmuter::Config = deps.querier.query_wasm_smart(
        config.transmuter_contract.clone(),
        &membrane::transmuter::QueryMsg::Config {},
    )?;
    
    let current_time = env.block.time.seconds();
    
    for (user, deposits) in pending_locks {
        // Calculate total deposit amount for this user
        let total_deposit_amount: Uint128 = deposits.iter()
            .map(|d| d.amount)
            .fold(Uint128::zero(), |acc, x| acc.checked_add(x).unwrap_or(acc));
        
        // Calculate user's total deposit points
        let mut user_points = Uint128::zero();
        for deposit in &deposits {
            let points = calculate_deposit_points(
                deposit.amount,
                deposit.intended_lock_days,
                transmuter_config.lock_ceiling,
                config.maximum_boost,
            )?;
            user_points = user_points.checked_add(points)
                .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
        }
        
        // Calculate share_of_claims: user_points / total_deposit_points
        let share_of_claims = if total_deposit_points.is_zero() {
            Decimal::zero()
        } else {
            Decimal::from_ratio(user_points, total_deposit_points)
        };
        
        // Load existing history for user or create new
        let mut history = USER_LOCKDROP_HISTORY
            .may_load(deps.storage, user.clone())?
            .unwrap_or_default();
        
        // Calculate running_total_claims by summing all previous entries' deposit amounts
        let running_total_claims: Uint128 = history.iter()
            .map(|h| h.deposit)
            .fold(Uint128::zero(), |acc, x| acc.checked_add(x).unwrap_or(acc));
        
        // Create new history entry
        let history_entry = UserLockdropHistory {
            deposit: total_deposit_amount,
            running_total_claims,
            share_of_claims,
            time: current_time,
        };
        
        history.push(history_entry);
        
        // Prune history to max 100 entries (keep most recent)
        const MAX_HISTORY_SIZE: usize = 100;
        if history.len() > MAX_HISTORY_SIZE {
            let excess = history.len() - MAX_HISTORY_SIZE;
            history.drain(0..excess);
        }
        
        USER_LOCKDROP_HISTORY.save(deps.storage, user.clone(), &history)?;
        
        // Process deposits and create messages
        for deposit in deposits {
            // Calculate effective lock days (reduce by days since withdrawal end)
            let effective_lock_days = if deposit.intended_lock_days > days_since_withdrawal_end {
                deposit.intended_lock_days - days_since_withdrawal_end
            } else {
                0 // Lock has fully expired
            };
            
            // Call transmuter EnterVault with lock_days
            let enter_vault_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.transmuter_contract.clone(),
                msg: to_json_binary(&TransmuterExecuteMsg::EnterVault {
                    recipient: Some(user.clone()),
                    lock_days: Some(effective_lock_days),
                    affiliate_address: None,
                })?,
                funds: vec![coin(deposit.amount.u128(), config.deposit_token.clone())],
            });
            messages.push(enter_vault_msg);
        }
        processed_users.push(user.clone());
    }
    
    // Remove processed users from pending locks
    for user in &processed_users {
        PENDING_LOCKS.remove(deps.storage, user.clone());
    }
    
    Ok(Response::new()
        .add_messages(messages)
        .add_attributes(vec![
            attr("action", "complete_locks"),
            attr("processed_users", processed_users.len().to_string()),
        ]))
}

fn execute_claim(
    mut deps: DepsMut,
    env: Env,
    users: Vec<String>,
    mbrn_intent: Option<MbrnClaimIntent>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let lockdrop = CURRENT_LOCKDROP.load(deps.storage)?;
    
    // Check if withdrawal period has ended
    let current_time = env.block.time.seconds();
    if current_time <= lockdrop.withdrawal_end {
        return Err(ContractError::WithdrawalPeriodEnded {});
    }
    
    // Check if total_deposit_points is calculated (CompleteLocks must be called first)
    let total_deposit_points = lockdrop.total_deposit_points
        .ok_or_else(|| ContractError::Validation(
            "total_deposit_points not calculated. CompleteLocks must be called first".into(),
        ))?;
    
    if total_deposit_points.is_zero() {
        return Err(ContractError::Validation(
            "total_deposit_points is zero".into(),
        ));
    }
    
    // Load user deposits from USER_DEPOSITS (users in PENDING_LOCKS can't claim)
    let all_deposits: Vec<(String, Vec<UserDeposit>)> = users.iter()
        .filter_map(|user| {
            // Check if user is in PENDING_LOCKS - if yes, they can't claim
            if PENDING_LOCKS.may_load(deps.storage, user.clone()).ok().flatten().is_some() {
                return None; // Skip users with pending locks
            }
            
            // Check if user exists in USER_DEPOSITS - if no, they've already claimed
            if let Ok(Some(deposits)) = USER_DEPOSITS.may_load(deps.storage, user.clone()) {
                if !deposits.is_empty() {
                    return Some((user.clone(), deposits));
                }
            }
            None // User has already claimed or never deposited
        })
        .collect();
    
    // Query transmuter for lock_ceiling
    let transmuter_config: membrane::transmuter::Config = deps.querier.query_wasm_smart(
        config.transmuter_contract.clone(),
        &membrane::transmuter::QueryMsg::Config {},
    )?;
    
    // Calculate user deposit points with boost
    let mut user_deposit_points: Vec<(String, Uint128)> = Vec::new();
    
    for (user, deposits) in &all_deposits {
        let mut user_points = Uint128::zero();
        for deposit in deposits {
            let points = calculate_deposit_points(
                deposit.amount,
                deposit.intended_lock_days,
                transmuter_config.lock_ceiling,
                config.maximum_boost,
            )?;
            user_points = user_points.checked_add(points)
                .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
        }
        user_deposit_points.push((user.clone(), user_points));
    }
    
    // Calculate and process rewards for each user with intent logic
    let mut messages: Vec<CosmosMsg> = vec![];
    let mut claimed_users = Vec::new();
    
    for (user, user_points) in user_deposit_points {
        // Calculate user's share of rewards using stored total_deposit_points
        let user_reward = decimal_multiplication(
            Decimal::from_ratio(config.lockdrop_incentive_size, Uint128::one()),
            Decimal::from_ratio(user_points, total_deposit_points),
        )?.to_uint_floor();
        
        if user_reward.is_zero() {
            continue;
        }
        
        // Handle intent logic
        let user_intent_msgs = process_user_intents(
            deps.branch(),
            &env,
            &config,
            &user,
            user_reward,
            &mbrn_intent,
            &all_deposits.iter().find(|(u, _)| u == &user).map(|(_, d)| d),
        )?;
        
        messages.extend(user_intent_msgs);
        
        // Remove user from USER_DEPOSITS (marking as claimed)
        USER_DEPOSITS.remove(deps.storage, user.clone());
        claimed_users.push(user);
    }
    
    Ok(Response::new()
        .add_messages(messages)
        .add_attributes(vec![
            attr("action", "claim"),
            attr("claimed_users", claimed_users.len().to_string()),
        ]))
}

fn process_user_intents(
    mut deps: DepsMut,
    env: &Env,
    config: &Config,
    user: &String,
    user_reward: Uint128,
    mbrn_intent: &Option<MbrnClaimIntent>,
    user_deposits: &Option<&Vec<UserDeposit>>,
) -> Result<Vec<CosmosMsg>, ContractError> {
    let mut messages: Vec<CosmosMsg> = vec![];
    
    // Determine which intents to use
    let intents_to_use: Option<Vec<MbrnIntentOption>> = if let Some(ref intent) = mbrn_intent {
        // If set_ongoing=true, save to USER_INTENTS
        if intent.set_ongoing {
            USER_INTENTS.save(deps.storage, user.clone(), &intent.intents.clone())?;
        }
        // If apply_now=true, use intents from mbrn_intent
        if intent.apply_now {
            Some(intent.intents.clone())
        } else {
            // Check for stored intents
            USER_INTENTS.may_load(deps.storage, user.clone())?
        }
    } else {
        // No mbrn_intent provided, check for stored intents or deposit intents
        if let Some(stored) = USER_INTENTS.may_load(deps.storage, user.clone())? {
            Some(stored)
        } else if let Some(deposits) = user_deposits {
            // Use intents from first deposit (if all deposits have same intents, or use first)
            deposits.first().and_then(|d| d.intents.clone())
        } else {
            None
        }
    };
    
    // Query IntentBoosts from system_discounts contract if configured and intents exist
    let intent_boosts: Vec<Decimal> = if let Some(ref intents) = intents_to_use {
        if !config.discounts_contract.is_empty() {
            match deps.querier.query_wasm_smart::<IntentBoostsResponse>(
                config.discounts_contract.clone(),
                &DiscountQueryMsg::IntentBoosts {
                    intents: intents.clone(),
                },
            ) {
                Ok(response) => response.boosts,
                Err(_) => {
                    // If query fails, proceed without boost (all zeros)
                    vec![Decimal::zero(); intents.len()]
                }
            }
        } else {
            // No discounts contract configured, proceed without boost
            vec![Decimal::zero(); intents.len()]
        }
    } else {
        vec![]
    };
    
    // Calculate total boosted amount to mint
    let mut total_to_mint = user_reward;
    if let Some(ref intents) = intents_to_use {
        let mut total_boost_amount = Uint128::zero();
        for (i, intent) in intents.iter().enumerate() {
            let base_amount = decimal_multiplication(
                Decimal::from_ratio(user_reward, Uint128::one()),
                intent.ratio
            )?.to_uint_floor();
            
            // Apply boost: boosted_amount = base_amount * (1 + boost)
            let boost = intent_boosts.get(i).copied().unwrap_or(Decimal::zero());
            let boost_multiplier = Decimal::one() + boost;
            let boosted_amount = decimal_multiplication(
                Decimal::from_ratio(base_amount, Uint128::one()),
                boost_multiplier
            )?.to_uint_floor();
            
            total_boost_amount = total_boost_amount.checked_add(boosted_amount)
                .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
        }
        
        // Calculate boost excess (amount beyond user_reward that needs to be minted)
        if total_boost_amount > user_reward {
            let boost_excess = total_boost_amount.checked_sub(user_reward)
                .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
            total_to_mint = user_reward.checked_add(boost_excess)
                .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
        }
    }
    
    // Mint total amount (user_reward + any boost excess) to contract
    let contract_addr = env.contract.address.clone();
    let mint_to_contract_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.neutron_proxy.clone(),
        msg: to_json_binary(&NeutronProxyExecuteMsg::MintTokens {
            denom: config.mbrn_denom.clone(),
            amount: total_to_mint,
            mint_to_address: contract_addr.to_string(),
        })?,
        funds: vec![],
    });
    messages.push(mint_to_contract_msg);
    
    let mut remaining_to_mint = total_to_mint;
    
    // Process intents if provided
    if let Some(intents) = intents_to_use {
        // Validate ratios sum to ~1.0
        let total_ratio: Decimal = intents.iter()
            .map(|i| i.ratio)
            .fold(Decimal::zero(), |acc, x| acc + x);
        let diff = if total_ratio > Decimal::one() {
            total_ratio - Decimal::one()
        } else {
            Decimal::one() - total_ratio
        };
        if diff > Decimal::percent(1) {
            return Err(ContractError::Validation(
                "MBRN intent ratios must sum to approximately 1.0".into(),
            ));
        }
        
        // Process each intent
        for (i, intent) in intents.iter().enumerate() {
            let base_amount = decimal_multiplication(
                Decimal::from_ratio(user_reward, Uint128::one()),
                intent.ratio
            )?.to_uint_floor();
            
            // Apply boost: boosted_amount = base_amount * (1 + boost)
            let boost = intent_boosts.get(i).copied().unwrap_or(Decimal::zero());
            let boost_multiplier = Decimal::one() + boost;
            let amount = decimal_multiplication(
                Decimal::from_ratio(base_amount, Uint128::one()),
                boost_multiplier
            )?.to_uint_floor();
            
            if amount.is_zero() {
                continue;
            }
            
            match &intent.intent_type {
                MbrnIntentType::Stake {} => {
                    if let Some(staking_addr) = &config.staking_contract {
                        let stake_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                            contract_addr: staking_addr.clone(),
                            msg: to_json_binary(&StakingExecuteMsg::Stake {
                                user: Some(user.clone()),
                                locked: intent.lock.clone(),
                            })?,
                            funds: vec![coin(amount.u128(), config.mbrn_denom.clone())],
                        });
                        messages.push(stake_msg);
                        remaining_to_mint = remaining_to_mint.checked_sub(amount)
                            .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
                    } else {
                        return Err(ContractError::Validation(
                            "staking_contract not configured but Stake intent provided".into(),
                        ));
                    }
                }
                MbrnIntentType::DepositViaMarsMirror { asset, target_ltv, target_max_borrow_ltv } => {
                    if let (Some(ltv_disco_addr), Some(mars_mirror_addr)) = (&config.ltv_disco_contract, &config.mars_mirror_contract) {
                        // Create deposit input
                        let deposit_input = BackingDepositInput {
                            asset: asset.clone(),
                            ltv: target_ltv.unwrap_or(Decimal::zero()),
                            max_borrow_ltv: target_max_borrow_ltv.unwrap_or(Decimal::zero()),
                        };
                        
                        // Deposit into ltv_disco with mars_mirror as manager
                        let deposit_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                            contract_addr: ltv_disco_addr.clone(),
                            msg: to_json_binary(&LtvDiscoExecuteMsg::SubmitDeposit {
                                deposit_input,
                                deposit_owner: Some(user.clone()),
                                locked: intent.lock.clone(),
                                deposit_id: None,
                                manager: Some(mars_mirror_addr.clone()),
                                affiliate_address: None,
                            })?,
                            funds: vec![coin(amount.u128(), config.mbrn_denom.clone())],
                        });
                        messages.push(deposit_msg);
                        remaining_to_mint = remaining_to_mint.checked_sub(amount)
                            .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
                    } else {
                        return Err(ContractError::Validation(
                            "ltv_disco_contract and mars_mirror_contract must be configured for DepositViaMarsMirror intent".into(),
                        ));
                    }
                }
                MbrnIntentType::SendToAddress { address } => {
                    // Validate the recipient address
                    let recipient_addr = deps.api.addr_validate(address)
                        .map_err(|e| ContractError::Validation(
                            format!("Invalid recipient address: {}", e)
                        ))?;
                    
                    // Send MBRN to the specified address
                    let send_msg = CosmosMsg::Bank(BankMsg::Send {
                        to_address: recipient_addr.to_string(),
                        amount: vec![coin(amount.u128(), config.mbrn_denom.clone())],
                    });
                    messages.push(send_msg);
                    remaining_to_mint = remaining_to_mint.checked_sub(amount)
                        .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
                }
            }
        }
    }
    
    // Send remaining amount (after intents) from contract to user
    if !remaining_to_mint.is_zero() {
        // Send MBRN from contract to user
        let send_msg = CosmosMsg::Bank(BankMsg::Send {
            to_address: user.clone(),
            amount: vec![coin(remaining_to_mint.u128(), config.mbrn_denom.clone())],
        });
        messages.push(send_msg);
    }
    
    Ok(messages)
}

fn execute_update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    transmuter_contract: Option<String>,
    neutron_proxy: Option<String>,
    lockdrop_incentive_size: Option<Uint128>,
    deposit_period_days: Option<u64>,
    withdrawal_period_days: Option<u64>,
    deposit_token: Option<String>,
    minimum_deposit: Option<Uint128>,
    mbrn_denom: Option<String>,
    staking_contract: Option<String>,
    mars_mirror_contract: Option<String>,
    ltv_disco_contract: Option<String>,
    discounts_contract: Option<String>,
    maximum_boost: Option<Decimal>,
    minimum_lock_days: Option<u64>,
    emissions_voting_contract: Option<String>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    ensure_owner(&config, &info.sender)?;
    
    if let Some(owner_str) = owner {
        config.owner = deps.api.addr_validate(&owner_str)?;
    }
    
    if let Some(contract) = transmuter_contract {
        let _ = deps.api.addr_validate(&contract)?;
        config.transmuter_contract = contract;
    }
    
    if let Some(proxy) = neutron_proxy {
        let _ = deps.api.addr_validate(&proxy)?;
        config.neutron_proxy = proxy;
    }
    
    if let Some(size) = lockdrop_incentive_size {
        config.lockdrop_incentive_size = size;
    }
    
    if let Some(days) = deposit_period_days {
        if days == 0 {
            return Err(ContractError::Validation(
                "deposit_period_days must be greater than zero".into(),
            ));
        }
        config.deposit_period_days = days;
    }
    
    if let Some(days) = withdrawal_period_days {
        if days == 0 {
            return Err(ContractError::Validation(
                "withdrawal_period_days must be greater than zero".into(),
            ));
        }
        config.withdrawal_period_days = days;
    }
    
    if let Some(token) = deposit_token {
        config.deposit_token = token;
    }
    
    if let Some(min) = minimum_deposit {
        if min.is_zero() {
            return Err(ContractError::Validation(
                "minimum_deposit must be greater than zero".into(),
            ));
        }
        config.minimum_deposit = min;
    }
    
    if let Some(denom) = mbrn_denom {
        config.mbrn_denom = denom;
    }
    
    if let Some(staking) = staking_contract {
        let _ = deps.api.addr_validate(&staking)?;
        config.staking_contract = Some(staking);
    }
    
    if let Some(mirror) = mars_mirror_contract {
        let _ = deps.api.addr_validate(&mirror)?;
        config.mars_mirror_contract = Some(mirror);
    }
    
    if let Some(ltv_disco) = ltv_disco_contract {
        let _ = deps.api.addr_validate(&ltv_disco)?;
        config.ltv_disco_contract = Some(ltv_disco);
    }
    
    if let Some(discounts) = discounts_contract {
        let _ = deps.api.addr_validate(&discounts)?;
        config.discounts_contract = discounts;
    }
    
    if let Some(boost) = maximum_boost {
        if boost < Decimal::zero() {
            return Err(ContractError::Validation(
                "maximum_boost must be greater than or equal to zero".into(),
            ));
        }
        config.maximum_boost = boost;
    }
    
    if let Some(min_lock) = minimum_lock_days {
        if min_lock == 0 {
            return Err(ContractError::Validation(
                "minimum_lock_days must be greater than zero".into(),
            ));
        }
        config.minimum_lock_days = min_lock;
    }
    
    if let Some(emissions_voting) = emissions_voting_contract {
        let _ = deps.api.addr_validate(&emissions_voting)?;
        config.emissions_voting_contract = Some(emissions_voting);
    }
    
    CONFIG.save(deps.storage, &config)?;
    
    Ok(Response::new().add_attribute("action", "update_config"))
}

/// Handle voting result from emissions voting contract
fn execute_receive_voting_result(
    deps: DepsMut,
    info: MessageInfo,
    label: String,
    result_uint128: Option<Uint128>,
    _result_decimal: Option<Decimal>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    
    // Authorization: Only emissions_voting_contract can call this
    let emissions_voting = config.emissions_voting_contract.clone()
        .ok_or_else(|| ContractError::Unauthorized {})?;
    if info.sender != deps.api.addr_validate(&emissions_voting)? {
        return Err(ContractError::Unauthorized {});
    }
    
    // Only process "transmuter_lockdrop" label
    if label != "transmuter_lockdrop" {
        return Ok(Response::new()
            .add_attribute("action", "receive_voting_result")
            .add_attribute("status", "ignored")
            .add_attribute("label", label));
    }
    
    // Update lockdrop_incentive_size from result_uint128
    let new_incentive_size = result_uint128
        .ok_or_else(|| ContractError::Validation(
            "transmuter_lockdrop graph must return Uint128 result".into(),
        ))?;
    config.lockdrop_incentive_size = new_incentive_size;
    
    // Query emissions_voting for graph period_days
    let graph_response: membrane::emissions_voting::GraphResponse = deps.querier.query_wasm_smart(
        emissions_voting.clone(),
        &membrane::emissions_voting::QueryMsg::Graph {
            label: "transmuter_lockdrop".to_string(),
        },
    )?;
    
    let period_days = graph_response.graph.period_days();
    
    // Calculate periods using 5:2 ratio (total 7 parts)
    // deposit_period_days = (period_days * 5) / 7
    // withdrawal_period_days = (period_days * 2) / 7
    let deposit_period = (period_days * 5) / 7;
    let withdrawal_period = (period_days * 2) / 7;
    
    // Ensure minimum of 1 day for each period
    config.deposit_period_days = deposit_period.max(1);
    config.withdrawal_period_days = withdrawal_period.max(1);
    
    CONFIG.save(deps.storage, &config)?;
    
    Ok(Response::new()
        .add_attribute("action", "receive_voting_result")
        .add_attribute("label", label)
        .add_attribute("new_incentive_size", new_incentive_size.to_string())
        .add_attribute("deposit_period_days", config.deposit_period_days.to_string())
        .add_attribute("withdrawal_period_days", config.withdrawal_period_days.to_string())
    )
}

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&ConfigResponse {
            config: CONFIG.load(deps.storage)?,
        }),
        QueryMsg::CurrentLockdrop {} => {
            let lockdrop = CURRENT_LOCKDROP.may_load(deps.storage)?;
            to_json_binary(&CurrentLockdropResponse { lockdrop })
        }
        QueryMsg::UserDeposits { user } => {
            let deposits = USER_DEPOSITS
                .may_load(deps.storage, user)?
                .unwrap_or_default();
            to_json_binary(&UserDepositsResponse { deposits })
        }
        QueryMsg::PendingLocks {} => {
            let users: Vec<String> = PENDING_LOCKS
                .keys(deps.storage, None, None, cosmwasm_std::Order::Ascending)
                .collect::<StdResult<Vec<_>>>()?;
            to_json_binary(&PendingLocksResponse { users })
        }
        QueryMsg::UserClaims { user, limit, start_after } => {
            query_user_claims(deps, user, limit, start_after)
        }
        QueryMsg::LockdropHistory {} => {
            let history = LOCKDROP_HISTORY.may_load(deps.storage)?
                .unwrap_or_default();
            to_json_binary(&membrane::transmuter_lockdrop::LockdropHistoryResponse { history })
        }
        QueryMsg::UserHistory { user } => {
            let history = USER_LOCKDROP_HISTORY
                .may_load(deps.storage, user)?
                .unwrap_or_default();
            to_json_binary(&UserHistoryResponse { history })
        }
    }
}

fn query_user_claims(
    deps: Deps,
    user: Option<String>,
    limit: Option<u32>,
    start_after: Option<String>,
) -> StdResult<Binary> {
    let max_limit = limit.unwrap_or(50).min(100) as usize;
    
    if let Some(user_addr) = user {
        // Query single user - if they're in USER_DEPOSITS, they haven't claimed yet
        let has_deposits = USER_DEPOSITS.may_load(deps.storage, user_addr.clone())?.is_some();
        
        let claims = if has_deposits {
            vec![] // User hasn't claimed yet
        } else {
            // User has claimed (not in USER_DEPOSITS)
            // We can't determine the claim amount without storing it, so return empty
            vec![]
        };
        
        let claims_clone = claims.clone();
        to_json_binary(&UserClaimsResponse {
            claims,
            total: claims_clone.len() as u64,
            next_start_after: None,
        })
    } else {
        // Query all users - those not in USER_DEPOSITS have claimed
        // This query can't determine claim amounts without storing them
        // Return empty for now - claim amounts aren't stored after claiming
        to_json_binary(&UserClaimsResponse {
            claims: vec![],
            total: 0,
            next_start_after: None,
        })
    }
}

fn ensure_owner(config: &Config, sender: &Addr) -> Result<(), ContractError> {
    if &config.owner != sender {
        return Err(ContractError::Unauthorized {});
    }
    Ok(())
}

