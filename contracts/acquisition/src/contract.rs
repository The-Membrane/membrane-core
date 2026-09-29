use std::cmp::min;

use cosmwasm_std::{
    attr, coin, entry_point, to_json_binary, Addr, Binary, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, Response, StdError, StdResult, Storage, Uint128, WasmMsg,
};
use cw2::set_contract_version;

use membrane::acquisition::{
    Config, ExecuteMsg, InstantiateMsg, QueryMsg, AcquisitionWindow, AcquisitionDeposit,
    AcquisitionModelConfig, AcquisitionModelState, AcquisitionHistoryEntry,
    ConfigResponse, CurrentAcquisitionWindowResponse, ActiveAcquisitionWindowResponse,
    UserAcquisitionDepositResponse, AcquisitionConfigResponse, AcquisitionModelStateResponse,
    AcquisitionHistoryResponse,
    MbrnClaimIntent, MbrnIntentOption, MbrnIntentType,
};
use membrane::transmuter::{ExecuteMsg as TransmuterExecuteMsg, QueryMsg as TransmuterQueryMsg, VaultInfoResponse};
use membrane::cdp::ExecuteMsg as CdpExecuteMsg;
use membrane::neutron_proxy::ExecuteMsg as NeutronProxyExecuteMsg;
use membrane::math::{decimal_multiplication, decimal_division};
// use membrane::staking::ExecuteMsg as StakingExecuteMsg;
use membrane::ltv_disco::{ExecuteMsg as LtvDiscoExecuteMsg, QueryMsg as LtvDiscoQueryMsg, BackingDepositInput, AssetQueueResponse};
// DISABLED: UserBoostResponse and DiscountConfig - GetBoost query removed from system
// use membrane::system_discounts::{QueryMsg as DiscountQueryMsg, IntentBoostsResponse, UserBoostResponse, Config as DiscountConfig};
// DISABLED: Intent boosts disabled
// use membrane::system_discounts::{QueryMsg as DiscountQueryMsg, IntentBoostsResponse};

use crate::error::ContractError;
use crate::state::{CONFIG, CURRENT_WINDOW_ID, CURRENT_ACQUISITION_WINDOW, USER_ACQUISITION_DEPOSITS, USER_INTENTS, ACQUISITION_CONFIG, ACQUISITION_STATE, ACQUISITION_HISTORY, append_acquisition_history};

const CONTRACT_NAME: &str = "membrane-acquisition";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");
const SECONDS_PER_DAY: u64 = 86_400;

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let owner = deps.api.addr_validate(&msg.owner)?;

    // Validate addresses
    let _ = deps.api.addr_validate(&msg.transmuter_contract)?;
    let _ = deps.api.addr_validate(&msg.neutron_proxy)?;
    let _ = deps.api.addr_validate(&msg.cdp_contract)?;
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
    if msg.minimum_deposit.is_zero() {
        return Err(ContractError::Validation(
            "minimum_deposit must be greater than zero".into(),
        ));
    }
    if msg.cliff_period_days == 0 {
        return Err(ContractError::Validation(
            "cliff_period_days must be greater than zero".into(),
        ));
    }

    // Validate acquisition model config
    let acq_config = &msg.acquisition_config;
    if acq_config.base_acquisition_rate.is_zero() {
        return Err(ContractError::Validation("base_acquisition_rate must be > 0".into()));
    }
    if acq_config.max_mbrn_emission.is_zero() {
        return Err(ContractError::Validation("max_mbrn_emission must be > 0".into()));
    }
    if acq_config.target_utilization.is_zero() || acq_config.target_utilization > Decimal::one() {
        return Err(ContractError::Validation("target_utilization must be in (0, 1]".into()));
    }
    if acq_config.bump_interval_seconds == 0 {
        return Err(ContractError::Validation("bump_interval_seconds must be > 0".into()));
    }
    if acq_config.reduction_speed_multiplier.is_zero() {
        return Err(ContractError::Validation("reduction_speed_multiplier must be > 0".into()));
    }
    if acq_config.max_bump_rate.is_zero() {
        return Err(ContractError::Validation("max_bump_rate must be > 0".into()));
    }
    if acq_config.mutation_cooldown_seconds == 0 {
        return Err(ContractError::Validation("mutation_cooldown_seconds must be > 0".into()));
    }
    if acq_config.accrual_ramp_seconds == 0 {
        return Err(ContractError::Validation("accrual_ramp_seconds must be > 0".into()));
    }
    if acq_config.accrual_ramp_start_fraction.is_zero() || acq_config.accrual_ramp_start_fraction > Decimal::one() {
        return Err(ContractError::Validation("accrual_ramp_start_fraction must be in (0, 1]".into()));
    }

    let config = Config {
        owner,
        transmuter_contract: msg.transmuter_contract,
        neutron_proxy: msg.neutron_proxy,
        cdp_contract: msg.cdp_contract,
        deposit_period_days: msg.deposit_period_days,
        withdrawal_period_days: msg.withdrawal_period_days,
        deposit_token: msg.deposit_token,
        minimum_deposit: msg.minimum_deposit,
        mbrn_denom: msg.mbrn_denom,
        staking_contract: msg.staking_contract,
        mars_mirror_contract: msg.mars_mirror_contract,
        ltv_disco_contract: msg.ltv_disco_contract,
        discounts_contract: msg.discounts_contract,
        emissions_voting_contract: msg.emissions_voting_contract,
        cliff_period_seconds: msg.cliff_period_days * SECONDS_PER_DAY,
    };

    CONFIG.save(deps.storage, &config)?;
    ACQUISITION_CONFIG.save(deps.storage, &msg.acquisition_config)?;
    CURRENT_WINDOW_ID.save(deps.storage, &0u64)?;
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
        ExecuteMsg::StartAcquisitionWindow {} => {
            execute_start_acquisition_window(deps, env, info)
        }
        ExecuteMsg::Deposit { intents, affiliate_address, affiliate_label } => execute_deposit(deps, env, info, intents, affiliate_address, affiliate_label),
        ExecuteMsg::Withdraw { amount } => execute_withdraw(deps, env, info, amount),
        ExecuteMsg::AccruePool {} => execute_accrue_pool(deps, env, info),
        ExecuteMsg::Claim { window_id, mbrn_intent } => execute_claim(deps, env, info, window_id, mbrn_intent),
        ExecuteMsg::ClaimForUser { user, window_id, mbrn_intent } => {
            execute_claim_for_user(deps, env, info, user, window_id, mbrn_intent)
        }
        ExecuteMsg::SendAcquisitionRewardsToDisco { window_id, mbrn_intent } => {
            execute_send_acquisition_rewards_to_disco(deps, env, info, window_id, mbrn_intent)
        }
        ExecuteMsg::UpdateConfig {
            owner,
            transmuter_contract,
            neutron_proxy,
            cdp_contract,
            deposit_period_days,
            withdrawal_period_days,
            deposit_token,
            minimum_deposit,
            mbrn_denom,
            staking_contract,
            mars_mirror_contract,
            ltv_disco_contract,
            discounts_contract,
            emissions_voting_contract,
            cliff_period_days,
            acquisition_config,
        } => execute_update_config(
            deps,
            info,
            owner,
            transmuter_contract,
            neutron_proxy,
            cdp_contract,
            deposit_period_days,
            withdrawal_period_days,
            deposit_token,
            minimum_deposit,
            mbrn_denom,
            staking_contract,
            mars_mirror_contract,
            ltv_disco_contract,
            discounts_contract,
            emissions_voting_contract,
            cliff_period_days,
            acquisition_config,
        ),
        ExecuteMsg::AcquisitionRateAssurance {} => execute_acquisition_rate_assurance(deps, env, info),
    }
}

