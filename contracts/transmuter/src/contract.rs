
use cosmwasm_std::{
    Addr, BankMsg, Binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, Int128, MessageInfo, QuerierWrapper, Response, StdError, StdResult, Storage, SubMsg, Timestamp, Uint128, WasmMsg, attr, coin, entry_point, to_json_binary
};
use cw2::set_contract_version;
// use cw_storage_plus::Bound;

use membrane::helpers::get_contract_balances;
use membrane::math::{decimal_multiplication, decimal_subtraction};
use membrane::cdp::{QueryMsg as CDP_QueryMsg, ExecuteMsg as CDP_ExecuteMsg};
use membrane::transmuter::{
    AssetPair, Config, ExecuteMsg, InstantiateMsg, QueryMsg, TransmuteHistoryResponse, SwapRecord,
    VaultInfoResponse, VolumeHistoryResponse, VolumeWindowResponse, RateLimitStatus, RateLimitStatusResponse, RateLimitManyResponse,
    GlobalRateLimitResponse, RateHistoryResponse, RateHistoryEntry,
};
use membrane::types::StringEntry;
use membrane::types::AssetInfo;
use membrane::revenue_distributor::ExecuteMsg as RevenueDistributorExecuteMsg;
use membrane::system_discounts::{QueryMsg as SystemsDiscountsQueryMsg, UserBoostResponse};

use crate::error::ContractError;
use crate::state::{
    append_transmute_snapshot, append_volume_window, append_rate_history_entry, apply_volume_update, history_slice,
    history_total, init_history, new_volume_window, CONFIG, TRANSMUTE_HISTORY, VOLUME_HISTORY,
    VOLUME_WINDOW, DEPOSIT_TOTAL, TransmuteSnapshot, RATE_LIMIT_FLOWS, FlowEntry, DEPLOYED_PAIRED_ASSET,
    TOKEN_RATE_ASSURANCE, TokenRateAssurance, GLOBAL_RATE_LIMIT_FLOWS, PENDING_REVENUE, CUMULATIVE_VOLUME,
    RATE_HISTORY, LAST_RATE_UPDATE, USER_DEPOSITS, UserDeposit, MIN_DEPOSIT_AMOUNT, MAX_DEPOSITS_PER_USER, DEPOSIT_CONSOLIDATION_WINDOW_SECS,
    EmissionsEvent, RetentionWeightTracking, WeightTimeCliff, EMISSIONS_EVENTS, RETENTION_WEIGHT_TRACKING, GLOBAL_RETENTION_WEIGHT_TRACKING, LAST_EMISSIONS_DISTRIBUTION,
    CURRENT_DEPOSIT_ID,
};

const CONTRACT_NAME: &str = "membrane-transmuter";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

// Retention emissions ramp constants (mirroring MBRN discounts)
const RETENTION_FIRST_MONTH_DAYS: u64 = 30; // First month duration in days
const RETENTION_FIRST_MONTH_WEIGHT: Decimal = Decimal::raw(600_000_000_000_000_000u128); // 60% of max weight (0.6)
const RETENTION_REMAINING_WEIGHT: Decimal = Decimal::raw(400_000_000_000_000_000u128); // 40% of max weight (0.4)
const RETENTION_CURVE_DURATION_DAYS: u64 = 90; // Total curve duration in days (3 months)

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
        return Err(ContractError::Validation(
            "swap_history_cap must be greater than zero".into(),
        ));
    }
    if msg.volume_history_cap == 0 {
        return Err(ContractError::Validation(
            "volume_history_cap must be greater than zero".into(),
        ));
    }
    if msg.cdt_target_ratio > Decimal::one() {
        return Err(ContractError::Validation(
            "cdt_target_ratio must be less than or equal to 1".into(),
        ));
    }

    //Save revenue_distributions early to avoid partial move
    let revenue_distributions = msg.revenue_distributions.clone();

    //Save values early to avoid partial move issues
    let cdp_contract = msg.cdp_contract.clone();
    let discounts_contract = msg.discounts_contract.clone();
    let swap_history_cap = msg.swap_history_cap;
    let volume_history_cap = msg.volume_history_cap;

    //Validate the revenue distributor, cdp, and discounts contract addresses
    let revenue_distributor_addr = if let Some(addr_str) = msg.revenue_distributor_addr {
        Some(deps.api.addr_validate(&addr_str)?)
    } else {
        None
    };
    let _ = deps.api.addr_validate(&cdp_contract)?;
    let _ = deps.api.addr_validate(&discounts_contract)?;

    // Default usage_fee to 1%
    let usage_fee = msg
        .usage_fee
        .unwrap_or(Decimal::percent(1));
    if usage_fee > Decimal::one() {
        return Err(ContractError::Validation(
            "usage_fee must be less than or equal to 1".into(),
        ));
    }

    // Default usage_fee_utilization_threshold to 80%
    let usage_fee_utilization_threshold = msg
        .usage_fee_utilization_threshold
        .unwrap_or(Decimal::percent(80));
    if usage_fee_utilization_threshold > Decimal::one() {
        return Err(ContractError::Validation(
            "usage_fee_utilization_threshold must be less than or equal to 1".into(),
        ));
    }

    // Validate affiliate_fee (required, no default)
    let affiliate_fee = msg.affiliate_fee;
    if affiliate_fee > Decimal::one() {
        return Err(ContractError::Validation(
            "affiliate_fee must be less than or equal to 1".into(),
        ));
    }

    // Validate lock_ceiling
    let lock_ceiling = msg.lock_ceiling;
    if lock_ceiling == 0 {
        return Err(ContractError::Validation(
            "lock_ceiling must be greater than zero".into(),
        ));
    }

    // Defaults for per-address rate limiting
    let rate_limit_window_secs = msg.rate_limit_window_secs.unwrap_or(60 * 60 * 8); // 8 hours
    if rate_limit_window_secs == 0 {
        return Err(ContractError::Validation(
            "rate_limit_window_secs must be greater than zero".into(),
        ));
    }
    let rate_limit_threshold = msg
        .rate_limit_threshold
        .unwrap_or(Decimal::percent(5));
    if rate_limit_threshold.is_zero() || rate_limit_threshold > Decimal::one() {
        return Err(ContractError::Validation(
            "rate_limit_threshold must be > 0 and <= 1".into(),
        ));
    }

    // Defaults for global rate limiting
    let global_rate_limit_window_secs = msg.global_rate_limit_window_secs.unwrap_or(60 * 60 * 24); // 24 hours
    if global_rate_limit_window_secs == 0 {
        return Err(ContractError::Validation(
            "global_rate_limit_window_secs must be greater than zero".into(),
        ));
    }
    let global_rate_limit_threshold = msg
        .global_rate_limit_threshold
        .unwrap_or(Decimal::percent(5));
    if global_rate_limit_threshold.is_zero() || global_rate_limit_threshold > Decimal::one() {
        return Err(ContractError::Validation(
            "global_rate_limit_threshold must be > 0 and <= 1".into(),
        ));
    }

    // Validate revenue distributions if provided
    let revenue_distributions = revenue_distributions.unwrap_or_default();
    if !revenue_distributions.is_empty() {
        // If revenue distributions are provided, revenue distributor address must also be provided
        if revenue_distributor_addr.is_none() {
            return Err(ContractError::Validation(
                "revenue_distributor_addr must be provided when revenue_distributions are specified".into(),
            ));
        }
        // Validate that ratios sum to 1.0
        let total_ratio: Decimal = revenue_distributions.iter()
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
    }

    // Default send_swap_fee to true (send fees to revenue distributor)
    let send_swap_fee = msg.send_swap_fee.unwrap_or(true);

    // Default revenue_distributor_fee_percentage to 20%
    let revenue_distributor_fee_percentage = msg.revenue_distributor_fee_percentage.unwrap_or(Decimal::percent(20));
    if revenue_distributor_fee_percentage > Decimal::one() {
        return Err(ContractError::Validation(
            "revenue_distributor_fee_percentage must be less than or equal to 1".into(),
        ));
    }

    let config = Config {
        owner: owner.clone(),
        tokenfactory_contract: None,
        cdp_contract,
        discounts_contract, 
        deposit_pair: msg.deposit_pair,
        composition_leeway: msg.composition_leeway,
        cdt_target_ratio: msg.cdt_target_ratio,  
        usage_fee,
        usage_fee_utilization_threshold,
        swap_history_cap,
        volume_history_cap,
        rate_limit_window_secs,
        rate_limit_threshold,
        allowlist: msg.allowlist.unwrap_or_default(),
        allowlist_rate_limit_threshold: msg.allowlist_rate_limit_threshold.unwrap_or(rate_limit_threshold),
        global_rate_limit_window_secs,
        global_rate_limit_threshold,
        revenue_distributor_addr,
        revenue_distributions,
        lock_ceiling,
        affiliate_fee,
        send_swap_fee,
        revenue_distributor_fee_percentage,
        emissions_voting_contract: msg.emissions_voting_contract
            .map(|s| deps.api.addr_validate(&s))
            .transpose()?,
        acquisition_contract: msg.acquisition_contract
            .map(|s| deps.api.addr_validate(&s))
            .transpose()?,
        points_system_contract: msg.points_system_contract
            .map(|s| deps.api.addr_validate(&s))
            .transpose()?,
    };

    CONFIG.save(deps.storage, &config)?;
    DEPOSIT_TOTAL.save(deps.storage, &Uint128::zero())?;
    init_history(deps.storage)?;
    CUMULATIVE_VOLUME.save(deps.storage, &Uint128::zero())?;
    let cumulative_volume = CUMULATIVE_VOLUME.load(deps.storage)?;
    VOLUME_WINDOW.save(deps.storage, &new_volume_window(env.block.time, cumulative_volume))?;
    DEPLOYED_PAIRED_ASSET.save(deps.storage, &Uint128::zero())?;
    GLOBAL_RATE_LIMIT_FLOWS.save(deps.storage, &Vec::new())?;
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

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::UpdateConfig {
            owner,
            deposit_pair,
            composition_leeway,
            cdt_target_ratio,
            tokenfactory_contract: _,
            discounts_contract,
            cdp_contract,
            usage_fee,
            usage_fee_utilization_threshold,
            swap_history_cap,
            volume_history_cap,
            rate_limit_window_secs,
            rate_limit_threshold,
            allowlist,
            allowlist_rate_limit_threshold,
            global_rate_limit_window_secs,
            global_rate_limit_threshold,
            revenue_distributor_addr,
            revenue_distributions,
            lock_ceiling,
            affiliate_fee,
            send_swap_fee,
            revenue_distributor_fee_percentage,
            emissions_voting_contract,
            acquisition_contract,
            points_system_contract,
        } => execute_update_config(
            deps,
            env,
            info.clone(),
            owner,
            deposit_pair,
            composition_leeway,
            cdt_target_ratio,
            discounts_contract,
            cdp_contract,
            usage_fee,
            usage_fee_utilization_threshold,
            swap_history_cap,
            volume_history_cap,
            rate_limit_window_secs,
            rate_limit_threshold,
            allowlist,
            allowlist_rate_limit_threshold,
            global_rate_limit_window_secs,
            global_rate_limit_threshold,
            revenue_distributor_addr,
            revenue_distributions,
            lock_ceiling,
            affiliate_fee,
            send_swap_fee,
            revenue_distributor_fee_percentage,
            emissions_voting_contract,
            acquisition_contract,
            points_system_contract,
        ),
        ExecuteMsg::EnterVault { recipient, lock_days, affiliate_address, affiliate_label } => execute_enter_vault(deps, env, info.clone(), recipient, lock_days, affiliate_address, affiliate_label),
        ExecuteMsg::DepositFee {} => execute_deposit_fee(deps, env, info.clone()),
        ExecuteMsg::ExitVault {
            recipient,
            withdraw_as,
            user,
            deposit_id,
            amount,
        } => execute_exit_vault(deps, env, info.clone(), recipient, withdraw_as, user, deposit_id, amount),
        ExecuteMsg::Lock { amount, lock_days } => execute_lock(deps, env, info.clone(), amount, lock_days),
        ExecuteMsg::Transmute { recipient } => execute_transmute(deps, env, info.clone(), recipient),
        ExecuteMsg::UpdateVolumeWindow {} => execute_update_volume_window(deps, env),
        ExecuteMsg::RateAssurance {} => execute_rate_assurance(deps, env, info.clone()),
        ExecuteMsg::SetAffiliate { user, affiliate_address, label } => execute_set_affiliate(deps, env, info, user, affiliate_address, label),
        ExecuteMsg::AddToRateHistory {} => execute_add_to_rate_history(deps, env),
        ExecuteMsg::RepayUserDebt { user_info, repayment } => execute_repay_user_debt(deps, env, info, user_info, repayment),
        ExecuteMsg::DistributeRetentionEmissions {} => execute_distribute_retention_emissions(deps, env, info),
        ExecuteMsg::ClaimRetentionEmissions {} => execute_claim_retention_emissions(deps, env, info),
        ExecuteMsg::TransferDepositOwnership { user, deposit_id, new_owner } => {
            execute_transfer_deposit_ownership(deps, env, info, user, deposit_id, new_owner)
        }
    }
}

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
    usage_fee: Option<Decimal>,
    usage_fee_utilization_threshold: Option<Decimal>,
    swap_history_cap: Option<u32>,
    volume_history_cap: Option<u32>,
    rate_limit_window_secs: Option<u64>,
    rate_limit_threshold: Option<Decimal>,
    allowlist: Option<Vec<StringEntry>>,
    allowlist_rate_limit_threshold: Option<Decimal>,
    global_rate_limit_window_secs: Option<u64>,
    global_rate_limit_threshold: Option<Decimal>,
    revenue_distributor_addr: Option<String>,
    revenue_distributions: Option<Vec<membrane::types::DistributionEntry>>,
    lock_ceiling: Option<u64>,
    affiliate_fee: Decimal,
    send_swap_fee: Option<bool>,
    revenue_distributor_fee_percentage: Option<Decimal>,
    emissions_voting_contract: Option<String>,
    acquisition_contract: Option<String>,
    points_system_contract: Option<String>,
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
            return Err(ContractError::Validation(
                "composition_leeway must be less than or equal to 1".into(),
            ))
        }
        config.composition_leeway = leeway;
    }

    if let Some(target) = cdt_target_ratio {
        if target > Decimal::one() {
            return Err(ContractError::Validation(
                "target ratio must be less than or equal to 1".into(),
            ));
        }
        config.cdt_target_ratio = target;
    }

    if let Some(disss) = discounts_contract {
        //Validate the address
        deps.api.addr_validate(&disss)?;
        config.discounts_contract = disss;
    }


    if let Some(cdp) = cdp_contract {
        //Validate the address
        deps.api.addr_validate(&cdp)?;
        config.cdp_contract = cdp;
    }

    if let Some(fee) = usage_fee {
        if fee > Decimal::one() {
            return Err(ContractError::Validation(
                "usage_fee must be less than or equal to 1".into(),
            ));
        }
        config.usage_fee = fee;
    }

    if let Some(threshold) = usage_fee_utilization_threshold {
        if threshold > Decimal::one() {
            return Err(ContractError::Validation(
                "usage_fee_utilization_threshold must be less than or equal to 1".into(),
            ));
        }
        config.usage_fee_utilization_threshold = threshold;
    }

    if let Some(cap) = swap_history_cap {
        if cap == 0 {
            return Err(ContractError::Validation(
                "swap_history_cap must be greater than zero".into(),
            ));
        }
        config.swap_history_cap = cap;
    }

    if let Some(cap) = volume_history_cap {
        if cap == 0 {
            return Err(ContractError::Validation(
                "volume_history_cap must be greater than zero".into(),
            ));
        }
        config.volume_history_cap = cap;
    }

    if let Some(window) = rate_limit_window_secs {
        if window == 0 {
            return Err(ContractError::Validation(
                "rate_limit_window_secs must be greater than zero".into(),
            ));
        }
        config.rate_limit_window_secs = window;
    }

    if let Some(threshold) = rate_limit_threshold {
        if threshold.is_zero() || threshold > Decimal::one() {
            return Err(ContractError::Validation(
                "rate_limit_threshold must be > 0 and <= 1".into(),
            ));
        }
        config.rate_limit_threshold = threshold;
    }

    if let Some(entries) = allowlist {
        // apply add/remove semantics
        for e in entries {
            if e.remove {
                config.allowlist.retain(|addr| addr != &e.entry);
            } else if !config.allowlist.iter().any(|addr| addr == &e.entry) {
                config.allowlist.push(e.entry);
            }
        }
    }

    if let Some(wl_threshold) = allowlist_rate_limit_threshold {
        if wl_threshold.is_zero() || wl_threshold > Decimal::one() {
            return Err(ContractError::Validation(
                "allowlist_rate_limit_threshold must be > 0 and <= 1".into(),
            ));
        }
        config.allowlist_rate_limit_threshold = wl_threshold;
    }

    if let Some(window) = global_rate_limit_window_secs {
        if window == 0 {
            return Err(ContractError::Validation(
                "global_rate_limit_window_secs must be greater than zero".into(),
            ));
        }
        config.global_rate_limit_window_secs = window;
    }

    if let Some(threshold) = global_rate_limit_threshold {
        if threshold.is_zero() || threshold > Decimal::one() {
            return Err(ContractError::Validation(
                "global_rate_limit_threshold must be > 0 and <= 1".into(),
            ));
        }
        config.global_rate_limit_threshold = threshold;
    }

    if let Some(rd_addr_str) = revenue_distributor_addr {
        let validated_addr = deps.api.addr_validate(&rd_addr_str)?;
        config.revenue_distributor_addr = Some(validated_addr);
    }

    if let Some(distributions) = revenue_distributions {
        // Apply add/remove distribution routes
        for entry in distributions {
            if entry.remove {
                config.revenue_distributions.retain(|liq_asset| liq_asset.info != entry.asset.info);
            } else {
                // Add or update distribution entry
                config.revenue_distributions.retain(|liq_asset| liq_asset.info != entry.asset.info);
                config.revenue_distributions.push(entry.asset);
            }
        }
        // Validate that ratios sum to 1.0
        let total_ratio: Decimal = config.revenue_distributions.iter()
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
    }

    if let Some(ceiling) = lock_ceiling {
        if ceiling == 0 {
            return Err(ContractError::Validation(
                "lock_ceiling must be greater than zero".into(),
            ));
        }
        config.lock_ceiling = ceiling;
    }

    // Validate and update affiliate_fee (required)
    if affiliate_fee > Decimal::one() {
        return Err(ContractError::Validation(
            "affiliate_fee must be less than or equal to 1".into(),
        ));
    }
    config.affiliate_fee = affiliate_fee;

    if let Some(send_fee) = send_swap_fee {
        config.send_swap_fee = send_fee;
    }

    if let Some(fee_percentage) = revenue_distributor_fee_percentage {
        if fee_percentage > Decimal::one() {
            return Err(ContractError::Validation(
                "revenue_distributor_fee_percentage must be less than or equal to 1".into(),
            ));
        }
        config.revenue_distributor_fee_percentage = fee_percentage;
    }

    if let Some(ev_addr_str) = emissions_voting_contract {
        let validated_addr = deps.api.addr_validate(&ev_addr_str)?;
        config.emissions_voting_contract = Some(validated_addr);
    }

    if let Some(acq_addr_str) = acquisition_contract {
        let validated_addr = deps.api.addr_validate(&acq_addr_str)?;
        config.acquisition_contract = Some(validated_addr);
    }

    if let Some(ps_addr_str) = points_system_contract {
        let validated_addr = deps.api.addr_validate(&ps_addr_str)?;
        config.points_system_contract = Some(validated_addr);
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("action", "update_config"))
}

