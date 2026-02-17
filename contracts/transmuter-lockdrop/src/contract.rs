use std::cmp::min;

use cosmwasm_std::{
    attr, coin, entry_point, to_json_binary, Addr, BankMsg, Binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, Response, StdError, StdResult, Storage, Timestamp, Uint128, WasmMsg,
};
use cw2::set_contract_version;

use membrane::transmuter_lockdrop::{
    Config, ExecuteMsg, InstantiateMsg, QueryMsg, AcquisitionWindow, AcquisitionDeposit,
    ConfigResponse, CurrentAcquisitionWindowResponse, ActiveAcquisitionWindowResponse,
    UserAcquisitionDepositResponse, MbrnClaimIntent, MbrnIntentOption, MbrnIntentType,
};
use membrane::transmuter::ExecuteMsg as TransmuterExecuteMsg;
use membrane::neutron_proxy::ExecuteMsg as NeutronProxyExecuteMsg;
use membrane::math::decimal_multiplication;
use membrane::staking::ExecuteMsg as StakingExecuteMsg;
use membrane::ltv_disco::{ExecuteMsg as LtvDiscoExecuteMsg, QueryMsg as LtvDiscoQueryMsg, BackingDepositInput, LTVQueueResponse};
use membrane::system_discounts::{QueryMsg as DiscountQueryMsg, IntentBoostsResponse, UserBoostResponse, Config as DiscountConfig};

use crate::error::ContractError;
use crate::state::{CONFIG, CURRENT_WINDOW_ID, CURRENT_ACQUISITION_WINDOW, USER_ACQUISITION_DEPOSITS, USER_INTENTS};

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
    
    // Validate cliff_period_seconds
    if msg.cliff_period_days == 0 {
        return Err(ContractError::Validation(
            "cliff_period_days must be greater than zero".into(),
        ));
    }
    
    let config = Config {
        owner,
        transmuter_contract: msg.transmuter_contract,
        neutron_proxy: msg.neutron_proxy,
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
    
    // Initialize window ID counter
    CURRENT_WINDOW_ID.save(deps.storage, &0u64)?;
    
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
        ExecuteMsg::StartAcquisitionWindow {  } => {
            execute_start_acquisition_window(deps, env, info)
        }
        ExecuteMsg::Deposit { intents } => execute_deposit(deps, env, info, intents),
        ExecuteMsg::Withdraw { amount } => execute_withdraw(deps, env, info, amount),
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
        } => execute_update_config(
            deps,
            info,
            owner,
            transmuter_contract,
            neutron_proxy,
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
        ),
    }
}

fn execute_start_acquisition_window(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    
    // Only owner can start acquisition window
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
    
    // Use provided periods or config defaults
    let deposit_period = config.deposit_period_days;
    let withdrawal_period = config.withdrawal_period_days;
    
    // Get next window ID
    let window_id = CURRENT_WINDOW_ID.may_load(deps.storage)?
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| ContractError::Std(StdError::generic_err("Window ID overflow")))?;
    CURRENT_WINDOW_ID.save(deps.storage, &window_id)?;
    
    // Calculate budget from emissions-voting contract
    let acquisition_budget = if let Some(ref emissions_voting) = config.emissions_voting_contract {
        // Query total emissions
        let total_emissions_response: membrane::emissions_voting::CurrentResultResponse = deps.querier.query_wasm_smart(
            emissions_voting.clone(),
            &membrane::emissions_voting::QueryMsg::CurrentResult {
                label: membrane::transmuter::TOTAL_EMISSIONS_GRAPH_LABEL.to_string(),
            },
        )?;
        
        // Query acquisition percentage
        let acquisition_pct_response: membrane::emissions_voting::CurrentResultResponse = deps.querier.query_wasm_smart(
            emissions_voting.clone(),
            &membrane::emissions_voting::QueryMsg::CurrentResult {
                label: membrane::transmuter::ACQUISITION_PERCENTAGE_GRAPH_LABEL.to_string(),
            },
        )?;
        
        let total_emissions = total_emissions_response.result_uint128
            .ok_or_else(|| ContractError::Std(StdError::generic_err("Total emissions not found")))?;
        let acquisition_pct = acquisition_pct_response.result_decimal
            .ok_or_else(|| ContractError::Std(StdError::generic_err("Acquisition percentage not found")))?;
        
        // Budget = total_emissions * acquisition_pct
        decimal_multiplication(
            Decimal::from_ratio(total_emissions, Uint128::one()),
            acquisition_pct,
        )?.to_uint_floor()
    } else {
        Uint128::zero()
    };
    
    let current_time = env.block.time.seconds();
    let deposit_end = current_time + (deposit_period * SECONDS_PER_DAY);
    let withdrawal_end = deposit_end + (withdrawal_period * SECONDS_PER_DAY);
    
    let window = AcquisitionWindow {
        window_id,
        start_time: current_time,
        deposit_end,
        withdrawal_end,
        deposit_period_days: deposit_period,
        total_deposit_amount: Uint128::zero(),
        acquisition_budget,
    };
    
    CURRENT_ACQUISITION_WINDOW.save(deps.storage, &window)?;
    
    Ok(Response::new()
        .add_attributes(vec![
            attr("action", "start_acquisition_window"),
            attr("window_id", window_id.to_string()),
            attr("acquisition_budget", acquisition_budget.to_string()),
        ]))
}