// ─── StartAcquisitionWindow ───────────────────────────────────────────────

fn execute_start_acquisition_window(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    ensure_owner(&config, &info.sender)?;

    // Check if there's an active window
    if let Some(current_window) = CURRENT_ACQUISITION_WINDOW.may_load(deps.storage)? {
        let current_time = env.block.time.seconds();
        if current_time < current_window.withdrawal_end {
            return Err(ContractError::Validation(
                "Current acquisition window is still active".into(),
            ));
        }
    }

    let acq_config = ACQUISITION_CONFIG.load(deps.storage)?;

    // Get next window ID
    let window_id = CURRENT_WINDOW_ID.may_load(deps.storage)?
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| ContractError::Std(StdError::generic_err("Window ID overflow")))?;
    CURRENT_WINDOW_ID.save(deps.storage, &window_id)?;

    let current_time = env.block.time.seconds();

    // Lazy timers: deposit_end and withdrawal_end are u64::MAX until first deposit triggers them
    let window = AcquisitionWindow {
        window_id,
        start_time: current_time,
        deposit_end: u64::MAX,
        withdrawal_end: u64::MAX,
        deposit_period_days: config.deposit_period_days,
        total_deposit_amount: Uint128::zero(),
        acquisition_budget: Uint128::zero(), // Set to accrued_pool at withdrawal end
    };
    CURRENT_ACQUISITION_WINDOW.save(deps.storage, &window)?;

    // Initialize AcquisitionModelState
    // Carry over bump_rate and converged rate from previous window
    let previous_state = ACQUISITION_STATE.may_load(deps.storage)?;
    let (carry_bump_rate, carry_last_bump_time) = match &previous_state {
        Some(prev) => (prev.bump_rate, prev.last_bump_time),
        None => (Decimal::zero(), current_time),
    };
    // Cross-window rate memory: carry previous converged rate, clamped to base_rate ceiling.
    // Only downward convergence persists; if demand was poor, resets to base.
    let carry_rate = previous_state
        .as_ref()
        .map(|p| min(p.current_acquisition_rate, acq_config.base_acquisition_rate))
        .unwrap_or(acq_config.base_acquisition_rate);

    let new_state = AcquisitionModelState {
        current_acquisition_rate: carry_rate,
        accrued_pool: Uint128::zero(),
        last_accrual_time: current_time,
        pool_maxed: false,
        bump_rate: carry_bump_rate,
        last_bump_time: carry_last_bump_time,
        window_timers_started: false,
        first_deposit_time: None,
        deposit_event_count: 0,
        total_new_deposits: Uint128::zero(),
        last_mutation_efficiency: None,
        last_mutation_time: None,
        efficiency_clamped: false,
    };
    ACQUISITION_STATE.save(deps.storage, &new_state)?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("action", "start_acquisition_window"),
            attr("window_id", window_id.to_string()),
            attr("carry_bump_rate", carry_bump_rate.to_string()),
            attr("carry_acquisition_rate", carry_rate.to_string()),
        ]))
}

// ─── Deposit ──────────────────────────────────────────────────────────────

fn execute_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    intents: Option<Vec<MbrnIntentOption>>,
    affiliate_address: Option<String>,
    affiliate_label: Option<String>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let acq_config = ACQUISITION_CONFIG.load(deps.storage)?;
    let mut window = CURRENT_ACQUISITION_WINDOW.load(deps.storage)?;
    let current_time = env.block.time.seconds();

    // Check if in deposit period
    if current_time > window.deposit_end {
        return Err(ContractError::DepositPeriodEnded {});
    }

    // Assert only the deposit token is sent
    if info.funds.len() != 1 || info.funds[0].denom != config.deposit_token {
        return Err(ContractError::InvalidFunds {
            reason: format!("Expected {} deposit", config.deposit_token),
        });
    }
    let deposit_amount = info.funds[0].amount;

    if deposit_amount < config.minimum_deposit {
        return Err(ContractError::Validation(
            format!("Deposit amount {} is below minimum {}", deposit_amount, config.minimum_deposit),
        ));
    }

    // Validate intents if provided
    if let Some(ref intents) = intents {
        validate_intent_ratios(intents)?;
    }

    // Accrue pool before processing deposit
    let (mut state, mut accrual_msgs) = accrue_pool_internal(
        deps.storage,
        &deps.querier,
        &env,
        &config,
        &acq_config,
    )?;

    // First deposit triggers window timers
    if !state.window_timers_started {
        state.window_timers_started = true;
        state.first_deposit_time = Some(current_time);
        window.deposit_end = current_time + (config.deposit_period_days * SECONDS_PER_DAY);
        window.withdrawal_end = window.deposit_end + (config.withdrawal_period_days * SECONDS_PER_DAY);
    }

    // Track deposit for efficiency
    state.deposit_event_count += 1;
    state.total_new_deposits = state.total_new_deposits.checked_add(deposit_amount)
        .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;

    ACQUISITION_STATE.save(deps.storage, &state)?;

    // Forward funds to transmuter EnterVault (deposit owned by contract)
    let contract_addr = env.contract.address.clone();

    // Check if user already has a deposit entry for current window
    let existing_deposit = USER_ACQUISITION_DEPOSITS.may_load(deps.storage, (info.sender.to_string(), window.window_id))?;

    if let Some(mut deposit) = existing_deposit {
        deposit.amount = deposit.amount.checked_add(deposit_amount)
            .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
        USER_ACQUISITION_DEPOSITS.save(deps.storage, (info.sender.to_string(), window.window_id), &deposit)?;
    } else {
        let vested_at = current_time + config.cliff_period_seconds;
        let new_deposit = AcquisitionDeposit {
            amount: deposit_amount,
            deposit_time: current_time,
            vested_at,
            sent_to_disco: None,
            mbrn_claim_amount: None,
            disco_deposit_id: None,
            claimed_mbrn_amount: None,
            disco_asset: None,
            disco_slot: None,
        };
        USER_ACQUISITION_DEPOSITS.save(deps.storage, (info.sender.to_string(), window.window_id), &new_deposit)?;
    }

    let enter_vault_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.transmuter_contract.clone(),
        msg: to_json_binary(&TransmuterExecuteMsg::EnterVault {
            recipient: Some(contract_addr.to_string()),
            affiliate_address,
            affiliate_label,
        })?,
        funds: vec![coin(deposit_amount.u128(), config.deposit_token.clone())],
    });

    // Update total_deposit_amount
    window.total_deposit_amount = window.total_deposit_amount.checked_add(deposit_amount)
        .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
    CURRENT_ACQUISITION_WINDOW.save(deps.storage, &window)?;

    // Save intents if provided
    if let Some(intents) = intents {
        USER_INTENTS.save(deps.storage, info.sender.to_string(), &intents)?;
    }

    accrual_msgs.push(enter_vault_msg);

    // Acquisition rate assurance callback (runs after transmuter processes EnterVault)
    accrual_msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&membrane::acquisition::ExecuteMsg::AcquisitionRateAssurance {})?,
        funds: vec![],
    }));

    Ok(Response::new()
        .add_messages(accrual_msgs)
        .add_attributes(vec![
            attr("action", "deposit"),
            attr("user", info.sender.to_string()),
            attr("amount", deposit_amount.to_string()),
            attr("window_id", window.window_id.to_string()),
        ]))
}

