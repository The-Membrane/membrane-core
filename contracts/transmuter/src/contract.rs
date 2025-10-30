
use cosmwasm_std::{
    attr, coin, entry_point, to_json_binary, Addr, BankMsg, Binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, Int128, MessageInfo, QuerierWrapper, Response, StdError, StdResult, Storage, Timestamp, Uint128, WasmMsg
};
use cw2::set_contract_version;
// use cw_storage_plus::Bound;

use membrane::helpers::get_contract_balances;
use membrane::math::{decimal_division, decimal_multiplication, decimal_subtraction};
use membrane::stability_pool_vault::{calculate_base_tokens, calculate_vault_tokens};
use membrane::cdp::{QueryMsg as CDP_QueryMsg};
use membrane::tokenfactory::{burn_msg, create_denom_msg, mint_msg};
use membrane::transmuter::{
    AssetPair, Config, ExecuteMsg, InstantiateMsg, QueryMsg, TransmuteHistoryResponse, SwapRecord,
    VaultInfoResponse, VolumeHistoryResponse, VolumeWindowResponse, RateLimitStatus, RateLimitStatusResponse, RateLimitManyResponse,
    GlobalRateLimitResponse,
};
use membrane::neutron_proxy::ExecuteMsg as NeutronProxyExecuteMsg;
use membrane::types::StringEntry;
use membrane::types::AssetInfo;
use membrane::revenue_distributor::ExecuteMsg as RevenueDistributorExecuteMsg;

use crate::error::ContractError;
use crate::state::{
    append_transmute_snapshot, append_volume_window, apply_volume_update, history_slice,
    history_total, init_history, new_volume_window, CONFIG, TRANSMUTE_HISTORY, VOLUME_HISTORY,
    VOLUME_WINDOW, VAULT_TOKEN_SUPPLY, TransmuteSnapshot, RATE_LIMIT_FLOWS, FlowEntry, DEPLOYED_PAIRED_ASSET,
    TOKEN_RATE_ASSURANCE, TokenRateAssurance, GLOBAL_RATE_LIMIT_FLOWS, PENDING_REVENUE,
    USER_INCENTIVES, INCENTIVE_SCHEDULE, INCENTIVE_EVENTS, UserIncentives, IncentiveSchedule, IncentiveEvent
};