fn execute_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    intents: Option<Vec<MbrnIntentOption>>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let window = CURRENT_ACQUISITION_WINDOW.load(deps.storage)?;
    
    // Check if in deposit period
    let current_time = env.block.time.seconds();
    if current_time > window.deposit_end {
        return Err(ContractError::DepositPeriodEnded {});
    }
    
    //Assert only the deposit token is sent
    if info.funds.len() != 1 || info.funds[0].denom != config.deposit_token {
        return Err(ContractError::InvalidFunds {
            reason: format!("Expected {} deposit", config.deposit_token),
        });
    }

    // Get deposit amount from funds
    let deposit_amount = info.funds[0].amount;
    
    // Validate minimum deposit
    if deposit_amount < config.minimum_deposit {
        return Err(ContractError::Validation(
            format!("Deposit amount {} is below minimum {}", deposit_amount, config.minimum_deposit),
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
    
    // Forward funds to transmuter EnterVault (no lock) - deposit owned by contract
    let contract_addr = env.contract.address.clone();
    
    // Query transmuter for current deposit ID (will be assigned to the new deposit)
    // The transmuter will auto-consolidate deposits within the acquisition deposit window
    let current_deposit_id_response: membrane::transmuter::CurrentDepositIdResponse = deps.querier.query_wasm_smart(
        config.transmuter_contract.clone(),
        &membrane::transmuter::QueryMsg::CurrentDepositId {
            user: contract_addr.to_string(),
        },
    )?;
    
    // The next deposit ID that will be assigned
    let next_deposit_id = current_deposit_id_response.deposit_id;
    
    // Check if user already has a deposit entry for current acquisition window
    let existing_deposit = USER_ACQUISITION_DEPOSITS.may_load(deps.storage, (info.sender.to_string(), window.window_id))?;
    
    if let Some(mut deposit) = existing_deposit {
        // Update existing entry - add new deposit amount to total amount (keep same deposit_id)
        deposit.amount = deposit.amount.checked_add(deposit_amount)
            .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
        USER_ACQUISITION_DEPOSITS.save(deps.storage, (info.sender.to_string(), window.window_id), &deposit)?;
    } else {
        // Use the next deposit ID that will be assigned
        let deposit_id = next_deposit_id;
        let vested_at = current_time + config.cliff_period_seconds;
        
        // Create new entry
        let new_deposit = AcquisitionDeposit {
            deposit_id,
            amount: deposit_amount,
            deposit_time: current_time,
            vested_at,
            disco_deposit_id: None,
            claimed_mbrn_amount: None,
            disco_asset: None,
            disco_ltv: None,
            disco_max_borrow_ltv: None,
            disco_epoch_start_time: None,
        };
        USER_ACQUISITION_DEPOSITS.save(deps.storage, (info.sender.to_string(), window.window_id), &new_deposit)?;
    }
    
    let enter_vault_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.transmuter_contract.clone(),
        msg: to_json_binary(&TransmuterExecuteMsg::EnterVault {
            recipient: Some(contract_addr.to_string()),
            lock_days: None,
            affiliate_address: None,
        })?,
        funds: vec![coin(deposit_amount.u128(), config.deposit_token.clone())],
    });
    
    // Update total_deposit_amount for current acquisition window
    let mut window = CURRENT_ACQUISITION_WINDOW.load(deps.storage)?;
    window.total_deposit_amount = window.total_deposit_amount.checked_add(deposit_amount)
        .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
    CURRENT_ACQUISITION_WINDOW.save(deps.storage, &window)?;
    
    Ok(Response::new()
        .add_message(enter_vault_msg)
        .add_attributes(vec![
            attr("action", "deposit"),
            attr("user", info.sender.to_string()),
            attr("amount", deposit_amount.to_string()),
            attr("window_id", window.window_id.to_string()),
        ]))
}