// ─── Withdraw ─────────────────────────────────────────────────────────────

fn execute_withdraw(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    mut amount: Uint128,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let acq_config = ACQUISITION_CONFIG.load(deps.storage)?;
    let window = CURRENT_ACQUISITION_WINDOW.load(deps.storage)?;
    let current_time = env.block.time.seconds();

    let is_during_withdrawal_period = current_time >= window.deposit_end && current_time <= window.withdrawal_end;
    let is_after_withdrawal_period = current_time > window.withdrawal_end;

    if current_time < window.deposit_end {
        return Err(ContractError::NotInWithdrawalPeriod {});
    }

    // Accrue pool before processing
    let (mut state, accrual_msgs) = accrue_pool_internal(
        deps.storage,
        &deps.querier,
        &env,
        &config,
        &acq_config,
    )?;

    // Load user's deposit
    let mut deposit = USER_ACQUISITION_DEPOSITS
        .may_load(deps.storage, (info.sender.to_string(), window.window_id))?
        .ok_or_else(|| ContractError::InvalidFunds {
            reason: "User has no deposit for this acquisition window".into(),
        })?;

    let should_clawback = deposit.disco_deposit_id.is_some();
    let before_cliff = current_time < deposit.vested_at;
    let after_cliff = current_time >= deposit.vested_at;

    if is_during_withdrawal_period {
        // Normal withdrawal during withdrawal period (no rewards sent yet)
    } else if is_after_withdrawal_period {
        if after_cliff {
            return Err(ContractError::Validation(
                format!("Cliff has passed. Claim ownership first, then withdraw through transmuter: {}", config.transmuter_contract),
            ));
        } else {
            // Before cliff but after withdrawal period: force full withdrawal + clawback
            amount = deposit.amount;
        }
    }

    // Clawback: if rewards sent to Disco, request unstake and burn MBRN
    let mut clawback_messages: Vec<CosmosMsg> = vec![];
    if should_clawback && is_after_withdrawal_period && before_cliff {
        let (disco_deposit_id, claimed_amount) = (
            deposit.disco_deposit_id.unwrap(),
            deposit.claimed_mbrn_amount.unwrap(),
        );
        let ltv_disco_addr = config.ltv_disco_contract.clone()
            .ok_or_else(|| ContractError::Validation("ltv_disco_contract not configured".into()))?;

        let disco_asset = deposit.disco_asset.clone()
            .ok_or_else(|| ContractError::Validation("disco_asset not set".into()))?;
        let disco_slot = deposit.disco_slot
            .ok_or_else(|| ContractError::Validation("disco_slot not set".into()))?;

        // Request unstake from Disco
        clawback_messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: ltv_disco_addr.clone(),
            msg: to_json_binary(&LtvDiscoExecuteMsg::RequestUnstake {
                asset: disco_asset,
                slot: disco_slot,
                deposit_id: disco_deposit_id,
                amount: Some(claimed_amount),
            })?,
            funds: vec![],
        }));

        // Burn withdrawn MBRN
        clawback_messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.neutron_proxy.clone(),
            msg: to_json_binary(&NeutronProxyExecuteMsg::BurnTokens {
                denom: config.mbrn_denom.clone(),
                amount: claimed_amount,
                burn_from_address: env.contract.address.to_string(),
            })?,
            funds: vec![],
        }));

        deposit.disco_deposit_id = None;
        deposit.claimed_mbrn_amount = None;
        deposit.disco_asset = None;
        deposit.disco_slot = None;
        deposit.sent_to_disco = None;
    }

    if amount > deposit.amount {
        return Err(ContractError::InvalidFunds {
            reason: format!("Withdrawal amount {} exceeds deposit {}", amount, deposit.amount),
        });
    }

    let exit_vault_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.transmuter_contract.clone(),
        msg: to_json_binary(&TransmuterExecuteMsg::ExitVault {
            recipient: Some(info.sender.to_string()),
            withdraw_as: None,
            user: None, // info.sender = acquisition contract = the depositor
            deposit_id: None,
            amount: Some(amount),
        })?,
        funds: vec![],
    });

    // Update deposit
    if amount >= deposit.amount {
        USER_ACQUISITION_DEPOSITS.remove(deps.storage, (info.sender.to_string(), window.window_id));
    } else {
        deposit.amount = deposit.amount.checked_sub(amount)
            .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
        USER_ACQUISITION_DEPOSITS.save(deps.storage, (info.sender.to_string(), window.window_id), &deposit)?;
    }

    // Update window total
    let mut window = CURRENT_ACQUISITION_WINDOW.load(deps.storage)?;
    window.total_deposit_amount = window.total_deposit_amount.checked_sub(amount)
        .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
    CURRENT_ACQUISITION_WINDOW.save(deps.storage, &window)?;

    // Update total_new_deposits so efficiency reflects net deposits
    state.total_new_deposits = state.total_new_deposits.saturating_sub(amount);
    ACQUISITION_STATE.save(deps.storage, &state)?;

    let mut all_messages = accrual_msgs;
    all_messages.extend(clawback_messages);
    all_messages.push(exit_vault_msg);

    // Acquisition rate assurance callback (runs after transmuter processes ExitVault)
    all_messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&membrane::acquisition::ExecuteMsg::AcquisitionRateAssurance {})?,
        funds: vec![],
    }));

    Ok(Response::new()
        .add_messages(all_messages)
        .add_attributes(vec![
            attr("action", "withdraw"),
            attr("user", info.sender.to_string()),
            attr("amount", amount.to_string()),
            attr("window_id", window.window_id.to_string()),
        ]))
}