/// Build an AccruePool notification message for the acquisition contract (if configured).
/// Fire-and-forget: if acquisition contract errors, the transmuter tx reverts to keep accrual in sync.
fn build_acquisition_notification(config: &Config) -> Option<CosmosMsg> {
    config.acquisition_contract.as_ref().map(|addr| {
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: addr.to_string(),
            msg: to_json_binary(&membrane::acquisition::ExecuteMsg::AccruePool {}).unwrap(),
            funds: vec![],
        })
    })
}

fn execute_enter_vault(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    recipient: Option<String>,
    lock_days: Option<u64>,
    affiliate_address: Option<String>,
    affiliate_label: Option<String>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut deposit_total = DEPOSIT_TOTAL.load(deps.storage)?;
    let mut messages: Vec<CosmosMsg> = vec![];


    //Split the funds into the two assets
    let (deposit_a, deposit_b) = segregate_funds(&config.deposit_pair, &info)?;
    if deposit_a.is_zero() && deposit_b.is_zero() {
        return Err(ContractError::InvalidFunds {
            reason: "no valid funds provided".into(),
        });
    }

    // println!("deposit_a: {:?}", deposit_a);
    // println!("deposit_b: {:?}", deposit_b);
    //If the sender is not the revenue distributor, ensure the deposit is aligned with the deposit pair
    //Revenue distributor can deposit any ratio into the contract.
    //Which will tend to be 100% CDT.
    // Currently revenue is going to the Disco though.
    let is_revenue_distributor = config.revenue_distributor_addr.as_ref()
        .map(|rd_addr| info.sender == *rd_addr)
        .unwrap_or(false);
    if !is_revenue_distributor && !deposit_total.is_zero() {
        // Compute effective CDT target ratio considering deployed paired asset
        let effective_target = compute_effective_cdt_cdt_target_ratio(deps.as_ref(), &env, &config)?;
        // Ensure the deposit is aligned with the effective target
        ensure_deposit_alignment(
            deps.querier,
            &env,
            &config,
            effective_target,
            deposit_a,
            deposit_b,
        )?;
    }

    //Calc user deposit value (1:1 tracking: CDT + paired asset)
    let user_deposit_value = deposit_a.checked_add(deposit_b)
        .map_err(|e| ContractError::Std(StdError::from(e)))?;
    if user_deposit_value.is_zero() {
        return Err(ContractError::InvalidFunds {
            reason: "deposit value is zero".into(),
        });
    }

    // Validate minimum deposit amount
    if user_deposit_value < MIN_DEPOSIT_AMOUNT {
        return Err(ContractError::Validation(
            format!("Deposit amount {} is below minimum of {}", user_deposit_value, MIN_DEPOSIT_AMOUNT),
        ));
    }

    //Calc & save deposit total for rate assurance
    // Save current DEPOSIT_TOTAL before adding new deposit
    TOKEN_RATE_ASSURANCE.save(deps.storage, &TokenRateAssurance {
        pre_deposit_total: deposit_total,
    })?;

    //Add rate assurance callback msg
    if !deposit_total.is_zero() {
        messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::RateAssurance {})?,
            funds: vec![],
        }));
    }

    //Get the recipient address
    let recipient_addr = recipient
        .map(|r| deps.api.addr_validate(&r))
        .transpose()?;
    let recipient_addr = recipient_addr.unwrap_or_else(|| info.sender.clone());

    // Validate lock_days if provided
    if let Some(lock_days) = lock_days {
        if lock_days > config.lock_ceiling {
            return Err(ContractError::Validation(
                format!("lock_days ({}) exceeds lock_ceiling ({})", lock_days, config.lock_ceiling),
            ));
        }
        if lock_days == 0 {
            return Err(ContractError::Validation(
                "lock_days must be greater than zero".into(),
            ));
        }
    }

    // Update DEPOSIT_TOTAL
    deposit_total = deposit_total.checked_add(user_deposit_value)
        .map_err(|e| ContractError::Std(StdError::from(e)))?;
    // Handle affiliate if provided
    if let Some(affiliate_addr) = affiliate_address {
        add_affiliate_from_deposit(
            deps.storage,
            deps.api,
            recipient_addr.to_string(),
            affiliate_addr,
            config.affiliate_fee,
            env.block.time.seconds(),
            affiliate_label,
        )?;
    }

    DEPOSIT_TOTAL.save(deps.storage, &deposit_total)?;

    // Track deposit for discount & incentive calculations
    let current_time = env.block.time.seconds();
    let locked_info = if let Some(lock_days) = lock_days {
        const SECONDS_PER_DAY: u64 = 86_400;
        let locked_until = current_time
            .checked_add(lock_days.checked_mul(SECONDS_PER_DAY).ok_or_else(|| {
                ContractError::Validation("lock_days overflow when calculating seconds".into())
            })?)
            .ok_or_else(|| ContractError::Validation("locked_until timestamp overflow".into()))?;
        Some(membrane::types::Locked {
            locked_until,
            perpetual_lock: None,
            intended_lock_days: Some(lock_days),
        })
    } else {
        None
    };

    // Assign deposit ID
    let deposit_id = CURRENT_DEPOSIT_ID
        .may_load(deps.storage)?
        .unwrap_or(Uint128::one());
    CURRENT_DEPOSIT_ID.save(deps.storage, &deposit_id.checked_add(Uint128::one())
        .map_err(|e| ContractError::Std(StdError::generic_err(format!("Deposit ID overflow: {}", e))))?)?;

    // Add deposit to user's deposit list
    add_user_deposit(
        deps.storage,
        &recipient_addr.to_string(),
        UserDeposit {
            deposit_id,
            amount: user_deposit_value,
            deposit_time: current_time,
            locked: locked_info,
            start_time: current_time,
        },
    )?;

    // Update retention weight tracking
    let user_deposits_after = USER_DEPOSITS
        .may_load(deps.storage, recipient_addr.to_string())?
        .unwrap_or_default();
    update_retention_weight_tracking(
        deps.storage,
        &recipient_addr.to_string(),
        &user_deposits_after,
        &env,
        config.lock_ceiling,
    )?;

    let mut response = Response::new()
        .add_messages(messages)
        .add_attributes(vec![
            attr("action", "enter_vault"),
            attr("deposit_amount", user_deposit_value.to_string()),
            attr("recipient", recipient_addr.as_str()),
        ]);

    // Notify acquisition contract of utilization change
    if let Some(acq_msg) = build_acquisition_notification(&config) {
        response = response.add_message(acq_msg);
    }

    Ok(response)
}

/// Add a user deposit with consolidation logic to prevent state bloat
fn add_user_deposit(
    storage: &mut dyn Storage,
    user: &str,
    new_deposit: UserDeposit,
) -> Result<(), ContractError> {
    let mut deposits = USER_DEPOSITS
        .may_load(storage, user.to_string())?
        .unwrap_or_default();

    // Try to consolidate with existing deposits
    let mut consolidated = false;
    let current_time = new_deposit.deposit_time;
    
    for deposit in &mut deposits {
        // Check if deposits have same lock characteristics
        let same_lock = match (&deposit.locked, &new_deposit.locked) {
            (Some(d_lock), Some(n_lock)) => d_lock.locked_until == n_lock.locked_until,
            (None, None) => true,
            _ => false,
        };

        // Check if within consolidation window (1 week)
        let time_diff = if deposit.deposit_time > current_time {
            deposit.deposit_time - current_time
        } else {
            current_time - deposit.deposit_time
        };

        if same_lock && time_diff <= DEPOSIT_CONSOLIDATION_WINDOW_SECS {
            // Merge deposits: combine amounts, use earlier start_time
            deposit.amount = deposit.amount.checked_add(new_deposit.amount)
                .map_err(|e| ContractError::Std(StdError::from(e)))?;
            deposit.start_time = deposit.start_time.min(new_deposit.start_time);
            consolidated = true;
            break;
        }
    }

    // If not consolidated, add as new deposit
    if !consolidated {
        deposits.push(new_deposit);
    }

    // If we exceed max deposits, consolidate oldest deposits with same lock characteristics
    if deposits.len() > MAX_DEPOSITS_PER_USER {
        // Sort by deposit_time (oldest first)
        deposits.sort_by_key(|d| d.deposit_time);
        
        // Group by lock characteristics and consolidate within groups
        let mut consolidated_deposits: Vec<UserDeposit> = Vec::new();
        
        for deposit in deposits {
            let mut found = false;
            for cons_deposit in &mut consolidated_deposits {
                let same_lock = match (&cons_deposit.locked, &deposit.locked) {
                    (Some(c_lock), Some(d_lock)) => c_lock.locked_until == d_lock.locked_until,
                    (None, None) => true,
                    _ => false,
                };
                
                if same_lock {
                    cons_deposit.amount = cons_deposit.amount.checked_add(deposit.amount)
                        .map_err(|e| ContractError::Std(StdError::from(e)))?;
                    cons_deposit.start_time = cons_deposit.start_time.min(deposit.start_time);
                    found = true;
                    break;
                }
            }
            
            if !found {
                consolidated_deposits.push(deposit);
            }
        }
        
        deposits = consolidated_deposits;
    }

    // Remove deposits below minimum amount
    deposits.retain(|d| d.amount >= MIN_DEPOSIT_AMOUNT);

    USER_DEPOSITS.save(storage, user.to_string(), &deposits)?;
    Ok(())
}

/// Update user deposits when vault tokens are withdrawn
fn update_user_deposits_on_withdrawal(
    storage: &mut dyn Storage,
    user: &str,
    withdrawn_amount: Uint128,
) -> Result<(), ContractError> {
    let deposits = USER_DEPOSITS
        .may_load(storage, user.to_string())?
        .unwrap_or_default();
    
    let mut deposits = deposits;

    if deposits.is_empty() {
        return Ok(());
    }

    // Calculate total deposit amount
    let total_deposit_amount: Uint128 = deposits.iter()
        .map(|d| d.amount)
        .sum();

    if total_deposit_amount.is_zero() || withdrawn_amount >= total_deposit_amount {
        // Remove all deposits if withdrawing all or more
        deposits.clear();
    } else {
        // Reduce deposits proportionally
        let reduction_ratio = Decimal::from_ratio(withdrawn_amount, total_deposit_amount);
        
        for deposit in &mut deposits {
            let reduction = decimal_multiplication(
                Decimal::from_ratio(deposit.amount, Uint128::one()),
                reduction_ratio,
            )?.to_uint_floor();
            
            deposit.amount = deposit.amount.checked_sub(reduction)
                .unwrap_or(Uint128::zero());
        }

        // Remove deposits below minimum amount
        deposits.retain(|d| d.amount >= MIN_DEPOSIT_AMOUNT);
    }

    if deposits.is_empty() {
        USER_DEPOSITS.remove(storage, user.to_string());
    } else {
        USER_DEPOSITS.save(storage, user.to_string(), &deposits)?;
    }

    Ok(())
}

fn execute_deposit_fee(deps: DepsMut, _env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    // let mut total_deposits = get_total_deposit_value(deps.querier, &env, &config)?;

    let (deposit_a, deposit_b) = segregate_funds(&config.deposit_pair, &info)?;
    if deposit_a.is_zero() && deposit_b.is_zero() {
        return Err(ContractError::InvalidFunds {
            reason: "no valid funds provided".into(),
        });
    }

    // let deposit_base = sum_base_value(deposit_a, deposit_b, config.asset_a_to_b_rate)?;
    // total_deposits = total_deposits
    //     .checked_add(deposit_base)
    //     .map_err(|err| ContractError::Std(err.into()))?;

    // VAULT_TOKEN_SUPPLY.save(deps.storage, &total_deposits)?;

    Ok(Response::new().add_attributes(vec![
        attr("action", "deposit_fee"),
        attr("asset_a", deposit_a.to_string()),
        attr("asset_b", deposit_b.to_string()),
    ]))
}