fn execute_withdraw(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    mut amount: Uint128,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let window = CURRENT_ACQUISITION_WINDOW.load(deps.storage)?;
    
    // Check timing
    let current_time = env.block.time.seconds();
    let is_during_withdrawal_period = current_time >= window.deposit_end && current_time <= window.withdrawal_end;
    let is_after_withdrawal_period = current_time > window.withdrawal_end;
    
    if current_time < window.deposit_end {
        return Err(ContractError::NotInWithdrawalPeriod {});
    }
    
    // Load user's acquisition deposit
    let mut deposit = USER_ACQUISITION_DEPOSITS
        .may_load(deps.storage, (info.sender.to_string(), window.window_id))?
        .ok_or_else(|| ContractError::InvalidFunds {
            reason: "User has no deposit for this acquisition window".into(),
        })?;
    
    // Determine if we should do clawback (rewards exist)
    let should_clawback = deposit.disco_deposit_id.is_some();
    let before_cliff = current_time < deposit.vested_at;
    let after_cliff = current_time >= deposit.vested_at;
    
    // Handle different time periods
    if is_during_withdrawal_period {
        // During withdrawal period: withdraw normally (no rewards yet, so no clawback)
        // No clawback during withdrawal period (rewards haven't been sent yet)
    } else if is_after_withdrawal_period {
        // After withdrawal period
        if after_cliff {
            // After withdrawal period and after cliff: error and tell user to claim
            return Err(ContractError::Validation(
                format!("Cliff has passed. User must claim ownership of the deposit first & then withdraw through the transmuter contract: {}", config.transmuter_contract),
            ));
        } else {
            // After withdrawal period but before cliff: withdraw everything & clawback everything
            amount = deposit.amount;
        }
    }
    
    // Clawback logic: if rewards were sent to Disco, withdraw and burn MBRN
    // This only applies after withdrawal period (before cliff)
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
        let disco_ltv = deposit.disco_ltv
            .ok_or_else(|| ContractError::Validation("disco_ltv not set".into()))?;
        let disco_max_borrow_ltv = deposit.disco_max_borrow_ltv
            .ok_or_else(|| ContractError::Validation("disco_max_borrow_ltv not set".into()))?;
        let disco_epoch_start_time = deposit.disco_epoch_start_time
            .ok_or_else(|| ContractError::Validation("disco_epoch_start_time not set".into()))?;
        
        // Withdraw from Disco
        clawback_messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: ltv_disco_addr.clone(),
            msg: to_json_binary(&LtvDiscoExecuteMsg::WithdrawDeposit {
                asset: disco_asset,
                ltv: disco_ltv,
                max_borrow_ltv: disco_max_borrow_ltv,
                deposit_id: disco_deposit_id,
                amount: Some(claimed_amount),
                epoch_start_time: disco_epoch_start_time,
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
        
        // Clear disco deposit tracking
        deposit.disco_deposit_id = None;
        deposit.claimed_mbrn_amount = None;
        deposit.disco_asset = None;
        deposit.disco_ltv = None;
        deposit.disco_max_borrow_ltv = None;
        deposit.disco_epoch_start_time = None;
    }
    
    // Validate withdrawal amount
    if amount > deposit.amount {
        return Err(ContractError::InvalidFunds {
            reason: format!("Withdrawal amount {} exceeds deposit amount {}", amount, deposit.amount),
        });
    }
    
    // Call transmuter ExitVault for the specified amount and deposit ID
    let exit_vault_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.transmuter_contract.clone(),
        msg: to_json_binary(&TransmuterExecuteMsg::ExitVault {
            recipient: Some(info.sender.to_string()),
            withdraw_as: None,
            user: Some(env.contract.address.to_string()), // Deposit is owned by the contract
            deposit_id: Some(deposit.deposit_id),
            amount: Some(amount),
        })?,
        funds: vec![],
    });
    
    // Update USER_ACQUISITION_DEPOSITS
    if amount >= deposit.amount {
        // Full withdrawal - remove entry
        USER_ACQUISITION_DEPOSITS.remove(deps.storage, (info.sender.to_string(), window.window_id));
    } else {
        // Partial withdrawal - update amount
        deposit.amount = deposit.amount.checked_sub(amount)
            .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
        USER_ACQUISITION_DEPOSITS.save(deps.storage, (info.sender.to_string(), window.window_id), &deposit)?;
    }
    
    // Update total_deposit_amount for current acquisition window
    let mut window = CURRENT_ACQUISITION_WINDOW.load(deps.storage)?;
    window.total_deposit_amount = window.total_deposit_amount.checked_sub(amount)
        .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
    CURRENT_ACQUISITION_WINDOW.save(deps.storage, &window)?;
    
    let mut all_messages = clawback_messages;
    all_messages.push(exit_vault_msg);
    
    Ok(Response::new()
        .add_messages(all_messages)
        .add_attributes(vec![
            attr("action", "withdraw"),
            attr("user", info.sender.to_string()),
            attr("amount", amount.to_string()),
            attr("window_id", window.window_id.to_string()),
        ]))
}