// ─── AccruePool (permissionless) ──────────────────────────────────────────

fn execute_accrue_pool(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let acq_config = ACQUISITION_CONFIG.load(deps.storage)?;

    let (_state, msgs) = accrue_pool_internal(
        deps.storage,
        &deps.querier,
        &env,
        &config,
        &acq_config,
    )?;

    Ok(Response::new()
        .add_messages(msgs)
        .add_attribute("action", "accrue_pool"))
}

// ─── Core Accrual Engine ──────────────────────────────────────────────────

/// The core acquisition model engine. Called on every deposit, withdraw, and AccruePool.
/// Returns updated state and any CDP messages to send.
fn accrue_pool_internal(
    storage: &mut dyn Storage,
    querier: &cosmwasm_std::QuerierWrapper,
    env: &Env,
    config: &Config,
    acq_config: &AcquisitionModelConfig,
) -> Result<(AcquisitionModelState, Vec<CosmosMsg>), ContractError> {
    let mut state = match ACQUISITION_STATE.may_load(storage)? {
        Some(s) => s,
        None => return Ok((AcquisitionModelState {
            current_acquisition_rate: acq_config.base_acquisition_rate,
            accrued_pool: Uint128::zero(),
            last_accrual_time: env.block.time.seconds(),
            pool_maxed: false,
            bump_rate: Decimal::zero(),
            last_bump_time: env.block.time.seconds(),
            window_timers_started: false,
            first_deposit_time: None,
            deposit_event_count: 0,
            total_new_deposits: Uint128::zero(),
            last_mutation_efficiency: None,
            last_mutation_time: None,
            efficiency_clamped: false,
        }, vec![])),
    };

    let window = match CURRENT_ACQUISITION_WINDOW.may_load(storage)? {
        Some(w) => w,
        None => {
            ACQUISITION_STATE.save(storage, &state)?;
            return Ok((state, vec![]));
        }
    };

    let current_time = env.block.time.seconds();
    let elapsed = current_time.saturating_sub(state.last_accrual_time);

    if elapsed == 0 {
        return Ok((state, vec![]));
    }

    let mut msgs: Vec<CosmosMsg> = vec![];

    // Query transmuter utilization
    let vault_info: VaultInfoResponse = querier.query_wasm_smart(
        config.transmuter_contract.clone(),
        &TransmuterQueryMsg::VaultInfo {},
    )?;

    let utilization = if vault_info.total_deposit_value.is_zero() {
        Decimal::zero()
    } else {
        // util = 1 - (paired_asset_balance / total_deposit_value)
        let paired_ratio = Decimal::from_ratio(vault_info.paired_asset_balance, vault_info.total_deposit_value);
        if paired_ratio >= Decimal::one() {
            Decimal::zero()
        } else {
            Decimal::one() - paired_ratio
        }
    };

    let above_target = utilization >= acq_config.target_utilization;

    // ── Pool accrual: only if not maxed and util >= target ──
    if !state.pool_maxed && above_target {
        // Time-gated accrual ramp: start at ramp_start_fraction, linearly ramp to 100%
        let effective_rate = if let Some(first_dep) = state.first_deposit_time {
            let time_since_first = current_time.saturating_sub(first_dep);
            if time_since_first < acq_config.accrual_ramp_seconds {
                let ramp_progress = Decimal::from_ratio(
                    Uint128::from(time_since_first),
                    Uint128::from(acq_config.accrual_ramp_seconds),
                );
                let ramp_factor = acq_config.accrual_ramp_start_fraction
                    + decimal_multiplication(
                        ramp_progress,
                        Decimal::one() - acq_config.accrual_ramp_start_fraction,
                    )?;
                decimal_multiplication(state.current_acquisition_rate, ramp_factor)?
            } else {
                state.current_acquisition_rate
            }
        } else {
            // No deposits yet — use start fraction only
            decimal_multiplication(
                state.current_acquisition_rate,
                acq_config.accrual_ramp_start_fraction,
            )?
        };

        let new_mbrn = decimal_multiplication(
            effective_rate,
            Decimal::from_ratio(Uint128::from(elapsed), Uint128::one()),
        )?.to_uint_floor();

        let new_pool = state.accrued_pool.checked_add(new_mbrn)
            .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;

        if new_pool >= acq_config.max_mbrn_emission {
            state.accrued_pool = acq_config.max_mbrn_emission;
            state.pool_maxed = true;
        } else {
            state.accrued_pool = new_pool;
        }
    }

    // ── Bump rate mechanism (only when pool is maxed) ──
    let prev_bump_rate = state.bump_rate;

    if state.pool_maxed {
        let bump_elapsed = current_time.saturating_sub(state.last_bump_time);

        if above_target {
            // Above target: bump increases per bump_interval
            let ticks = bump_elapsed / acq_config.bump_interval_seconds;
            if ticks > 0 {
                for _ in 0..ticks {
                    state.bump_rate = state.bump_rate + acq_config.bump_increment;
                }
                // Clamp to max_bump_rate
                if state.bump_rate > acq_config.max_bump_rate {
                    state.bump_rate = acq_config.max_bump_rate;
                }
                state.last_bump_time += ticks * acq_config.bump_interval_seconds;
            }
        } else {
            // Below target: bump decreases faster (interval / reduction_multiplier)
            let reduction_interval = decimal_division(
                Decimal::from_ratio(Uint128::from(acq_config.bump_interval_seconds), Uint128::one()),
                acq_config.reduction_speed_multiplier,
            )?.to_uint_floor().u128() as u64;

            let reduction_interval = if reduction_interval == 0 { 1 } else { reduction_interval };
            let ticks = bump_elapsed / reduction_interval;
            if ticks > 0 {
                for _ in 0..ticks {
                    if state.bump_rate > acq_config.bump_increment {
                        state.bump_rate = state.bump_rate - acq_config.bump_increment;
                    } else {
                        state.bump_rate = Decimal::zero();
                    }
                }
                state.last_bump_time += ticks * reduction_interval;
            }
        }
    } else {
        // Pool not maxed but below target: bump still decays
        if !above_target && !state.bump_rate.is_zero() {
            let bump_elapsed = current_time.saturating_sub(state.last_bump_time);
            let reduction_interval = decimal_division(
                Decimal::from_ratio(Uint128::from(acq_config.bump_interval_seconds), Uint128::one()),
                acq_config.reduction_speed_multiplier,
            )?.to_uint_floor().u128() as u64;

            let reduction_interval = if reduction_interval == 0 { 1 } else { reduction_interval };
            let ticks = bump_elapsed / reduction_interval;
            if ticks > 0 {
                for _ in 0..ticks {
                    if state.bump_rate > acq_config.bump_increment {
                        state.bump_rate = state.bump_rate - acq_config.bump_increment;
                    } else {
                        state.bump_rate = Decimal::zero();
                    }
                }
                state.last_bump_time += ticks * reduction_interval;
            }
        }
    }

    // Send SetAcquisitionBumpRate to CDP if bump_rate changed
    if state.bump_rate != prev_bump_rate {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.cdp_contract.clone(),
            msg: to_json_binary(&CdpExecuteMsg::SetAcquisitionBumpRate {
                rate: state.bump_rate,
            })?,
            funds: vec![],
        }));
    }

    // ── Efficiency control system ──
    // Only mutate after day 1 of deposit period, and only if we have accrued something
    if state.window_timers_started && !state.accrued_pool.is_zero() {
        if let Some(first_deposit) = state.first_deposit_time {
            let day_1_end = first_deposit + SECONDS_PER_DAY;
            if current_time >= day_1_end {
                // Efficiency = total_deposits / accrued_pool (higher = better)
                let current_efficiency = Decimal::from_ratio(
                    state.total_new_deposits,
                    state.accrued_pool,
                );

                if let Some(last_efficiency) = state.last_mutation_efficiency {
                    if !last_efficiency.is_zero() {
                        if current_efficiency >= last_efficiency {
                            // IMPROVING: No cooldown, always apply rate decrease
                            let delta = decimal_division(
                                current_efficiency - last_efficiency,
                                last_efficiency,
                            )?;
                            let clamped_delta = if delta > acq_config.max_rate_change_per_mutation {
                                acq_config.max_rate_change_per_mutation
                            } else {
                                delta
                            };
                            let rate_change = decimal_multiplication(
                                clamped_delta,
                                state.current_acquisition_rate,
                            )?;
                            if state.current_acquisition_rate > rate_change {
                                state.current_acquisition_rate = state.current_acquisition_rate - rate_change;
                            } else {
                                state.current_acquisition_rate = Decimal::zero();
                            }

                            // Clamp rate to [0, max_emission / deposit_period_seconds]
                            let deposit_period_seconds = window.deposit_period_days * SECONDS_PER_DAY;
                            if deposit_period_seconds > 0 {
                                let max_rate = Decimal::from_ratio(
                                    acq_config.max_mbrn_emission,
                                    Uint128::from(deposit_period_seconds),
                                );
                                if state.current_acquisition_rate > max_rate {
                                    state.current_acquisition_rate = max_rate;
                                }
                            }

                            // Update mutation tracking
                            state.last_mutation_efficiency = Some(current_efficiency);
                            state.last_mutation_time = Some(current_time);
                        } else {
                            // WORSENING: Check cooldown before applying
                            let cooldown_passed = match state.last_mutation_time {
                                Some(last_time) => current_time >= last_time + acq_config.mutation_cooldown_seconds,
                                None => true,
                            };

                            if cooldown_passed {
                                let neg_delta = decimal_division(
                                    last_efficiency - current_efficiency,
                                    last_efficiency,
                                )?;
                                let clamped_delta = if neg_delta > acq_config.max_rate_change_per_mutation {
                                    acq_config.max_rate_change_per_mutation
                                } else {
                                    neg_delta
                                };
                                let rate_change = decimal_multiplication(
                                    clamped_delta,
                                    state.current_acquisition_rate,
                                )?;
                                state.current_acquisition_rate = state.current_acquisition_rate + rate_change;

                                // Clamp rate to [0, max_emission / deposit_period_seconds]
                                let deposit_period_seconds = window.deposit_period_days * SECONDS_PER_DAY;
                                if deposit_period_seconds > 0 {
                                    let max_rate = Decimal::from_ratio(
                                        acq_config.max_mbrn_emission,
                                        Uint128::from(deposit_period_seconds),
                                    );
                                    if state.current_acquisition_rate > max_rate {
                                        state.current_acquisition_rate = max_rate;
                                    }
                                }

                                // Update mutation tracking
                                state.last_mutation_efficiency = Some(current_efficiency);
                                state.last_mutation_time = Some(current_time);
                            }
                            // If cooldown NOT passed: skip entirely, don't update tracking fields
                        }
                    }
                } else {
                    // First mutation: record baseline efficiency, no rate change
                    state.last_mutation_efficiency = Some(current_efficiency);
                    state.last_mutation_time = Some(current_time);
                }
            }
        }
    }

    state.last_accrual_time = current_time;
    ACQUISITION_STATE.save(storage, &state)?;

    // Record history snapshot
    let window_id = CURRENT_WINDOW_ID.may_load(storage)?.unwrap_or(0);
    append_acquisition_history(storage, AcquisitionHistoryEntry {
        window_id,
        timestamp: current_time,
        current_acquisition_rate: state.current_acquisition_rate,
        bump_rate: state.bump_rate,
        accrued_pool: state.accrued_pool,
        pool_maxed: state.pool_maxed,
        utilization,
        efficiency: state.last_mutation_efficiency,
        total_new_deposits: state.total_new_deposits,
    })?;

    Ok((state, msgs))
}

