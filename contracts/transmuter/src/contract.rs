
use cosmwasm_std::{
    Addr, BankMsg, Binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo,
    Order, QuerierWrapper, Response, StdError, StdResult, Storage, Timestamp, Uint128,
    WasmMsg, attr, coin, entry_point, to_json_binary,
};
use cw2::set_contract_version;

use membrane::helpers::get_contract_balances;
use membrane::math::{decimal_multiplication, decimal_subtraction};
use membrane::cdp::{QueryMsg as CDP_QueryMsg, ExecuteMsg as CDP_ExecuteMsg};
use membrane::collateral::{QueryMsg as CollateralQueryMsg, BasketCollateral};
use membrane::debt::ExecuteMsg as DebtExecuteMsg;
use membrane::transmuter::{
    AllTranchesResponse, AssetPair, Config, ExecuteMsg, InstantiateMsg, MaxAssignableBadDebtResponse,
    QueryMsg, SwapRecord, TrancheConversionRateResponse, TrancheInfoResponse, TrancheState,
    TransmuteHistoryResponse, UserTranchesResponse, VaultInfoResponse, VolumeHistoryResponse,
    VolumeWindowResponse, RateHistoryResponse, RateHistoryEntry, TrancheSplit,
};
use membrane::types::{AssetInfo, StringEntry};
use membrane::revenue_distributor::ExecuteMsg as RevenueDistributorExecuteMsg;
use membrane::stability_pool_vault::{calculate_base_tokens, calculate_vault_tokens};

use crate::error::ContractError;
use crate::state::{
    append_transmute_snapshot, append_volume_window, append_rate_history_entry, apply_volume_update,
    history_slice, history_total, init_history, junior_str, new_volume_window, CONFIG,
    TRANSMUTE_HISTORY, VOLUME_HISTORY, VOLUME_WINDOW, ACQUISITION_DEPOSIT_TOTAL,
    TransmuteSnapshot, DEPLOYED_PAIRED_ASSET, PENDING_REVENUE, CUMULATIVE_VOLUME,
    RATE_HISTORY, LAST_RATE_UPDATE, USER_TRANCHES, TRANCHE_STATES, TRANCHE_RATE_ASSURANCE,
    TOTAL_TRANCHE_STAKED, UNDISTRIBUTED_FEES,
};

const CONTRACT_NAME: &str = "membrane-transmuter";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

// ================= Instantiate =================

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let owner = msg.clone()
        .owner
        .map(|o| deps.api.addr_validate(&o))
        .transpose()?;
    let owner = owner.unwrap_or_else(|| info.sender.clone());

    validate_asset_pair(&msg.deposit_pair)?;

    if msg.swap_history_cap == 0 {
        return Err(ContractError::Validation("swap_history_cap must be greater than zero".into()));
    }
    if msg.volume_history_cap == 0 {
        return Err(ContractError::Validation("volume_history_cap must be greater than zero".into()));
    }
    if msg.cdt_target_ratio > Decimal::one() {
        return Err(ContractError::Validation("cdt_target_ratio must be <= 1".into()));
    }

    let revenue_distributions = msg.revenue_distributions.clone();
    let cdp_contract = msg.cdp_contract.clone();
    let discounts_contract = msg.discounts_contract.clone();
    let collateral_contract = msg.collateral_contract.clone();

    let revenue_distributor_addr = msg.revenue_distributor_addr
        .map(|addr_str| deps.api.addr_validate(&addr_str))
        .transpose()?;
    let _ = deps.api.addr_validate(&cdp_contract)?;
    let _ = deps.api.addr_validate(&discounts_contract)?;
    let _ = deps.api.addr_validate(&collateral_contract)?;

    let usage_fee = msg.usage_fee.unwrap_or(Decimal::percent(1));
    if usage_fee > Decimal::one() {
        return Err(ContractError::Validation("usage_fee must be <= 1".into()));
    }
    if usage_fee.is_zero() {
        return Err(ContractError::Validation("usage_fee must be > 0".into()));
    }

    let usage_fee_utilization_threshold = msg.usage_fee_utilization_threshold.unwrap_or(Decimal::percent(80));
    if usage_fee_utilization_threshold > Decimal::one() {
        return Err(ContractError::Validation("usage_fee_utilization_threshold must be <= 1".into()));
    }

    let affiliate_fee = msg.affiliate_fee;
    if affiliate_fee > Decimal::one() {
        return Err(ContractError::Validation("affiliate_fee must be <= 1".into()));
    }

    let revenue_distributions = revenue_distributions.unwrap_or_default();
    if !revenue_distributions.is_empty() {
        if revenue_distributor_addr.is_none() {
            return Err(ContractError::Validation(
                "revenue_distributor_addr must be provided when revenue_distributions are specified".into(),
            ));
        }
        validate_distribution_ratios(&revenue_distributions)?;
    }

    let send_swap_fee = msg.send_swap_fee.unwrap_or(true);
    let revenue_distributor_fee_percentage = msg.revenue_distributor_fee_percentage.unwrap_or(Decimal::percent(20));
    if revenue_distributor_fee_percentage > Decimal::one() {
        return Err(ContractError::Validation("revenue_distributor_fee_percentage must be <= 1".into()));
    }

    let config = Config {
        owner: owner.clone(),
        cdp_contract,
        discounts_contract,
        collateral_contract,
        deposit_pair: msg.deposit_pair,
        composition_leeway: msg.composition_leeway,
        cdt_target_ratio: msg.cdt_target_ratio,
        usage_fee,
        usage_fee_utilization_threshold,
        swap_history_cap: msg.swap_history_cap,
        volume_history_cap: msg.volume_history_cap,
        allowlist: msg.allowlist.unwrap_or_default(),
        revenue_distributor_addr,
        revenue_distributions,
        affiliate_fee,
        send_swap_fee,
        revenue_distributor_fee_percentage,
        acquisition_contract: msg.acquisition_contract
            .map(|s| deps.api.addr_validate(&s))
            .transpose()?,
        points_system_contract: msg.points_system_contract
            .map(|s| deps.api.addr_validate(&s))
            .transpose()?,
        retention_multiplier: msg.retention_multiplier.unwrap_or(Decimal::zero()),
    };

    CONFIG.save(deps.storage, &config)?;
    ACQUISITION_DEPOSIT_TOTAL.save(deps.storage, &Uint128::zero())?;
    TOTAL_TRANCHE_STAKED.save(deps.storage, &Uint128::zero())?;
    UNDISTRIBUTED_FEES.save(deps.storage, &Uint128::zero())?;
    init_history(deps.storage)?;
    CUMULATIVE_VOLUME.save(deps.storage, &Uint128::zero())?;
    let cumulative_volume = CUMULATIVE_VOLUME.load(deps.storage)?;
    VOLUME_WINDOW.save(deps.storage, &new_volume_window(env.block.time, cumulative_volume))?;
    DEPLOYED_PAIRED_ASSET.save(deps.storage, &Uint128::zero())?;
    PENDING_REVENUE.save(deps.storage, &Uint128::zero())?;
    RATE_HISTORY.save(deps.storage, &Vec::new())?;
    LAST_RATE_UPDATE.save(deps.storage, &Timestamp::from_seconds(0))?;

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("action", "instantiate"),
            attr("owner", owner.as_str()),
        ]))
}

// ================= Execute =================

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::UpdateConfig {
            owner, deposit_pair, composition_leeway, cdt_target_ratio,
            discounts_contract, cdp_contract, collateral_contract,
            usage_fee, usage_fee_utilization_threshold,
            swap_history_cap, volume_history_cap, allowlist,
            revenue_distributor_addr, revenue_distributions,
            affiliate_fee, send_swap_fee, revenue_distributor_fee_percentage,
            acquisition_contract, points_system_contract, retention_multiplier,
        } => execute_update_config(
            deps, env, info, owner, deposit_pair, composition_leeway, cdt_target_ratio,
            discounts_contract, cdp_contract, collateral_contract,
            usage_fee, usage_fee_utilization_threshold,
            swap_history_cap, volume_history_cap, allowlist,
            revenue_distributor_addr, revenue_distributions,
            affiliate_fee, send_swap_fee, revenue_distributor_fee_percentage,
            acquisition_contract, points_system_contract, retention_multiplier,
        ),
        ExecuteMsg::EnterVault { recipient, affiliate_address, affiliate_label, tranches } => {
            execute_enter_vault(deps, env, info, recipient, affiliate_address, affiliate_label, tranches)
        }
        ExecuteMsg::ExitVault { recipient, withdraw_as, user, amount, tranche_withdrawals } => {
            execute_exit_vault(deps, env, info, recipient, withdraw_as, user, amount, tranche_withdrawals)
        }
        ExecuteMsg::Transmute { recipient } => execute_transmute(deps, env, info, recipient),
        ExecuteMsg::UpdateVolumeWindow {} => execute_update_volume_window(deps, env),
        ExecuteMsg::RateAssurance {} => execute_rate_assurance(deps, env, info),
        ExecuteMsg::SetAffiliate { user, affiliate_address, label } => {
            execute_set_affiliate(deps, env, info, user, affiliate_address, label)
        }
        ExecuteMsg::AddToRateHistory {} => execute_add_to_rate_history(deps, env),
        ExecuteMsg::AddRevenue { collateral_denom, junior } => {
            execute_add_revenue(deps, env, info, collateral_denom, junior)
        }
        ExecuteMsg::DistributeSwapFees {} => execute_distribute_swap_fees(deps, env),
        ExecuteMsg::AssignBadDebt { collateral_denom, amount } => {
            execute_assign_bad_debt(deps, env, info, collateral_denom, amount)
        }
        ExecuteMsg::RepayUserDebt { user_info, repayment } => {
            execute_repay_user_debt(deps, env, info, user_info, repayment)
        }
        ExecuteMsg::TransferDepositOwnership { user, amount, new_owner } => {
            execute_transfer_deposit_ownership(deps, env, info, user, amount, new_owner)
        }
    }
}