fn execute_claim(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    window_id: u64,
    mbrn_intent: Option<MbrnClaimIntent>,
) -> Result<Response, ContractError> {
    let user = info.sender.to_string();
    execute_transfer_deposit_ownership(deps, env, info, user, window_id)
}

fn execute_claim_for_user(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    user: String,
    window_id: u64,
    mbrn_intent: Option<MbrnClaimIntent>,
) -> Result<Response, ContractError> {
    execute_transfer_deposit_ownership(deps, env, info, user, window_id)
}


fn execute_send_acquisition_rewards_to_disco(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    window_id: u64,
    mbrn_intent: Option<MbrnClaimIntent>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let user = info.sender.to_string();
    
    // Load acquisition window
    let window = CURRENT_ACQUISITION_WINDOW.load(deps.storage)?;
    if window.window_id != window_id {
        return Err(ContractError::Validation(
            format!("Window ID mismatch. Expected {}, got {}", window.window_id, window_id),
        ));
    }
    
    // Validate window is finished (not cliff)
    let current_time = env.block.time.seconds();
    if current_time < window.withdrawal_end {
        return Err(ContractError::Validation(
            format!("Window is not finished yet. Withdrawal ends at: {}", window.withdrawal_end),
        ));
    }
    
    // Load user's acquisition deposit
    let mut deposit = USER_ACQUISITION_DEPOSITS
        .may_load(deps.storage, (user.clone(), window_id))?
        .ok_or_else(|| ContractError::Validation(
            format!("No deposit found for user {} in window {}", user, window_id),
        ))?;
    
    // Query transmuter to verify deposit still exists and get current amount
    let deposit_response: membrane::transmuter::DepositByIdResponse = deps.querier.query_wasm_smart(
        config.transmuter_contract.clone(),
        &membrane::transmuter::QueryMsg::DepositById {
            user: env.contract.address.to_string(),
            deposit_id: deposit.deposit_id,
        },
    ).map_err(|_| {
        USER_ACQUISITION_DEPOSITS.remove(deps.storage, (user.clone(), window_id));
        ContractError::Validation(
            format!("Deposit with ID {} not found in transmuter", deposit.deposit_id),
        )
    })?;
    
    // Use deposit amount from transmuter response (current actual amount)
    let transmuter_deposit_amount = deposit_response.deposit.amount;
    
    // Calculate claim amount using transmuter deposit amount
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
    
    // Query discounts_contract for MBRN boost
    let boost_response: UserBoostResponse = deps.querier.query_wasm_smart(
        config.discounts_contract.clone(),
        &DiscountQueryMsg::UserBoost {
            user: user.clone(),
        },
    )?;
    
    let discount_config: DiscountConfig = deps.querier.query_wasm_smart(
        config.discounts_contract.clone(),
        &DiscountQueryMsg::Config {},
    )?;
    let max_boost = discount_config.max_boost;
    
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
    
    // Extract Disco intent details
    let (asset, target_ltv, target_max_borrow_ltv) = match &disco_intent.intent_type {
        MbrnIntentType::DepositViaMarsMirror { asset, target_ltv, target_max_borrow_ltv } => {
            (asset.clone(), *target_ltv, *target_max_borrow_ltv)
        }
        _ => return Err(ContractError::Validation("Invalid intent type".into())),
    };
    
    // Calculate boosted amount for this intent
    let intent_boosts: Vec<Decimal> = if !config.discounts_contract.is_empty() {
        match deps.querier.query_wasm_smart::<IntentBoostsResponse>(
            config.discounts_contract.clone(),
            &DiscountQueryMsg::IntentBoosts {
                intents: vec![disco_intent.clone()],
            },
        ) {
            Ok(response) => response.boosts,
            Err(_) => vec![Decimal::zero()],
        }
    } else {
        vec![Decimal::zero()]
    };
    
    let intent_boost = intent_boosts.get(0).copied().unwrap_or(Decimal::zero());
    let total_boost = min(boost_response.boost + intent_boost, max_boost);
    let boost_multiplier = Decimal::one() + total_boost;
    
    let base_amount = decimal_multiplication(
        Decimal::from_ratio(claim_amount, Uint128::one()),
        disco_intent.ratio
    )?.to_uint_floor();
    
    let boosted_amount = decimal_multiplication(
        Decimal::from_ratio(base_amount, Uint128::one()),
        boost_multiplier
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
    
    // Create deposit input with intent's LTV params
    let deposit_input = BackingDepositInput {
        asset: asset.clone(),
        ltv: target_ltv.unwrap_or(Decimal::zero()),
        max_borrow_ltv: target_max_borrow_ltv.unwrap_or(Decimal::zero()),
        epoch_start_time: Some(env.block.time.seconds()),
    };
    
    // Query Disco contract for the current deposit ID (this will be the ID assigned to our deposit)
    let ltv_disco_addr = config.ltv_disco_contract.clone()
        .ok_or_else(|| ContractError::Validation("ltv_disco_contract not configured".into()))?;
    
    let ltv_queue_response: LTVQueueResponse = deps.querier.query_wasm_smart(
        ltv_disco_addr.clone(),
        &LtvDiscoQueryMsg::GetLTVQueue {
            assets: vec![asset.clone()],
            limit: None,
            start_after: None,
        },
    )?;

    // The current_deposit_id is the ID that will be assigned to our new deposit
    // When deposit_id is None, Disco will use current_deposit_id and increment it
    let next_deposit_id = ltv_queue_response.queues.first()
        .map(|(_, queue)| queue.current_deposit_id)
        .unwrap_or(Uint128::zero());
    
    // Submit to Disco with contract as owner, user as manager/revenue_destination
    // Pass deposit_id: None to let Disco create a new deposit with the current_deposit_id
    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: ltv_disco_addr.clone(),
        msg: to_json_binary(&LtvDiscoExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: Some(env.contract.address.to_string()), // Contract owns
            locked: disco_intent.lock.clone(),
            deposit_id: None, // Let Disco assign the ID (will use current_deposit_id we queried)
            manager: Some(user.clone()), // User is manager
            affiliate_address: None,
            revenue_destination: Some(user.clone()), // User receives revenue
        })?,
        funds: vec![coin(boosted_amount.u128(), config.mbrn_denom.clone())],
    }));
    
    // Store disco deposit info for clawback
    // Save the queried current_deposit_id, which will be the ID assigned to our deposit
    deposit.disco_deposit_id = Some(next_deposit_id);
    deposit.disco_asset = Some(asset.clone());
    deposit.disco_ltv = Some(deposit_input.ltv);
    deposit.disco_max_borrow_ltv = Some(deposit_input.max_borrow_ltv);
    deposit.disco_epoch_start_time = deposit_input.epoch_start_time;
    deposit.claimed_mbrn_amount = Some(boosted_amount);

    
    // Save updated deposit (with disco info) but DON'T remove from USER_ACQUISITION_DEPOSITS
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