fn execute_lock(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    amount: Uint128,
    lock_days: u64,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    
    // Validate lock_days
    if lock_days > config.lock_ceiling {
        return Err(ContractError::Validation(
            format!("lock_days ({}) exceeds lock_ceiling ({})", lock_days, config.lock_ceiling),
        ));
    }
    if lock_days == 0 {
        return Err(ContractError::Validation(
            "lock_days must be greater than zero".into(),
        ));
    }
    
    if amount.is_zero() {
        return Err(ContractError::Validation(
            "Amount must be greater than zero".into(),
        ));
    }
    
    // Load user deposits
    let mut user_deposits = USER_DEPOSITS
        .may_load(deps.storage, info.sender.to_string())?
        .unwrap_or_default();
    
    if user_deposits.is_empty() {
        return Err(ContractError::Validation("No deposits found for user".into()));
    }
    
    // Sort deposits by deposit_time (oldest first)
    user_deposits.sort_by_key(|d| d.deposit_time);
    
    // Calculate lock_until timestamp
    const SECONDS_PER_DAY: u64 = 86_400;
    let current_time = env.block.time.seconds();
    let locked_until = current_time
        .checked_add(lock_days.checked_mul(SECONDS_PER_DAY).ok_or_else(|| {
            ContractError::Validation("lock_days overflow when calculating seconds".into())
        })?)
        .ok_or_else(|| ContractError::Validation("locked_until timestamp overflow".into()))?;
    
    // Create locked info
    let locked_info = Some(membrane::types::Locked {
        locked_until,
        perpetual_lock: None,
        intended_lock_days: Some(lock_days),
    });
    
    // Lock deposits starting from oldest first until amount is locked
    let mut remaining_to_lock = amount;
    let mut updated_deposits: Vec<UserDeposit> = Vec::new();
    
    for deposit in user_deposits.into_iter() {
        if remaining_to_lock.is_zero() {
            // Keep remaining deposits as-is
            updated_deposits.push(deposit);
            continue;
        }
        
        // Only lock unlocked deposits
        if deposit.locked.is_none() {
            if deposit.amount <= remaining_to_lock {
                // Fully lock this deposit
                updated_deposits.push(UserDeposit {
                    deposit_id: deposit.deposit_id,
                    amount: deposit.amount,
                    deposit_time: deposit.deposit_time,
                    locked: locked_info.clone(),
                    start_time: current_time,
                });
                remaining_to_lock = remaining_to_lock.checked_sub(deposit.amount)
                    .map_err(|e| ContractError::Std(StdError::from(e)))?;
            } else {
                // Partially lock this deposit - split into locked and unlocked
                // Assign new deposit ID for the locked portion
                let locked_deposit_id = CURRENT_DEPOSIT_ID
                    .may_load(deps.storage)?
                    .unwrap_or(Uint128::one());
                CURRENT_DEPOSIT_ID.save(deps.storage, &locked_deposit_id.checked_add(Uint128::one())
                    .map_err(|e| ContractError::Std(StdError::generic_err(format!("Deposit ID overflow: {}", e))))?)?;
                
                updated_deposits.push(UserDeposit {
                    deposit_id: locked_deposit_id,
                    amount: remaining_to_lock,
                    locked: locked_info.clone(),
                    deposit_time: deposit.deposit_time,
                    start_time: current_time,
                });
                
                updated_deposits.push(UserDeposit {
                    deposit_id: deposit.deposit_id,
                    amount: deposit.amount.checked_sub(remaining_to_lock)
                        .map_err(|e| ContractError::Std(StdError::from(e)))?,
                    locked: None,
                    deposit_time: deposit.deposit_time,
                    start_time: deposit.start_time,
                });
                
                remaining_to_lock = Uint128::zero();
            }
        } else {
            // Already locked, keep as-is
            updated_deposits.push(deposit);
        }
    }
    
    if !remaining_to_lock.is_zero() {
        return Err(ContractError::Validation(
            format!("Insufficient unlocked deposits to lock. Requested: {}, Available: {}", 
                amount, amount.checked_sub(remaining_to_lock).unwrap_or(Uint128::zero())),
        ));
    }
    
    // Save updated deposits
    USER_DEPOSITS.save(deps.storage, info.sender.to_string(), &updated_deposits)?;
    
    // Update retention weight tracking
    update_retention_weight_tracking(
        deps.storage,
        &info.sender.to_string(),
        &updated_deposits,
        &env,
        config.lock_ceiling,
    )?;
    
    Ok(Response::new()
        .add_attributes(vec![
            attr("action", "lock"),
            attr("amount", amount.to_string()),
            attr("lock_days", lock_days.to_string()),
            attr("locked_until", locked_until.to_string()),
        ]))
}


fn execute_exit_vault(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    recipient: Option<String>,
    withdraw_as: Option<String>,
    user: Option<String>,
    deposit_id: Option<Uint128>,
    amount: Option<Uint128>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut deposit_total = DEPOSIT_TOTAL.load(deps.storage)?;

    if deposit_total.is_zero() {
        return Err(ContractError::Validation("no deposits in contract".into()));
    }

    // Determine which user to exit for
    // If user is provided, only the contract itself can use it
    let user_addr = if let Some(user_str) = user {
        if info.sender != env.contract.address {
            return Err(ContractError::Unauthorized {});
        }
        deps.api.addr_validate(&user_str)?
    } else {
        info.sender.clone()
    };

    //Get the recipient address
    let recipient_addr = recipient
        .map(|r| deps.api.addr_validate(&r))
        .transpose()?;
    let recipient_addr = recipient_addr.unwrap_or_else(|| user_addr.clone());

    let current_time = env.block.time.seconds();
    
    // Load user deposits
    let mut user_deposits = USER_DEPOSITS
        .may_load(deps.storage, user_addr.to_string())?
        .unwrap_or_default();
    
    if user_deposits.is_empty() {
        return Err(ContractError::Validation("no deposits found for user".into()));
    }

    // Handle deposit_id-based withdrawal
    let (withdrawable_amount, updated_deposits) = if let Some(target_deposit_id) = deposit_id {
        // Find the specific deposit
        let deposit_index = user_deposits.iter()
            .position(|d| d.deposit_id == target_deposit_id)
            .ok_or_else(|| ContractError::Validation(
                format!("Deposit with ID {} not found", target_deposit_id)
            ))?;
        
        let deposit = &user_deposits[deposit_index];
        
        // Verify deposit is unlocked
        let is_unlocked = match &deposit.locked {
            Some(locked) => locked.locked_until <= current_time,
            None => true,
        };
        
        if !is_unlocked {
            return Err(ContractError::Validation(
                format!("Deposit with ID {} is locked", target_deposit_id)
            ));
        }
        
        // Calculate withdrawal amount
        let withdrawal_amount = if let Some(requested_amount) = amount {
            // Withdraw min(requested_amount, deposit.amount)
            if requested_amount > deposit.amount {
                deposit.amount
            } else {
                requested_amount
            }
        } else {
            // Withdraw entire deposit
            deposit.amount
        };
        
        if withdrawal_amount.is_zero() {
            return Err(ContractError::Validation("Withdrawal amount is zero".into()));
        }
        
        // Update the specific deposit
        let mut updated_deposits = user_deposits.clone();
        if withdrawal_amount >= deposit.amount {
            // Fully withdraw this deposit - remove it
            updated_deposits.remove(deposit_index);
        } else {
            // Partially withdraw this deposit
            updated_deposits[deposit_index] = UserDeposit {
                deposit_id: deposit.deposit_id,
                amount: deposit.amount.checked_sub(withdrawal_amount)
                    .map_err(|e| ContractError::Std(StdError::from(e)))?,
                deposit_time: deposit.deposit_time,
                locked: deposit.locked.clone(),
                start_time: deposit.start_time,
            };
        }
        
        (withdrawal_amount, updated_deposits)
    } else {
        // Original behavior: withdraw all unlocked deposits (newest first)
        // Sort deposits by deposit_time (newest first)
        user_deposits.sort_by(|a, b| b.deposit_time.cmp(&a.deposit_time));
        
        let mut withdrawable_amount = Uint128::zero();
        
        // Calculate total withdrawable (all unlocked deposits)
        for deposit in &user_deposits {
            let is_unlocked = match &deposit.locked {
                Some(locked) => locked.locked_until <= current_time,
                None => true,
            };
            if is_unlocked {
                withdrawable_amount = withdrawable_amount.checked_add(deposit.amount)
                    .map_err(|e| ContractError::Std(StdError::from(e)))?;
            }
        }
        
        if withdrawable_amount.is_zero() {
            return Err(ContractError::Validation("no unlocked deposits available for withdrawal".into()));
        }
        
        // Update user deposits: remove or reduce withdrawn deposits (newest first)
        let mut remaining_withdraw = withdrawable_amount;
        let mut updated_deposits: Vec<UserDeposit> = Vec::new();
        
        for deposit in user_deposits.into_iter() {
            if remaining_withdraw.is_zero() {
                updated_deposits.push(deposit);
                continue;
            }
            
            let is_unlocked = match &deposit.locked {
                Some(locked) => locked.locked_until <= current_time,
                None => true,
            };
            
            if is_unlocked {
                if deposit.amount <= remaining_withdraw {
                    // Fully withdraw this deposit
                    remaining_withdraw = remaining_withdraw.checked_sub(deposit.amount)
                        .map_err(|e| ContractError::Std(StdError::from(e)))?;
                    // Don't add to updated_deposits (fully withdrawn)
                } else {
                    // Partially withdraw this deposit
                    let new_amount = deposit.amount.checked_sub(remaining_withdraw)
                        .map_err(|e| ContractError::Std(StdError::from(e)))?;
                    updated_deposits.push(UserDeposit {
                        deposit_id: deposit.deposit_id,
                        amount: new_amount,
                        deposit_time: deposit.deposit_time,
                        locked: deposit.locked,
                        start_time: deposit.start_time,
                    });
                    remaining_withdraw = Uint128::zero();
                }
            } else {
                // Locked deposit, keep it
                updated_deposits.push(deposit);
            }
        }
        
        (withdrawable_amount, updated_deposits)
    };

    // Save DEPOSIT_TOTAL before withdrawal for rate assurance
    TOKEN_RATE_ASSURANCE.save(deps.storage, &TokenRateAssurance {
        pre_deposit_total: deposit_total,
    })?;

    // Calculate user's share of assets based on withdrawable amount
    let user_share = Decimal::from_ratio(withdrawable_amount, deposit_total);

    //Get the balances of the contract
    let balances = current_balances(deps.querier, &env, &config.deposit_pair)?;
    
    //Calc user share of asset A
    let asset_a_share = decimal_multiplication(
        Decimal::from_ratio(balances.0, Uint128::one()),
        user_share
    )?.to_uint_floor();
    //Calc user share of asset B
    let asset_b_share = decimal_multiplication(
        Decimal::from_ratio(balances.1, Uint128::one()),
        user_share
    )?.to_uint_floor();
    
    // Save updated deposits
    if updated_deposits.is_empty() {
        USER_DEPOSITS.remove(deps.storage, user_addr.to_string());
        // Remove weight tracking if no deposits
        RETENTION_WEIGHT_TRACKING.remove(deps.storage, user_addr.to_string());
    } else {
        USER_DEPOSITS.save(deps.storage, user_addr.to_string(), &updated_deposits)?;
        // Update retention weight tracking
        update_retention_weight_tracking(
            deps.storage,
            &user_addr.to_string(),
            &updated_deposits,
            &env,
            config.lock_ceiling,
        )?;
    }
    
    // Update DEPOSIT_TOTAL
    deposit_total = deposit_total.checked_sub(withdrawable_amount)
        .map_err(|e| ContractError::Std(StdError::from(e)))?;
    DEPOSIT_TOTAL.save(deps.storage, &deposit_total)?;

    let mut send_coins: Vec<Coin> = vec![];
    let mut total_a_needed = asset_a_share;
    let mut total_b_needed = asset_b_share;

    //Send the assets to the recipient (1:1 conversion for withdraw_as)
    match withdraw_as {
        Some(ref denom) if denom == &config.deposit_pair.cdt => {
            // Convert paired asset share to CDT (1:1)
            total_a_needed = asset_a_share
                .checked_add(asset_b_share)
                .map_err(|err| ContractError::Std(err.into()))?;
            if balances.0 < total_a_needed {
                return Err(ContractError::InsufficientLiquidity(config.deposit_pair.cdt.clone()));
            }
            if !total_a_needed.is_zero() {
                send_coins.push(coin(total_a_needed.u128(), denom));
            }
        }
        Some(ref denom) if denom == &config.deposit_pair.paired_asset => {
            // Convert CDT share to paired asset (1:1)
            total_b_needed = asset_a_share
                .checked_add(asset_b_share)
                .map_err(|err| ContractError::Std(err.into()))?;
            if balances.1 < total_b_needed {
                return Err(ContractError::InsufficientLiquidity(config.deposit_pair.paired_asset.clone()));
            }
            if !total_b_needed.is_zero() {
                send_coins.push(coin(total_b_needed.u128(), denom));
            }
        }
        Some(denom) => {
            return Err(ContractError::InvalidAsset { denom });
        }
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
        .add_attribute("action", "exit_vault")
        .add_attribute("user", user_addr.as_str())
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

    //Add rate assurance callback msg
    if !deposit_total.is_zero() {
        response = response.add_message(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::RateAssurance {})?,
            funds: vec![],
        }));
    }

    // Notify acquisition contract of utilization change
    if let Some(acq_msg) = build_acquisition_notification(&config) {
        response = response.add_message(acq_msg);
    }

    Ok(response)
}