// ================= Update Config =================

fn execute_update_config(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    owner: Option<String>,
    deposit_pair: Option<AssetPair>,
    composition_leeway: Option<Decimal>,
    cdt_target_ratio: Option<Decimal>,
    discounts_contract: Option<String>,
    cdp_contract: Option<String>,
    collateral_contract: Option<String>,
    usage_fee: Option<Decimal>,
    usage_fee_utilization_threshold: Option<Decimal>,
    swap_history_cap: Option<u32>,
    volume_history_cap: Option<u32>,
    allowlist: Option<Vec<StringEntry>>,
    revenue_distributor_addr: Option<String>,
    revenue_distributions: Option<Vec<membrane::types::DistributionEntry>>,
    affiliate_fee: Decimal,
    send_swap_fee: Option<bool>,
    revenue_distributor_fee_percentage: Option<Decimal>,
    acquisition_contract: Option<String>,
    points_system_contract: Option<String>,
    retention_multiplier: Option<Decimal>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    ensure_owner(&config, &info.sender)?;

    if let Some(owner_str) = owner {
        config.owner = deps.api.addr_validate(&owner_str)?;
    }
    if let Some(pair) = deposit_pair {
        validate_asset_pair(&pair)?;
        config.deposit_pair = pair;
    }
    if let Some(leeway) = composition_leeway {
        if leeway > Decimal::one() {
            return Err(ContractError::Validation("composition_leeway must be <= 1".into()));
        }
        config.composition_leeway = leeway;
    }
    if let Some(target) = cdt_target_ratio {
        if target > Decimal::one() {
            return Err(ContractError::Validation("target ratio must be <= 1".into()));
        }
        config.cdt_target_ratio = target;
    }
    if let Some(d) = discounts_contract {
        deps.api.addr_validate(&d)?;
        config.discounts_contract = d;
    }
    if let Some(cdp) = cdp_contract {
        deps.api.addr_validate(&cdp)?;
        config.cdp_contract = cdp;
    }
    if let Some(col) = collateral_contract {
        deps.api.addr_validate(&col)?;
        config.collateral_contract = col;
    }
    if let Some(fee) = usage_fee {
        if fee > Decimal::one() {
            return Err(ContractError::Validation("usage_fee must be <= 1".into()));
        }
        if fee.is_zero() {
            return Err(ContractError::Validation("usage_fee must be > 0".into()));
        }
        config.usage_fee = fee;
    }
    if let Some(threshold) = usage_fee_utilization_threshold {
        if threshold > Decimal::one() {
            return Err(ContractError::Validation("usage_fee_utilization_threshold must be <= 1".into()));
        }
        config.usage_fee_utilization_threshold = threshold;
    }
    if let Some(cap) = swap_history_cap {
        if cap == 0 {
            return Err(ContractError::Validation("swap_history_cap must be > 0".into()));
        }
        config.swap_history_cap = cap;
    }
    if let Some(cap) = volume_history_cap {
        if cap == 0 {
            return Err(ContractError::Validation("volume_history_cap must be > 0".into()));
        }
        config.volume_history_cap = cap;
    }
    if let Some(entries) = allowlist {
        for e in entries {
            if e.remove {
                config.allowlist.retain(|addr| addr != &e.entry);
            } else if !config.allowlist.iter().any(|addr| addr == &e.entry) {
                config.allowlist.push(e.entry);
            }
        }
    }
    if let Some(rd_addr_str) = revenue_distributor_addr {
        config.revenue_distributor_addr = Some(deps.api.addr_validate(&rd_addr_str)?);
    }
    if let Some(distributions) = revenue_distributions {
        for entry in distributions {
            if entry.remove {
                config.revenue_distributions.retain(|liq_asset| liq_asset.info != entry.asset.info);
            } else {
                config.revenue_distributions.retain(|liq_asset| liq_asset.info != entry.asset.info);
                config.revenue_distributions.push(entry.asset);
            }
        }
        validate_distribution_ratios(&config.revenue_distributions)?;
    }
    if affiliate_fee > Decimal::one() {
        return Err(ContractError::Validation("affiliate_fee must be <= 1".into()));
    }
    config.affiliate_fee = affiliate_fee;
    if let Some(send_fee) = send_swap_fee {
        config.send_swap_fee = send_fee;
    }
    if let Some(fee_pct) = revenue_distributor_fee_percentage {
        if fee_pct > Decimal::one() {
            return Err(ContractError::Validation("revenue_distributor_fee_percentage must be <= 1".into()));
        }
        config.revenue_distributor_fee_percentage = fee_pct;
    }
    if let Some(mult) = retention_multiplier {
        config.retention_multiplier = mult;
    }
    if let Some(acq) = acquisition_contract {
        config.acquisition_contract = Some(deps.api.addr_validate(&acq)?);
    }
    if let Some(ps) = points_system_contract {
        config.points_system_contract = Some(deps.api.addr_validate(&ps)?);
    }

    CONFIG.save(deps.storage, &config)?;
    Ok(Response::new().add_attribute("action", "update_config"))
}

// ================= Enter Vault (Deposit) =================

fn execute_enter_vault(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    recipient: Option<String>,
    affiliate_address: Option<String>,
    affiliate_label: Option<String>,
    tranches: Vec<(String, bool)>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut messages: Vec<CosmosMsg> = vec![];

    // Validate tranches non-empty
    if tranches.is_empty() {
        return Err(ContractError::Validation("tranches must be non-empty".into()));
    }

    // Validate no duplicate (denom, junior) pairs
    validate_no_duplicate_tranches(&tranches)?;

    // Validate each denom is a valid collateral
    validate_collateral_denoms(&deps.querier, &config, &tranches)?;

    // Segregate funds
    let (deposit_a, deposit_b) = segregate_funds(&config.deposit_pair, &info)?;
    if deposit_a.is_zero() && deposit_b.is_zero() {
        return Err(ContractError::InvalidFunds { reason: "no valid funds provided".into() });
    }

    // Check deposit alignment
    let acq_total = ACQUISITION_DEPOSIT_TOTAL.may_load(deps.storage)?.unwrap_or(Uint128::zero());
    let total_tranche_staked = TOTAL_TRANCHE_STAKED.load(deps.storage)?;
    if !total_tranche_staked.is_zero() || !acq_total.is_zero() {
        let effective_target = compute_effective_cdt_target_ratio(deps.as_ref(), &env, &config)?;
        ensure_deposit_alignment(
            deps.querier, &env, &config, effective_target, deposit_a, deposit_b,
        )?;
    }

    let user_deposit_value = deposit_a.checked_add(deposit_b)
        .map_err(|e| ContractError::Std(StdError::from(e)))?;
    if user_deposit_value.is_zero() {
        return Err(ContractError::InvalidFunds { reason: "deposit value is zero".into() });
    }

    let recipient_addr = recipient
        .map(|r| deps.api.addr_validate(&r))
        .transpose()?
        .unwrap_or_else(|| info.sender.clone());

    // Check if acquisition deposit
    let is_acquisition_deposit = config.acquisition_contract
        .as_ref()
        .map(|acq| recipient_addr == *acq)
        .unwrap_or(false);

    if is_acquisition_deposit {
        // Acquisition deposits bypass tranche system
        let mut acq = ACQUISITION_DEPOSIT_TOTAL.load(deps.storage)?;
        acq = acq.checked_add(user_deposit_value)?;
        ACQUISITION_DEPOSIT_TOTAL.save(deps.storage, &acq)?;
    } else {
        // Split deposit equally across tranches (last gets remainder)
        let n = tranches.len() as u128;
        let base_split = user_deposit_value.u128() / n;
        let remainder = user_deposit_value.u128() % n;

        let mut total_staked_delta = Uint128::zero();

        for (i, (denom, junior)) in tranches.iter().enumerate() {
            let split_value = if i == tranches.len() - 1 {
                Uint128::new(base_split + remainder)
            } else {
                Uint128::new(base_split)
            };

            if split_value.is_zero() {
                continue;
            }

            let js = junior_str(*junior);

            // Load tranche state (default empty)
            let mut tranche = TRANCHE_STATES
                .may_load(deps.storage, (denom.as_str(), js))?
                .unwrap_or(TrancheState {
                    vault_token_supply: Uint128::zero(),
                    total_staked: Uint128::zero(),
                });

            // Save pre-rate for rate assurance
            TRANCHE_RATE_ASSURANCE.save(
                deps.storage,
                (denom.as_str(), js),
                &membrane::transmuter::TrancheRateAssurance {
                    pre_vault_token_supply: tranche.vault_token_supply,
                    pre_total_staked: tranche.total_staked,
                },
            )?;

            // Calculate virtual vault tokens
            let new_vts = calculate_vault_tokens(split_value, tranche.total_staked, tranche.vault_token_supply)
                .map_err(ContractError::Std)?;

            // Update tranche state
            tranche.vault_token_supply = tranche.vault_token_supply.checked_add(new_vts)?;
            tranche.total_staked = tranche.total_staked.checked_add(split_value)?;
            TRANCHE_STATES.save(deps.storage, (denom.as_str(), js), &tranche)?;

            // Update user tranche allocation
            let user_key = (recipient_addr.as_str(), (denom.as_str(), js));
            let existing = USER_TRANCHES.may_load(deps.storage, user_key)?.unwrap_or(Uint128::zero());
            USER_TRANCHES.save(deps.storage, user_key, &(existing.checked_add(new_vts)?))?;

            total_staked_delta = total_staked_delta.checked_add(split_value)?;
        }

        // Update global total
        let total = TOTAL_TRANCHE_STAKED.load(deps.storage)?;
        TOTAL_TRANCHE_STAKED.save(deps.storage, &total.checked_add(total_staked_delta)?)?;

        // Rate assurance callback
        if !total_staked_delta.is_zero() {
            messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: env.contract.address.to_string(),
                msg: to_json_binary(&ExecuteMsg::RateAssurance {})?,
                funds: vec![],
            }));
        }
    }

    // Handle affiliate
    if let Some(affiliate_addr) = affiliate_address {
        add_affiliate_from_deposit(
            deps.storage, deps.api, recipient_addr.to_string(),
            affiliate_addr, config.affiliate_fee, env.block.time.seconds(), affiliate_label,
        )?;
    }

    let mut response = Response::new()
        .add_messages(messages)
        .add_attributes(vec![
            attr("action", "enter_vault"),
            attr("deposit_amount", user_deposit_value.to_string()),
            attr("recipient", recipient_addr.as_str()),
        ]);

    // Notify acquisition contract
    if let Some(acq_msg) = build_acquisition_notification(&config) {
        response = response.add_message(acq_msg);
    }

    Ok(response)
}