// ─── Efficiency Clamp (applied once at withdrawal end) ────────────────────

/// Apply efficiency clamp if not yet done and withdrawal period has ended.
/// Reduces accrued_pool if MBRN/deposit ratio exceeds baseline * (1 + threshold).
fn apply_efficiency_clamp(
    storage: &mut dyn Storage,
    env: &Env,
    acq_config: &AcquisitionModelConfig,
) -> Result<(), ContractError> {
    let mut state = ACQUISITION_STATE.load(storage)?;
    let mut window = CURRENT_ACQUISITION_WINDOW.load(storage)?;
    let current_time = env.block.time.seconds();

    if state.efficiency_clamped || current_time < window.withdrawal_end {
        return Ok(());
    }

    if !state.total_new_deposits.is_zero() && !state.accrued_pool.is_zero() {
        // realized_ratio = accrued_pool / total_new_deposits (MBRN per deposit)
        let realized_ratio = Decimal::from_ratio(state.accrued_pool, state.total_new_deposits);

        // baseline_pool = base_rate * deposit_period_seconds
        let deposit_period_seconds = window.deposit_period_days * SECONDS_PER_DAY;
        let baseline_pool = decimal_multiplication(
            acq_config.base_acquisition_rate,
            Decimal::from_ratio(Uint128::from(deposit_period_seconds), Uint128::one()),
        )?.to_uint_floor();

        if !baseline_pool.is_zero() {
            let baseline_ratio = Decimal::from_ratio(baseline_pool, state.total_new_deposits);
            let max_acceptable = decimal_multiplication(
                baseline_ratio,
                Decimal::one() + acq_config.efficiency_threshold,
            )?;

            if realized_ratio > max_acceptable {
                // Reduce pool: accrued_pool = total_new_deposits * max_acceptable
                state.accrued_pool = decimal_multiplication(
                    Decimal::from_ratio(state.total_new_deposits, Uint128::one()),
                    max_acceptable,
                )?.to_uint_floor();
            }
        }
    }

    // Set acquisition_budget on window
    window.acquisition_budget = state.accrued_pool;
    state.efficiency_clamped = true;

    ACQUISITION_STATE.save(storage, &state)?;
    CURRENT_ACQUISITION_WINDOW.save(storage, &window)?;

    Ok(())
}