fn execute_transmute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    recipient: Option<String>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let pair = &config.deposit_pair;
    //Get the recipient address
    let recipient_addr = recipient
        .map(|r| deps.api.addr_validate(&r))
        .transpose()?;
    let recipient_addr = recipient_addr.unwrap_or_else(|| info.sender.clone());

    let (mut funds_a, mut funds_b) = segregate_funds(pair, &info)?;
    if funds_a.is_zero() && funds_b.is_zero() {
        return Err(ContractError::InvalidFunds {
            reason: "no valid funds provided".into(),
        });
    }
    if !funds_a.is_zero() && !funds_b.is_zero() {
        return Err(ContractError::InvalidFunds {
            reason: "only one asset may be sent for transmute".into(),
        });
    }

    // CDT→paired_asset is restricted to CDP contract only.
    // All other users must swap paired_asset→CDT.
    if !funds_a.is_zero()
        && info.sender != env.contract.address
        && info.sender.to_string() != config.cdp_contract
    {
        return Err(ContractError::CdtToPairedAssetRestricted {});
    }

    // Determine allowlist status
    let is_allowlisted = is_allowlisted_sender(&deps.querier, &info.sender, &config)?;

    //Calc user value sent BEFORE fee deduction (1:1 tracking)
    let user_value_sent = funds_a.checked_add(funds_b)
        .map_err(|e| ContractError::Std(StdError::from(e)))?;

    // Usage fee: one-sided, CDT→paired_asset only, utilization-gated.
    // We're adding friction once LP inventory gets low to try and retain optional exit for LPs.
    // Slows the velocity of liquidity consumption & compensates LPs for being last in line.
    let fee_info: Option<(Uint128, String)> = if !funds_a.is_zero() {
        // Calculate paired_asset utilization to determine if fee should activate.
        // Utilization = 1 - (paired_asset_balance / total_deposit_value).
        // High utilization means paired_asset is scarce.
        let (total_value, pa_balance) = get_total_deposit_value(deps.querier, &env, &config)?;
        let utilization = if total_value.is_zero() {
            Decimal::one()
        } else {
            decimal_subtraction(
                Decimal::one(),
                Decimal::from_ratio(pa_balance, total_value),
            ).unwrap_or(Decimal::one())
        };

        if utilization >= config.usage_fee_utilization_threshold
            && config.usage_fee > Decimal::zero()
            && config.usage_fee < Decimal::one()
        {
            // Fee activates: paired_asset inventory is low, charge fee on CDT→paired_asset
            let fee_rate = config.usage_fee;
            let fee_amount_a = decimal_multiplication(
                Decimal::from_ratio(funds_a, Uint128::one()),
                fee_rate
            )?.to_uint_floor();

            let fee_amount_val = fee_amount_a;
            let fee_denom_val = pair.cdt.clone();

            // Deduct fee from CDT amount
            let usage_fee = decimal_subtraction(Decimal::one(), config.usage_fee)?;
            funds_a = decimal_multiplication(
                Decimal::from_ratio(funds_a, Uint128::one()),
                usage_fee
            )?.to_uint_floor();

            Some((fee_amount_val, fee_denom_val))
        } else if config.usage_fee == Decimal::one() && !is_allowlisted && info.sender != env.contract.address {
            // 100% fee blocks non-allowlisted CDT→paired_asset usage entirely
            return Err(ContractError::InvalidFunds {
                reason: "Blocking non-CDP & non-deployable venue usage".into(),
            });
        } else {
            None
        }
    } else {
        // paired_asset→CDT: no fee
        None
    };
    
    // 1:1 swaps: CDT -> paired asset or paired asset -> CDT at 1:1 value ratio
    let (offered_asset, offered_amount, received_asset, received_amount) = if !funds_a.is_zero() {
        // CDT -> paired asset: 1:1 swap
        let receive_amount = funds_a; // 1:1 value
        ensure_contract_balance(deps.querier, &env, &pair.paired_asset, receive_amount)?;
        (
            pair.cdt.clone(),
            funds_a,
            pair.paired_asset.clone(),
            receive_amount,
        )
    } else {
        // paired asset -> CDT: 1:1 swap
        let receive_amount = funds_b; // 1:1 value
        ensure_contract_balance(deps.querier, &env, &pair.cdt, receive_amount)?;
        (
            pair.paired_asset.clone(),
            funds_b,
            pair.cdt.clone(),
            receive_amount,
        )
    };

    // Per-address sliding window rate limit for both directions with netting.
    // Positive amount for asset_b -> asset_a (USDC->CDT), negative for asset_a -> asset_b.
    let current_time: Timestamp = env.block.time;
    let net_flow_signed: Int128 = if offered_asset == pair.paired_asset {
        // USDC->CDT: positive in base A units equals amount of A received
        Int128::new(received_amount.u128() as i128)
    } else {
        // CDT->USDC: negative in base A units equals amount of A offered
        Int128::new(-(offered_amount.u128() as i128))
    };
    // load and prune existing flows for sender
    let mut entries = RATE_LIMIT_FLOWS.may_load(deps.storage, info.sender.to_string().clone())?.unwrap_or_default();
    let start_secs = current_time.seconds().saturating_sub(config.rate_limit_window_secs);
    let window_start = Timestamp::from_seconds(start_secs);
    entries.retain(|e| e.block_time >= window_start);
    // compute net total including current
    let mut net_total: i128 = 0;
    for e in &entries {
        net_total = net_total.saturating_add(e.amount_base.i128());
    }
    // println!("net_total: {:?}", net_total);
    // println!("net_flow_signed: {:?}", net_flow_signed);
    net_total = net_total.saturating_add(net_flow_signed.i128());

    let net_total_abs = if net_total < 0 { (-net_total) as u128 } else { net_total as u128 };

    //Need to subtract the current deposit value from the total deposits to get the correct threshold
    let total_deposits = get_total_deposit_value(
        deps.querier,
        &env,
        &config
    )?.0 - user_value_sent;
    // choose threshold based on whitelist membership (includes CDP contract)
    let is_allowlisted = is_allowlisted_sender(&deps.querier, &info.sender, &config)?;
    let active_threshold = if is_allowlisted { config.allowlist_rate_limit_threshold } else { config.rate_limit_threshold };
    let threshold_amount = decimal_multiplication(
        Decimal::from_ratio(total_deposits, Uint128::one()),
        active_threshold,
    )?.to_uint_floor();
    // println!("total_deposits: {:?}", total_deposits);
    // println!("active_threshold: {:?}", active_threshold);
    // println!("threshold_amount: {:?}", threshold_amount);
    // println!("net_total_abs: {:?}", net_total_abs);
    if Uint128::from(net_total_abs) > threshold_amount {
        return Err(ContractError::RateLimitExceeded { address: info.sender.to_string() });
    }

    // append current entry and persist
    entries.push(FlowEntry { amount_base: net_flow_signed, block_time: current_time });
    RATE_LIMIT_FLOWS.save(deps.storage, info.sender.to_string().clone(), &entries)?;

    // Global rate limit check for non-whitelisted addresses
    if !is_allowlisted {
        // Load and prune global flows
        let mut global_entries = GLOBAL_RATE_LIMIT_FLOWS.load(deps.storage).unwrap_or_default();
        let global_start_secs = current_time.seconds().saturating_sub(config.global_rate_limit_window_secs);
        let global_window_start = Timestamp::from_seconds(global_start_secs);
        global_entries.retain(|e| e.block_time >= global_window_start);
        
        // Compute net total including current swap
        let mut global_net_total: i128 = 0;
        for e in &global_entries {
            global_net_total = global_net_total.saturating_add(e.amount_base.i128());
        }
        global_net_total = global_net_total.saturating_add(net_flow_signed.i128());

        let global_net_total_abs = if global_net_total < 0 { (-global_net_total) as u128 } else { global_net_total as u128 };

        // Calculate global threshold (percentage of total deposits)
        let global_threshold_amount = decimal_multiplication(
            Decimal::from_ratio(total_deposits, Uint128::one()),
            config.global_rate_limit_threshold,
        )?.to_uint_floor();

        if Uint128::from(global_net_total_abs) > global_threshold_amount {
            return Err(ContractError::GlobalRateLimitExceeded {});
        }

        // Append current entry to global flows and persist
        global_entries.push(FlowEntry { amount_base: net_flow_signed, block_time: current_time });
        GLOBAL_RATE_LIMIT_FLOWS.save(deps.storage, &global_entries)?;
    }

    // Update outstanding paired_asset for allowlisted flows
    if is_allowlisted && is_deployment_venue(&deps.querier, &info.sender, &config)? {
        let current = DEPLOYED_PAIRED_ASSET.load(deps.storage).unwrap_or_else(|_| Uint128::zero());
        let new_value = if offered_asset == pair.paired_asset {
            current.saturating_sub(offered_amount)
        } else {
            current
                .checked_add(received_amount)
                .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?
        };
        DEPLOYED_PAIRED_ASSET.save(deps.storage, &new_value)?;
    }

    let mut send_coins: Vec<Coin> = vec![];
    if !received_amount.is_zero() {
        send_coins.push(coin(received_amount.u128(), received_asset.clone()));
    }

    append_transmute_snapshot(
        deps.storage,
        config.swap_history_cap,
        TransmuteSnapshot {
            offered_asset: offered_asset.clone(),
            offered_amount,
            received_asset: received_asset.clone(),
            received_amount,
            block_time: env.block.time,
        },
    )?;

    let mut window = VOLUME_WINDOW.load(deps.storage)?;
    apply_volume_update(
        deps.storage,
        &mut window,
        if offered_asset == pair.cdt {
            offered_amount
        } else {
            Uint128::zero()
        },
        if received_asset == pair.cdt {
            received_amount
        } else {
            Uint128::zero()
        },
        if offered_asset == pair.paired_asset {
            offered_amount
        } else {
            Uint128::zero()
        },
        if received_asset == pair.paired_asset {
            received_amount
        } else {
            Uint128::zero()
        },
    )?;
    VOLUME_WINDOW.save(deps.storage, &window)?;

    // println!("window: {:?}", VOLUME_WINDOW.load(deps.storage)?);

    let mut response = Response::new()
        .add_attribute("action", "transmute")
        .add_attribute("offered_asset", offered_asset)
        .add_attribute("offered_amount", offered_amount.to_string())
        .add_attribute("received_asset", received_asset)
        .add_attribute("received_amount", received_amount.to_string())
        .add_attribute("recipient", recipient_addr.as_str());

    // Add swap fee attributes (even if zero for tracking purposes)
    if let Some((fee_amount_val, fee_denom)) = fee_info.clone() {
        response = response
            .add_attribute("swap_fee", fee_amount_val)
            .add_attribute("swap_fee_denom", fee_denom.clone());
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

    //Collect and distribute fees if any - pass storage access correctly
    if let Some((fee_amount_val, fee_denom)) = fee_info {
        // Only send fees if send_swap_fee is true and revenue distributor is configured, otherwise fees stay in contract
        if config.send_swap_fee {
            if let Some(rd_addr) = &config.revenue_distributor_addr {
                let messages = collect_and_distribute_fees(
                    deps.storage,
                    &deps.querier,
                    &env,
                    &config,
                    fee_amount_val,
                    fee_denom,
                    rd_addr.clone()
                )?;
                for msg in messages {
                    response = response.add_message(msg);
                }
            }
        }
        // If send_swap_fee is false or revenue distributor not configured, fees remain in the contract balance
    }

    // Notify acquisition contract of utilization change
    if let Some(acq_msg) = build_acquisition_notification(&config) {
        response = response.add_message(acq_msg);
    }

    Ok(response)
}

fn execute_update_volume_window(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let window = VOLUME_WINDOW.load(deps.storage)?;

    append_volume_window(
        deps.storage,
        config.volume_history_cap,
        window,
    )
    .map_err(|err| ContractError::Std(err.into()))?;

    let cumulative_volume = CUMULATIVE_VOLUME.load(deps.storage)?;
    VOLUME_WINDOW.save(deps.storage, &new_volume_window(env.block.time, cumulative_volume))?;

    Ok(Response::new().add_attribute("action", "update_volume_window"))
}

fn execute_add_to_rate_history(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
    // Check if at least 24 hours have passed since last update
    let last_update = LAST_RATE_UPDATE.may_load(deps.storage)?.unwrap_or(Timestamp::from_seconds(0));
    let current_time = env.block.time;
    let seconds_since_last_update = current_time.seconds().saturating_sub(last_update.seconds());
    const SECONDS_PER_DAY: u64 = 86400;
    
    if seconds_since_last_update < SECONDS_PER_DAY {
        return Err(ContractError::Validation(
            format!("Rate history can only be updated once per day. Last update was {} seconds ago", seconds_since_last_update)
        ));
    }

    // Get current conversion rate (1:1 tracking, so 1 deposit unit = 1 base unit)
    let config = CONFIG.load(deps.storage)?;
    let deposit_total = DEPOSIT_TOTAL.load(deps.storage)?;
    let (total_deposit_value, _) = get_total_deposit_value(deps.querier, &env, &config)?;

    // Since 1:1 tracking, conversion rate is 1:1 (1 deposit unit = 1 base unit)
    // Store as 1_000_000_000_000 for consistency with previous format
    let conversion_rate = if deposit_total.is_zero() {
        Uint128::new(1_000_000_000_000)
    } else {
        // Ratio is 1:1, so conversion_rate = (total_deposit_value / deposit_total) * 1_000_000_000_000
        let ratio = Decimal::from_ratio(total_deposit_value, deposit_total);
        decimal_multiplication(ratio, Decimal::from_ratio(Uint128::new(1_000_000_000_000), Uint128::one()))?
            .to_uint_floor()
    };

    // Create rate history entry
    let entry = RateHistoryEntry {
        conversion_rate,
        timestamp: current_time,
    };

    // Append to history (capped at 365 entries)
    append_rate_history_entry(
        deps.storage,
        365,
        entry,
    )
    .map_err(|err| ContractError::Std(err.into()))?;

    // Update last update timestamp
    LAST_RATE_UPDATE.save(deps.storage, &current_time)?;

    Ok(Response::new()
        .add_attribute("action", "add_to_rate_history")
        .add_attribute("conversion_rate", conversion_rate.to_string())
        .add_attribute("timestamp", current_time.seconds().to_string()))
}

fn execute_repay_user_debt(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    user_info: membrane::types::UserInfo,
    repayment: Uint128,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    //Ensure the caller is only the position owner or the cdp _contract
    if info.sender.to_string() != user_info.position_owner.clone() 
    && info.sender.to_string() != config.cdp_contract.to_string() {
        return Err(ContractError::Unauthorized {  });
    }

    //Get retrievable_cdt to know how much can be withdrawn (only unlocked deposits)
    let user_retrievable_cdt = query_retrievable_cdt(
        deps.storage, 
        deps.querier,
        env.clone(), 
        user_info.position_owner.clone()
    )?;
    
    if user_retrievable_cdt.is_zero() {
        return Err(ContractError::Validation("no unlocked CDT available for repayment".into()));
    }

    // Exit vault for the user (withdrawing as CDT to the contract)
    // The contract calls exit_vault with the user parameter
    let exit_response = execute_exit_vault(
        deps,
        env.clone(),
        MessageInfo {
            sender: env.contract.address.clone(),
            funds: vec![],
        },
        Some(env.contract.address.to_string()), // Contract receives the withdrawn CDT
        Some(config.deposit_pair.cdt.clone()), // Withdraw as CDT
        Some(user_info.position_owner.clone()), // Exit for this user
        None, // deposit_id - withdraw all unlocked deposits
        None, // amount - withdraw all unlocked deposits
    )?;

    // Extract cdt_withdrawn from exit response attributes
    // The cdt_withdrawn attribute contains the actual CDT amount withdrawn
    let cdt_withdrawn = exit_response
        .attributes
        .iter()
        .find(|attr| attr.key == "cdt_withdrawn")
        .and_then(|attr| attr.value.parse::<u128>().ok())
        .map(Uint128::from)
        .unwrap_or_else(Uint128::zero);

    if cdt_withdrawn.is_zero() {
        return Err(ContractError::Validation("no CDT was withdrawn from vault exit".into()));
    }

    // Calculate actual repayment amount: min of requested repayment and CDT actually withdrawn
    let actual_repayment = std::cmp::min(repayment, cdt_withdrawn);

    // Repay the user's debt using the actual CDT withdrawn
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

    // Build response with exit messages and repay message
    let mut response = exit_response
        .add_message(repay_msg)
        .add_attributes(vec![
            attr("action", "repay_user_debt"),
            attr("position_id", user_info.position_id.to_string()),
            attr("position_owner", user_info.position_owner.clone().to_string()),
            attr("requested_repayment", repayment.to_string()),
            attr("cdt_withdrawn", cdt_withdrawn.to_string()),
            attr("actual_repayment", actual_repayment.to_string()),
        ]);

    // If there's excess CDT withdrawn (more than needed for repayment), send it back to user
    if cdt_withdrawn > actual_repayment {
        let excess = cdt_withdrawn.checked_sub(actual_repayment)
            .map_err(|e| ContractError::Std(StdError::from(e)))?;
        response = response.add_message(BankMsg::Send {
            to_address: user_info.position_owner.clone(),
            amount: vec![coin(excess.u128(), config.deposit_pair.cdt.clone())],
        });
    }

    Ok(response)
}

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
            let amount = DEPLOYED_PAIRED_ASSET.load(deps.storage).unwrap_or_else(|_| Uint128::zero());
            to_json_binary(&membrane::transmuter::DeployedPairedAssetResponse { amount })
        }
        QueryMsg::EffectiveTarget {} => {
            let target = compute_effective_cdt_cdt_target_ratio(deps, &env, &CONFIG.load(deps.storage)?)?;
            to_json_binary(&membrane::transmuter::EffectiveTargetResponse { target })
        }
        QueryMsg::RateLimitMany { addresses, start_after, limit } => {
            to_json_binary(&query_rate_limit_many(deps, env, addresses, start_after, limit)?)
        }
        QueryMsg::GlobalRateLimit {} => {
            to_json_binary(&query_global_rate_limit(deps, env)?)
        }
        QueryMsg::GetAffiliates { user } => {
            to_json_binary(&crate::state::AFFILIATES.load(deps.storage, user).unwrap_or_else(|_| vec![]))
        }
        QueryMsg::UserDeposits { user } => {
            let deposits = USER_DEPOSITS
                .may_load(deps.storage, user.clone())?
                .unwrap_or_default();
            // Convert internal UserDeposit to membrane::transmuter::UserDeposit
            let response_deposits: Vec<membrane::transmuter::UserDeposit> = deposits
                .into_iter()
                .map(|d| membrane::transmuter::UserDeposit {
                    deposit_id: d.deposit_id,
                    amount: d.amount,
                    deposit_time: d.deposit_time,
                    locked: d.locked,
                    start_time: d.start_time,
                })
                .collect();
            to_json_binary(&membrane::transmuter::UserDepositsResponse { 
                deposits: response_deposits 
            })
        }
        QueryMsg::RetrievableCDT { user } => {
            to_json_binary(&query_retrievable_cdt(deps.storage, deps.querier, env, user)?)
        }
        QueryMsg::UserRetentionEmissions { user } => to_json_binary(&query_user_retention_emissions(deps, env, user)?),
        QueryMsg::GlobalRetentionWeight {} => to_json_binary(&query_global_retention_weight(deps, env)?),
        QueryMsg::EmissionsConfig {} => to_json_binary(&query_emissions_config(deps)?),
        QueryMsg::CurrentDepositId { user } => to_json_binary(&query_current_deposit_id(deps, user)?),
        QueryMsg::DepositById { user, deposit_id } => to_json_binary(&query_deposit_by_id(deps, user, deposit_id)?),
        QueryMsg::VaultTokenUnderlying { vault_token_amount } => to_json_binary(&query_vault_token_underlying(deps, env, vault_token_amount)?),
    }
}

fn query_vault_info(deps: Deps, env: Env) -> StdResult<VaultInfoResponse> {
    let config = CONFIG.load(deps.storage)?;
    let (total_deposit_value, paired_asset_balance) = get_total_deposit_value(deps.querier, &env, &config).map_err(|_err| StdError::generic_err("Failed to query the contract for the total deposit value"))?;
    let deposit_total = DEPOSIT_TOTAL.load(deps.storage)?;
    let cdt_balance = total_deposit_value.saturating_sub(paired_asset_balance);

    Ok(VaultInfoResponse {
        total_deposit_value,
        deposit_total,
        cdt_balance,
        paired_asset_balance,
    })
}