// ================= Exit Vault (Withdraw) =================

fn execute_exit_vault(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    recipient: Option<String>,
    withdraw_as: Option<String>,
    user: Option<String>,
    amount: Option<Uint128>,
    tranche_withdrawals: Option<Vec<(String, bool, Uint128)>>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let acq_total = ACQUISITION_DEPOSIT_TOTAL.may_load(deps.storage)?.unwrap_or(Uint128::zero());

    let is_acquisition = config.acquisition_contract
        .as_ref()
        .map(|acq| info.sender == *acq)
        .unwrap_or(false);

    let balances = current_balances(deps.querier, &env, &config.deposit_pair)?;
    let total_balance = balances.0.checked_add(balances.1)?;

    let mut messages: Vec<CosmosMsg> = vec![];

    let (withdrawable_amount, asset_a_share, asset_b_share) = if is_acquisition {
        // Acquisition path: uses amount param, 1:1 value, no tranches
        let withdraw_amount = amount.ok_or_else(|| {
            ContractError::Validation("amount required for acquisition withdrawal".into())
        })?;
        let (a, b) = calculate_withdrawal_assets(withdraw_amount, total_balance, balances.0, balances.1)?;

        let new_acq = acq_total.checked_sub(withdraw_amount)?;
        ACQUISITION_DEPOSIT_TOTAL.save(deps.storage, &new_acq)?;

        (withdraw_amount, a, b)
    } else {
        // Determine the user whose tranches to withdraw from
        let withdraw_user = if let Some(u) = &user {
            // Only contract itself can withdraw for another user
            if info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }
            deps.api.addr_validate(u)?
        } else {
            info.sender.clone()
        };

        // Build withdrawal list
        let withdrawals = if let Some(tw) = tranche_withdrawals {
            // Validate no duplicate (denom, junior) pairs
            let pairs: Vec<(String, bool)> = tw.iter().map(|(d, j, _)| (d.clone(), *j)).collect();
            validate_no_duplicate_tranches(&pairs)?;
            tw
        } else {
            // Proportional withdrawal from all user tranches
            build_proportional_withdrawals(deps.storage, &withdraw_user)?
        };

        if withdrawals.is_empty() {
            return Err(ContractError::NoTranchePositions {});
        }

        let mut total_base_value = Uint128::zero();

        for (denom, junior, vt_amount) in &withdrawals {
            let js = junior_str(*junior);
            let user_key = (withdraw_user.as_str(), (denom.as_str(), js));

            // Validate user has enough
            let user_vt = USER_TRANCHES.may_load(deps.storage, user_key)?
                .unwrap_or(Uint128::zero());
            if user_vt < *vt_amount {
                return Err(ContractError::InsufficientTrancheBalance {
                    have: user_vt.to_string(),
                    need: vt_amount.to_string(),
                });
            }

            // Load tranche state
            let mut tranche = TRANCHE_STATES.load(deps.storage, (denom.as_str(), js))?;

            // Save pre-rate for rate assurance
            TRANCHE_RATE_ASSURANCE.save(
                deps.storage,
                (denom.as_str(), js),
                &membrane::transmuter::TrancheRateAssurance {
                    pre_vault_token_supply: tranche.vault_token_supply,
                    pre_total_staked: tranche.total_staked,
                },
            )?;

            // Calculate base value
            let base_value = calculate_base_tokens(*vt_amount, tranche.total_staked, tranche.vault_token_supply)
                .map_err(ContractError::Std)?;

            // Update tranche state
            tranche.vault_token_supply = tranche.vault_token_supply.checked_sub(*vt_amount)?;
            tranche.total_staked = tranche.total_staked.checked_sub(base_value)?;
            TRANCHE_STATES.save(deps.storage, (denom.as_str(), js), &tranche)?;

            // Update user allocation
            let new_user_vt = user_vt.checked_sub(*vt_amount)?;
            if new_user_vt.is_zero() {
                USER_TRANCHES.remove(deps.storage, user_key);
            } else {
                USER_TRANCHES.save(deps.storage, user_key, &new_user_vt)?;
            }

            total_base_value = total_base_value.checked_add(base_value)?;
        }

        // Update global total
        let total = TOTAL_TRANCHE_STAKED.load(deps.storage)?;
        TOTAL_TRANCHE_STAKED.save(deps.storage, &total.checked_sub(total_base_value)?)?;

        let (a, b) = calculate_withdrawal_assets(total_base_value, total_balance, balances.0, balances.1)?;

        // Rate assurance callback
        let remaining_total = TOTAL_TRANCHE_STAKED.load(deps.storage)?;
        let remaining_acq = ACQUISITION_DEPOSIT_TOTAL.may_load(deps.storage)?.unwrap_or(Uint128::zero());
        if !remaining_total.is_zero() || !remaining_acq.is_zero() {
            messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: env.contract.address.to_string(),
                msg: to_json_binary(&ExecuteMsg::RateAssurance {})?,
                funds: vec![],
            }));
        }

        (total_base_value, a, b)
    };

    let recipient_addr = recipient
        .map(|r| deps.api.addr_validate(&r))
        .transpose()?
        .unwrap_or_else(|| info.sender.clone());

    // Build send coins
    let mut send_coins: Vec<Coin> = vec![];
    let mut total_a_needed = asset_a_share;
    let mut total_b_needed = asset_b_share;

    match withdraw_as {
        Some(ref denom) if denom == &config.deposit_pair.cdt => {
            total_a_needed = asset_a_share.checked_add(asset_b_share)?;
            if balances.0 < total_a_needed {
                return Err(ContractError::InsufficientLiquidity(config.deposit_pair.cdt.clone()));
            }
            if !total_a_needed.is_zero() {
                send_coins.push(coin(total_a_needed.u128(), denom));
            }
        }
        Some(ref denom) if denom == &config.deposit_pair.paired_asset => {
            total_b_needed = asset_a_share.checked_add(asset_b_share)?;
            if balances.1 < total_b_needed {
                return Err(ContractError::InsufficientLiquidity(config.deposit_pair.paired_asset.clone()));
            }
            if !total_b_needed.is_zero() {
                send_coins.push(coin(total_b_needed.u128(), denom));
            }
        }
        Some(denom) => return Err(ContractError::InvalidAsset { denom }),
        None => {
            if !asset_a_share.is_zero() {
                send_coins.push(coin(asset_a_share.u128(), config.deposit_pair.cdt.clone()));
            }
            if !asset_b_share.is_zero() {
                send_coins.push(coin(asset_b_share.u128(), config.deposit_pair.paired_asset.clone()));
            }
        }
    }

    let mut response = Response::new()
        .add_messages(messages)
        .add_attribute("action", "exit_vault")
        .add_attribute("user", info.sender.as_str())
        .add_attribute("withdrawable_amount", withdrawable_amount.to_string())
        .add_attribute("cdt_withdrawn", total_a_needed.to_string())
        .add_attribute("paired_asset_withdrawn", total_b_needed.to_string())
        .add_attribute("recipient", recipient_addr.as_str());

    if !send_coins.is_empty() {
        response = response.add_message(BankMsg::Send {
            to_address: recipient_addr.to_string(),
            amount: send_coins,
        });
    }

    if let Some(acq_msg) = build_acquisition_notification(&config) {
        response = response.add_message(acq_msg);
    }

    Ok(response)
}