fn execute_transfer_deposit_ownership(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    user: String,
    window_id: u64,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    
    // Load user's acquisition deposit
    let deposit = USER_ACQUISITION_DEPOSITS
        .may_load(deps.storage, (user.clone(), window_id))?
        .ok_or_else(|| ContractError::Validation(
            format!("No deposit found for user {} in window {}", user, window_id),
        ))?;
    
    // Verify transmuter deposit still exists
    let deposit_check: Result<membrane::transmuter::DepositByIdResponse, _> = deps.querier.query_wasm_smart(
        config.transmuter_contract.clone(),
        &membrane::transmuter::QueryMsg::DepositById {
            user: env.contract.address.to_string(),
            deposit_id: deposit.deposit_id,
        },
    );
    
    if deposit_check.is_err() {
        USER_ACQUISITION_DEPOSITS.remove(deps.storage, (user.clone(), window_id));
        return Err(ContractError::Validation(
            format!("Deposit with ID {} not found in transmuter", deposit.deposit_id),
        ));
    }
    
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
            deposit_id: deposit.deposit_id,
            new_owner: user.clone(),
        })?,
        funds: vec![],
    }));
    
    // Check if disco_deposit_id exists and transfer Disco deposit ownership
    if let Some(disco_deposit_id) = deposit.disco_deposit_id {
        // Transfer Disco deposit ownership to user
        let ltv_disco_addr = config.ltv_disco_contract.clone()
            .ok_or_else(|| ContractError::Validation("ltv_disco_contract not configured".into()))?;
        
        let disco_asset = deposit.disco_asset.clone()
            .ok_or_else(|| ContractError::Validation("disco_asset not set".into()))?;
        let disco_ltv = deposit.disco_ltv
            .ok_or_else(|| ContractError::Validation("disco_ltv not set".into()))?;
        let disco_max_borrow_ltv = deposit.disco_max_borrow_ltv
            .ok_or_else(|| ContractError::Validation("disco_max_borrow_ltv not set".into()))?;
        let disco_epoch_start_time = deposit.disco_epoch_start_time
            .ok_or_else(|| ContractError::Validation("disco_epoch_start_time not set".into()))?;
        
        messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: ltv_disco_addr.clone(),
            msg: to_json_binary(&LtvDiscoExecuteMsg::UpdateDeposit {
                asset: disco_asset,
                ltv: disco_ltv,
                max_borrow_ltv: disco_max_borrow_ltv,
                deposit_id: disco_deposit_id,
                deposit_owner: Some(user.clone()), // Transfer ownership to user
                manager: None, // Keep existing manager
                revenue_destination: None, // Keep existing revenue_destination
                epoch_start_time: disco_epoch_start_time,
            })?,
            funds: vec![],
        }));
    }
    
    // Remove deposit from USER_ACQUISITION_DEPOSITS
    USER_ACQUISITION_DEPOSITS.remove(deps.storage, (user.clone(), window_id));
    
    Ok(Response::new()
        .add_messages(messages)
        .add_attributes(vec![
            attr("action", "transfer_deposit_ownership"),
            attr("user", user),
            attr("window_id", window_id.to_string()),
        ]))
}