fn query_retrievable_cdt(storage: &dyn Storage, querier: QuerierWrapper, env: Env, user: String) -> StdResult<Uint128> {
    let config: Config = CONFIG.load(storage)?;
    
    // Get user's deposits and calculate only unlocked deposits
    let deposits = USER_DEPOSITS
        .may_load(storage, user.clone())?
        .unwrap_or_default();
    
    let current_time = env.block.time.seconds();
    
    // Calculate only unlocked deposit amount
    let unlocked_deposit_total: Uint128 = deposits
        .iter()
        .filter_map(|d| {
            let is_unlocked = match &d.locked {
                Some(locked) => locked.locked_until <= current_time,
                None => true,
            };
            if is_unlocked {
                Some(d.amount)
            } else {
                None
            }
        })
        .sum();
    
    if unlocked_deposit_total.is_zero() {
        return Ok(Uint128::zero());
    }
    
    // Get DEPOSIT_TOTAL and contract balances
    let deposit_total = DEPOSIT_TOTAL.load(storage)?;
    let balances = current_balances(querier, &env, &config.deposit_pair)?;
    let cdt_balance = balances.0;
    let paired_asset_balance = balances.1;
    
    // Calculate user's share based on unlocked deposits only
    let user_share = Decimal::from_ratio(unlocked_deposit_total, deposit_total);
    
    // Calculate user's share of CDT and paired asset
    let cdt_share = decimal_multiplication(
        Decimal::from_ratio(cdt_balance, Uint128::one()),
        user_share
    )?.to_uint_floor();
    
    let paired_asset_share = decimal_multiplication(
        Decimal::from_ratio(paired_asset_balance, Uint128::one()),
        user_share
    )?.to_uint_floor();
    
    // Convert paired asset share to CDT (1:1 now, so direct addition)
    let total_retrievable_cdt = cdt_share
        .checked_add(paired_asset_share)
        .map_err(|_| StdError::generic_err("Overflow calculating total retrievable CDT"))?;
    
    // Return min of total retrievable CDT and contract CDT balance
    Ok(std::cmp::min(total_retrievable_cdt, cdt_balance))
}

fn build_rate_limit_status(
    deps: Deps,
    env: &Env,
    address: &str,
) -> StdResult<RateLimitStatusResponse> {
    let config = CONFIG.load(deps.storage)?;
    let key = address.to_string();
    let now = env.block.time;
    let start_secs = now.seconds().saturating_sub(config.rate_limit_window_secs);
    let window_start = Timestamp::from_seconds(start_secs);
    let entries = RATE_LIMIT_FLOWS
        .may_load(deps.storage, key.clone())?
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.block_time >= window_start)
        .collect::<Vec<_>>();
    let mut net_total: i128 = 0;
    for e in &entries {
        net_total = net_total.saturating_add(e.amount_base.i128());
    }
    let (total_deposits, _) = get_total_deposit_value(deps.querier, env, &config)
        .map_err(|_e| StdError::generic_err("Failed to query the contract for the total deposit value"))?;
    let is_allowlisted = config.allowlist.iter().any(|a| a == &key);
    let active_threshold = if is_allowlisted { config.allowlist_rate_limit_threshold } else { config.rate_limit_threshold };
    let threshold_base = decimal_multiplication(
        Decimal::from_ratio(total_deposits, Uint128::one()),
        active_threshold,
    )?.to_uint_floor();
    let abs_net = if net_total < 0 { (-net_total) as u128 } else { net_total as u128 };
    let remaining_base = if abs_net >= threshold_base.u128() { Uint128::zero() } else { Uint128::from(threshold_base.u128() - abs_net) };
    Ok(RateLimitStatusResponse {
        address: key,
        status: RateLimitStatus {
            net_flow_base: net_total,
            threshold_base,
            is_allowlisted,
            remaining_base,
            entries_count: entries.len() as u64,
        }
    })
}

fn query_rate_limit_many(
    deps: Deps,
    env: Env,
    addresses: Option<Vec<String>>,
    start_after: Option<u64>,
    limit: Option<u32>,
) -> StdResult<RateLimitManyResponse> {
    let config = CONFIG.load(deps.storage)?;
    let list = addresses.unwrap_or_else(|| config.allowlist.clone());
    let total = list.len() as u64;
    let start_index = start_after
        .and_then(|i| i.checked_add(1))
        .unwrap_or(0)
        .min(total as u64) as usize;
    let max = limit.unwrap_or(50).min(100) as usize;
    let end_index = (start_index + max).min(list.len());
    let mut records: Vec<RateLimitStatusResponse> = Vec::new();
    for addr in &list[start_index..end_index] {
        records.push(build_rate_limit_status(deps, &env, addr)?);
    }
    let next_start_after = if end_index < list.len() { Some((end_index - 1) as u64) } else { None };
    Ok(RateLimitManyResponse { records, total, next_start_after })
}

fn query_swap_history(
    deps: Deps,
    start_after: Option<u64>,
    limit: Option<u32>,
) -> StdResult<TransmuteHistoryResponse> {
    let history = TRANSMUTE_HISTORY.load(deps.storage)?;
    println!("history: {:?}", history);
    let total = history_total(&history);
    let (slice, start_index) = history_slice(&history, start_after, limit);

    let records: Vec<SwapRecord> = slice
        .into_iter()
        .map(|snapshot| SwapRecord {
            offered_asset: snapshot.offered_asset,
            offered_amount: snapshot.offered_amount,
            received_asset: snapshot.received_asset,
            received_amount: snapshot.received_amount,
            block_time: snapshot.block_time,
        })
        .collect();

    let next_start_after = if records.is_empty() {
        None
    } else {
        let last_index = start_index + records.len() - 1;
        if (last_index as u64) + 1 < total {
            Some(last_index as u64)
        } else {
            None
        }
    };

    Ok(TransmuteHistoryResponse {
        records,
        total,
        next_start_after,
    })
}

fn query_volume_history(
    deps: Deps,
    start_after: Option<u64>,
    limit: Option<u32>,
) -> StdResult<VolumeHistoryResponse> {
    let mut history = VOLUME_HISTORY.load(deps.storage)?;
    let window = VOLUME_WINDOW.load(deps.storage)?;
    //Add current window to the history
    history.push(window);

    let total = history_total(&history);
    let (records, start_index) = history_slice(&history, start_after, limit);

    let next_start_after = if records.is_empty() {
        None
    } else {
        let last_index = start_index + records.len() - 1;
        if (last_index as u64) + 1 < total {
            Some(last_index as u64)
        } else {
            None
        }
    };

    Ok(VolumeHistoryResponse {
        records,
        total,
        next_start_after,
    })
}

fn query_rate_history(
    deps: Deps,
    start_after: Option<u64>,
    limit: Option<u32>,
) -> StdResult<RateHistoryResponse> {
    let history = RATE_HISTORY.load(deps.storage)?;
    let total = history_total(&history);
    let (records, start_index) = history_slice(&history, start_after, limit);

    let next_start_after = if records.is_empty() {
        None
    } else {
        let last_index = start_index + records.len() - 1;
        if (last_index as u64) + 1 < total {
            Some(last_index as u64)
        } else {
            None
        }
    };

    Ok(RateHistoryResponse {
        records,
        total,
        next_start_after,
    })
}

/// Standard vault interface: returns underlying value for a given vault token amount.
/// Conversion rate = total_deposit_value / deposit_total, applied to vault_token_amount.
fn query_vault_token_underlying(deps: Deps, env: Env, vault_token_amount: Uint128) -> StdResult<Uint128> {
    let config = CONFIG.load(deps.storage)?;
    let deposit_total = DEPOSIT_TOTAL.load(deps.storage)?;
    let (total_deposit_value, _) = get_total_deposit_value(deps.querier, &env, &config)
        .map_err(|_| StdError::generic_err("Failed to query total deposit value"))?;

    if deposit_total.is_zero() {
        return Ok(vault_token_amount); // 1:1 if no deposits
    }

    let ratio = Decimal::from_ratio(total_deposit_value, deposit_total);
    let underlying = decimal_multiplication(ratio, Decimal::from_ratio(vault_token_amount, Uint128::one()))?
        .to_uint_floor();

    Ok(underlying)
}

fn validate_asset_pair(pair: &AssetPair) -> Result<(), ContractError> {
    if pair.cdt.is_empty() || pair.paired_asset.is_empty() {
        return Err(ContractError::Validation(
            "asset denoms must be non-empty".into(),
        ));
    }
    if pair.cdt == pair.paired_asset {
        return Err(ContractError::Validation(
            "asset A and asset B must differ".into(),
        ));
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
    // Query CDP for active deployment venues for the sender
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

fn segregate_funds(
    pair: &AssetPair,
    info: &MessageInfo,
) -> Result<(Uint128, Uint128), ContractError> {
    let mut asset_a_total = Uint128::zero();
    let mut asset_b_total = Uint128::zero();

    for fund in &info.funds {
        if fund.denom == pair.cdt {
            asset_a_total = asset_a_total
                .checked_add(fund.amount)
                .map_err(|err| ContractError::Std(err.into()))?;
        } else if fund.denom == pair.paired_asset {
            asset_b_total = asset_b_total
                .checked_add(fund.amount)
                .map_err(|err| ContractError::Std(err.into()))?;
        } else {
            return Err(ContractError::InvalidFunds {
                reason: format!("unsupported denom: {}", fund.denom),
            });
        }
    }

    Ok((asset_a_total, asset_b_total))
}

fn extract_coin_amount(info: &MessageInfo, denom: &str) -> Result<Uint128, ContractError> {
    let mut amount = Uint128::zero();
    for fund in &info.funds {
        if fund.denom == denom {
            amount = amount
                .checked_add(fund.amount)
                .map_err(|err| ContractError::Std(err.into()))?;
        }
    }
    Ok(amount)
}

fn current_balances(
    querier: QuerierWrapper,
    env: &Env,
    pair: &AssetPair,
) -> StdResult<(Uint128, Uint128)> {
    let assets = vec![
        AssetInfo::NativeToken { denom: pair.cdt.clone() },
        AssetInfo::NativeToken { denom: pair.paired_asset.clone() },
    ];
    let balances = get_contract_balances(querier, env.clone(), assets)?;
    Ok((balances[0], balances[1]))
}


fn compute_effective_cdt_cdt_target_ratio(deps: Deps, env: &Env, config: &Config) -> StdResult<Decimal> {
    // total deposits (1:1 tracking: CDT + paired asset)
    let (total_deposits, _) = get_total_deposit_value(deps.querier.clone(), env, config)
        .map_err(|e| StdError::generic_err(format!("{e}")))?;
    if total_deposits.is_zero() {
        return Ok(config.cdt_target_ratio);
    }
    // value of deployed paired asset (1:1, so direct addition)
    let deployed_paired = DEPLOYED_PAIRED_ASSET
        .load(deps.storage)
        .unwrap_or_else(|_| Uint128::zero());
    // Since 1:1, deployed_paired is already in the same units as total_deposits
    let min_target = Decimal::from_ratio(deployed_paired, total_deposits);
    Ok(if min_target > config.cdt_target_ratio { min_target } else { config.cdt_target_ratio })
}


/// Ensures deposits are pushing the ratio closer to the target ratio or keeping it stagnant
fn ensure_deposit_alignment(
    querier: QuerierWrapper,
    env: &Env,
    config: &Config,
    effective_target: Decimal,
    deposit_a: Uint128,
    deposit_b: Uint128,
) -> Result<(), ContractError> {
    if deposit_a.is_zero() && deposit_b.is_zero() {
        return Err(ContractError::InvalidFunds {
            reason: "no deposit value provided".into(),
        });
    }

    //Get the balances after the deposit
    let balances_after = current_balances(querier, env, &config.deposit_pair)?;
    let pre_a = balances_after
        .0
        .checked_sub(deposit_a)
        .map_err(|err| ContractError::Std(err.into()))?;
    let pre_b = balances_after
        .1
        .checked_sub(deposit_b)
        .map_err(|err| ContractError::Std(err.into()))?;

    //Get the total deposit value before the deposit (1:1 tracking)
    let total_before = pre_a.checked_add(pre_b)
        .map_err(|e| ContractError::Std(StdError::from(e)))?;
    //Get the user deposit value (1:1 tracking)
    let deposit_value = deposit_a.checked_add(deposit_b)
        .map_err(|e| ContractError::Std(StdError::from(e)))?;
    if deposit_value.is_zero() {
        return Err(ContractError::InvalidFunds {
            reason: "deposit value is zero".into(),
        });
    }

    let target = effective_target;
    let leeway = config.composition_leeway;

    //If the total contract value before the deposit is zero,
    //...we need to ensure the user deposit value is within the leeway
    if total_before.is_zero() {
        // Check deposit ratio directly
        let deposit_ratio = Decimal::from_ratio(deposit_a, deposit_value);
        ensure_within_leeway(deposit_ratio, target, leeway)?;
        return Ok(());
    }

    //Get the current ratio (pre-deposit) - CDT ratio
    let current_ratio = Decimal::from_ratio(pre_a, total_before);

    let new_a = balances_after.0;
    let new_b = balances_after.1;
    //Get the total contract value after the deposit (1:1 tracking)
    let total_after = new_a.checked_add(new_b)
        .map_err(|e| ContractError::Std(StdError::from(e)))?;
    if total_after.is_zero() {
        return Err(ContractError::InvalidFunds {
            reason: "resulting deposit value is zero".into(),
        });
    }

    //Get the new ratio of Asset A (post-deposit) - CDT ratio
    let new_ratio = Decimal::from_ratio(new_a, total_after);

    //Get the difference between the current ratio and the target ratio
    let current_diff = decimal_abs_diff(current_ratio, target)?;
    //Get the difference between the new ratio and the target ratio
    let new_diff = decimal_abs_diff(new_ratio, target)?;


    //Ensure the new ratio is within the leeway
    //or it at least puts the contract composition closer to the target ratio
    match ensure_within_leeway(new_ratio, target, leeway){
        Ok(()) => Ok(()),
        Err(_) => {
            //OR, if the new ratio is outside the leeway,
            //..we error ONLY IF the deposit puts the balance in a worse position
            if new_diff > current_diff {
                return Err(ContractError::Validation(
                    "deposit would move composition away from target".into(),
                ));
            }
            //Otherwise, the deposit is valid bc it moves the composition closer to the target ratio or keeps it static
            Ok(())
        }
    }


}

fn ensure_within_leeway(
    actual: Decimal,
    target: Decimal,
    leeway: Decimal,
) -> Result<(), ContractError> {
    let diff = decimal_abs_diff(actual, target)?;

    if diff > leeway {
        return Err(ContractError::SlippageExceeded {});
    }

    Ok(())
}

fn ensure_contract_balance(
    querier: QuerierWrapper,
    env: &Env,
    denom: &str,
    amount: Uint128,
) -> Result<(), ContractError> {
    if amount.is_zero() {
        return Ok(());
    }
    let balance = querier.query_balance(env.contract.address.clone(), denom.to_string())?.amount;
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

/// Returns (total_deposit_value, paired_asset_balance)
fn get_total_deposit_value(
    querier: QuerierWrapper,
    env: &Env,
    config: &Config,
) -> Result<(Uint128, Uint128), ContractError> {
    let balances = current_balances(querier, env, &config.deposit_pair)?;
    // 1:1 tracking: CDT + paired asset
    let total = balances.0.checked_add(balances.1)
        .map_err(|e| ContractError::Std(StdError::from(e)))?;
    Ok((total, balances.1))
}


///Rate assurance
/// Ensures that the conversion rate is static for deposits & withdrawals
fn execute_rate_assurance(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    //Load config    
    let config = CONFIG.load(deps.storage)?;

    //Error if not the contract calling
    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized {});
    }

    //Load State
    let token_rate_assurance = TOKEN_RATE_ASSURANCE.load(deps.storage)?;
    let deposit_total = DEPOSIT_TOTAL.load(deps.storage)?;

    //Get total deposit value from contract balances (1:1 tracking)
    let (total_deposit_value, _) = get_total_deposit_value(deps.querier.clone(), &env, &config)?;

    //Verify that DEPOSIT_TOTAL matches contract balances (within 1 unit for rounding)
    if deposit_total > total_deposit_value.checked_add(Uint128::one()).unwrap_or(deposit_total)
        || deposit_total < total_deposit_value.saturating_sub(Uint128::one()) {
        return Err(ContractError::CustomError { 
            val: format!("Deposit total assurance failed. DEPOSIT_TOTAL: {:?}, Contract balances: {:?}. If its 1 off just try again.", 
                deposit_total, total_deposit_value) 
        });
    }
    
    // Note: DEPOSIT_TOTAL can decrease during exits, which is expected behavior
    // The main check above ensures DEPOSIT_TOTAL matches contract balances
    //We're adding 1 to stop errors for rounding errors.

    Ok(Response::new())
}

/// Collect and distribute fees to revenue distributor
/// Returns messages to add to response
/// Never errors - always continues silently (returns empty Vec on any error). 
/// SIKE, errors if total distribution amount is greater than total fee amount. This is a sanity check since ratio total is capped on config update.
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
    // Only proceed if there's a revenue distributor configured and fee is non-zero
    if current_fee_amount.is_zero() {
        return Ok(messages);
    }
    
    // Load pending revenue
    let pending_revenue = PENDING_REVENUE.load(storage).unwrap_or_else(|_| Uint128::zero());
    
    // If this fee is in the same denomination as pending (paired asset), add them together
    let (total_fee_amount, total_fee_denom) = if current_fee_denom == config.deposit_pair.paired_asset {
        // Fee is in paired_asset, add to pending
        let new_pending = pending_revenue.checked_add(current_fee_amount)
            .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
        (new_pending, config.deposit_pair.paired_asset.clone())
    } else {
        // Fee is in CDT
        (current_fee_amount, current_fee_denom)
    };
    
    // Calculate the amount to send to revenue distributor based on percentage
    let amount_to_send = decimal_multiplication(
        Decimal::from_ratio(total_fee_amount, Uint128::one()),
        config.revenue_distributor_fee_percentage
    )?.to_uint_floor();
    
    // If amount_to_send is zero, nothing to send (remaining fees stay in contract)
    if amount_to_send.is_zero() {
        // If fee is in paired_asset, save as pending revenue
        if total_fee_denom == config.deposit_pair.paired_asset {
            PENDING_REVENUE.save(storage, &total_fee_amount)?;
        }
        // If fee is in CDT, it stays in contract balance (no action needed)
        return Ok(messages);
    }
    
    // If the total fee is in CDT, send it directly to revenue distributor.
    //THIS IS SAYING, FOR THE TRANSMUTER'S REVENUE, WHICH DISCO DO WE WANT TO DISTRIBUTE IT TO?
    if total_fee_denom == config.deposit_pair.cdt {
        // Convert revenue_distributions ratios to Asset amounts
        let mut ltv_disco_distribution = Vec::new();
        for liq_asset in &config.revenue_distributions {
            let amount: Uint128 = decimal_multiplication(
                Decimal::from_ratio(amount_to_send, Uint128::one()),
                liq_asset.amount
            )?.to_uint_floor();
            if !amount.is_zero() {
                ltv_disco_distribution.push(membrane::types::Asset {
                    info: liq_asset.info.clone(),
                    amount,
                });
            }
        }

        //Ensure the total distribution amount is less than the amount to send
        let total_distribution_amount = ltv_disco_distribution.iter().map(|asset| asset.amount).sum::<Uint128>();
        if total_distribution_amount > amount_to_send {
            return Err(ContractError::Std(StdError::generic_err("Total distribution amount is greater than amount to send")));
        }
        
        // Send CDT to revenue distributor via SetPromises
        // Remaining fees (total_fee_amount - amount_to_send) stay in contract balance
        let set_promises_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: rd_addr.to_string(),
            msg: to_json_binary(&RevenueDistributorExecuteMsg::SetPromises {
                promises: vec![],
                ltv_disco_distribution: Some(ltv_disco_distribution),
            })?,
            funds: vec![coin(amount_to_send.u128(), config.deposit_pair.cdt.clone())],
        });
        messages.push(set_promises_msg);
        
        return Ok(messages);
    }
    
    // If total fee is in paired_asset, need to transmute to CDT first
    // Query contract CDT balance
    let cdt_balance = querier.query_balance(&env.contract.address, &config.deposit_pair.cdt)?.amount;
    
    // Calculate how much CDT we can transmute from the paired_asset balance
    // Use the exchange rate to figure out max transmutable
    if cdt_balance.is_zero() {
        // No CDT available, save the total_fee_amount as pending
        // The amount_to_send portion will be transmuted later when CDT is available
        PENDING_REVENUE.save(storage, &total_fee_amount)?;
        return Ok(messages);
    }
    
    // Calculate how much paired_asset we can transmute based on CDT available (1:1)
    // We want to transmute min(amount_to_send, cdt_balance)
    let amount_to_transmute = if amount_to_send <= cdt_balance {
        amount_to_send
    } else {
        cdt_balance
    };
    
    // Calculate how much CDT we'll get from this transmutation (1:1 swap)
    let cdt_received = amount_to_transmute; // 1:1 value
    
    // Update pending revenue with remaining amount that couldn't be transmuted
    // This includes: (total_fee_amount - amount_to_send) + (amount_to_send - amount_to_transmute)
    // = total_fee_amount - amount_to_transmute
    let remaining_pending = total_fee_amount.checked_sub(amount_to_transmute)
        .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
    PENDING_REVENUE.save(storage, &remaining_pending)?;
    
    // Convert revenue_distributions ratios to Asset amounts for the CDT received
    let mut ltv_disco_distribution = Vec::new();
    for liq_asset in &config.revenue_distributions {
        let amount = decimal_multiplication(
            Decimal::from_ratio(cdt_received, Uint128::one()),
            liq_asset.amount
        )?.to_uint_floor();
        if !amount.is_zero() {
            ltv_disco_distribution.push(membrane::types::Asset {
                info: liq_asset.info.clone(),
                amount,
            });
        }
    }
    
    // Send CDT to revenue distributor via SetPromises
    // Remaining fees (total_fee_amount - amount_to_transmute) stay as pending revenue
    if !cdt_received.is_zero() {
        let set_promises_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: rd_addr.to_string(),
            msg: to_json_binary(&RevenueDistributorExecuteMsg::SetPromises {
                promises: vec![],
                ltv_disco_distribution: Some(ltv_disco_distribution),
            })?,
            funds: vec![coin(cdt_received.u128(), config.deposit_pair.cdt.clone())],
        });
        messages.push(set_promises_msg);
    }
    
    Ok(messages)
}