/// Build proportional withdrawal list from all user tranches
fn build_proportional_withdrawals(
    storage: &dyn Storage,
    user: &Addr,
) -> Result<Vec<(String, bool, Uint128)>, ContractError> {
    let mut withdrawals = vec![];
    let prefix = USER_TRANCHES.prefix(user.as_str());
    for item in prefix.range(storage, None, None, Order::Ascending) {
        let ((denom, js), vt_amount) = item?;
        if !vt_amount.is_zero() {
            let junior = js == "j";
            withdrawals.push((denom, junior, vt_amount));
        }
    }
    Ok(withdrawals)
}

// ================= Add Revenue =================

fn execute_add_revenue(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    collateral_denom: String,
    junior: bool,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Segregate received funds
    let (deposit_a, deposit_b) = segregate_funds(&config.deposit_pair, &info)?;
    let revenue_value = deposit_a.checked_add(deposit_b)
        .map_err(|e| ContractError::Std(StdError::from(e)))?;

    if revenue_value.is_zero() {
        return Err(ContractError::InvalidFunds { reason: "no revenue funds sent".into() });
    }

    let js = junior_str(junior);

    // Load tranche state
    let mut tranche = TRANCHE_STATES
        .may_load(deps.storage, (collateral_denom.as_str(), js))?
        .unwrap_or(TrancheState {
            vault_token_supply: Uint128::zero(),
            total_staked: Uint128::zero(),
        });

    // Increase total_staked (conversion rate goes up)
    tranche.total_staked = tranche.total_staked.checked_add(revenue_value)?;
    TRANCHE_STATES.save(deps.storage, (collateral_denom.as_str(), js), &tranche)?;

    // Update global total
    let total = TOTAL_TRANCHE_STAKED.load(deps.storage)?;
    TOTAL_TRANCHE_STAKED.save(deps.storage, &total.checked_add(revenue_value)?)?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("action", "add_revenue"),
            attr("collateral_denom", collateral_denom),
            attr("junior", junior.to_string()),
            attr("amount", revenue_value.to_string()),
        ]))
}

// ================= Distribute Swap Fees =================

fn execute_distribute_swap_fees(
    deps: DepsMut,
    _env: Env,
) -> Result<Response, ContractError> {
    let undistributed = UNDISTRIBUTED_FEES.load(deps.storage)?;
    if undistributed.is_zero() {
        return Ok(Response::new().add_attribute("action", "distribute_swap_fees").add_attribute("distributed", "0"));
    }

    let total_tranche_staked = TOTAL_TRANCHE_STAKED.load(deps.storage)?;
    if total_tranche_staked.is_zero() {
        // No tranches to distribute to; fees remain undistributed
        return Ok(Response::new().add_attribute("action", "distribute_swap_fees").add_attribute("distributed", "0"));
    }

    // Iterate all tranche states and distribute proportionally
    let all_tranches: Vec<((String, String), TrancheState)> = TRANCHE_STATES
        .range(deps.storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<_>>>()?;

    let mut distributed = Uint128::zero();
    let num_tranches = all_tranches.len();

    for (i, ((denom, js), mut tranche)) in all_tranches.into_iter().enumerate() {
        if tranche.total_staked.is_zero() {
            continue;
        }

        let share = if i == num_tranches - 1 {
            // Last tranche gets remainder to avoid rounding loss
            undistributed.checked_sub(distributed)?
        } else {
            undistributed.multiply_ratio(tranche.total_staked, total_tranche_staked)
        };

        if !share.is_zero() {
            tranche.total_staked = tranche.total_staked.checked_add(share)?;
            TRANCHE_STATES.save(deps.storage, (denom.as_str(), js.as_str()), &tranche)?;
            distributed = distributed.checked_add(share)?;
        }
    }

    // Update global total
    let total = TOTAL_TRANCHE_STAKED.load(deps.storage)?;
    TOTAL_TRANCHE_STAKED.save(deps.storage, &total.checked_add(distributed)?)?;

    // Reset undistributed
    UNDISTRIBUTED_FEES.save(deps.storage, &Uint128::zero())?;

    Ok(Response::new()
        .add_attribute("action", "distribute_swap_fees")
        .add_attribute("distributed", distributed.to_string()))
}

// ================= Assign Bad Debt =================

fn execute_assign_bad_debt(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    collateral_denom: String,
    amount: Uint128,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Calculate max assignable
    let junior_state = TRANCHE_STATES
        .may_load(deps.storage, (collateral_denom.as_str(), "j"))?
        .unwrap_or(TrancheState { vault_token_supply: Uint128::zero(), total_staked: Uint128::zero() });
    let senior_state = TRANCHE_STATES
        .may_load(deps.storage, (collateral_denom.as_str(), "s"))?
        .unwrap_or(TrancheState { vault_token_supply: Uint128::zero(), total_staked: Uint128::zero() });

    let max_assignable = junior_state.total_staked.checked_add(senior_state.total_staked)?;
    if amount > max_assignable {
        return Err(ContractError::BadDebtExceedsMax {
            amount: amount.to_string(),
            max: max_assignable.to_string(),
        });
    }

    // Waterfall: junior absorbs first
    let mut remaining = amount;
    let mut total_loss = Uint128::zero();

    // Junior
    if !remaining.is_zero() {
        let mut junior = junior_state;
        let junior_loss = std::cmp::min(remaining, junior.total_staked);
        junior.total_staked = junior.total_staked.checked_sub(junior_loss)?;
        TRANCHE_STATES.save(deps.storage, (collateral_denom.as_str(), "j"), &junior)?;
        remaining = remaining.checked_sub(junior_loss)?;
        total_loss = total_loss.checked_add(junior_loss)?;
    }

    // Senior
    if !remaining.is_zero() {
        let mut senior = senior_state;
        let senior_loss = std::cmp::min(remaining, senior.total_staked);
        senior.total_staked = senior.total_staked.checked_sub(senior_loss)?;
        TRANCHE_STATES.save(deps.storage, (collateral_denom.as_str(), "s"), &senior)?;
        total_loss = total_loss.checked_add(senior_loss)?;
    }

    // Update global total
    let total = TOTAL_TRANCHE_STAKED.load(deps.storage)?;
    TOTAL_TRANCHE_STAKED.save(deps.storage, &total.checked_sub(total_loss)?)?;

    // Send CDT to CDP.FulfillBadDebt
    let fulfill_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.cdp_contract.to_string(),
        msg: to_json_binary(&DebtExecuteMsg::FulfillBadDebt {})?,
        funds: vec![coin(amount.u128(), config.deposit_pair.cdt.clone())],
    });

    Ok(Response::new()
        .add_message(fulfill_msg)
        .add_attributes(vec![
            attr("action", "assign_bad_debt"),
            attr("collateral_denom", collateral_denom),
            attr("amount", amount.to_string()),
            attr("total_loss", total_loss.to_string()),
        ]))
}

// ================= Rate Assurance =================

fn execute_rate_assurance(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized {});
    }

    // Iterate all entries in TRANCHE_RATE_ASSURANCE
    let assurances: Vec<((String, String), membrane::transmuter::TrancheRateAssurance)> =
        TRANCHE_RATE_ASSURANCE
            .range(deps.storage, None, None, Order::Ascending)
            .collect::<StdResult<Vec<_>>>()?;

    for ((denom, js), pre) in &assurances {
        let tranche = TRANCHE_STATES.load(deps.storage, (denom.as_str(), js.as_str()))?;

        if !tranche.vault_token_supply.is_zero() && !pre.pre_vault_token_supply.is_zero() {
            let new_rate = Decimal::from_ratio(tranche.total_staked, tranche.vault_token_supply);
            let pre_rate = Decimal::from_ratio(pre.pre_total_staked, pre.pre_vault_token_supply);

            if new_rate < pre_rate {
                return Err(ContractError::TrancheRateDecreased {
                    pre_rate: format!("{}/{}", pre.pre_total_staked, pre.pre_vault_token_supply),
                    post_rate: format!("{}/{}", tranche.total_staked, tranche.vault_token_supply),
                });
            }
        }

        // Clear this assurance entry
        TRANCHE_RATE_ASSURANCE.remove(deps.storage, (denom.as_str(), js.as_str()));
    }

    Ok(Response::new())
}

// ================= Transmute (Swap) =================