// ─── Claim (post-cliff) ──────────────────────────────────────────────────

fn execute_claim(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    window_id: u64,
    mbrn_intent: Option<MbrnClaimIntent>,
) -> Result<Response, ContractError> {
    let user = info.sender.to_string();
    execute_transfer_deposit_ownership(deps, env, info, user, window_id, mbrn_intent)
}

fn execute_claim_for_user(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    user: String,
    window_id: u64,
    mbrn_intent: Option<MbrnClaimIntent>,
) -> Result<Response, ContractError> {
    execute_transfer_deposit_ownership(deps, env, info, user, window_id, mbrn_intent)
}

fn execute_transfer_deposit_ownership(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    user: String,
    window_id: u64,
    _mbrn_intent: Option<MbrnClaimIntent>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let acq_config = ACQUISITION_CONFIG.load(deps.storage)?;

    // Apply efficiency clamp if needed
    apply_efficiency_clamp(deps.storage, &env, &acq_config)?;

    let window = CURRENT_ACQUISITION_WINDOW.load(deps.storage)?;

    // Load user's deposit
    let deposit = USER_ACQUISITION_DEPOSITS
        .may_load(deps.storage, (user.clone(), window_id))?
        .ok_or_else(|| ContractError::Validation(
            format!("No deposit found for user {} in window {}", user, window_id),
        ))?;

    // Check if cliff has passed
    let current_time = env.block.time.seconds();
    if current_time < deposit.vested_at {
        return Err(ContractError::Validation(
            format!("Cliff has not passed yet. Vested at: {}", deposit.vested_at),
        ));
    }

    let mut messages: Vec<CosmosMsg> = vec![];

    // Transfer transmuter deposit ownership to user
    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.transmuter_contract.clone(),
        msg: to_json_binary(&TransmuterExecuteMsg::TransferDepositOwnership {
            user: env.contract.address.to_string(),
            amount: deposit.amount,
            new_owner: user.clone(),
        })?,
        funds: vec![],
    }));

    // Handle Disco deposit transfer (if sent to Disco)
    if deposit.sent_to_disco == Some(true) {
        if let Some(disco_deposit_id) = deposit.disco_deposit_id {
            let ltv_disco_addr = config.ltv_disco_contract.clone()
                .ok_or_else(|| ContractError::Validation("ltv_disco_contract not configured".into()))?;
            let disco_asset = deposit.disco_asset.clone()
                .ok_or_else(|| ContractError::Validation("disco_asset not set".into()))?;
            let disco_slot = deposit.disco_slot
                .ok_or_else(|| ContractError::Validation("disco_slot not set".into()))?;

            messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: ltv_disco_addr,
                msg: to_json_binary(&LtvDiscoExecuteMsg::UpdateDeposit {
                    asset: disco_asset,
                    slot: disco_slot,
                    deposit_id: disco_deposit_id,
                    deposit_owner: Some(user.clone()),
                    manager: None,
                    revenue_destination: None,
                })?,
                funds: vec![],
            }));
        }
    } else {
        // Direct route: mint MBRN to user at claim time
        // Calculate pro-rata share
        if !window.acquisition_budget.is_zero() && !window.total_deposit_amount.is_zero() {
            let claim_amount = decimal_multiplication(
                Decimal::from_ratio(window.acquisition_budget, Uint128::one()),
                Decimal::from_ratio(deposit.amount, window.total_deposit_amount),
            )?.to_uint_floor();

            if !claim_amount.is_zero() {
                // DISABLED: GetBoost query removed from system
                // let boost_response: UserBoostResponse = deps.querier.query_wasm_smart(
                //     config.discounts_contract.clone(),
                //     &DiscountQueryMsg::UserBoost { user: user.clone() },
                // )?;
                // let discount_config: DiscountConfig = deps.querier.query_wasm_smart(
                //     config.discounts_contract.clone(),
                //     &DiscountQueryMsg::Config {},
                // )?;
                // let boost_multiplier = Decimal::one() + min(boost_response.boost, discount_config.max_boost);
                let boost_multiplier = Decimal::one();
                let boosted_amount = decimal_multiplication(
                    Decimal::from_ratio(claim_amount, Uint128::one()),
                    boost_multiplier,
                )?.to_uint_floor();

                if !boosted_amount.is_zero() {
                    // Mint MBRN to user
                    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: config.neutron_proxy.clone(),
                        msg: to_json_binary(&NeutronProxyExecuteMsg::MintTokens {
                            denom: config.mbrn_denom.clone(),
                            amount: boosted_amount,
                            mint_to_address: user.clone(),
                        })?,
                        funds: vec![],
                    }));
                }
            }
        }
    }

    // Remove deposit
    USER_ACQUISITION_DEPOSITS.remove(deps.storage, (user.clone(), window_id));

    Ok(Response::new()
        .add_messages(messages)
        .add_attributes(vec![
            attr("action", "claim"),
            attr("user", user),
            attr("window_id", window_id.to_string()),
        ]))
}