// fn process_user_intents(
//     mut deps: DepsMut,
//     env: &Env,
//     config: &Config,
//     user: &String,
//     claim_amount: Uint128,
//     user_boost: Decimal,
//     max_boost: Decimal,
//     mbrn_intent: &Option<MbrnClaimIntent>,
//     user_deposits: &Option<&Vec<AcquisitionDeposit>>,
// ) -> Result<(Vec<CosmosMsg>, Uint128), ContractError> {
//     let mut messages: Vec<CosmosMsg> = vec![];
    
//     // Determine which intents to use
//     let intents_to_use: Option<Vec<MbrnIntentOption>> = if let Some(ref intent) = mbrn_intent {
//         // If set_ongoing=true, save to USER_INTENTS
//         if intent.set_ongoing {
//             USER_INTENTS.save(deps.storage, user.clone(), &intent.intents.clone())?;
//         }
//         // If apply_now=true, use intents from mbrn_intent
//         if intent.apply_now {
//             Some(intent.intents.clone())
//         } else {
//             // Check for stored intents
//             USER_INTENTS.may_load(deps.storage, user.clone())?
//         }
//     } else {
//         // No mbrn_intent provided, check for stored intents
//         USER_INTENTS.may_load(deps.storage, user.clone())?
//     };
    