fn execute_transmute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    recipient: Option<String>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let pair = &config.deposit_pair;
    let recipient_addr = recipient
        .map(|r| deps.api.addr_validate(&r))
        .transpose()?
        .unwrap_or_else(|| info.sender.clone());

    let (mut funds_a, mut funds_b) = segregate_funds(pair, &info)?;
    if funds_a.is_zero() && funds_b.is_zero() {
        return Err(ContractError::InvalidFunds { reason: "no valid funds provided".into() });
    }
    if !funds_a.is_zero() && !funds_b.is_zero() {
        return Err(ContractError::InvalidFunds { reason: "only one asset may be sent for transmute".into() });
    }

    // CDT→paired_asset restricted to CDP contract
    if !funds_a.is_zero()
        && info.sender != env.contract.address
        && info.sender.to_string() != config.cdp_contract
    {
        return Err(ContractError::CdtToPairedAssetRestricted {});
    }

    let is_allowlisted = is_allowlisted_sender(&deps.querier, &info.sender, &config)?;

    // Usage fee
    let fee_info: Option<(Uint128, String)> = if is_allowlisted || info.sender == env.contract.address {
        None
    } else if config.usage_fee > Decimal::zero() && config.usage_fee < Decimal::one() {
        if !funds_a.is_zero() {
            let fee_amount = decimal_multiplication(
                Decimal::from_ratio(funds_a, Uint128::one()), config.usage_fee
            )?.to_uint_floor();
            let after_fee = decimal_subtraction(Decimal::one(), config.usage_fee)?;
            funds_a = decimal_multiplication(
                Decimal::from_ratio(funds_a, Uint128::one()), after_fee
            )?.to_uint_floor();
            Some((fee_amount, pair.cdt.clone()))
        } else {
            let fee_amount = decimal_multiplication(
                Decimal::from_ratio(funds_b, Uint128::one()), config.usage_fee
            )?.to_uint_floor();
            let after_fee = decimal_subtraction(Decimal::one(), config.usage_fee)?;
            funds_b = decimal_multiplication(
                Decimal::from_ratio(funds_b, Uint128::one()), after_fee
            )?.to_uint_floor();
            Some((fee_amount, pair.paired_asset.clone()))
        }
    } else if config.usage_fee == Decimal::one() {
        return Err(ContractError::InvalidFunds { reason: "Blocking non-allowlisted usage".into() });
    } else {
        return Err(ContractError::Validation("usage_fee is zero but must always be charged".into()));
    };

    // 1:1 swap
    let (offered_asset, offered_amount, received_asset, received_amount) = if !funds_a.is_zero() {
        let receive_amount = funds_a;
        ensure_contract_balance(deps.querier, &env, &pair.paired_asset, receive_amount)?;
        (pair.cdt.clone(), funds_a, pair.paired_asset.clone(), receive_amount)
    } else {
        let receive_amount = funds_b;
        ensure_contract_balance(deps.querier, &env, &pair.cdt, receive_amount)?;
        (pair.paired_asset.clone(), funds_b, pair.cdt.clone(), receive_amount)
    };

    // Track deployed paired asset for allowlisted venues
    let is_allowlisted = is_allowlisted_sender(&deps.querier, &info.sender, &config)?;
    if is_allowlisted && is_deployment_venue(&deps.querier, &info.sender, &config)? {
        let current = DEPLOYED_PAIRED_ASSET.load(deps.storage).unwrap_or(Uint128::zero());
        let new_value = if offered_asset == pair.paired_asset {
            current.saturating_sub(offered_amount)
        } else {
            current.checked_add(received_amount)
                .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?
        };
        DEPLOYED_PAIRED_ASSET.save(deps.storage, &new_value)?;
    }

    let mut send_coins: Vec<Coin> = vec![];
    if !received_amount.is_zero() {
        send_coins.push(coin(received_amount.u128(), received_asset.clone()));
    }

    append_transmute_snapshot(
        deps.storage, config.swap_history_cap,
        TransmuteSnapshot {
            offered_asset: offered_asset.clone(), offered_amount,
            received_asset: received_asset.clone(), received_amount,
            block_time: env.block.time,
        },
    )?;

    let mut window = VOLUME_WINDOW.load(deps.storage)?;
    apply_volume_update(
        deps.storage, &mut window,
        if offered_asset == pair.cdt { offered_amount } else { Uint128::zero() },
        if received_asset == pair.cdt { received_amount } else { Uint128::zero() },
        if offered_asset == pair.paired_asset { offered_amount } else { Uint128::zero() },
        if received_asset == pair.paired_asset { received_amount } else { Uint128::zero() },
    )?;
    VOLUME_WINDOW.save(deps.storage, &window)?;

    let mut response = Response::new()
        .add_attribute("action", "transmute")
        .add_attribute("offered_asset", &offered_asset)
        .add_attribute("offered_amount", offered_amount.to_string())
        .add_attribute("received_asset", &received_asset)
        .add_attribute("received_amount", received_amount.to_string())
        .add_attribute("recipient", recipient_addr.as_str());

    if let Some((fee_amount_val, fee_denom)) = fee_info.clone() {
        response = response
            .add_attribute("swap_fee", fee_amount_val)
            .add_attribute("swap_fee_denom", &fee_denom);
    } else {
        response = response
            .add_attribute("swap_fee", Uint128::zero())
            .add_attribute("swap_fee_denom", "");
    }

    if !send_coins.is_empty() {
        response = response.add_message(BankMsg::Send {
            to_address: recipient_addr.to_string(),
            amount: send_coins,
        });
    }

    // Fee distribution
    if let Some((fee_amount_val, fee_denom)) = fee_info {
        if config.send_swap_fee {
            if let Some(rd_addr) = &config.revenue_distributor_addr {
                let msgs = collect_and_distribute_fees(
                    deps.storage, &deps.querier, &env, &config,
                    fee_amount_val, fee_denom, rd_addr.clone(),
                )?;
                for msg in msgs {
                    response = response.add_message(msg);
                }
            }
        } else {
            // Fees stay in contract → track as undistributed for tranche distribution
            let undist = UNDISTRIBUTED_FEES.load(deps.storage)?;
            UNDISTRIBUTED_FEES.save(deps.storage, &undist.checked_add(fee_amount_val)?)?;
        }
    }

    if let Some(acq_msg) = build_acquisition_notification(&config) {
        response = response.add_message(acq_msg);
    }

    Ok(response)
}

// ================= Volume Window =================

fn execute_update_volume_window(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let window = VOLUME_WINDOW.load(deps.storage)?;
    append_volume_window(deps.storage, config.volume_history_cap, window)?;
    let cumulative_volume = CUMULATIVE_VOLUME.load(deps.storage)?;
    VOLUME_WINDOW.save(deps.storage, &new_volume_window(env.block.time, cumulative_volume))?;
    Ok(Response::new().add_attribute("action", "update_volume_window"))
}

// ================= Rate History =================

fn execute_add_to_rate_history(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
    let last_update = LAST_RATE_UPDATE.may_load(deps.storage)?.unwrap_or(Timestamp::from_seconds(0));
    let current_time = env.block.time;
    let seconds_since = current_time.seconds().saturating_sub(last_update.seconds());
    const SECONDS_PER_DAY: u64 = 86400;

    if seconds_since < SECONDS_PER_DAY {
        return Err(ContractError::Validation(
            format!("Rate history can only be updated once per day. Last update was {} seconds ago", seconds_since),
        ));
    }

    let config = CONFIG.load(deps.storage)?;
    let total_tranche_staked = TOTAL_TRANCHE_STAKED.load(deps.storage)?;
    let (total_deposit_value, _) = get_total_deposit_value(deps.querier, &env, &config, deps.storage)?;

    // Use total deposit value as a combined rate metric
    let conversion_rate = if total_tranche_staked.is_zero() {
        Uint128::new(1_000_000_000_000)
    } else {
        let ratio = Decimal::from_ratio(total_deposit_value, total_tranche_staked);
        decimal_multiplication(ratio, Decimal::from_ratio(Uint128::new(1_000_000_000_000), Uint128::one()))?
            .to_uint_floor()
    };

    append_rate_history_entry(deps.storage, 365, RateHistoryEntry {
        conversion_rate,
        timestamp: current_time,
    })?;

    LAST_RATE_UPDATE.save(deps.storage, &current_time)?;

    Ok(Response::new()
        .add_attribute("action", "add_to_rate_history")
        .add_attribute("conversion_rate", conversion_rate.to_string()))
}

// ================= Repay User Debt =================

fn execute_repay_user_debt(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    user_info: membrane::types::UserInfo,
    repayment: Uint128,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if info.sender.to_string() != user_info.position_owner
        && info.sender.to_string() != config.cdp_contract
    {
        return Err(ContractError::Unauthorized {});
    }

    let user_retrievable_cdt = query_retrievable_cdt(
        deps.storage, deps.querier, env.clone(), user_info.position_owner.clone(),
    )?;

    if user_retrievable_cdt.is_zero() {
        return Err(ContractError::Validation("no unlocked CDT available for repayment".into()));
    }

    let exit_response = execute_exit_vault(
        deps, env.clone(),
        MessageInfo { sender: env.contract.address.clone(), funds: vec![] },
        Some(env.contract.address.to_string()),
        Some(config.deposit_pair.cdt.clone()),
        Some(user_info.position_owner.clone()),
        None,
        None, // proportional from all tranches
    )?;

    let cdt_withdrawn = exit_response.attributes.iter()
        .find(|a| a.key == "cdt_withdrawn")
        .and_then(|a| a.value.parse::<u128>().ok())
        .map(Uint128::from)
        .unwrap_or(Uint128::zero());

    if cdt_withdrawn.is_zero() {
        return Err(ContractError::Validation("no CDT was withdrawn from vault exit".into()));
    }

    let actual_repayment = std::cmp::min(repayment, cdt_withdrawn);

    let repay_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.cdp_contract.to_string(),
        msg: to_json_binary(&CDP_ExecuteMsg::Repay {
            position_id: user_info.position_id.clone(),
            position_owner: Some(user_info.position_owner.clone()),
            send_excess_to: Some(user_info.position_owner.clone()),
            debt_split: None,
        })?,
        funds: vec![coin(actual_repayment.u128(), config.deposit_pair.cdt.clone())],
    });

    let mut response = exit_response
        .add_message(repay_msg)
        .add_attributes(vec![
            attr("action", "repay_user_debt"),
            attr("position_id", user_info.position_id.to_string()),
            attr("position_owner", &user_info.position_owner),
            attr("requested_repayment", repayment.to_string()),
            attr("actual_repayment", actual_repayment.to_string()),
        ]);

    if cdt_withdrawn > actual_repayment {
        let excess = cdt_withdrawn.checked_sub(actual_repayment)?;
        response = response.add_message(BankMsg::Send {
            to_address: user_info.position_owner.clone(),
            amount: vec![coin(excess.u128(), config.deposit_pair.cdt.clone())],
        });
    }

    Ok(response)
}