fn query_global_rate_limit(
    deps: Deps,
    env: Env,
) -> StdResult<GlobalRateLimitResponse> {
    let config = CONFIG.load(deps.storage)?;
    let now = env.block.time;
    let start_secs = now.seconds().saturating_sub(config.global_rate_limit_window_secs);
    let window_start = Timestamp::from_seconds(start_secs);
    let entries = GLOBAL_RATE_LIMIT_FLOWS
        .load(deps.storage)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.block_time >= window_start)
        .collect::<Vec<_>>();
    let mut net_total: i128 = 0;
    for e in &entries {
        net_total = net_total.saturating_add(e.amount_base.i128());
    }
    let (total_deposits, _) = get_total_deposit_value(deps.querier, &env, &config)
        .map_err(|_e| StdError::generic_err("Failed to query the contract for the total deposit value"))?;
    let threshold_base = decimal_multiplication(
        Decimal::from_ratio(total_deposits, Uint128::one()),
        config.global_rate_limit_threshold,
    )?.to_uint_floor();
    let abs_net = if net_total < 0 { (-net_total) as u128 } else { net_total as u128 };
    let remaining_base = if abs_net >= threshold_base.u128() { Uint128::zero() } else { Uint128::from(threshold_base.u128() - abs_net) };
    Ok(GlobalRateLimitResponse {
        net_flow_base: net_total,
        threshold_base,
        remaining_base,
        entries_count: entries.len() as u64,
    })
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
    label: Option<String>,
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
        return Err(ContractError::Std(StdError::generic_err(
            format!("Can't add more than {} affiliations", crate::state::AFFILIATE_LIMIT)
        )));
    }
    
    // Add new affiliate
    affiliations.push(membrane::types::AffiliateData {
        affiliate_address: affiliate_address.clone(),
        affiliate_fee,
        time_affiliated: current_time,
        label,
    });
    
    // Save
    crate::state::AFFILIATES.save(storage, user, &affiliations)?;
    
    Ok(())
}

/// Set affiliate for a user
fn execute_set_affiliate(
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
            return Err(ContractError::Std(StdError::generic_err(
                format!("Can't add more than {} affiliations", crate::state::AFFILIATE_LIMIT)
            )));
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

// ================= Retention Emissions Functions =================

/// Calculate retention weight for a deposit using two-phase ramp (mirroring MBRN discounts)
/// First month: 60% weight ramps quickly over 30 days
/// Remaining: 40% weight ramps over remaining 60 days
/// Lock weight: lock_days / lock_ceiling (no +1)
/// Combined: (time_weight + lock_weight).min(1.0)
/// Final: deposit_amount * combined_weight
fn calculate_retention_weight(
    deposit_amount: Uint128,
    deposit_time: u64,
    lock_days: Option<u64>,
    current_time: u64,
    lock_ceiling: u64,
) -> Result<Uint128, ContractError> {
    const SECONDS_PER_DAY: u64 = 86_400;
    
    let days_since_deposit = (current_time.saturating_sub(deposit_time)) / SECONDS_PER_DAY;
    
    // Time-based weight: two-phase ramp (60% first month, 40% remaining)
    let time_weight = if days_since_deposit >= RETENTION_CURVE_DURATION_DAYS {
        Decimal::one()
    } else {
        // First month progress: min(days_since_deposit, first_month_days) / first_month_days
        let first_month_progress = Decimal::from_ratio(
            days_since_deposit.min(RETENTION_FIRST_MONTH_DAYS),
            RETENTION_FIRST_MONTH_DAYS,
        );
        let first_month_weight = decimal_multiplication(
            RETENTION_FIRST_MONTH_WEIGHT,
            first_month_progress,
        )?;
        
        // Remaining days: days_since_deposit - first_month_days (if > first_month_days)
        let remaining_days = if days_since_deposit > RETENTION_FIRST_MONTH_DAYS {
            days_since_deposit - RETENTION_FIRST_MONTH_DAYS
        } else {
            0
        };
        
        // Remaining duration: curve_duration - first_month_days
        let remaining_duration_days = RETENTION_CURVE_DURATION_DAYS - RETENTION_FIRST_MONTH_DAYS;
        let remaining_progress = if remaining_duration_days > 0 {
            Decimal::from_ratio(
                remaining_days.min(remaining_duration_days),
                remaining_duration_days,
            )
        } else {
            Decimal::zero()
        };
        let remaining_weight = decimal_multiplication(
            RETENTION_REMAINING_WEIGHT,
            remaining_progress,
        )?;
        
        // Total time weight: first_month_weight + remaining_weight (capped at 1.0)
        (first_month_weight + remaining_weight).min(Decimal::one())
    };
    
    // Lock-based weight: lock_days / lock_ceiling (no +1)
    let lock_weight = if let Some(lock_days) = lock_days {
        if lock_ceiling == 0 {
            Decimal::zero()
        } else {
            Decimal::from_ratio(lock_days, lock_ceiling)
        }
    } else {
        Decimal::zero()
    };
    
    // Combined weight: time_weight + lock_weight (capped at 1.0)
    let combined_weight = (time_weight + lock_weight).min(Decimal::one());
    
    // Final weight = deposit_amount * combined_weight
    let weight = decimal_multiplication(
        Decimal::from_ratio(deposit_amount, Uint128::one()),
        combined_weight,
    )?.to_uint_floor();
    
    Ok(weight)
}

/// Calculate retention weight at a specific timestamp using tracking data
/// Similar to calculate_lvt_at_time() in ltv_disco
fn calculate_retention_weight_at_time(
    base_weight: Uint128,
    reference_time: u64,
    base_daily_delta: Int128,
    time_cliffs: &[WeightTimeCliff],
    timestamp: u64,
) -> Result<Uint128, ContractError> {
    const ONE_DAY_SECONDS: u64 = 86_400;
    
    if timestamp == reference_time {
        return Ok(base_weight);
    }
    
    let mut current_weight = Int128::from(base_weight.u128() as i128);
    let mut current_daily_delta = base_daily_delta;
    
    if timestamp > reference_time {
        // Forward time: process cliffs up to timestamp
        let mut last_time = reference_time;
        
        for cliff in time_cliffs {
            if cliff.timestamp > timestamp {
                break;
            }
            
            // Skip if cliff timestamp is before last_time (shouldn't happen if sorted, but safety check)
            if cliff.timestamp < last_time {
                // Update daily delta but don't apply weight change
                current_daily_delta = current_daily_delta + cliff.delta_change;
                continue;
            }
            
            // Apply delta from last_time to cliff.timestamp
            let days_elapsed = (cliff.timestamp - last_time) / ONE_DAY_SECONDS;
            current_weight = current_weight + (current_daily_delta * Int128::from(days_elapsed as i128));
            
            // Update daily delta
            current_daily_delta = current_daily_delta + cliff.delta_change;
            last_time = cliff.timestamp;
        }
        
        // Apply remaining delta from last cliff (or reference_time) to timestamp
        // Safety check: ensure timestamp >= last_time to prevent underflow
        if timestamp >= last_time {
            let days_elapsed = (timestamp - last_time) / ONE_DAY_SECONDS;
            current_weight = current_weight + (current_daily_delta * Int128::from(days_elapsed as i128));
        }
    } else {
        // Backward time: process cliffs in reverse
        let mut last_time = reference_time;
        
        for cliff in time_cliffs.iter().rev() {
            if cliff.timestamp < timestamp {
                break;
            }
            
            // Skip if cliff timestamp is after last_time (shouldn't happen if sorted, but safety check)
            if cliff.timestamp > last_time {
                // Revert daily delta change but don't apply weight change
                current_daily_delta = current_daily_delta - cliff.delta_change;
                continue;
            }
            
            // Apply delta backward from last_time to cliff.timestamp
            let days_elapsed = (last_time - cliff.timestamp) / ONE_DAY_SECONDS;
            current_weight = current_weight - (current_daily_delta * Int128::from(days_elapsed as i128));
            
            // Revert daily delta change
            current_daily_delta = current_daily_delta - cliff.delta_change;
            last_time = cliff.timestamp;
        }
        
        // Apply remaining delta backward from last cliff (or reference_time) to timestamp
        let days_elapsed = (last_time - timestamp) / ONE_DAY_SECONDS;
        current_weight = current_weight - (current_daily_delta * Int128::from(days_elapsed as i128));
    }
    
    // Ensure weight doesn't go negative
    if current_weight < Int128::zero() {
        Ok(Uint128::zero())
    } else {
        // Convert Int128 to u128 (safe since we checked it's non-negative)
        // Int128 stores as i128, so we can safely cast to u128 if non-negative
        let weight_i128 = current_weight.i128();
        if weight_i128 < 0 {
            Ok(Uint128::zero())
        } else {
            Ok(Uint128::from(weight_i128 as u128))
        }
    }
}

/// Calculate deposit contribution to retention weight (similar to calculate_deposit_contribution in ltv_disco)
/// Returns: (base_weight, daily_delta, cliffs)
fn calculate_deposit_weight_contribution(
    deposit: &UserDeposit,
    env: &Env,
    lock_ceiling: u64,
) -> Result<(Uint128, Int128, Vec<WeightTimeCliff>), ContractError> {
    const ONE_DAY_SECONDS: u64 = 86_400;
    let deposit_amount = deposit.amount;
    let current_time = env.block.time.seconds();
    
    // Handle locks
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
        // Two-phase ramp: 60% in first month (30 days), 40% in remaining (60 days)
        // First month: daily_delta = (deposit_amount * 0.6) / 30
        // Remaining: daily_delta = (deposit_amount * 0.4) / 60
        let first_month_weight = decimal_multiplication(
            Decimal::from_ratio(deposit_amount, Uint128::one()),
            RETENTION_FIRST_MONTH_WEIGHT,
        )?.to_uint_floor();
        let first_month_daily_delta = Int128::from(
            (first_month_weight.u128() as i128) / (RETENTION_FIRST_MONTH_DAYS as i128)
        );
        
        let remaining_weight = decimal_multiplication(
            Decimal::from_ratio(deposit_amount, Uint128::one()),
            RETENTION_REMAINING_WEIGHT,
        )?.to_uint_floor();
        let remaining_duration_days = RETENTION_CURVE_DURATION_DAYS - RETENTION_FIRST_MONTH_DAYS;
        let remaining_daily_delta = Int128::from(
            (remaining_weight.u128() as i128) / (remaining_duration_days as i128)
        );
        
        // Create cliffs for phase transitions
        let mut cliffs = Vec::new();
        
        // Cliff at end of first month: change daily_delta from first_month to remaining
        let first_month_end_time = deposit.deposit_time + (RETENTION_FIRST_MONTH_DAYS * ONE_DAY_SECONDS);
        if first_month_end_time > current_time {
            cliffs.push(WeightTimeCliff {
                timestamp: first_month_end_time,
                delta_change: remaining_daily_delta - first_month_daily_delta, // Change delta
            });
        }
        
        // Cliff at end of curve: stop the ramp
        let curve_end_time = deposit.deposit_time + (RETENTION_CURVE_DURATION_DAYS * ONE_DAY_SECONDS);
        if curve_end_time > current_time {
            cliffs.push(WeightTimeCliff {
                timestamp: curve_end_time,
                delta_change: -remaining_daily_delta, // Stop the daily increase
            });
        }
        
        return Ok((
            Uint128::zero(), // Base: 0 at deposit time
            first_month_daily_delta, // Initial daily delta: first month ramp
            cliffs
        ));
    };
    
    // Calculate current lock days
    let lock_days = if locked_until > current_time {
        (locked_until - current_time) / ONE_DAY_SECONDS
    } else {
        0
    };
    
    // Calculate current weight using two-phase ramp formula
    let days_since_deposit = (current_time.saturating_sub(deposit.deposit_time)) / ONE_DAY_SECONDS;
    let time_weight_ratio = if days_since_deposit >= RETENTION_CURVE_DURATION_DAYS {
        Decimal::one()
    } else {
        // First month progress: min(days_since_deposit, first_month_days) / first_month_days
        let first_month_progress = Decimal::from_ratio(
            days_since_deposit.min(RETENTION_FIRST_MONTH_DAYS),
            RETENTION_FIRST_MONTH_DAYS,
        );
        let first_month_weight = decimal_multiplication(
            RETENTION_FIRST_MONTH_WEIGHT,
            first_month_progress,
        )?;
        
        // Remaining days: days_since_deposit - first_month_days (if > first_month_days)
        let remaining_days = if days_since_deposit > RETENTION_FIRST_MONTH_DAYS {
            days_since_deposit - RETENTION_FIRST_MONTH_DAYS
        } else {
            0
        };
        
        // Remaining duration: curve_duration - first_month_days
        let remaining_duration_days = RETENTION_CURVE_DURATION_DAYS - RETENTION_FIRST_MONTH_DAYS;
        let remaining_progress = if remaining_duration_days > 0 {
            Decimal::from_ratio(
                remaining_days.min(remaining_duration_days),
                remaining_duration_days,
            )
        } else {
            Decimal::zero()
        };
        let remaining_weight = decimal_multiplication(
            RETENTION_REMAINING_WEIGHT,
            remaining_progress,
        )?;
        
        // Total time weight: first_month_weight + remaining_weight (capped at 1.0)
        (first_month_weight + remaining_weight).min(Decimal::one())
    };
    
    let lock_weight_ratio = if lock_ceiling == 0 {
        Decimal::zero()
    } else {
        Decimal::from_ratio(lock_days, lock_ceiling)
    };
    
    let combined_ratio = (time_weight_ratio + lock_weight_ratio).min(Decimal::one());
    let current_weight = decimal_multiplication(
        Decimal::from_ratio(deposit_amount, Uint128::one()),
        combined_ratio,
    )?.to_uint_floor();
    
    // Calculate daily delta based on current phase of two-phase ramp
    let days_since_deposit_for_delta = (current_time.saturating_sub(deposit.deposit_time)) / ONE_DAY_SECONDS;
    let daily_delta = if days_since_deposit_for_delta >= RETENTION_CURVE_DURATION_DAYS {
        // Past curve duration: no increase
        Int128::zero()
    } else if days_since_deposit_for_delta >= RETENTION_FIRST_MONTH_DAYS {
        // In remaining phase: (deposit_amount * 0.4) / 60
        let remaining_weight = decimal_multiplication(
            Decimal::from_ratio(deposit_amount, Uint128::one()),
            RETENTION_REMAINING_WEIGHT,
        )?.to_uint_floor();
        let remaining_duration_days = RETENTION_CURVE_DURATION_DAYS - RETENTION_FIRST_MONTH_DAYS;
        Int128::from((remaining_weight.u128() as i128) / (remaining_duration_days as i128))
    } else {
        // In first month phase: (deposit_amount * 0.6) / 30
        let first_month_weight = decimal_multiplication(
            Decimal::from_ratio(deposit_amount, Uint128::one()),
            RETENTION_FIRST_MONTH_WEIGHT,
        )?.to_uint_floor();
        Int128::from((first_month_weight.u128() as i128) / (RETENTION_FIRST_MONTH_DAYS as i128))
    };
    
    // Create cliffs for phase transitions and lock expiration
    let mut cliffs = Vec::new();
    
    // Cliff at end of first month: change daily_delta from first_month to remaining
    let first_month_end_time = deposit.deposit_time + (RETENTION_FIRST_MONTH_DAYS * ONE_DAY_SECONDS);
    if first_month_end_time > current_time {
        let first_month_weight = decimal_multiplication(
            Decimal::from_ratio(deposit_amount, Uint128::one()),
            RETENTION_FIRST_MONTH_WEIGHT,
        )?.to_uint_floor();
        let first_month_daily_delta = Int128::from(
            (first_month_weight.u128() as i128) / (RETENTION_FIRST_MONTH_DAYS as i128)
        );
        
        let remaining_weight = decimal_multiplication(
            Decimal::from_ratio(deposit_amount, Uint128::one()),
            RETENTION_REMAINING_WEIGHT,
        )?.to_uint_floor();
        let remaining_duration_days = RETENTION_CURVE_DURATION_DAYS - RETENTION_FIRST_MONTH_DAYS;
        let remaining_daily_delta = Int128::from(
            (remaining_weight.u128() as i128) / (remaining_duration_days as i128)
        );
        
        cliffs.push(WeightTimeCliff {
            timestamp: first_month_end_time,
            delta_change: remaining_daily_delta - first_month_daily_delta, // Change delta
        });
    }
    
    // Cliff at end of curve: stop the ramp
    let curve_end_time = deposit.deposit_time + (RETENTION_CURVE_DURATION_DAYS * ONE_DAY_SECONDS);
    if curve_end_time > current_time {
        let remaining_weight = decimal_multiplication(
            Decimal::from_ratio(deposit_amount, Uint128::one()),
            RETENTION_REMAINING_WEIGHT,
        )?.to_uint_floor();
        let remaining_duration_days = RETENTION_CURVE_DURATION_DAYS - RETENTION_FIRST_MONTH_DAYS;
        let remaining_daily_delta = Int128::from(
            (remaining_weight.u128() as i128) / (remaining_duration_days as i128)
        );
        
        cliffs.push(WeightTimeCliff {
            timestamp: curve_end_time,
            delta_change: -remaining_daily_delta, // Stop the daily increase
        });
    }
    
    if !is_perpetual && locked_until > current_time {
        // Add cliff when lock expires (lock weight drops to 0)
        cliffs.push(WeightTimeCliff {
            timestamp: locked_until,
            delta_change: -Int128::from((deposit_amount.u128() as i128) / (lock_ceiling as i128)), // Negative delta when lock expires
        });
    }
    
    Ok((current_weight, daily_delta, cliffs))
}