// ─── SendAcquisitionRewardsToDisco ────────────────────────────────────────

fn execute_send_acquisition_rewards_to_disco(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    window_id: u64,
    mbrn_intent: Option<MbrnClaimIntent>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let acq_config = ACQUISITION_CONFIG.load(deps.storage)?;
    let user = info.sender.to_string();

    // Apply efficiency clamp if needed
    apply_efficiency_clamp(deps.storage, &env, &acq_config)?;

    let window = CURRENT_ACQUISITION_WINDOW.load(deps.storage)?;
    if window.window_id != window_id {
        return Err(ContractError::Validation(
            format!("Window ID mismatch. Expected {}, got {}", window.window_id, window_id),
        ));
    }

    // Validate window is finished
    let current_time = env.block.time.seconds();
    if current_time < window.withdrawal_end {
        return Err(ContractError::Validation(
            format!("Window is not finished yet. Withdrawal ends at: {}", window.withdrawal_end),
        ));
    }

    // Load user's deposit
    let mut deposit = USER_ACQUISITION_DEPOSITS
        .may_load(deps.storage, (user.clone(), window_id))?
        .ok_or_else(|| ContractError::Validation(
            format!("No deposit found for user {} in window {}", user, window_id),
        ))?;

    // Use the deposit amount tracked in the acquisition contract
    let transmuter_deposit_amount = deposit.amount;

    // Calculate claim amount using acquisition_budget (set by efficiency clamp)
    let claim_amount = if window.total_deposit_amount.is_zero() {
        Uint128::zero()
    } else {
        decimal_multiplication(
            Decimal::from_ratio(window.acquisition_budget, Uint128::one()),
            Decimal::from_ratio(transmuter_deposit_amount, window.total_deposit_amount),
        )?.to_uint_floor()
    };

    if claim_amount.is_zero() {
        return Err(ContractError::Validation("Claim amount is zero".into()));
    }

    // DISABLED: GetBoost query removed from system
    // let boost_response: UserBoostResponse = deps.querier.query_wasm_smart(
    //     config.discounts_contract.clone(),
    //     &DiscountQueryMsg::UserBoost { user: user.clone() },
    // )?;
    // let discount_config: DiscountConfig = deps.querier.query_wasm_smart(
    //     config.discounts_contract.clone(),
    //     &DiscountQueryMsg::Config {},
    // )?;
    // let max_boost = discount_config.max_boost;
    let _max_boost = Decimal::zero();

    // Find Disco intent - REQUIRED
    let disco_intent = mbrn_intent
        .as_ref()
        .and_then(|intent| {
            if intent.apply_now {
                intent.intents.iter().find(|i| matches!(i.intent_type, MbrnIntentType::DepositViaMarsMirror { .. })).cloned()
            } else {
                None
            }
        })
        .or_else(|| {
            USER_INTENTS.may_load(deps.storage, user.clone())
                .ok()
                .flatten()
                .and_then(|intents| intents.iter().find(|i| matches!(i.intent_type, MbrnIntentType::DepositViaMarsMirror { .. })).cloned())
        })
        .ok_or_else(|| ContractError::Validation(
            "DepositViaMarsMirror intent required for send_acquisition_rewards_to_disco".into(),
        ))?;

    let (asset, slot) = match &disco_intent.intent_type {
        MbrnIntentType::DepositViaMarsMirror { asset, slot } => {
            (asset.clone(), *slot)
        }
        _ => return Err(ContractError::Validation("Invalid intent type".into())),
    };

    // DISABLED: Intent boosts disabled
    // let intent_boosts: Vec<Decimal> = if !config.discounts_contract.is_empty() {
    //     match deps.querier.query_wasm_smart::<IntentBoostsResponse>(
    //         config.discounts_contract.clone(),
    //         &DiscountQueryMsg::IntentBoosts {
    //             intents: vec![disco_intent.clone()],
    //         },
    //     ) {
    //         Ok(response) => response.boosts,
    //         Err(_) => vec![Decimal::zero()],
    //     }
    // } else {
    //     vec![Decimal::zero()]
    // };
    //
    // let intent_boost = intent_boosts.get(0).copied().unwrap_or(Decimal::zero());
    // let total_boost = min(Decimal::zero() + intent_boost, max_boost);
    // let boost_multiplier = Decimal::one() + total_boost;
    let boost_multiplier = Decimal::one();

    let base_amount = decimal_multiplication(
        Decimal::from_ratio(claim_amount, Uint128::one()),
        disco_intent.ratio,
    )?.to_uint_floor();

    let boosted_amount = decimal_multiplication(
        Decimal::from_ratio(base_amount, Uint128::one()),
        boost_multiplier,
    )?.to_uint_floor();

    // Mint MBRN to contract
    let contract_addr = env.contract.address.clone();
    let mut messages: Vec<CosmosMsg> = vec![];

    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.neutron_proxy.clone(),
        msg: to_json_binary(&NeutronProxyExecuteMsg::MintTokens {
            denom: config.mbrn_denom.clone(),
            amount: boosted_amount,
            mint_to_address: contract_addr.to_string(),
        })?,
        funds: vec![],
    }));

    // Create deposit input
    let deposit_input = BackingDepositInput {
        asset: asset.clone(),
        slot,
    };

    let ltv_disco_addr = config.ltv_disco_contract.clone()
        .ok_or_else(|| ContractError::Validation("ltv_disco_contract not configured".into()))?;

    let ltv_queue_response: AssetQueueResponse = deps.querier.query_wasm_smart(
        ltv_disco_addr.clone(),
        &LtvDiscoQueryMsg::GetAssetQueue {
            assets: vec![asset.clone()],
            limit: None,
            start_after: None,
        },
    )?;

    let next_deposit_id = ltv_queue_response.queues.first()
        .map(|(_, queue)| queue.current_deposit_id)
        .unwrap_or(Uint128::zero());

    // Submit to Disco: contract owns, user manages + receives revenue
    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: ltv_disco_addr.clone(),
        msg: to_json_binary(&LtvDiscoExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: Some(env.contract.address.to_string()),
            deposit_id: None,
            manager: Some(user.clone()),
            affiliate_address: None,
            revenue_destination: Some(user.clone()),
        })?,
        funds: vec![coin(boosted_amount.u128(), config.mbrn_denom.clone())],
    }));

    // Store disco info for clawback + mark as sent_to_disco
    deposit.sent_to_disco = Some(true);
    deposit.mbrn_claim_amount = Some(claim_amount);
    deposit.disco_deposit_id = Some(next_deposit_id);
    deposit.disco_asset = Some(asset.clone());
    deposit.disco_slot = Some(slot);
    deposit.claimed_mbrn_amount = Some(boosted_amount);

    USER_ACQUISITION_DEPOSITS.save(deps.storage, (user.clone(), window_id), &deposit)?;

    Ok(Response::new()
        .add_messages(messages)
        .add_attributes(vec![
            attr("action", "send_acquisition_rewards_to_disco"),
            attr("user", user),
            attr("window_id", window_id.to_string()),
            attr("claim_amount", claim_amount.to_string()),
            attr("boosted_amount", boosted_amount.to_string()),
            attr("disco_asset", asset),
        ]))
}

