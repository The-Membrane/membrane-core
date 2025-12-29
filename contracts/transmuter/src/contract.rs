
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
    GlobalRateLimitResponse, RateHistoryResponse, RateHistoryEntry,
};
use membrane::types::StringEntry;
use membrane::types::AssetInfo;
use membrane::revenue_distributor::ExecuteMsg as RevenueDistributorExecuteMsg;

use crate::error::ContractError;
use crate::state::{
    append_transmute_snapshot, append_volume_window, append_rate_history_entry, apply_volume_update, history_slice,
    history_total, init_history, new_volume_window, CONFIG, TRANSMUTE_HISTORY, VOLUME_HISTORY,
    VOLUME_WINDOW, VAULT_TOKEN_SUPPLY, TransmuteSnapshot, RATE_LIMIT_FLOWS, FlowEntry, DEPLOYED_PAIRED_ASSET,
    TOKEN_RATE_ASSURANCE, TokenRateAssurance, GLOBAL_RATE_LIMIT_FLOWS, PENDING_REVENUE, CUMULATIVE_VOLUME,
    RATE_HISTORY, LAST_RATE_UPDATE,
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
    if msg.cdt_target_ratio > Decimal::one() {
        return Err(ContractError::Validation(
            "cdt_target_ratio must be less than or equal to 1".into(),
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
    let cdp_contract = msg.cdp_contract.clone();
    let discounts_contract = msg.discounts_contract.clone();
    let swap_history_cap = msg.swap_history_cap;
    let volume_history_cap = msg.volume_history_cap;
    let vault_subdenom = msg.vault_subdenom.clone();
    let tokenfactory_contract = msg.tokenfactory_contract.clone();

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

    let config = Config {
        owner: owner.clone(),
        tokenfactory_contract,
        cdp_contract,
        discounts_contract, 
        vault_token: vault_token.clone(),
        deposit_pair: msg.deposit_pair,
        composition_leeway: msg.composition_leeway,
        asset_a_to_b_rate: msg.asset_a_to_b_rate,
        cdt_target_ratio: msg.cdt_target_ratio,  
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
        lock_ceiling,
        affiliate_fee,
        send_swap_fee,
    };

    CONFIG.save(deps.storage, &config)?;
    VAULT_TOKEN_SUPPLY.save(deps.storage, &Uint128::zero())?;
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
            cdt_target_ratio,
            tokenfactory_contract,
            discounts_contract,
            cdp_contract,
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
            lock_ceiling,
            affiliate_fee,
            send_swap_fee,
        } => execute_update_config(
            deps,
            env,
            info.clone(),
            owner,
            deposit_pair,
            composition_leeway,
            asset_a_to_b_rate,
            cdt_target_ratio,
            tokenfactory_contract,
            discounts_contract,
            cdp_contract,
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
            lock_ceiling,
            affiliate_fee,
            send_swap_fee,
        ),
        ExecuteMsg::EnterVault { recipient, lock_days, affiliate_address } => execute_enter_vault(deps, env, info.clone(), recipient, lock_days, affiliate_address),
        ExecuteMsg::DepositFee {} => execute_deposit_fee(deps, env, info.clone()),
        ExecuteMsg::ExitVault {
            recipient,
            withdraw_as,
        } => execute_exit_vault(deps, env, info.clone(), recipient, withdraw_as),
        ExecuteMsg::Transmute { recipient } => execute_transmute(deps, env, info.clone(), recipient),
        ExecuteMsg::UpdateVolumeWindow {} => execute_update_volume_window(deps, env),
        ExecuteMsg::RateAssurance {} => execute_rate_assurance(deps, env, info.clone()),
        ExecuteMsg::SetAffiliate { user, affiliate_address, label } => execute_set_affiliate(deps, env, info, user, affiliate_address, label),
        ExecuteMsg::AddToRateHistory {} => execute_add_to_rate_history(deps, env),
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
    cdt_target_ratio: Option<Decimal>,
    tokenfactory_contract: Option<Addr>,
    discounts_contract: Option<String>,
    cdp_contract: Option<String>,
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
    lock_ceiling: Option<u64>,
    affiliate_fee: Decimal,
    send_swap_fee: Option<bool>,
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

    if let Some(target) = cdt_target_ratio {
        if target > Decimal::one() {
            return Err(ContractError::Validation(
                "target ratio must be less than or equal to 1".into(),
            ));
        }
        config.cdt_target_ratio = target;
    }

    if let Some(tf_addr) = tokenfactory_contract {
        config.tokenfactory_contract = Some(tf_addr);
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

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("action", "update_config"))
}

fn execute_enter_vault(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    recipient: Option<String>,
    lock_days: Option<u64>,
    affiliate_address: Option<String>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let total_deposits = get_total_deposit_value(deps.querier.clone(), &env, &config)?;
    let mut vault_supply = VAULT_TOKEN_SUPPLY.load(deps.storage)?;
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
    let is_revenue_distributor = config.revenue_distributor_addr.as_ref()
        .map(|rd_addr| info.sender == *rd_addr)
        .unwrap_or(false);
    if !is_revenue_distributor && !vault_supply.is_zero() {
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

    //Add rate assurance callback msg
    if !total_deposits.is_zero() && !vault_supply.is_zero() {
        println!("adding rate assurance callback msg");
        messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::RateAssurance {})?,
            funds: vec![],
        }));
    }


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

    // Mint VT to recipient
    if !vault_tokens_to_mint.is_zero() {
        // If locking, mint to contract first, otherwise mint directly to recipient
        if lock_days.is_some() {
            let mint_to_contract = mint_msg(
                config.tokenfactory_contract.clone(),
                env.contract.address.as_str(),
                &config.vault_token,
                vault_tokens_to_mint,
                env.contract.address.as_str(),
            )?.into();
            messages.push(mint_to_contract);
            
            // Store locked vault token entry
            if let Some(lock_days) = lock_days {
                const SECONDS_PER_DAY: u64 = 86_400;
                let locked_until = env.block.time.seconds()
                    .checked_add(lock_days.checked_mul(SECONDS_PER_DAY).ok_or_else(|| {
                        ContractError::Validation("lock_days overflow when calculating seconds".into())
                    })?)
                    .ok_or_else(|| ContractError::Validation("locked_until timestamp overflow".into()))?;
                
                let mut locked_tokens = crate::state::LOCKED_VAULT_TOKENS
                    .may_load(deps.storage, recipient_addr.to_string())?
                    .unwrap_or_default();
                locked_tokens.push(crate::state::LockedVaultToken {
                    amount: vault_tokens_to_mint,
                    locked_until,
                });
                crate::state::LOCKED_VAULT_TOKENS.save(deps.storage, recipient_addr.to_string(), &locked_tokens)?;
            }
        } else {
            // Send minted tokens directly to recipient
            messages.push(BankMsg::Send {
                to_address: recipient_addr.to_string(),
                amount: vec![coin(vault_tokens_to_mint.u128(), config.vault_token.clone())],
            }.into());
        }
        
        //Update the total vault supply
        vault_supply = increment_vault_supply(deps.storage, vault_supply, vault_tokens_to_mint)?;
    }
    // Handle affiliate if provided
    if let Some(affiliate_addr) = affiliate_address {
        add_affiliate_from_deposit(
            deps.storage,
            deps.api,
            recipient_addr.to_string(),
            affiliate_addr,
            config.affiliate_fee,
            env.block.time.seconds(),
        )?;
    }

    // println!("vault_tokens_to_mint: {:?}", vault_tokens_to_mint);
    VAULT_TOKEN_SUPPLY.save(deps.storage, &vault_supply)?;

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
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let total_deposits = get_total_deposit_value(deps.querier, &env, &config)?;
    let mut vault_supply = VAULT_TOKEN_SUPPLY.load(deps.storage)?;
    let mut messages: Vec<CosmosMsg> = vec![];

    if vault_supply.is_zero() {
        return Err(ContractError::Validation("no vault tokens in circulation".into()));
    }

    //Get the amount of vault tokens sent
    let vault_tokens = extract_coin_amount(&info, &config.vault_token)?;
    if vault_tokens.is_zero() {
        return Err(ContractError::InvalidFunds { reason: "no vault tokens provided".into() });
    }
    if vault_tokens > vault_supply {
        return Err(ContractError::InvalidFunds {
            reason: "vault token amount exceeds supply".into(),
        });
    }

    // Check for locked vault tokens
    let user_key = info.sender.to_string();
    let mut locked_tokens = crate::state::LOCKED_VAULT_TOKENS
        .may_load(deps.storage, user_key.clone())?
        .unwrap_or_default();
    
    // Calculate total locked amount (tokens that are still locked)
    let current_time = env.block.time.seconds();
    let mut total_locked: Uint128 = Uint128::zero();
    let mut expired_locked: Uint128 = Uint128::zero();
    
    // Separate expired and still-locked tokens
    let mut still_locked = Vec::new();
    for locked in locked_tokens.iter() {
        if locked.locked_until > current_time {
            total_locked = total_locked.checked_add(locked.amount)
                .unwrap_or(total_locked);
            still_locked.push(locked.clone());
        } else {
            // Lock has expired - these can be exited
            expired_locked = expired_locked.checked_add(locked.amount)
                .unwrap_or(expired_locked);
        }
    }
    
    // Update locked tokens list to only include still-locked ones
    crate::state::LOCKED_VAULT_TOKENS.save(deps.storage, user_key.clone(), &still_locked)?;
    
    // Get user's vault token balance
    let user_balance = deps.querier.query_balance(
        &deps.api.addr_validate(&user_key)?,
        &config.vault_token
    )?.amount;
    
    // Get contract's vault token balance (for expired locked tokens)
    let contract_balance = deps.querier.query_balance(
        &env.contract.address,
        &config.vault_token
    )?.amount;
    
    // Available balance = user balance + expired locked tokens in contract
    // (still-locked tokens are not available)
    let available_balance = user_balance
        .checked_add(expired_locked)
        .unwrap_or(user_balance);
    
    // Check if user is trying to exit more than available
    if vault_tokens > available_balance {
        return Err(ContractError::Validation(
            format!("Cannot exit {} vault tokens: only {} available ({} still locked until expiration)", 
                vault_tokens, available_balance, total_locked)
        ));
    }
    
    // If user needs expired locked tokens from contract, transfer them first
    if vault_tokens > user_balance {
        let needed_from_contract = vault_tokens.checked_sub(user_balance)
            .unwrap_or(Uint128::zero());
        
        if needed_from_contract > expired_locked {
            return Err(ContractError::Validation(
                format!("Cannot exit {} vault tokens: insufficient expired locked tokens available", 
                    vault_tokens)
            ));
        }
        
        if needed_from_contract > contract_balance {
            return Err(ContractError::Validation(
                format!("Cannot exit {} vault tokens: contract balance insufficient", 
                    vault_tokens)
            ));
        }
        
        // Transfer expired locked tokens from contract to user before exit
        messages.push(BankMsg::Send {
            to_address: user_key.clone(),
            amount: vec![coin(needed_from_contract.u128(), config.vault_token.clone())],
        }.into());
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
        .add_messages(messages)
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

    // Get current conversion rate
    let config = CONFIG.load(deps.storage)?;
    let total_deposit_tokens = get_total_deposit_value(deps.querier, &env, &config)?;
    let vault_token_supply = VAULT_TOKEN_SUPPLY.load(deps.storage)?;

    // Calculate conversion rate: base tokens per 1_000_000_000_000 vault tokens (1 VT)
    let conversion_rate = calculate_base_tokens(
        Uint128::new(1_000_000_000_000),
        total_deposit_tokens,
        vault_token_supply,
    )?;

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
        QueryMsg::LockedVaultTokens { user } => {
            let locked_tokens = crate::state::LOCKED_VAULT_TOKENS
                .may_load(deps.storage, user)?
                .unwrap_or_default();
            to_json_binary(&membrane::transmuter::LockedVaultTokensResponse { 
                locked_tokens: locked_tokens.into_iter().map(|t| membrane::transmuter::LockedVaultToken {
                    amount: t.amount,
                    locked_until: t.locked_until,
                }).collect()
            })
        }
        QueryMsg::GetAffiliates { user } => {
            to_json_binary(&crate::state::AFFILIATES.load(deps.storage, user).unwrap_or_else(|_| vec![]))
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

fn compute_effective_cdt_cdt_target_ratio(deps: Deps, env: &Env, config: &Config) -> StdResult<Decimal> {
    // total deposits in base A (cdt) units
    let total_deposits = get_total_deposit_value(deps.querier.clone(), env, config)
        .map_err(|e| StdError::generic_err(format!("{e}")))?;
    if total_deposits.is_zero() {
        return Ok(config.cdt_target_ratio);
    }
    // value of deployed paired asset converted to base A
    let deployed_paired = DEPLOYED_PAIRED_ASSET
        .load(deps.storage)
        .unwrap_or_else(|_| Uint128::zero());
    let deployed_value_in_a = convert_asset_b_to_a(deployed_paired, config.asset_a_to_b_rate)
        .map_err(|e| StdError::generic_err(format!("{e}")))?;
    let min_target = Decimal::from_ratio(deployed_value_in_a, total_deposits);
    Ok(if min_target > config.cdt_target_ratio { min_target } else { config.cdt_target_ratio })
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
        // let deposit_ratio = Decimal::from_ratio(deposit_a, deposit_value);
        // ensure_within_leeway(deposit_ratio, target, leeway)?;
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
    
    // If the total fee is in CDT, send it directly to revenue distributor.
    //THIS IS SAYING, FOR THE TRANSMUTER'S REVENUE, WHICH DISCO DO WE WANT TO DISTRIBUTE IT TO?
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
        return Err(ContractError::Std(StdError::generic_err(
            format!("Can't add more than {} affiliations", crate::state::AFFILIATE_LIMIT)
        )));
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
        return Err(StdError::GenericErr { 
            msg: format!("Sum of affiliate fees is greater than the affiliate fee: {} > {}", sum_of_affiliate_fees, affiliate_fee) 
        });
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