/// Update user's retention weight tracking when deposits change
/// Similar to update_deposit_lvt_tracking() in ltv_disco
fn update_retention_weight_tracking(
    storage: &mut dyn Storage,
    user: &str,
    deposits: &[UserDeposit],
    env: &Env,
    lock_ceiling: u64,
) -> Result<(), ContractError> {
    let reference_time = env.block.time.seconds();
    
    // Calculate total weight contribution from all deposits
    let mut total_base_weight = Uint128::zero();
    let mut total_daily_delta = Int128::zero();
    let mut all_cliffs: Vec<WeightTimeCliff> = Vec::new();
    
    for deposit in deposits {
        let (base_weight, daily_delta, cliffs) = calculate_deposit_weight_contribution(
            deposit,
            env,
            lock_ceiling,
        )?;
        
        total_base_weight = total_base_weight.checked_add(base_weight)
            .map_err(|e| ContractError::Std(StdError::from(e)))?;
        total_daily_delta = total_daily_delta.checked_add(daily_delta)
            .map_err(|_| ContractError::Std(StdError::generic_err("Daily delta overflow")))?;
        all_cliffs.extend(cliffs);
    }
    
    // Load existing tracking
    let existing_tracking = RETENTION_WEIGHT_TRACKING.may_load(storage, user.to_string())?;
    
    // Adjust base_weight to new reference_time if needed
    let adjusted_base_weight = if let Some(existing) = &existing_tracking {
        if reference_time != existing.reference_time {
            calculate_retention_weight_at_time(
                existing.base_weight,
                existing.reference_time,
                existing.daily_delta,
                &existing.time_cliffs,
                reference_time,
            )?
        } else {
            existing.base_weight
        }
    } else {
        total_base_weight
    };
    
    // Merge cliffs and sort by timestamp
    all_cliffs.sort_by_key(|c| c.timestamp);
    
    // Save updated user tracking
    let tracking = RetentionWeightTracking {
        base_weight: adjusted_base_weight,
        reference_time,
        daily_delta: total_daily_delta,
        time_cliffs: all_cliffs.clone(),
    };
    
    RETENTION_WEIGHT_TRACKING.save(storage, user.to_string(), &tracking)?;
    
    // Update global tracking (similar to update_group_lvt_tracking_for_deposit in ltv_disco)
    let mut global_tracking = crate::state::GLOBAL_RETENTION_WEIGHT_TRACKING
        .may_load(storage)?
        .unwrap_or(RetentionWeightTracking {
            base_weight: Uint128::zero(),
            reference_time,
            daily_delta: Int128::zero(),
            time_cliffs: vec![],
        });
    
    // Remove old user contribution if it existed
    if let Some(old) = existing_tracking {
        let old_weight_at_ref = calculate_retention_weight_at_time(
            old.base_weight,
            old.reference_time,
            old.daily_delta,
            &old.time_cliffs,
            reference_time,
        )?;
        
        // Subtract old contribution
        global_tracking.base_weight = global_tracking.base_weight.saturating_sub(old_weight_at_ref);
        global_tracking.daily_delta = global_tracking.daily_delta.checked_sub(old.daily_delta)
            .map_err(|_| ContractError::Std(StdError::generic_err("Global daily delta underflow")))?;
        
        // Remove old cliffs (subtract their delta_change)
        for old_cliff in &old.time_cliffs {
            if let Some(pos) = global_tracking.time_cliffs.iter().position(|c| c.timestamp == old_cliff.timestamp) {
                global_tracking.time_cliffs[pos].delta_change = 
                    global_tracking.time_cliffs[pos].delta_change.checked_sub(old_cliff.delta_change)
                        .map_err(|_| ContractError::Std(StdError::generic_err("Cliff delta underflow")))?;
                if global_tracking.time_cliffs[pos].delta_change.is_zero() {
                    global_tracking.time_cliffs.remove(pos);
                }
            } else {
                // Add inverse cliff
                global_tracking.time_cliffs.push(WeightTimeCliff {
                    timestamp: old_cliff.timestamp,
                    delta_change: Int128::zero().checked_sub(old_cliff.delta_change)
                        .map_err(|_| ContractError::Std(StdError::generic_err("Cliff delta underflow")))?,
                });
            }
        }
    }
    
    // Add new user contribution
    let new_weight_at_ref = adjusted_base_weight; // Already calculated at reference_time
    
    global_tracking.base_weight = global_tracking.base_weight.checked_add(new_weight_at_ref)
        .map_err(|e| ContractError::Std(StdError::from(e)))?;
    global_tracking.daily_delta = global_tracking.daily_delta.checked_add(total_daily_delta)
        .map_err(|_| ContractError::Std(StdError::generic_err("Global daily delta overflow")))?;
    
    // Merge new cliffs
    for new_cliff in &all_cliffs {
        if let Some(pos) = global_tracking.time_cliffs.iter().position(|c| c.timestamp == new_cliff.timestamp) {
            global_tracking.time_cliffs[pos].delta_change = 
                global_tracking.time_cliffs[pos].delta_change.checked_add(new_cliff.delta_change)
                    .map_err(|_| ContractError::Std(StdError::generic_err("Cliff delta overflow")))?;
            if global_tracking.time_cliffs[pos].delta_change.is_zero() {
                global_tracking.time_cliffs.remove(pos);
            }
        } else {
            global_tracking.time_cliffs.push(new_cliff.clone());
        }
    }
    
    // Update reference_time and sort cliffs
    global_tracking.reference_time = reference_time;
    global_tracking.time_cliffs.sort_by_key(|c| c.timestamp);
    
    crate::state::GLOBAL_RETENTION_WEIGHT_TRACKING.save(storage, &global_tracking)?;
    
    Ok(())
}

/// Query emissions-voting for Uint128 result (total emissions)
fn query_emissions_voting_uint128(
    querier: &QuerierWrapper,
    contract: &Addr,
    graph_label: &str,
) -> Result<Uint128, ContractError> {
    let response: membrane::emissions_voting::CurrentResultResponse = querier.query_wasm_smart(
        contract,
        &membrane::emissions_voting::QueryMsg::CurrentResult {
            label: graph_label.to_string(),
        },
    )?;
    
    response.result_uint128.ok_or_else(|| {
        ContractError::Std(StdError::generic_err(format!(
            "Graph {} did not return Uint128 result",
            graph_label
        )))
    })
}

/// Query emissions-voting for Decimal result (acquisition percentage)
fn query_emissions_voting_decimal(
    querier: &QuerierWrapper,
    contract: &Addr,
    graph_label: &str,
) -> Result<Decimal, ContractError> {
    let response: membrane::emissions_voting::CurrentResultResponse = querier.query_wasm_smart(
        contract,
        &membrane::emissions_voting::QueryMsg::CurrentResult {
            label: graph_label.to_string(),
        },
    )?;
    
    response.result_decimal.ok_or_else(|| {
        ContractError::Std(StdError::generic_err(format!(
            "Graph {} did not return Decimal result",
            graph_label
        )))
    })
}

/// Query user boost from discounts contract
/// Returns the boost multiplier (e.g., 1.5 for 50% boost)
/// If query fails, returns 1.0 (no boost) - errors are silently ignored
fn query_user_boost(
    querier: &QuerierWrapper,
    api: &dyn cosmwasm_std::Api,
    discounts_contract: &str,
    user: &str,
) -> Decimal {
    // Validate and convert string address to Addr
    let discounts_addr = match api.addr_validate(discounts_contract) {
        Ok(addr) => addr,
        Err(_) => return Decimal::one(), // Invalid address, return no boost
    };
    
    let response: Result<UserBoostResponse, _> = querier.query_wasm_smart(
        &discounts_addr,
        &SystemsDiscountsQueryMsg::UserBoost {
            user: user.to_string(),
        },
    );
    
    match response {
        Ok(boost_response) => {
            // Boost is returned as a Decimal (e.g., 0.5 for 50% boost)
            // Convert to multiplier: 1.0 + boost (e.g., 1.0 + 0.5 = 1.5 for 50% boost)
            Decimal::one() + boost_response.boost
        }
        Err(_) => {
            // If query fails, default to no boost (1.0)
            Decimal::one()
        }
    }
}

/// Calculate global retention weight at a specific timestamp
/// Uses pre-aggregated global tracking (O(1) instead of O(n))
fn calculate_global_retention_weight_at_time(
    deps: Deps,
    env: &Env,
    timestamp: u64,
) -> Result<Uint128, ContractError> {
    let global_tracking = GLOBAL_RETENTION_WEIGHT_TRACKING
        .may_load(deps.storage)?
        .unwrap_or(RetentionWeightTracking {
            base_weight: Uint128::zero(),
            reference_time: env.block.time.seconds(),
            daily_delta: Int128::zero(),
            time_cliffs: vec![],
        });
    
    calculate_retention_weight_at_time(
        global_tracking.base_weight,
        global_tracking.reference_time,
        global_tracking.daily_delta,
        &global_tracking.time_cliffs,
        timestamp,
    )
}