//     // Query IntentBoosts from system_discounts contract if configured and intents exist
//     let intent_boosts: Vec<Decimal> = if let Some(ref intents) = intents_to_use {
//         if !config.discounts_contract.is_empty() {
//             match deps.querier.query_wasm_smart::<IntentBoostsResponse>(
//                 config.discounts_contract.clone(),
//                 &DiscountQueryMsg::IntentBoosts {
//                     intents: intents.clone(),
//                 },
//             ) {
//                 Ok(response) => response.boosts,
//                 Err(_) => {
//                     // If query fails, proceed without boost (all zeros)
//                     vec![Decimal::zero(); intents.len()]
//                 }
//             }
//         } else {
//             // No discounts contract configured, proceed without boost
//             vec![Decimal::zero(); intents.len()]
//         }
//     } else {
//         vec![]
//     };
    
//     // Calculate boosted amounts per intent using additive boosts (capped at max_boost)
//     let intent_amounts: Vec<Uint128> = if let Some(ref intents) = intents_to_use {
//         intents.iter().enumerate().map(|(i, intent)| -> Result<Uint128, ContractError> {
//             // Calculate base amount for this intent
//             let base_amount = decimal_multiplication(
//                 Decimal::from_ratio(claim_amount, Uint128::one()),
//                 intent.ratio
//             )?.to_uint_floor();
            
//             // Calculate additive boost: min(user_boost + intent_boost[i], max_boost)
//             let intent_boost = intent_boosts.get(i).copied().unwrap_or(Decimal::zero());
//             let total_boost = min(user_boost + intent_boost, max_boost);
            
//             // Apply boost: boosted_amount = base_amount * (1 + total_boost)
//             let boost_multiplier = Decimal::one() + total_boost;
//             Ok(decimal_multiplication(
//                 Decimal::from_ratio(base_amount, Uint128::one()),
//                 boost_multiplier
//             )?.to_uint_floor())
//         }).collect::<Result<Vec<Uint128>, ContractError>>()?
//     } else {
//         vec![]
//     };
    
//     // Calculate total amount to mint
//     let total_to_mint = if !intent_amounts.is_empty() {
//         intent_amounts.iter().fold(Uint128::zero(), |acc, amount| {
//             acc.checked_add(*amount)
//                 .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))
//                 .unwrap()
//         })
//     } else {
//         // No intents, apply user boost to entire claim_amount
//         let total_boost = min(user_boost, max_boost);
//         decimal_multiplication(
//             Decimal::from_ratio(claim_amount, Uint128::one()),
//             Decimal::one() + total_boost
//         )?.to_uint_floor()
//     };
    
//     // Mint total amount (calculated with additive boosts) to contract
//     let contract_addr = env.contract.address.clone();
//     let mint_to_contract_msg = CosmosMsg::Wasm(WasmMsg::Execute {
//         contract_addr: config.neutron_proxy.clone(),
//         msg: to_json_binary(&NeutronProxyExecuteMsg::MintTokens {
//             denom: config.mbrn_denom.clone(),
//             amount: total_to_mint,
//             mint_to_address: contract_addr.to_string(),
//         })?,
//         funds: vec![],
//     });
//     messages.push(mint_to_contract_msg);
    
//     let mut remaining_to_mint = total_to_mint;
    
//     // Process intents if provided
//     if let Some(intents) = intents_to_use {
//         // Validate ratios sum to ~1.0
//         let total_ratio: Decimal = intents.iter()
//             .map(|i| i.ratio)
//             .fold(Decimal::zero(), |acc, x| acc + x);
//         let diff = if total_ratio > Decimal::one() {
//             total_ratio - Decimal::one()
//         } else {
//             Decimal::one() - total_ratio
//         };
//         if diff > Decimal::percent(1) {
//             return Err(ContractError::Validation(
//                 "MBRN intent ratios must sum to approximately 1.0".into(),
//             ));
//         }
        
//         // Process each intent using pre-calculated boosted amounts
//         for (_i, (intent, amount)) in intents.into_iter().zip(intent_amounts.into_iter()).enumerate() {
//             if amount.is_zero() {
//                 continue;
//             }
            