// ================= Transfer Deposit Ownership =================

fn execute_transfer_deposit_ownership(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    user: String,
    amount: Uint128,
    new_owner: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let new_owner_addr = deps.api.addr_validate(&new_owner)?;

    // Only acquisition contract can transfer
    let is_from_acquisition = config.acquisition_contract
        .as_ref()
        .map(|acq| user == acq.to_string())
        .unwrap_or(false);
    if !is_from_acquisition {
        return Err(ContractError::Unauthorized {});
    }

    if amount.is_zero() {
        return Err(ContractError::Validation("transfer amount cannot be zero".into()));
    }

    // Decrement ACQUISITION_DEPOSIT_TOTAL
    let acq_total = ACQUISITION_DEPOSIT_TOTAL.load(deps.storage)?;
    let new_acq_total = acq_total.checked_sub(amount)
        .map_err(|_| ContractError::Validation(
            format!("transfer amount {} exceeds acquisition deposit total {}", amount, acq_total),
        ))?;
    ACQUISITION_DEPOSIT_TOTAL.save(deps.storage, &new_acq_total)?;

    // For transferred acquisition deposits, we need to put them into a default tranche
    // Since there's no tranche preference, we put into a "default" senior tranche
    // The caller should specify tranches via a separate EnterVault call
    // For now, add to TOTAL_TRANCHE_STAKED as unallocated
    // Actually, the new_owner should call EnterVault separately with tranche preferences.
    // We just convert the acquisition deposit to CDT and send to new_owner.
    let balances = current_balances(deps.querier, &env, &config.deposit_pair)?;
    let total_balance = balances.0.checked_add(balances.1)?;
    let (a, b) = calculate_withdrawal_assets(amount, total_balance, balances.0, balances.1)?;

    let mut send_coins = vec![];
    if !a.is_zero() {
        send_coins.push(coin(a.u128(), config.deposit_pair.cdt.clone()));
    }
    if !b.is_zero() {
        send_coins.push(coin(b.u128(), config.deposit_pair.paired_asset.clone()));
    }

    let mut response = Response::new()
        .add_attributes(vec![
            attr("action", "transfer_deposit_ownership"),
            attr("from", "acquisition"),
            attr("user", &user),
            attr("amount", amount.to_string()),
            attr("new_owner", &new_owner),
        ]);

    if !send_coins.is_empty() {
        response = response.add_message(BankMsg::Send {
            to_address: new_owner_addr.to_string(),
            amount: send_coins,
        });
    }

    Ok(response)
}

// ================= Affiliates =================