/// Create a new emissions event with accurate denominator
fn create_emissions_event(
    storage: &mut dyn Storage,
    env: &Env,
    daily_rate: Uint128,
    global_weight: Uint128,
) -> Result<(), ContractError> {
    // Ensure denominator is not zero
    if global_weight.is_zero() {
        return Err(ContractError::ZeroGlobalWeight {});
    }
    
    // Calculate amount_per_weight = daily_rate / global_weight
    let amount_per_weight = Decimal::from_ratio(daily_rate, global_weight);
    
    // Create event
    let event = EmissionsEvent {
        timestamp: env.block.time.seconds(),
        amount_per_weight,
        amount_to_be_claimed: daily_rate,
    };
    
    // Store event
    let mut events = EMISSIONS_EVENTS
        .may_load(storage)?
        .unwrap_or_default();
    events.push(event);
    EMISSIONS_EVENTS.save(storage, &events)?;
    
    Ok(())
}

/// Distribute retention emissions (creates daily event)
fn execute_distribute_retention_emissions(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    
    // Validate emissions voting contract is set
    let emissions_voting = config.emissions_voting_contract
        .ok_or_else(|| ContractError::EmissionsVotingContractNotSet {})?;
    
    // Query current total emissions
    let monthly_total = query_emissions_voting_uint128(
        &deps.as_ref().querier,
        &emissions_voting,
        membrane::transmuter::TOTAL_EMISSIONS_GRAPH_LABEL,
    )?;
    
    // Query acquisition percentage (0-20%)
    let acquisition_percentage = query_emissions_voting_decimal(
        &deps.as_ref().querier,
        &emissions_voting,
        membrane::transmuter::ACQUISITION_PERCENTAGE_GRAPH_LABEL,
    )?;
    
    // Validate acquisition percentage is within bounds (0-20%)
    const MAX_ACQUISITION_PERCENTAGE: Decimal = Decimal::percent(20);
    let acquisition_pct = if acquisition_percentage > MAX_ACQUISITION_PERCENTAGE {
        // Cap at 20% if voting exceeds limit
        MAX_ACQUISITION_PERCENTAGE
    } else if acquisition_percentage < Decimal::zero() {
        Decimal::zero()
    } else {
        acquisition_percentage
    };
    
    // Calculate retention percentage (remaining, guaranteed to be at least 80%)
    let retention_percentage = Decimal::one() - acquisition_pct;
    
    // Calculate retention monthly amount
    let retention_monthly = decimal_multiplication(
        Decimal::from_ratio(monthly_total, Uint128::one()),
        retention_percentage,
    )?.to_uint_floor();
    
    // Calculate daily rate (divide by 30)
    let daily_rate = retention_monthly / Uint128::from(30u128);
    
    // Calculate global retention weight at current time using tracking
    let global_weight = calculate_global_retention_weight_at_time(
        deps.as_ref(),
        &env,
        env.block.time.seconds(),
    )?;
    
    // Create emissions event
    create_emissions_event(
        deps.storage,
        &env,
        daily_rate,
        global_weight,
    )?;
    
    // Update last distribution timestamp
    LAST_EMISSIONS_DISTRIBUTION.save(deps.storage, &env.block.time)?;
    
    Ok(Response::new()
        .add_attribute("action", "distribute_retention_emissions")
        .add_attribute("monthly_total", monthly_total.to_string())
        .add_attribute("acquisition_percentage", acquisition_pct.to_string())
        .add_attribute("retention_percentage", retention_percentage.to_string())
        .add_attribute("retention_monthly", retention_monthly.to_string())
        .add_attribute("daily_rate", daily_rate.to_string())
        .add_attribute("global_weight", global_weight.to_string()))
}

/// Claim retention emissions for a user
fn execute_claim_retention_emissions(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let user = info.sender.to_string();
    
    // First, automatically call execute_distribute_retention_emissions() if needed
    let last_distribution = LAST_EMISSIONS_DISTRIBUTION.may_load(deps.storage)?;
    const SECONDS_PER_DAY: u64 = 86_400;
    let should_distribute = if let Some(last_time) = last_distribution {
        let time_since_last = env.block.time.seconds().saturating_sub(last_time.seconds());
        time_since_last >= SECONDS_PER_DAY
    } else {
        true // Never distributed, need to create first event
    };
    
    if should_distribute {
        // Use branch() to create a separate mutable reference for distribution
        execute_distribute_retention_emissions(
            deps.branch(),
            env.clone(),
            MessageInfo {
                sender: env.contract.address.clone(),
                funds: vec![],
            },
        )?;
    }
    
    // Load user deposits
    let user_deposits = USER_DEPOSITS
        .may_load(deps.storage, user.clone())?
        .unwrap_or_default();
    
    if user_deposits.is_empty() {
        return Err(ContractError::Std(StdError::generic_err(
            "No deposits found for user"
        )));
    }
    
    // Load user's weight tracking
    let user_tracking = RETENTION_WEIGHT_TRACKING
        .may_load(deps.storage, user.clone())?
        .ok_or_else(|| ContractError::Std(StdError::generic_err(
            "No weight tracking found for user"
        )))?;
    
    // Load emissions events
    let mut events = EMISSIONS_EVENTS
        .may_load(deps.storage)?
        .unwrap_or_default();
    
    // Query user boost from discounts contract
    let config = CONFIG.load(deps.storage)?;
    let user_boost_multiplier = query_user_boost(
        &deps.querier,
        deps.api,
        &config.discounts_contract,
        &user,
    );
    
    let mut total_claimed = Uint128::zero();
    
    // Process each event
    for event in &mut events {
        // Calculate user's weight at event timestamp using tracking
        let user_weight_at_event = calculate_retention_weight_at_time(
            user_tracking.base_weight,
            user_tracking.reference_time,
            user_tracking.daily_delta,
            &user_tracking.time_cliffs,
            event.timestamp,
        )?;
        
        // Calculate user share: user_weight_at_event * event.amount_per_weight
        let mut user_share = decimal_multiplication(
            Decimal::from_ratio(user_weight_at_event, Uint128::one()),
            event.amount_per_weight,
        )?.to_uint_floor();
        
        // Apply MBRN boost multiplier
        user_share = decimal_multiplication(
            Decimal::from_ratio(user_share, Uint128::one()),
            user_boost_multiplier,
        )?.to_uint_floor();
        
        if !user_share.is_zero() {
            // If user share is greater than amount to be claimed, cap it
            if event.amount_to_be_claimed < user_share {
                user_share = event.amount_to_be_claimed;
                event.amount_to_be_claimed = Uint128::zero();
            } else {
                event.amount_to_be_claimed = event.amount_to_be_claimed.checked_sub(user_share)
                    .map_err(|e| ContractError::Std(StdError::from(e)))?;
            }
            
            total_claimed = total_claimed.checked_add(user_share)
                .map_err(|e| ContractError::Std(StdError::from(e)))?;
        }
    }
    
    // Trim events with zero amount_to_be_claimed
    events.retain(|e| !e.amount_to_be_claimed.is_zero());
    EMISSIONS_EVENTS.save(deps.storage, &events)?;
    
    // Send claimed emissions to user, deducting affiliate fees
    let mut response = Response::new()
        .add_attribute("action", "claim_retention_emissions")
        .add_attribute("user", user.clone())
        .add_attribute("total_claimed", total_claimed.to_string())
        .add_attribute("boost_multiplier", user_boost_multiplier.to_string());

    let config = CONFIG.load(deps.storage)?;

    // Handle affiliate fees
    let mut affiliate_fees_total = Uint128::zero();
    if !total_claimed.is_zero() {
        let mut affiliates = crate::state::AFFILIATES.load(deps.storage, user.clone()).unwrap_or_default();
        affiliates.retain(|a| !a.affiliate_fee.is_zero() || a.time_affiliated != 0);

        if !affiliates.is_empty() {
            let affiliate_fee_splits = split_affiliate_fee(affiliates.clone(), config.affiliate_fee, env.block.time.seconds())?;
            let total_affiliate_fee_ratio: Decimal = affiliate_fee_splits.iter().sum();
            affiliate_fees_total = decimal_multiplication(
                Decimal::from_ratio(total_claimed, Uint128::one()),
                total_affiliate_fee_ratio,
            )?.to_uint_floor();

            for (i, affiliate_fee_ratio) in affiliate_fee_splits.into_iter().enumerate() {
                let affiliate_amount = decimal_multiplication(
                    Decimal::from_ratio(affiliate_fees_total, Uint128::one()),
                    affiliate_fee_ratio,
                )?.to_uint_floor();

                if !affiliate_amount.is_zero() {
                    // Send CDT to affiliate
                    response = response.add_message(BankMsg::Send {
                        to_address: affiliates[i].affiliate_address.clone(),
                        amount: vec![coin(affiliate_amount.u128(), config.deposit_pair.cdt.clone())],
                    });

                    // Award points to affiliate (reply_on_error so failure doesn't block claim)
                    if let Some(ref points_system) = config.points_system_contract {
                        let points_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                            contract_addr: points_system.to_string(),
                            msg: to_json_binary(&membrane::points_system::ExecuteMsg::GivePointsForAffiliateFee {
                                affiliate: affiliates[i].affiliate_address.clone(),
                                fee_amount: affiliate_amount,
                            })?,
                            funds: vec![],
                        });
                        response = response.add_submessage(SubMsg::reply_on_error(points_msg, 0));
                    }
                }
            }

            update_affiliates(deps.storage, affiliates, user.clone(), env.block.time.seconds())?;
        }
    }

    // Send remaining to user after affiliate fee deduction
    let user_amount = total_claimed.saturating_sub(affiliate_fees_total);
    if !user_amount.is_zero() {
        response = response.add_message(BankMsg::Send {
            to_address: user,
            amount: vec![coin(user_amount.u128(), config.deposit_pair.cdt.clone())],
        });
    }

    response = response.add_attribute("affiliate_fees", affiliate_fees_total.to_string());
    
    Ok(response)
}

/// Query user's retention emissions claimable amount
fn query_user_retention_emissions(
    deps: Deps,
    env: Env,
    user: String,
) -> StdResult<membrane::transmuter::UserRetentionEmissionsResponse> {
    // Load user deposits
    let user_deposits = USER_DEPOSITS
        .may_load(deps.storage, user.clone())?
        .unwrap_or_default();
    
    if user_deposits.is_empty() {
        return Ok(membrane::transmuter::UserRetentionEmissionsResponse {
            claimable: Uint128::zero(),
        });
    }
    
    // Load user's weight tracking
    let user_tracking = RETENTION_WEIGHT_TRACKING
        .may_load(deps.storage, user.clone())?
        .ok_or_else(|| StdError::generic_err("No weight tracking found for user"))?;
    
    // Load emissions events
    let events = EMISSIONS_EVENTS
        .may_load(deps.storage)?
        .unwrap_or_default();
    
    // Query user boost from discounts contract
    let config = CONFIG.load(deps.storage)?;
    let user_boost_multiplier = query_user_boost(
        &deps.querier,
        deps.api,
        &config.discounts_contract,
        &user,
    );
    
    let mut total_claimable = Uint128::zero();
    
    // Process each event
    for event in &events {
        // Calculate user's weight at event timestamp using tracking
        let user_weight_at_event = calculate_retention_weight_at_time(
            user_tracking.base_weight,
            user_tracking.reference_time,
            user_tracking.daily_delta,
            &user_tracking.time_cliffs,
            event.timestamp,
        ).map_err(|e| match e {
            ContractError::Std(se) => se,
            _ => StdError::generic_err(format!("Failed to calculate weight: {:?}", e)),
        })?;
        
        // Calculate user share: user_weight_at_event * event.amount_per_weight
        let mut user_share = decimal_multiplication(
            Decimal::from_ratio(user_weight_at_event, Uint128::one()),
            event.amount_per_weight,
        ).map_err(|e| StdError::from(e))?.to_uint_floor();
        
        // Apply MBRN boost multiplier
        user_share = decimal_multiplication(
            Decimal::from_ratio(user_share, Uint128::one()),
            user_boost_multiplier,
        ).map_err(|e| StdError::from(e))?.to_uint_floor();
        
        // Cap at amount_to_be_claimed
        let claimable_from_event = user_share.min(event.amount_to_be_claimed);
        total_claimable = total_claimable.checked_add(claimable_from_event)
            .map_err(|e| StdError::from(e))?;
    }
    
    Ok(membrane::transmuter::UserRetentionEmissionsResponse {
        claimable: total_claimable,
    })
}

/// Query global retention weight
fn query_global_retention_weight(
    deps: Deps,
    env: Env,
) -> StdResult<membrane::transmuter::GlobalRetentionWeightResponse> {
    let weight = calculate_global_retention_weight_at_time(
        deps,
        &env,
        env.block.time.seconds(),
    ).map_err(|e| match e {
        ContractError::Std(se) => se,
        _ => StdError::generic_err(format!("Failed to calculate global weight: {:?}", e)),
    })?;
    
    Ok(membrane::transmuter::GlobalRetentionWeightResponse {
        weight,
    })
}

/// Query emissions configuration
fn query_emissions_config(
    deps: Deps,
) -> StdResult<membrane::transmuter::EmissionsConfigResponse> {
    let config = CONFIG.load(deps.storage)?;
    
    Ok(membrane::transmuter::EmissionsConfigResponse {
        emissions_voting_contract: config.emissions_voting_contract,
    })
}

fn query_current_deposit_id(deps: Deps, _user: String) -> StdResult<membrane::transmuter::CurrentDepositIdResponse> {
    // Get the next deposit ID that will be assigned
    let next_id = CURRENT_DEPOSIT_ID
        .may_load(deps.storage)?
        .unwrap_or(Uint128::one());
    Ok(membrane::transmuter::CurrentDepositIdResponse {
        deposit_id: next_id,
    })
}

fn query_deposit_by_id(
    deps: Deps,
    user: String,
    deposit_id: Uint128,
) -> StdResult<membrane::transmuter::DepositByIdResponse> {
    let deposits = USER_DEPOSITS
        .may_load(deps.storage, user)?
        .unwrap_or_default();
    
    let deposit = deposits
        .into_iter()
        .find(|d| d.deposit_id == deposit_id)
        .ok_or_else(|| StdError::not_found(format!("Deposit with ID {} not found", deposit_id)))?;
    
    Ok(membrane::transmuter::DepositByIdResponse {
        deposit: membrane::transmuter::UserDeposit {
            deposit_id: deposit.deposit_id,
            amount: deposit.amount,
            deposit_time: deposit.deposit_time,
            locked: deposit.locked,
            start_time: deposit.start_time,
        },
    })
}

fn execute_transfer_deposit_ownership(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    user: String,
    deposit_id: Uint128,
    new_owner: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    
    // Only allow acquisition contract (or authorized contracts) to call this
    // For now, we'll allow the contract itself or check if there's an acquisition contract configured
    // TODO: Add proper authorization check when acquisition contract is configured
    
    let _user_addr = deps.api.addr_validate(&user)?;
    let _new_owner_addr = deps.api.addr_validate(&new_owner)?;
    
    // Load user's deposits
    let mut user_deposits = USER_DEPOSITS
        .may_load(deps.storage, user.clone())?
        .ok_or_else(|| ContractError::Validation(format!("User {} has no deposits", user)))?;
    
    // Find and remove the deposit
    let deposit_index = user_deposits
        .iter()
        .position(|d| d.deposit_id == deposit_id)
        .ok_or_else(|| ContractError::Validation(format!("Deposit with ID {} not found for user {}", deposit_id, user)))?;
    
    let deposit = user_deposits.remove(deposit_index);
    
    // Save updated user deposits (or remove if empty)
    if user_deposits.is_empty() {
        USER_DEPOSITS.remove(deps.storage, user.clone());
        // Remove weight tracking if no deposits
        crate::state::RETENTION_WEIGHT_TRACKING.remove(deps.storage, user.clone());
    } else {
        USER_DEPOSITS.save(deps.storage, user.clone(), &user_deposits)?;
        // Update retention weight tracking for old user
        update_retention_weight_tracking(
            deps.storage,
            &user,
            &user_deposits,
            &env,
            config.lock_ceiling,
        )?;
    }
    
    // Add deposit to new owner
    let mut new_owner_deposits = USER_DEPOSITS
        .may_load(deps.storage, new_owner.clone())?
        .unwrap_or_default();
    new_owner_deposits.push(deposit);
    USER_DEPOSITS.save(deps.storage, new_owner.clone(), &new_owner_deposits)?;
    
    // Update retention weight tracking for new owner
    update_retention_weight_tracking(
        deps.storage,
        &new_owner,
        &new_owner_deposits,
        &env,
        config.lock_ceiling,
    )?;
    
    Ok(Response::new()
        .add_attributes(vec![
            attr("action", "transfer_deposit_ownership"),
            attr("user", user),
            attr("deposit_id", deposit_id.to_string()),
            attr("new_owner", new_owner),
        ]))
}