//             match &intent.intent_type {
//                 MbrnIntentType::Stake {} => {
//                     if let Some(staking_addr) = &config.staking_contract {
//                         let stake_msg = CosmosMsg::Wasm(WasmMsg::Execute {
//                             contract_addr: staking_addr.clone(),
//                             msg: to_json_binary(&StakingExecuteMsg::Stake {
//                                 user: Some(user.clone()),
//                                 locked: intent.lock.clone(),
//                             })?,
//                             funds: vec![coin(amount.u128(), config.mbrn_denom.clone())],
//                         });
//                         messages.push(stake_msg);
//                         remaining_to_mint = remaining_to_mint.checked_sub(amount)
//                             .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
//                     } else {
//                         return Err(ContractError::Validation(
//                             "staking_contract not configured but Stake intent provided".into(),
//                         ));
//                     }
//                 }
//                 MbrnIntentType::DepositViaMarsMirror { asset, target_ltv, target_max_borrow_ltv } => {
//                     if let (Some(ltv_disco_addr), Some(mars_mirror_addr)) = (&config.ltv_disco_contract, &config.mars_mirror_contract) {
//                         // Create deposit input
//                         let deposit_input = BackingDepositInput {
//                             asset: asset.clone(),
//                             ltv: target_ltv.unwrap_or(Decimal::zero()),
//                             max_borrow_ltv: target_max_borrow_ltv.unwrap_or(Decimal::zero()),
//                             epoch_start_time: Some(env.block.time.seconds()),
//                         };
                        
//                         // Deposit into ltv_disco with mars_mirror as manager
//                         let deposit_msg = CosmosMsg::Wasm(WasmMsg::Execute {
//                             contract_addr: ltv_disco_addr.clone(),
//                             msg: to_json_binary(&LtvDiscoExecuteMsg::SubmitDeposit {
//                                 deposit_input,
//                                 deposit_owner: Some(user.clone()),
//                                 locked: intent.lock.clone(),
//                                 deposit_id: None,
//                                 manager: Some(mars_mirror_addr.clone()),
//                                 affiliate_address: None,
//                                 revenue_destination: None,
//                             })?,
//                             funds: vec![coin(amount.u128(), config.mbrn_denom.clone())],
//                         });
//                         messages.push(deposit_msg);
//                         remaining_to_mint = remaining_to_mint.checked_sub(amount)
//                             .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
//                     } else {
//                         return Err(ContractError::Validation(
//                             "ltv_disco_contract and mars_mirror_contract must be configured for DepositViaMarsMirror intent".into(),
//                         ));
//                     }
//                 }
//                 MbrnIntentType::SendToAddress { address } => {
//                     // Validate the recipient address
//                     let recipient_addr = deps.api.addr_validate(address)
//                         .map_err(|e| ContractError::Validation(
//                             format!("Invalid recipient address: {}", e)
//                         ))?;
                    
//                     // Send MBRN to the specified address
//                     let send_msg = CosmosMsg::Bank(BankMsg::Send {
//                         to_address: recipient_addr.to_string(),
//                         amount: vec![coin(amount.u128(), config.mbrn_denom.clone())],
//                     });
//                     messages.push(send_msg);
//                     remaining_to_mint = remaining_to_mint.checked_sub(amount)
//                         .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
//                 }
//             }
//         }
//     }
    
//     // Send remaining amount (after intents) from contract to user
//     if !remaining_to_mint.is_zero() {
//         // Send MBRN from contract to user
//         let send_msg = CosmosMsg::Bank(BankMsg::Send {
//             to_address: user.clone(),
//             amount: vec![coin(remaining_to_mint.u128(), config.mbrn_denom.clone())],
//         });
//         messages.push(send_msg);
//     }
    
//     Ok((messages, total_to_mint))
// }

fn execute_update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    transmuter_contract: Option<String>,
    neutron_proxy: Option<String>,
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
    
    if let Some(emissions_voting) = emissions_voting_contract {
        let _ = deps.api.addr_validate(&emissions_voting)?;
        config.emissions_voting_contract = Some(emissions_voting);
    }
    
    if let Some(cliff) = cliff_period_days {
        if cliff == 0 {
            return Err(ContractError::Validation(
                "cliff_period_days must be greater than zero".into(),
            ));
        }
        config.cliff_period_seconds = cliff * SECONDS_PER_DAY;
    }
    
    CONFIG.save(deps.storage, &config)?;
    
    Ok(Response::new().add_attribute("action", "update_config"))
}


#[entry_point]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&ConfigResponse {
            config: CONFIG.load(deps.storage)?,
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
    }
}


fn ensure_owner(config: &Config, sender: &Addr) -> Result<(), ContractError> {
    if &config.owner != sender {
        return Err(ContractError::Unauthorized {});
    }
    Ok(())
}