fn execute_set_affiliate(
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

    if let Some(existing) = affiliations.iter().find(|a| a.affiliate_address == affiliate_address) {
        if info.sender.to_string() != existing.affiliate_address {
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
            return Err(ContractError::Std(StdError::generic_err(
                format!("Can't add more than {} affiliations", crate::state::AFFILIATE_LIMIT),
            )));
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

fn add_affiliate_from_deposit(
    storage: &mut dyn Storage,
    api: &dyn cosmwasm_std::Api,
    user: String,
    affiliate_address: String,
    affiliate_fee: Decimal,
    current_time: u64,
    label: Option<String>,
) -> Result<(), ContractError> {
    let _valid_addr = api.addr_validate(&affiliate_address)?;
    let mut affiliations = crate::state::AFFILIATES.load(storage, user.clone()).unwrap_or_default();

    if affiliations.iter().any(|a| a.affiliate_address == affiliate_address) {
        return Ok(());
    }

    if affiliations.len() >= crate::state::AFFILIATE_LIMIT {
        return Err(ContractError::Std(StdError::generic_err(
            format!("Can't add more than {} affiliations", crate::state::AFFILIATE_LIMIT),
        )));
    }

    affiliations.push(membrane::types::AffiliateData {
        affiliate_address,
        affiliate_fee,
        time_affiliated: current_time,
        label,
    });

    crate::state::AFFILIATES.save(storage, user, &affiliations)?;
    Ok(())
}

// ================= Query =================

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::VaultInfo {} => to_json_binary(&query_vault_info(deps, env)?),
        QueryMsg::TransmuteHistory { start_after, limit } => {
            to_json_binary(&query_swap_history(deps, start_after, limit)?)
        }
        QueryMsg::VolumeHistory { start_after, limit } => {
            to_json_binary(&query_volume_history(deps, start_after, limit)?)
        }
        QueryMsg::RateHistory { start_after, limit } => {
            to_json_binary(&query_rate_history(deps, start_after, limit)?)
        }
        QueryMsg::VolumeWindow {} => {
            let window = VOLUME_WINDOW.load(deps.storage)?;
            to_json_binary(&VolumeWindowResponse { window })
        }
        QueryMsg::DeployedPairedAsset {} => {
            let amount = DEPLOYED_PAIRED_ASSET.load(deps.storage).unwrap_or(Uint128::zero());
            to_json_binary(&membrane::transmuter::DeployedPairedAssetResponse { amount })
        }
        QueryMsg::EffectiveTarget {} => {
            let target = compute_effective_cdt_target_ratio(deps, &env, &CONFIG.load(deps.storage)?)?;
            to_json_binary(&membrane::transmuter::EffectiveTargetResponse { target })
        }
        QueryMsg::GetAffiliates { user } => {
            to_json_binary(&crate::state::AFFILIATES.load(deps.storage, user).unwrap_or_default())
        }
        QueryMsg::RetrievableCDT { user } => {
            to_json_binary(&query_retrievable_cdt(deps.storage, deps.querier, env, user)?)
        }
        QueryMsg::AcquisitionDepositTotal {} => {
            let total = ACQUISITION_DEPOSIT_TOTAL.may_load(deps.storage)?.unwrap_or(Uint128::zero());
            to_json_binary(&membrane::transmuter::AcquisitionDepositTotalResponse { total })
        }
        QueryMsg::UserTranches { user } => to_json_binary(&query_user_tranches(deps, user)?),
        QueryMsg::TrancheInfo { collateral_denom, junior } => {
            to_json_binary(&query_tranche_info(deps, collateral_denom, junior)?)
        }
        QueryMsg::AllTranches {} => to_json_binary(&query_all_tranches(deps)?),
        QueryMsg::TrancheConversionRate { collateral_denom, junior } => {
            to_json_binary(&query_tranche_conversion_rate(deps, collateral_denom, junior)?)
        }
        QueryMsg::MaxAssignableBadDebt { collateral_denom } => {
            to_json_binary(&query_max_assignable_bad_debt(deps, collateral_denom)?)
        }
    }
}

fn query_vault_info(deps: Deps, env: Env) -> StdResult<VaultInfoResponse> {
    let config = CONFIG.load(deps.storage)?;
    let (total_deposit_value, paired_asset_balance) = get_total_deposit_value(
        deps.querier, &env, &config, deps.storage,
    ).map_err(|_| StdError::generic_err("Failed to query total deposit value"))?;
    let total_tranche_staked = TOTAL_TRANCHE_STAKED.load(deps.storage)?;
    let cdt_balance = total_deposit_value.saturating_sub(paired_asset_balance);

    Ok(VaultInfoResponse {
        total_deposit_value,
        total_tranche_staked,
        cdt_balance,
        paired_asset_balance,
    })
}

fn query_retrievable_cdt(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    user: String,
) -> StdResult<Uint128> {
    let config = CONFIG.load(storage)?;

    // Sum user's tranche base values
    let user_addr = cosmwasm_std::Addr::unchecked(&user);
    let prefix = USER_TRANCHES.prefix(user_addr.as_str());
    let mut total_underlying = Uint128::zero();

    for item in prefix.range(storage, None, None, Order::Ascending) {
        let ((denom, js), vt_amount) = item?;
        if vt_amount.is_zero() {
            continue;
        }
        let tranche = TRANCHE_STATES.may_load(storage, (denom.as_str(), js.as_str()))?
            .unwrap_or(TrancheState { vault_token_supply: Uint128::zero(), total_staked: Uint128::zero() });
        if !tranche.vault_token_supply.is_zero() {
            let base = calculate_base_tokens(vt_amount, tranche.total_staked, tranche.vault_token_supply)?;
            total_underlying = total_underlying.checked_add(base)?;
        }
    }

    // Cap at contract CDT balance
    let cdt_balance = querier.query_balance(&env.contract.address, &config.deposit_pair.cdt)?.amount;
    Ok(std::cmp::min(total_underlying, cdt_balance))
}

fn query_user_tranches(deps: Deps, user: String) -> StdResult<UserTranchesResponse> {
    let user_addr = cosmwasm_std::Addr::unchecked(&user);
    let prefix = USER_TRANCHES.prefix(user_addr.as_str());
    let mut tranches = vec![];

    for item in prefix.range(deps.storage, None, None, Order::Ascending) {
        let ((denom, js), vt_amount) = item?;
        if !vt_amount.is_zero() {
            tranches.push(TrancheSplit {
                asset: denom,
                junior: js == "j",
                vault_token_amount: vt_amount,
            });
        }
    }

    Ok(UserTranchesResponse { tranches })
}

fn query_tranche_info(deps: Deps, collateral_denom: String, junior: bool) -> StdResult<TrancheInfoResponse> {
    let js = junior_str(junior);
    let state = TRANCHE_STATES
        .may_load(deps.storage, (collateral_denom.as_str(), js))?
        .unwrap_or(TrancheState { vault_token_supply: Uint128::zero(), total_staked: Uint128::zero() });

    Ok(TrancheInfoResponse { collateral_denom, junior, state })
}

fn query_all_tranches(deps: Deps) -> StdResult<AllTranchesResponse> {
    let all: Vec<((String, String), TrancheState)> = TRANCHE_STATES
        .range(deps.storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<_>>>()?;

    let tranches = all.into_iter().map(|((denom, js), state)| {
        TrancheInfoResponse {
            collateral_denom: denom,
            junior: js == "j",
            state,
        }
    }).collect();

    Ok(AllTranchesResponse { tranches })
}

fn query_tranche_conversion_rate(deps: Deps, collateral_denom: String, junior: bool) -> StdResult<TrancheConversionRateResponse> {
    let js = junior_str(junior);
    let state = TRANCHE_STATES
        .may_load(deps.storage, (collateral_denom.as_str(), js))?
        .unwrap_or(TrancheState { vault_token_supply: Uint128::zero(), total_staked: Uint128::zero() });

    let rate = if state.vault_token_supply.is_zero() {
        Decimal::one()
    } else {
        Decimal::from_ratio(state.total_staked, state.vault_token_supply)
    };

    Ok(TrancheConversionRateResponse { rate })
}

fn query_max_assignable_bad_debt(deps: Deps, collateral_denom: String) -> StdResult<MaxAssignableBadDebtResponse> {
    let junior = TRANCHE_STATES
        .may_load(deps.storage, (collateral_denom.as_str(), "j"))?
        .map(|s| s.total_staked)
        .unwrap_or(Uint128::zero());
    let senior = TRANCHE_STATES
        .may_load(deps.storage, (collateral_denom.as_str(), "s"))?
        .map(|s| s.total_staked)
        .unwrap_or(Uint128::zero());

    Ok(MaxAssignableBadDebtResponse {
        max_amount: junior.checked_add(senior)?,
    })
}

fn query_swap_history(
    deps: Deps, start_after: Option<u64>, limit: Option<u32>,
) -> StdResult<TransmuteHistoryResponse> {
    let history = TRANSMUTE_HISTORY.load(deps.storage)?;
    let total = history_total(&history);
    let (slice, start_index) = history_slice(&history, start_after, limit);

    let records: Vec<SwapRecord> = slice.into_iter().map(|s| SwapRecord {
        offered_asset: s.offered_asset,
        offered_amount: s.offered_amount,
        received_asset: s.received_asset,
        received_amount: s.received_amount,
        block_time: s.block_time,
    }).collect();

    let next_start_after = if records.is_empty() {
        None
    } else {
        let last_index = start_index + records.len() - 1;
        if (last_index as u64) + 1 < total { Some(last_index as u64) } else { None }
    };

    Ok(TransmuteHistoryResponse { records, total, next_start_after })
}

fn query_volume_history(
    deps: Deps, start_after: Option<u64>, limit: Option<u32>,
) -> StdResult<VolumeHistoryResponse> {
    let mut history = VOLUME_HISTORY.load(deps.storage)?;
    let window = VOLUME_WINDOW.load(deps.storage)?;
    history.push(window);

    let total = history_total(&history);
    let (records, start_index) = history_slice(&history, start_after, limit);

    let next_start_after = if records.is_empty() {
        None
    } else {
        let last_index = start_index + records.len() - 1;
        if (last_index as u64) + 1 < total { Some(last_index as u64) } else { None }
    };

    Ok(VolumeHistoryResponse { records, total, next_start_after })
}

fn query_rate_history(
    deps: Deps, start_after: Option<u64>, limit: Option<u32>,
) -> StdResult<RateHistoryResponse> {
    let history = RATE_HISTORY.load(deps.storage)?;
    let total = history_total(&history);
    let (records, start_index) = history_slice(&history, start_after, limit);

    let next_start_after = if records.is_empty() {
        None
    } else {
        let last_index = start_index + records.len() - 1;
        if (last_index as u64) + 1 < total { Some(last_index as u64) } else { None }
    };

    Ok(RateHistoryResponse { records, total, next_start_after })
}

// ================= Helpers =================

fn validate_asset_pair(pair: &AssetPair) -> Result<(), ContractError> {
    if pair.cdt.is_empty() || pair.paired_asset.is_empty() {
        return Err(ContractError::Validation("asset denoms must be non-empty".into()));
    }
    if pair.cdt == pair.paired_asset {
        return Err(ContractError::Validation("asset A and asset B must differ".into()));
    }
    Ok(())
}

fn validate_distribution_ratios(distributions: &[membrane::types::LiqAsset]) -> Result<(), ContractError> {
    let total_ratio: Decimal = distributions.iter()
        .map(|liq| liq.amount)
        .fold(Decimal::zero(), |acc, x| acc + x);
    let diff = if total_ratio > Decimal::one() {
        total_ratio - Decimal::one()
    } else {
        Decimal::one() - total_ratio
    };
    if diff > Decimal::percent(1) {
        return Err(ContractError::Validation(
            "revenue_distributions ratios must sum to approximately 1.0".into(),
        ));
    }
    Ok(())
}

fn validate_no_duplicate_tranches(tranches: &[(String, bool)]) -> Result<(), ContractError> {
    let mut seen = std::collections::HashSet::new();
    for (denom, junior) in tranches {
        if !seen.insert((denom.clone(), *junior)) {
            return Err(ContractError::DuplicateTranche {
                denom: denom.clone(),
                junior: *junior,
            });
        }
    }
    Ok(())
}

fn validate_collateral_denoms(
    querier: &QuerierWrapper,
    config: &Config,
    tranches: &[(String, bool)],
) -> Result<(), ContractError> {
    // Query the collateral contract for valid collateral types
    let basket: BasketCollateral = querier.query_wasm_smart(
        config.collateral_contract.clone(),
        &CollateralQueryMsg::GetBasket {},
    ).map_err(|e| ContractError::Std(e))?;

    let valid_denoms: std::collections::HashSet<String> = basket.collateral_types.iter()
        .map(|c| c.asset.info.to_string())
        .collect();

    for (denom, _) in tranches {
        if !valid_denoms.contains(denom) {
            return Err(ContractError::InvalidCollateralDenom { denom: denom.clone() });
        }
    }
    Ok(())
}

fn ensure_owner(config: &Config, sender: &Addr) -> Result<(), ContractError> {
    if &config.owner != sender {
        return Err(ContractError::Unauthorized {});
    }
    Ok(())
}

fn is_allowlisted_sender(_querier: &QuerierWrapper, sender: &Addr, config: &Config) -> StdResult<bool> {
    if sender.to_string() == config.cdp_contract {
        return Ok(true);
    }
    Ok(config.allowlist.iter().any(|a| a == &sender.to_string()))
}

fn is_deployment_venue(querier: &QuerierWrapper, sender: &Addr, config: &Config) -> StdResult<bool> {
    let venues: Vec<String> = querier.query_wasm_smart(
        config.cdp_contract.to_string(),
        &CDP_QueryMsg::GetActiveDeploymentVenues {
            venue: Some(sender.to_string()),
            start_after: None,
            limit: None,
        },
    )?;
    Ok(!venues.is_empty())
}

fn segregate_funds(pair: &AssetPair, info: &MessageInfo) -> Result<(Uint128, Uint128), ContractError> {
    let mut asset_a_total = Uint128::zero();
    let mut asset_b_total = Uint128::zero();

    for fund in &info.funds {
        if fund.denom == pair.cdt {
            asset_a_total = asset_a_total.checked_add(fund.amount)
                .map_err(|err| ContractError::Std(err.into()))?;
        } else if fund.denom == pair.paired_asset {
            asset_b_total = asset_b_total.checked_add(fund.amount)
                .map_err(|err| ContractError::Std(err.into()))?;
        } else {
            return Err(ContractError::InvalidFunds {
                reason: format!("unsupported denom: {}", fund.denom),
            });
        }
    }

    Ok((asset_a_total, asset_b_total))
}

fn current_balances(
    querier: QuerierWrapper, env: &Env, pair: &AssetPair,
) -> StdResult<(Uint128, Uint128)> {
    let assets = vec![
        AssetInfo::NativeToken { denom: pair.cdt.clone() },
        AssetInfo::NativeToken { denom: pair.paired_asset.clone() },
    ];
    let balances = get_contract_balances(querier, env.clone(), assets)?;
    Ok((balances[0], balances[1]))
}

fn compute_effective_cdt_target_ratio(deps: Deps, env: &Env, config: &Config) -> StdResult<Decimal> {
    let (total_deposits, _) = get_total_deposit_value(deps.querier, env, config, deps.storage)
        .map_err(|e| StdError::generic_err(format!("{e}")))?;
    if total_deposits.is_zero() {
        return Ok(config.cdt_target_ratio);
    }
    let deployed_paired = DEPLOYED_PAIRED_ASSET.load(deps.storage).unwrap_or(Uint128::zero());
    let min_target = Decimal::from_ratio(deployed_paired, total_deposits);
    Ok(if min_target > config.cdt_target_ratio { min_target } else { config.cdt_target_ratio })
}

fn ensure_deposit_alignment(
    querier: QuerierWrapper, env: &Env, config: &Config,
    effective_target: Decimal, deposit_a: Uint128, deposit_b: Uint128,
) -> Result<(), ContractError> {
    if deposit_a.is_zero() && deposit_b.is_zero() {
        return Err(ContractError::InvalidFunds { reason: "no deposit value provided".into() });
    }

    let balances_after = current_balances(querier, env, &config.deposit_pair)?;
    let pre_a = balances_after.0.checked_sub(deposit_a).map_err(|e| ContractError::Std(e.into()))?;
    let pre_b = balances_after.1.checked_sub(deposit_b).map_err(|e| ContractError::Std(e.into()))?;

    let total_before = pre_a.checked_add(pre_b).map_err(|e| ContractError::Std(e.into()))?;
    let deposit_value = deposit_a.checked_add(deposit_b).map_err(|e| ContractError::Std(e.into()))?;
    if deposit_value.is_zero() {
        return Err(ContractError::InvalidFunds { reason: "deposit value is zero".into() });
    }

    let target = effective_target;
    let leeway = config.composition_leeway;

    if total_before.is_zero() {
        let deposit_ratio = Decimal::from_ratio(deposit_a, deposit_value);
        ensure_within_leeway(deposit_ratio, target, leeway)?;
        return Ok(());
    }

    let current_ratio = Decimal::from_ratio(pre_a, total_before);
    let total_after = balances_after.0.checked_add(balances_after.1).map_err(|e| ContractError::Std(e.into()))?;
    if total_after.is_zero() {
        return Err(ContractError::InvalidFunds { reason: "resulting deposit value is zero".into() });
    }

    let new_ratio = Decimal::from_ratio(balances_after.0, total_after);
    let current_diff = decimal_abs_diff(current_ratio, target)?;
    let new_diff = decimal_abs_diff(new_ratio, target)?;

    match ensure_within_leeway(new_ratio, target, leeway) {
        Ok(()) => Ok(()),
        Err(_) => {
            if new_diff > current_diff {
                return Err(ContractError::Validation(
                    "deposit would move composition away from target".into(),
                ));
            }
            Ok(())
        }
    }
}

fn ensure_within_leeway(actual: Decimal, target: Decimal, leeway: Decimal) -> Result<(), ContractError> {
    let diff = decimal_abs_diff(actual, target)?;
    if diff > leeway {
        return Err(ContractError::SlippageExceeded {});
    }
    Ok(())
}

fn ensure_contract_balance(
    querier: QuerierWrapper, env: &Env, denom: &str, amount: Uint128,
) -> Result<(), ContractError> {
    if amount.is_zero() {
        return Ok(());
    }
    let balance = querier.query_balance(env.contract.address.clone(), denom)?.amount;
    if balance < amount {
        return Err(ContractError::InsufficientLiquidity(denom.to_string()));
    }
    Ok(())
}

fn decimal_abs_diff(a: Decimal, b: Decimal) -> Result<Decimal, ContractError> {
    if a >= b {
        decimal_subtraction(a, b).map_err(|err| ContractError::Std(err.into()))
    } else {
        decimal_subtraction(b, a).map_err(|err| ContractError::Std(err.into()))
    }
}

fn get_total_deposit_value(
    querier: QuerierWrapper, env: &Env, config: &Config, storage: &dyn Storage,
) -> Result<(Uint128, Uint128), ContractError> {
    let balances = current_balances(querier, env, &config.deposit_pair)?;
    let total = balances.0.checked_add(balances.1).map_err(|e| ContractError::Std(e.into()))?;
    let acq_total = ACQUISITION_DEPOSIT_TOTAL.may_load(storage)?.unwrap_or(Uint128::zero());
    Ok((total.saturating_sub(acq_total), balances.1))
}

fn calculate_withdrawal_assets(
    effective_value: Uint128, total_balance: Uint128,
    balance_a: Uint128, balance_b: Uint128,
) -> Result<(Uint128, Uint128), ContractError> {
    if total_balance.is_zero() {
        return Ok((Uint128::zero(), Uint128::zero()));
    }
    let share = Decimal::from_ratio(effective_value, total_balance);
    let a_out = decimal_multiplication(
        Decimal::from_ratio(balance_a, Uint128::one()), share,
    )?.to_uint_floor();
    let b_out = decimal_multiplication(
        Decimal::from_ratio(balance_b, Uint128::one()), share,
    )?.to_uint_floor();
    Ok((a_out, b_out))
}

fn build_acquisition_notification(config: &Config) -> Option<CosmosMsg> {
    config.acquisition_contract.as_ref().map(|addr| {
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: addr.to_string(),
            msg: to_json_binary(&membrane::acquisition::ExecuteMsg::AccruePool {}).unwrap(),
            funds: vec![],
        })
    })
}