// ─── UpdateConfig ─────────────────────────────────────────────────────────

fn execute_update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    transmuter_contract: Option<String>,
    neutron_proxy: Option<String>,
    cdp_contract: Option<String>,
    deposit_period_days: Option<u64>,
    withdrawal_period_days: Option<u64>,
    deposit_token: Option<String>,
    minimum_deposit: Option<Uint128>,
    mbrn_denom: Option<String>,
    staking_contract: Option<String>,
    mars_mirror_contract: Option<String>,
    ltv_disco_contract: Option<String>,
    discounts_contract: Option<String>,
    emissions_voting_contract: Option<String>,
    cliff_period_days: Option<u64>,
    acquisition_config: Option<AcquisitionModelConfig>,
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
    if let Some(cdp) = cdp_contract {
        let _ = deps.api.addr_validate(&cdp)?;
        config.cdp_contract = cdp;
    }
    if let Some(days) = deposit_period_days {
        if days == 0 {
            return Err(ContractError::Validation("deposit_period_days must be > 0".into()));
        }
        config.deposit_period_days = days;
    }
    if let Some(days) = withdrawal_period_days {
        if days == 0 {
            return Err(ContractError::Validation("withdrawal_period_days must be > 0".into()));
        }
        config.withdrawal_period_days = days;
    }
    if let Some(token) = deposit_token {
        config.deposit_token = token;
    }
    if let Some(min) = minimum_deposit {
        if min.is_zero() {
            return Err(ContractError::Validation("minimum_deposit must be > 0".into()));
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
    if let Some(emissions_voting) = emissions_voting_contract {
        let _ = deps.api.addr_validate(&emissions_voting)?;
        config.emissions_voting_contract = Some(emissions_voting);
    }
    if let Some(cliff) = cliff_period_days {
        if cliff == 0 {
            return Err(ContractError::Validation("cliff_period_days must be > 0".into()));
        }
        config.cliff_period_seconds = cliff * SECONDS_PER_DAY;
    }

    CONFIG.save(deps.storage, &config)?;

    // Update acquisition config if provided
    if let Some(acq_cfg) = acquisition_config {
        ACQUISITION_CONFIG.save(deps.storage, &acq_cfg)?;
    }

    Ok(Response::new().add_attribute("action", "update_config"))
}

// ─── Acquisition Rate Assurance ───────────────────────────────────────────

fn execute_acquisition_rate_assurance(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    // Only callable by self (callback pattern)
    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized {});
    }

    let config = CONFIG.load(deps.storage)?;
    let window = CURRENT_ACQUISITION_WINDOW.load(deps.storage)?;

    // Query transmuter for ACQ_DT — must match our total exactly (within 1 for rounding)
    let response: membrane::transmuter::AcquisitionDepositTotalResponse =
        deps.querier.query_wasm_smart(
            config.transmuter_contract.clone(),
            &membrane::transmuter::QueryMsg::AcquisitionDepositTotal {},
        )?;

    let expected = window.total_deposit_amount;
    if response.total > expected.checked_add(Uint128::one()).unwrap_or(expected)
        || response.total < expected.saturating_sub(Uint128::one())
    {
        return Err(ContractError::CustomError {
            val: format!(
                "ACQ rate assurance failed — possible exploit. Transmuter ACQ_DT: {}, Expected: {}",
                response.total, expected
            ),
        });
    }
    Ok(Response::new().add_attribute("action", "acquisition_rate_assurance"))
}

// ─── Queries ──────────────────────────────────────────────────────────────

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&ConfigResponse {
            config: CONFIG.load(deps.storage)?,
        }),
        QueryMsg::AcquisitionConfig {} => to_json_binary(&AcquisitionConfigResponse {
            config: ACQUISITION_CONFIG.load(deps.storage)?,
        }),
        QueryMsg::CurrentAcquisitionWindow {} => {
            let window = CURRENT_ACQUISITION_WINDOW.may_load(deps.storage)?;
            to_json_binary(&CurrentAcquisitionWindowResponse { window })
        }
        QueryMsg::ActiveAcquisitionWindow {} => {
            let window = CURRENT_ACQUISITION_WINDOW.may_load(deps.storage)?;
            let active_window = if let Some(ref w) = window {
                let current_time = env.block.time.seconds();
                if current_time < w.withdrawal_end {
                    Some(w.clone())
                } else {
                    None
                }
            } else {
                None
            };
            to_json_binary(&ActiveAcquisitionWindowResponse { window: active_window })
        }
        QueryMsg::UserAcquisitionDeposit { user, window_id } => {
            let deposit = USER_ACQUISITION_DEPOSITS.may_load(deps.storage, (user, window_id))?;
            to_json_binary(&UserAcquisitionDepositResponse { deposit })
        }
        QueryMsg::AcquisitionModelState {} => {
            let state = ACQUISITION_STATE.may_load(deps.storage)?;
            to_json_binary(&AcquisitionModelStateResponse { state })
        }
        QueryMsg::AcquisitionHistory { window_id } => {
            let all = ACQUISITION_HISTORY.may_load(deps.storage)?.unwrap_or_default();
            let history = match window_id {
                Some(wid) => all.into_iter().filter(|e| e.window_id == wid).collect(),
                None => all,
            };
            to_json_binary(&AcquisitionHistoryResponse { history })
        }
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────

fn ensure_owner(config: &Config, sender: &Addr) -> Result<(), ContractError> {
    if &config.owner != sender {
        return Err(ContractError::Unauthorized {});
    }
    Ok(())
}

fn validate_intent_ratios(intents: &[MbrnIntentOption]) -> Result<(), ContractError> {
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
    for intent in intents {
        if intent.ratio > Decimal::one() || intent.ratio < Decimal::zero() {
            return Err(ContractError::Validation(
                "Intent ratios must be between 0 and 1".into(),
            ));
        }
    }
    Ok(())
}