const CONTRACT_NAME: &str = "membrane-transmuter";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

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
    if msg.asset_a_to_b_rate.is_zero() {
        return Err(ContractError::Validation(
            "asset_a_to_b_rate must be greater than zero".into(),
        ));
    }
    if msg.target_ratio > Decimal::one() {
        return Err(ContractError::Validation(
            "target_ratio must be less than or equal to 1".into(),
        ));
    }

    let vault_token = format!(
        "factory/{}/{}",
        env.contract.address,
        msg.vault_subdenom
    );

    //Save revenue_distributions early to avoid partial move
    let revenue_distributions = msg.revenue_distributions.clone();

    //Save values early to avoid partial move issues
    let revenue_contract = msg.revenue_contract.clone();
    let cdp_contract = msg.cdp_contract.clone();
    let swap_history_cap = msg.swap_history_cap;
    let volume_history_cap = msg.volume_history_cap;
    let vault_subdenom = msg.vault_subdenom.clone();
    let tokenfactory_contract = msg.tokenfactory_contract.clone();
    
    //Validate the revenue and cdp contract addresses
    let _ = deps.api.addr_validate(&revenue_contract)?;
    let _ = deps.api.addr_validate(&cdp_contract)?;

    // Default usage_fee to 1%
    let usage_fee = msg
        .usage_fee
        .unwrap_or(Decimal::percent(1));
    if usage_fee > Decimal::one() {
        return Err(ContractError::Validation(
            "usage_fee must be less than or equal to 1".into(),
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

    // Save and validate revenue distributor address if provided
    let revenue_distributor_addr_val = msg.revenue_distributor_addr.clone();
    let revenue_distributor_addr = revenue_distributor_addr_val
        .map(|addr| deps.api.addr_validate(&addr))
        .transpose()?;
    
    // Validate revenue distributions if provided
    let revenue_distributions = revenue_distributions.unwrap_or_default();
    if !revenue_distributions.is_empty() {
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

    let config = Config {
        owner: owner.clone(),
        tokenfactory_contract,
        revenue_contract,
        cdp_contract,
        vault_token: vault_token.clone(),
        deposit_pair: msg.deposit_pair,
        composition_leeway: msg.composition_leeway,
        asset_a_to_b_rate: msg.asset_a_to_b_rate,
        target_ratio: msg.target_ratio,  
        usage_fee,
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
        incentive_denom: None,
        neutron_proxy: None,
    };

    CONFIG.save(deps.storage, &config)?;
    VAULT_TOKEN_SUPPLY.save(deps.storage, &Uint128::zero())?;
    init_history(deps.storage)?;
    VOLUME_WINDOW.save(deps.storage, &new_volume_window(env.block.time))?;
    DEPLOYED_PAIRED_ASSET.save(deps.storage, &Uint128::zero())?;
    GLOBAL_RATE_LIMIT_FLOWS.save(deps.storage, &Vec::new())?;
    PENDING_REVENUE.save(deps.storage, &Uint128::zero())?;

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    // Initialize incentive schedule and events
    INCENTIVE_SCHEDULE.save(deps.storage, &IncentiveSchedule {
        last_accrued_time: env.block.time.seconds(),
        start_time: env.block.time.seconds(),
        total_monthly_emission: Uint128::zero(),
    })?;
    INCENTIVE_EVENTS.save(deps.storage, &Vec::new())?;

    let create_msg = create_denom_msg(
        config.tokenfactory_contract.clone(),
        env.contract.address.as_str(),
        &vault_subdenom,
    )?;

    Ok(Response::new()
        .add_message(create_msg)
        .add_attributes(vec![
            attr("action", "instantiate"),
            attr("owner", owner.as_str()),
            attr("vault_token", vault_token),
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
            asset_a_to_b_rate,
            target_ratio,
            tokenfactory_contract,
            cdp_contract,
            revenue_contract,
            usage_fee,
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
            monthly_incentive_max,
            incentive_denom,
            neutron_proxy,
        } => execute_update_config(
            deps,
            env,
            info.clone(),
            owner,
            deposit_pair,
            composition_leeway,
            asset_a_to_b_rate,
            target_ratio,
            tokenfactory_contract,
            cdp_contract,
            revenue_contract,
            usage_fee,
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
            monthly_incentive_max,
            incentive_denom,
            neutron_proxy,
        ),
        ExecuteMsg::EnterVault { recipient, deposit_for_incentives } => execute_enter_vault(deps, env, info.clone(), recipient, deposit_for_incentives),
        ExecuteMsg::DepositFee {} => execute_deposit_fee(deps, env, info.clone()),
        ExecuteMsg::ExitVault {
            recipient,
            withdraw_as,
            use_incentive_deposits,
        } => execute_exit_vault(deps, env, info.clone(), recipient, withdraw_as, use_incentive_deposits),
        ExecuteMsg::Transmute { recipient } => execute_transmute(deps, env, info.clone(), recipient),
        ExecuteMsg::UpdateVolumeWindow {} => execute_update_volume_window(deps, env),
        ExecuteMsg::DepositIncentives {} => execute_deposit_incentives(deps, env, info.clone()),
        ExecuteMsg::RateAssurance {} => execute_rate_assurance(deps, env, info.clone()),
        ExecuteMsg::ClaimIncentivesForUser { user, limit } => execute_claim_incentives(deps, env, user, limit),
    }
}

fn execute_update_config(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    owner: Option<String>,
    deposit_pair: Option<AssetPair>,
    composition_leeway: Option<Decimal>,
    asset_a_to_b_rate: Option<Decimal>,
    target_ratio: Option<Decimal>,
    tokenfactory_contract: Option<Addr>,
    cdp_contract: Option<String>,
    revenue_contract: Option<String>,
    usage_fee: Option<Decimal>,
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
    monthly_incentive_max: Option<Uint128>,
    incentive_denom: Option<String>,
    neutron_proxy: Option<String>,
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

    if let Some(rate) = asset_a_to_b_rate {
        if rate.is_zero() {
            return Err(ContractError::Validation(
                "asset_a_to_b_rate must be greater than zero".into(),
            ));
        }
        config.asset_a_to_b_rate = rate;
    }

    if let Some(target) = target_ratio {
        if target > Decimal::one() {
            return Err(ContractError::Validation(
                "target ratio must be less than or equal to 1".into(),
            ));
        }
        config.target_ratio = target;
    }

    if let Some(tf_addr) = tokenfactory_contract {
        config.tokenfactory_contract = Some(tf_addr);
    }

    if let Some(cdp) = cdp_contract {
        //Validate the address
        deps.api.addr_validate(&cdp)?;
        config.cdp_contract = cdp;
    }

    if let Some(rc) = revenue_contract {
        //Validate the address
        deps.api.addr_validate(&rc)?;
        config.revenue_contract = rc;
    }

    if let Some(fee) = usage_fee {
        if fee > Decimal::one() {
            return Err(ContractError::Validation(
                "usage_fee must be less than or equal to 1".into(),
            ));
        }
        config.usage_fee = fee;
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

    if let Some(rd_addr) = revenue_distributor_addr {
        config.revenue_distributor_addr = Some(deps.api.addr_validate(&rd_addr)?);
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

    if let Some(monthly) = monthly_incentive_max {
        INCENTIVE_SCHEDULE.update(deps.storage, |mut s| -> StdResult<_> {
            s.total_monthly_emission = monthly;
            Ok(s)
        })?;
    }
    if let Some(proxy) = neutron_proxy {
        let addr = deps.api.addr_validate(&proxy)?;
        config.neutron_proxy = Some(addr);
    }
    if let Some(denom) = incentive_denom {
        config.incentive_denom = Some(denom);
    }


    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("action", "update_config"))
}

fn execute_enter_vault(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    recipient: Option<String>,
    deposit_for_incentives: Option<bool>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let total_deposits = get_total_deposit_value(deps.querier.clone(), &env, &config)?;
    let mut vault_supply = VAULT_TOKEN_SUPPLY.load(deps.storage)?;

    //Split the funds into the two assets
    let (deposit_a, deposit_b) = segregate_funds(&config.deposit_pair, &info)?;
    if deposit_a.is_zero() && deposit_b.is_zero() {
        return Err(ContractError::InvalidFunds {
            reason: "no valid funds provided".into(),
        });
    }

    // println!("deposit_a: {:?}", deposit_a);
    // println!("deposit_b: {:?}", deposit_b);
    //If the sender is not the revenue contract, ensure the deposit is aligned with the deposit pair
    //Revenue contract can deposit any ratio into the contract.
    //Which will tend to be 100% CDT.
    if info.clone().sender.to_string() != config.revenue_contract {
        // Compute effective CDT target ratio considering deployed paired asset
        let effective_target = compute_effective_cdt_target_ratio(deps.as_ref(), &env, &config)?;
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

    //Calc user deposit value, denominated in asset A
    let user_deposit_value = sum_base_value(deposit_a, deposit_b, config.asset_a_to_b_rate)?;
    if user_deposit_value.is_zero() {
        return Err(ContractError::InvalidFunds {
            reason: "deposit value is zero".into(),
        });
    }
    // println!("user_deposit_value: {:?}", user_deposit_value);
    // println!("total_deposits: {:?}", total_deposits);
    // println!("vault_supply: {:?}", vault_supply);

    //Calc & save base token rates for rate assurance
    let pre_btokens_per_one = calculate_base_tokens(
        Uint128::new(1_000_000_000_000), 
        total_deposits - user_deposit_value, 
        vault_supply
    )?;
    TOKEN_RATE_ASSURANCE.save(deps.storage, &TokenRateAssurance {
        pre_btokens_per_one,
    })?;

    //Calc the amount of vault tokens to mint
    let vault_tokens_to_mint = calculate_vault_tokens(
        user_deposit_value,
         total_deposits - user_deposit_value,
          vault_supply
    )?;

    //Get the recipient address
    let recipient_addr = recipient
        .map(|r| deps.api.addr_validate(&r))
        .transpose()?;
    let recipient_addr = recipient_addr.unwrap_or_else(|| info.sender.clone());

    let mut messages: Vec<CosmosMsg> = vec![];
    // Mint VT to contract, then either keep for incentives or forward to recipient
    if !vault_tokens_to_mint.is_zero() {
        let mint_to_contract = mint_msg(
            config.tokenfactory_contract.clone(),
            env.contract.address.as_str(),
            &config.vault_token,
            vault_tokens_to_mint,
            env.contract.address.as_str(),
        )?.into();
        messages.push(mint_to_contract);
        //Update the total vault supply
        vault_supply = increment_vault_supply(deps.storage, vault_supply, vault_tokens_to_mint)?;

        if deposit_for_incentives.unwrap_or(false) {
            let now = env.block.time.seconds();
            //Load state
    let mut record = USER_INCENTIVES
                .may_load(deps.storage, recipient_addr.to_string())?
        .unwrap_or(UserIncentives {
            total_claimed: Uint128::zero(),
            vault_tokens_in_contract: Uint128::zero(),
            last_accrued: now,
        });
                //Add minted tokens to user state
            record.vault_tokens_in_contract = record.vault_tokens_in_contract
                .checked_add(vault_tokens_to_mint)
                .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
            //Save state
            USER_INCENTIVES.save(deps.storage, recipient_addr.to_string(), &record)?;
        } else {
            //Send minted tokens to recipient if not depositing for incentives
            messages.push(BankMsg::Send {
                to_address: recipient_addr.to_string(),
                amount: vec![coin(vault_tokens_to_mint.u128(), config.vault_token.clone())],
            }.into());
        }
    }
// println!("vault_tokens_to_mint: {:?}", vault_tokens_to_mint);
    VAULT_TOKEN_SUPPLY.save(deps.storage, &vault_supply)?;

    //Add rate assurance callback msg
    if !total_deposits.is_zero() && !vault_supply.is_zero() {
        messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::RateAssurance {})?,
            funds: vec![],
        }));
    }

    Ok(Response::new()
        .add_messages(messages)
        .add_attributes(vec![
            attr("action", "enter_vault"),
            attr("base_amount", user_deposit_value.to_string()),
            attr("vault_tokens", vault_tokens_to_mint.to_string()),
            attr("recipient", recipient_addr.as_str()),
        ]))
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

fn execute_exit_vault(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    recipient: Option<String>,
    withdraw_as: Option<String>,
    use_incentive_deposits: Option<Uint128>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let total_deposits = get_total_deposit_value(deps.querier, &env, &config)?;
    let mut vault_supply = VAULT_TOKEN_SUPPLY.load(deps.storage)?;

    if vault_supply.is_zero() {
        return Err(ContractError::Validation("no vault tokens in circulation".into()));
    }

    //Get the amount of vault tokens sent
    let mut vault_tokens = extract_coin_amount(&info, &config.vault_token)?;
    let incentive_held_tokens = use_incentive_deposits.unwrap_or(Uint128::zero());
    // optionally include incentive-held VT
    if !incentive_held_tokens.is_zero() || vault_tokens.is_zero() {
        let key = info.sender.to_string();
        if let Ok(mut user) = USER_INCENTIVES.load(deps.storage, key.clone()) {
            if !user.vault_tokens_in_contract.is_zero() {
                //Subtract the exiting vault_tokens from the incentive-held vault token state
                user.vault_tokens_in_contract = user.vault_tokens_in_contract.checked_sub(incentive_held_tokens)
                .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;

                //Add the incentive-held vault tokens to the total vault tokens
                vault_tokens = vault_tokens
                    .checked_add(incentive_held_tokens)
                    .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;

                //Save state
                USER_INCENTIVES.save(deps.storage, key, &user)?;
            }
        }
    }
    if vault_tokens.is_zero() {
        return Err(ContractError::InvalidFunds { reason: "no vault tokens provided".into() });
    }
    if vault_tokens > vault_supply {
        return Err(ContractError::InvalidFunds {
            reason: "vault token amount exceeds supply".into(),
        });
    }

    //Calc & save base token rates for rate assurance
    let pre_btokens_per_one = calculate_base_tokens(
        Uint128::new(1_000_000_000_000), 
        total_deposits, 
        vault_supply
    )?;
    TOKEN_RATE_ASSURANCE.save(deps.storage, &TokenRateAssurance {
        pre_btokens_per_one,
    })?;

    //Calc withdraw value, denominated in asset A
    let base_amount = calculate_base_tokens(
        vault_tokens,
         total_deposits,
          vault_supply
        )?;

    //Get the balances of the contract
    let balances = current_balances(deps.querier, &env, &config.deposit_pair)?;
    //Calc user share of assets
    let user_share_of_assets = Decimal::from_ratio(vault_tokens, vault_supply);
    //Calc user share of asset A
    let asset_a_share = decimal_multiplication(
        Decimal::from_ratio(balances.0, Uint128::one()),
        user_share_of_assets
    )?.to_uint_floor();
    //Calc user share of asset B
    let asset_b_share = decimal_multiplication(
        Decimal::from_ratio(balances.1, Uint128::one()),
        user_share_of_assets
    )?.to_uint_floor();

    //Get the recipient address
    let recipient_addr = recipient
        .map(|r| deps.api.addr_validate(&r))
        .transpose()?;
    let recipient_addr = recipient_addr.unwrap_or_else(|| info.sender.clone());

    let mut send_coins: Vec<Coin> = vec![];

    //Send the assets to the recipient & convert assets if needed
    match withdraw_as {
        Some(ref denom) if denom == &config.deposit_pair.cdt => {
            let converted = convert_asset_b_to_a(asset_b_share, config.asset_a_to_b_rate)?;
            let total_a_needed = asset_a_share
                .checked_add(converted)
                .map_err(|err| ContractError::Std(err.into()))?;
            if balances.0 < total_a_needed {
                return Err(ContractError::InsufficientLiquidity(config.deposit_pair.cdt.clone()));
            }
            if !total_a_needed.is_zero() {
                send_coins.push(coin(total_a_needed.u128(), denom));
            }
        }
        Some(ref denom) if denom == &config.deposit_pair.paired_asset => {
            let converted = convert_asset_a_to_b(asset_a_share, config.asset_a_to_b_rate)?;
            let total_b_needed = asset_b_share
                .checked_add(converted)
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

    let burn = burn_msg(
        config.tokenfactory_contract.clone(),
        env.contract.address.as_str(),
        &config.vault_token,
        vault_tokens,
        env.contract.address.as_str(),
    )?;

    //Update the total vault supply
    vault_supply = decrement_vault_supply(deps.storage, vault_supply, vault_tokens)?;
    VAULT_TOKEN_SUPPLY.save(deps.storage, &vault_supply)?;

    let mut response = Response::new()
        .add_message(burn)
        .add_attribute("action", "exit_vault")
        .add_attribute("base_amount", base_amount.to_string())
        .add_attribute("vault_tokens", vault_tokens.to_string())
        .add_attribute("recipient", recipient_addr.as_str());

    if !send_coins.is_empty() {
        response = response.add_message(BankMsg::Send {
            to_address: recipient_addr.to_string(),
            amount: send_coins,
        });
    }

    //Add rate assurance callback msg
    if !total_deposits.is_zero() && !vault_supply.is_zero() {
        response = response.add_message(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::RateAssurance {})?,
            funds: vec![],
        }));
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
    
    // Determine allowlist status
    let is_allowlisted = is_allowlisted_sender(&deps.querier, &info.sender, &config)?;

    //Calc user value sent BEFORE fee deduction, denominated in asset A
    let user_value_sent = sum_base_value(funds_a, funds_b, config.asset_a_to_b_rate)?;
    
    //Calculate fee amount before applying deduction
    let fee_info: Option<(Uint128, String)> = if !is_allowlisted && info.sender != env.contract.address {
        if config.usage_fee == Decimal::one() {
            return Err(ContractError::InvalidFunds {
                reason: "Blocking non-CDP & non-deployable venue usage".into(),
            });
        } else {
            //Calculate fee (original - post-fee)
            let fee_rate = config.usage_fee;
            let fee_amount_a = decimal_multiplication(
                Decimal::from_ratio(funds_a, Uint128::one()),
                fee_rate
            )?.to_uint_floor();
            let fee_amount_b = decimal_multiplication(
                Decimal::from_ratio(funds_b, Uint128::one()),
                fee_rate
            )?.to_uint_floor();
            
            //Determine which asset the fee is in
            let (fee_amount_val, fee_denom_val) = if !funds_a.is_zero() {
                (fee_amount_a, pair.cdt.clone())
            } else {
                (fee_amount_b, pair.paired_asset.clone())
            };
            
            //Set the usage fee
            let usage_fee = decimal_subtraction(Decimal::one(), config.usage_fee)?;
            //Subtract the usage fee from the amount of the asset A sent
            funds_a = decimal_multiplication(
                Decimal::from_ratio(funds_a, Uint128::one()), 
                usage_fee
            )?.to_uint_floor();
            //Subtract the usage fee from the amount of the asset B sent
            funds_b = decimal_multiplication(
                Decimal::from_ratio(funds_b, Uint128::one()), 
                usage_fee
            )?.to_uint_floor();
            
            Some((fee_amount_val, fee_denom_val))
        }
    } else {
        None
    };
    
    let (offered_asset, offered_amount, received_asset, received_amount) = if !funds_a.is_zero() {
        let receive_amount = convert_asset_a_to_b(funds_a, config.asset_a_to_b_rate)?;
        ensure_contract_balance(deps.querier, &env, &pair.paired_asset, receive_amount)?;
        (
            pair.cdt.clone(),
            funds_a,
            pair.paired_asset.clone(),
            receive_amount,
        )
    } else {
        let receive_amount = convert_asset_b_to_a(funds_b, config.asset_a_to_b_rate)?;
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
    )? - user_value_sent;
    // choose threshold based on whitelist membership
    let is_allowlisted = config.allowlist.iter().any(|a| a == &info.sender.to_string());
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

    VOLUME_WINDOW.update(deps.storage, |mut window| -> StdResult<_> {
        apply_volume_update(
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
        );
        Ok(window)
    })?;

    // println!("window: {:?}", VOLUME_WINDOW.load(deps.storage)?);

    let mut response = Response::new()
        .add_attribute("action", "transmute")
        .add_attribute("offered_asset", offered_asset)
        .add_attribute("offered_amount", offered_amount.to_string())
        .add_attribute("received_asset", received_asset)
        .add_attribute("received_amount", received_amount.to_string())
        .add_attribute("recipient", recipient_addr.as_str());

    if !send_coins.is_empty() {
        response = response.add_message(BankMsg::Send {
            to_address: recipient_addr.to_string(),
            amount: send_coins,
        });
    }

    //Collect and distribute fees if any - pass storage access correctly
    if let Some((fee_amount_val, fee_denom)) = fee_info {
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

    VOLUME_WINDOW.save(deps.storage, &new_volume_window(env.block.time))?;

    Ok(Response::new().add_attribute("action", "update_volume_window"))
}

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::VaultInfo {} => to_json_binary(&query_vault_info(deps, env)?),
        QueryMsg::VaultTokenUnderlying { vault_token_amount } => to_json_binary(&calculate_base_tokens(
            vault_token_amount,
             get_total_deposit_value(deps.querier, &env, &CONFIG.load(deps.storage)?).map_err(|_err| StdError::GenericErr { msg: format!("Failed to query the contract for the total deposit value") })?,
              VAULT_TOKEN_SUPPLY.load(deps.storage)?
            )?),
        QueryMsg::DepositTokenConversion { deposit_token_amount } => to_json_binary(&calculate_vault_tokens(
            deposit_token_amount,
             get_total_deposit_value(deps.querier, &env, &CONFIG.load(deps.storage)?).map_err(|_err| StdError::GenericErr { msg: format!("Failed to query the contract for the total deposit value") })?,
              VAULT_TOKEN_SUPPLY.load(deps.storage)?
            )?,),
        QueryMsg::TransmuteHistory { start_after, limit } => {
            to_json_binary(&query_swap_history(deps, start_after, limit)?)
        }
        QueryMsg::VolumeHistory { start_after, limit } => {
            to_json_binary(&query_volume_history(deps, start_after, limit)?)
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
            let target = compute_effective_cdt_target_ratio(deps, &env, &CONFIG.load(deps.storage)?)?;
            to_json_binary(&membrane::transmuter::EffectiveTargetResponse { target })
        }
        QueryMsg::RateLimitMany { addresses, start_after, limit } => {
            to_json_binary(&query_rate_limit_many(deps, env, addresses, start_after, limit)?)
        }
        QueryMsg::GlobalRateLimit {} => {
            to_json_binary(&query_global_rate_limit(deps, env)?)
        }
    }
}

fn query_vault_info(deps: Deps, env: Env) -> StdResult<VaultInfoResponse> {
    let config = CONFIG.load(deps.storage)?;
    let total_deposit_value = get_total_deposit_value(deps.querier, &env, &config).map_err(|_err| StdError::GenericErr { msg: format!("Failed to query the contract for the total deposit value") })?;
    let vault_token_supply = VAULT_TOKEN_SUPPLY.load(deps.storage)?;
    let balances = current_balances(deps.querier, &env, &config.deposit_pair)?;

    Ok(VaultInfoResponse {
        total_deposit_value,
        vault_token_supply,
        cdt_balance: balances.0,
        paired_asset_balance: balances.1,
    })
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
    let total_deposits = get_total_deposit_value(deps.querier, env, &config)
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

fn sum_base_value(
    asset_a_amount: Uint128,
    asset_b_amount: Uint128,
    rate: Decimal,
) -> Result<Uint128, ContractError> {
    let converted_b_to_a = convert_asset_b_to_a(asset_b_amount, rate)?;
    asset_a_amount
        .checked_add(converted_b_to_a)
        .map_err(|err| ContractError::Std(err.into()))
}

fn compute_effective_cdt_target_ratio(deps: Deps, env: &Env, config: &Config) -> StdResult<Decimal> {
    // total deposits in base A (cdt) units
    let total_deposits = get_total_deposit_value(deps.querier.clone(), env, config)
        .map_err(|e| StdError::generic_err(format!("{e}")))?;
    if total_deposits.is_zero() {
        return Ok(config.target_ratio);
    }
    // value of deployed paired asset converted to base A
    let deployed_paired = DEPLOYED_PAIRED_ASSET
        .load(deps.storage)
        .unwrap_or_else(|_| Uint128::zero());
    let deployed_value_in_a = convert_asset_b_to_a(deployed_paired, config.asset_a_to_b_rate)
        .map_err(|e| StdError::generic_err(format!("{e}")))?;
    let min_target = Decimal::from_ratio(deployed_value_in_a, total_deposits);
    Ok(if min_target > config.target_ratio { min_target } else { config.target_ratio })
}

fn convert_asset_a_to_b(amount: Uint128, rate: Decimal) -> Result<Uint128, ContractError> {
    let decimal_amount = Decimal::from_ratio(amount, Uint128::one());
    Ok(decimal_multiplication(rate, decimal_amount)?.to_uint_floor())
}

fn convert_asset_b_to_a(amount: Uint128, rate: Decimal) -> Result<Uint128, ContractError> {
    if rate.is_zero() {
        return Err(ContractError::Validation(
            "asset_a_to_b_rate must be greater than zero".into(),
        ));
    }
    let inv_rate = decimal_division(Decimal::one(), rate)?;
    let decimal_amount = Decimal::from_ratio(amount, Uint128::one());
    Ok(decimal_multiplication(inv_rate, decimal_amount)?.to_uint_floor())
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

    //Get the total deposit value before the deposit
    let total_before = sum_base_value(pre_a, pre_b, config.asset_a_to_b_rate)?;
    //Get the user deposit value
    let deposit_value = sum_base_value(deposit_a, deposit_b, config.asset_a_to_b_rate)?;
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
        let deposit_ratio = Decimal::from_ratio(deposit_a, deposit_value);
        ensure_within_leeway(deposit_ratio, target, leeway)?;
        return Ok(());
    }

    //Get the current ratio (pre-deposit)
    let current_ratio = Decimal::from_ratio(pre_a, total_before);

    let new_a = balances_after.0;
    let new_b = balances_after.1;
    //Get the total contract value after the deposit
    let total_after = sum_base_value(new_a, new_b, config.asset_a_to_b_rate)?;
    if total_after.is_zero() {
        return Err(ContractError::InvalidFunds {
            reason: "resulting deposit value is zero".into(),
        });
    }

    //Get the new ratio of Asset A (post-deposit)
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

fn get_total_deposit_value(
    querier: QuerierWrapper,
    env: &Env,
    config: &Config,
) -> Result<Uint128, ContractError> {
    let balances = current_balances(querier, env, &config.deposit_pair)?;
    sum_base_value(balances.0, balances.1, config.asset_a_to_b_rate)
}

fn increment_vault_supply(
    storage: &mut dyn Storage,
    current_supply: Uint128,
    amount: Uint128,
) -> Result<Uint128, ContractError> {
    let new_supply = current_supply
        .checked_add(amount)
        .map_err(|err| ContractError::Std(err.into()))?;
    VAULT_TOKEN_SUPPLY.save(storage, &new_supply)?;
    Ok(new_supply)
}

fn decrement_vault_supply(
    storage: &mut dyn Storage,
    current_supply: Uint128,
    amount: Uint128,
) -> Result<Uint128, ContractError> {
    let new_supply = current_supply
        .checked_sub(amount)
        .map_err(|err| ContractError::Std(err.into()))?;
    VAULT_TOKEN_SUPPLY.save(storage, &new_supply)?;
    Ok(new_supply)
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
    let total_vault_tokens = VAULT_TOKEN_SUPPLY.load(deps.storage)?;

    //Get total deposit tokens
    let total_deposit_tokens = get_total_deposit_value(deps.querier.clone(), &env, &config)?;

    //Calc the rate of vault tokens to deposit tokens
    let btokens_per_one = calculate_base_tokens(
        Uint128::new(1_000_000_000_000), 
        total_deposit_tokens, 
        total_vault_tokens
    )?;

    //Check that the rates are within 1 millionth
    if !(btokens_per_one + Uint128::one() >= token_rate_assurance.pre_btokens_per_one) {
        return Err(ContractError::CustomError { val: format!("Conversation rate assurance failed, should be equal or greater than. If its 1 off just try again. Deposit tokens per 1 pre-tx: {:?} --- post-tx: {:?}", token_rate_assurance.pre_btokens_per_one, btokens_per_one) });
    }
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
    
    // If the total fee is in CDT, send it directly to revenue distributor
    if total_fee_denom == config.deposit_pair.cdt {
        // Convert revenue_distributions ratios to Asset amounts
        let mut ltv_disco_distribution = Vec::new();
        for liq_asset in &config.revenue_distributions {
            let amount: Uint128 = decimal_multiplication(
                Decimal::from_ratio(total_fee_amount, Uint128::one()),
                liq_asset.amount
            )?.to_uint_floor();
            if !amount.is_zero() {
                ltv_disco_distribution.push(membrane::types::Asset {
                    info: liq_asset.info.clone(),
                    amount,
                });
            }
        }

        //Ensure the total distribution amount is less than the total fee amount
        let total_distribution_amount = ltv_disco_distribution.iter().map(|asset| asset.amount).sum::<Uint128>();
        if total_distribution_amount > total_fee_amount {
            return Err(ContractError::Std(StdError::generic_err("Total distribution amount is greater than total fee amount")));
        }
        
        // Send CDT to revenue distributor via SetPromises
        let set_promises_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: rd_addr.to_string(),
            msg: to_json_binary(&RevenueDistributorExecuteMsg::SetPromises {
                promises: vec![],
                ltv_disco_distribution: Some(ltv_disco_distribution),
            })?,
            funds: vec![coin(total_fee_amount.u128(), config.deposit_pair.cdt.clone())],
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
        // No CDT available, total_fee_amount already includes any previous pending + current fee
        // We've already added current_fee to pending in the logic above (lines 1583-1591)
        // So we just save the updated total_fee_amount which is the new pending
        PENDING_REVENUE.save(storage, &total_fee_amount)?;
        return Ok(messages);
    }
    
    // Calculate how much paired_asset we can transmute based on CDT available
    // We want to transmute min(total_fee_amount, amount_that_can_be_covered_by_cdt_balance)
    let max_paired_asset_transmutable = convert_asset_b_to_a(cdt_balance, config.asset_a_to_b_rate)?;
    let amount_to_transmute = if total_fee_amount <= max_paired_asset_transmutable {
        total_fee_amount
    } else {
        max_paired_asset_transmutable
    };
    
    // Calculate how much CDT we'll get from this transmutation
    let cdt_received = convert_asset_a_to_b(amount_to_transmute, config.asset_a_to_b_rate)?;
    
    // Update pending revenue with remaining amount that couldn't be transmuted
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
    let total_deposits = get_total_deposit_value(deps.querier, &env, &config)
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

// ================= Incentives =================
pub(crate) fn accrue_incentive_event(
    deps: DepsMut,
    env: &Env,
) -> Result<(), ContractError> {
    //Load the config   
    let config = CONFIG.load(deps.storage)?;
    //Load the incentive schedule
    let mut schedule = INCENTIVE_SCHEDULE.load(deps.storage)?;
    //If the total monthly emission is zero, return
    if schedule.total_monthly_emission.is_zero() { 
        return Ok(()); 
    }
    //If the last accrued time is greater than the current time, return
    let now = env.block.time.seconds();
    if now <= schedule.last_accrued_time { 
        return Ok(()); 
    }
    //Calculate the elapsed time since the last accrued time
    let elapsed = now - schedule.last_accrued_time;
    let month_secs: u64 = 30 * 24 * 60 * 60;
    //Calculate the rate per second
    let rate_per_sec = Decimal::from_ratio(schedule.total_monthly_emission, Uint128::from(month_secs));
    let mut amount_to_emit = rate_per_sec * Uint128::from(elapsed as u128);
    //If the amount to emit is not zero, calculate the amount per vault token
    if !amount_to_emit.is_zero() {

        //Query the vault token balance.
        //Big assumtption that all VT tokens in the contaract are deposited by a user for incentives.
        let vt_balance = deps.querier.query_balance(&env.contract.address, &config.vault_token)?.amount;
        //If the vault token supply is not zero, calculate the amount per vault token
        if !vt_balance.is_zero() {
            //Calculate the amount per vault token
            let amount_per_vt = Decimal::from_ratio(amount_to_emit, vt_balance);
            //Create a new incentive event
            let mut events = INCENTIVE_EVENTS.load(deps.storage).unwrap_or_default();
            events.push(IncentiveEvent { amount_per_vt, time_of_event: now, amount_left_to_claim: amount_to_emit });
            //Save the incentive events
            INCENTIVE_EVENTS.save(deps.storage, &events)?;
        }
    }
    //Set the last accrued time to the current time
    schedule.last_accrued_time = now;
    //Save the incentive schedule
    INCENTIVE_SCHEDULE.save(deps.storage, &schedule)?;
    //Return ok
    Ok(())
}

fn execute_claim_incentives(
    mut deps: DepsMut,
    env: Env,
    user: String,
    limit: Option<u32>,
) -> Result<Response, ContractError> {
    accrue_incentive_event(deps.branch(), &env)?;
    let mut events = INCENTIVE_EVENTS.load(deps.storage).unwrap_or_default();
    //Load the user incentives
    let mut record = USER_INCENTIVES
        .may_load(deps.storage, user.clone())?
        .unwrap_or(UserIncentives { total_claimed: Uint128::zero(), vault_tokens_in_contract: Uint128::zero(), last_accrued: 0 });
    //Get the vault token balance for the user
    let vt = record.vault_tokens_in_contract;
    if vt.is_zero() {
        //If the vault token balance is zero, return ok
        return Ok(Response::new().add_attribute("action", "claim_incentives").add_attribute("user", user)); 
    }
    //Calculate the maximum number of events to process
    let max_events = limit.unwrap_or(100).min(200) as usize;
    //Initialize the processed counter
    let mut processed = 0usize;
    let mut total_claimed = Uint128::zero();
    for ev in events.iter_mut() {
        if processed >= max_events { break; }
        //If the event time is less than (happened before) the last accrued time, continue
        if ev.time_of_event <= record.last_accrued { continue; }
        //If the amount left to claim is zero, continue
        if ev.amount_left_to_claim.is_zero() { continue; }
        //Calculate the share of the event
        let mut share = ev.amount_per_vt * vt;
        //If the user's share is zero, continue
        if share.is_zero() { processed += 1; continue; }
        //If the share is greater than the amount left to claim, set the share to the amount left to claim
        if share > ev.amount_left_to_claim { share = ev.amount_left_to_claim; }
        //Subtract the share from the amount left to claim
        ev.amount_left_to_claim = ev.amount_left_to_claim.saturating_sub(share);
        //Add the share to the total claimed
        total_claimed = total_claimed.saturating_add(share);
        //Increment the processed counter
        processed += 1;
    }
    //Remove events that have no amount left to claim
    events.retain(|e| !e.amount_left_to_claim.is_zero());
    //Save the incentive events
    INCENTIVE_EVENTS.save(deps.storage, &events)?;
    //Add the total claimed to the user's total claimed
    record.total_claimed = record.total_claimed.saturating_add(total_claimed);
    //Set the last accrued time to the current time
    record.last_accrued = env.block.time.seconds();
    //Save the user incentives state
    USER_INCENTIVES.save(deps.storage, user.clone(), &record)?;

    let mut resp = Response::new().add_attribute("action", "claim_incentives").add_attribute("user", user.clone()).add_attribute("claimed", total_claimed.to_string());
    //If the total claimed is not zero, mint the tokens
    if !total_claimed.is_zero() {
        let cfg = CONFIG.load(deps.storage)?;
        if let (Some(proxy), Some(denom)) = (cfg.neutron_proxy, cfg.incentive_denom) {
            //Create a mint tokens message
            let mint_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: proxy.to_string(),
                msg: to_json_binary(&NeutronProxyExecuteMsg::MintTokens { denom, amount: total_claimed, mint_to_address: user })?,
                funds: vec![]
            });
            //Add the mint tokens message to the response
            resp = resp.add_message(mint_msg);
        }
    }
    //Return the response
    Ok(resp)
}

fn execute_deposit_incentives(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    //Load the config
    let config = CONFIG.load(deps.storage)?;
    //Get the vault token amount
    let vt_amount = extract_coin_amount(&info, &config.vault_token)?;
    //If the vault token amount is zero, return an error
    if vt_amount.is_zero() {
        return Err(ContractError::InvalidFunds { reason: "no vault tokens provided".into() });
    }
    //Get the current time
    let now = env.block.time.seconds();
    //Load the user incentives
    let mut record = USER_INCENTIVES
        .may_load(deps.storage, info.sender.to_string())?
        .unwrap_or(UserIncentives { total_claimed: Uint128::zero(), vault_tokens_in_contract: Uint128::zero(), last_accrued: now });
    //Add the vault tokens to the user's vault tokens in contract
    record.vault_tokens_in_contract = record.vault_tokens_in_contract
        .checked_add(vt_amount)
        .map_err(|e| ContractError::Std(StdError::generic_err(e.to_string())))?;
    //Save the user incentives
    USER_INCENTIVES.save(deps.storage, info.sender.to_string(), &record)?;

    //Return the response
    Ok(Response::new()
        .add_attribute("action", "deposit_incentives")
        .add_attribute("amount", vt_amount.to_string())
        .add_attribute("user", info.sender.to_string())
    )
}