fn collect_and_distribute_fees(
    storage: &mut dyn Storage,
    querier: &QuerierWrapper,
    env: &Env,
    config: &Config,
    current_fee_amount: Uint128,
    current_fee_denom: String,
    rd_addr: Addr,
) -> Result<Vec<CosmosMsg>, ContractError> {
    let mut messages = Vec::new();
    if current_fee_amount.is_zero() {
        return Ok(messages);
    }

    let pending_revenue = PENDING_REVENUE.load(storage).unwrap_or(Uint128::zero());

    let (total_fee_amount, total_fee_denom) = if current_fee_denom == config.deposit_pair.paired_asset {
        let new_pending = pending_revenue.checked_add(current_fee_amount)
            .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
        (new_pending, config.deposit_pair.paired_asset.clone())
    } else {
        (current_fee_amount, current_fee_denom)
    };

    let amount_to_send = decimal_multiplication(
        Decimal::from_ratio(total_fee_amount, Uint128::one()),
        config.revenue_distributor_fee_percentage,
    )?.to_uint_floor();

    if amount_to_send.is_zero() {
        if total_fee_denom == config.deposit_pair.paired_asset {
            PENDING_REVENUE.save(storage, &total_fee_amount)?;
        }
        return Ok(messages);
    }

    if total_fee_denom == config.deposit_pair.cdt {
        let mut ltv_disco_distribution = Vec::new();
        for liq_asset in &config.revenue_distributions {
            let amount = decimal_multiplication(
                Decimal::from_ratio(amount_to_send, Uint128::one()), liq_asset.amount,
            )?.to_uint_floor();
            if !amount.is_zero() {
                ltv_disco_distribution.push(membrane::types::Asset {
                    info: liq_asset.info.clone(),
                    amount,
                });
            }
        }

        let total_dist = ltv_disco_distribution.iter().map(|a| a.amount).sum::<Uint128>();
        if total_dist > amount_to_send {
            return Err(ContractError::Std(StdError::generic_err("Total distribution > amount to send")));
        }

        messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: rd_addr.to_string(),
            msg: to_json_binary(&RevenueDistributorExecuteMsg::SetPromises {
                promises: vec![],
                per_asset_distribution: Some(ltv_disco_distribution),
            })?,
            funds: vec![coin(amount_to_send.u128(), config.deposit_pair.cdt.clone())],
        }));

        return Ok(messages);
    }

    // Paired asset fees: try to convert to CDT first
    let cdt_balance = querier.query_balance(&env.contract.address, &config.deposit_pair.cdt)?.amount;

    if cdt_balance.is_zero() {
        PENDING_REVENUE.save(storage, &total_fee_amount)?;
        return Ok(messages);
    }

    let amount_to_transmute = std::cmp::min(amount_to_send, cdt_balance);
    let cdt_received = amount_to_transmute;

    let remaining_pending = total_fee_amount.checked_sub(amount_to_transmute)
        .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
    PENDING_REVENUE.save(storage, &remaining_pending)?;

    if !cdt_received.is_zero() {
        let mut ltv_disco_distribution = Vec::new();
        for liq_asset in &config.revenue_distributions {
            let amount = decimal_multiplication(
                Decimal::from_ratio(cdt_received, Uint128::one()), liq_asset.amount,
            )?.to_uint_floor();
            if !amount.is_zero() {
                ltv_disco_distribution.push(membrane::types::Asset {
                    info: liq_asset.info.clone(),
                    amount,
                });
            }
        }

        messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: rd_addr.to_string(),
            msg: to_json_binary(&RevenueDistributorExecuteMsg::SetPromises {
                promises: vec![],
                per_asset_distribution: Some(ltv_disco_distribution),
            })?,
            funds: vec![coin(cdt_received.u128(), config.deposit_pair.cdt.clone())],
        }));
    }

    Ok(messages)
}
