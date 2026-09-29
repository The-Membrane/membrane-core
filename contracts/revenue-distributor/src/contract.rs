use cosmwasm_std::{
    entry_point, to_json_binary, BankMsg, CosmosMsg, DepsMut, Env, MessageInfo, Response,
    StdResult, Storage, SubMsg, WasmMsg, Reply, Uint128, Decimal, Coin, QueryRequest, WasmQuery,
};
use membrane::revenue_distributor::{Config, RevenueSplit, RevenuePromise, ExecuteMsg, InstantiateMsg, QueryMsg, RDVaultInfoMessage};
use membrane::math::decimal_multiplication;
use membrane::types::{Asset, AssetInfo};
use membrane::cdp::{ExecuteMsg as CDPExecuteMsg, QueryMsg as CDPQueryMsg};
use membrane::ltv_disco::{QueryMsg as LTVDiscoQueryMsg, AssetQueueResponse};
use membrane::auction::ExecuteMsg as AuctionExecuteMsg;
use membrane::transmuter::ExecuteMsg as TransmuterExecuteMsg;

use crate::error::ContractError;
use crate::query::{
    query_config,
    query_promises,
    query_failed_distributions,
    query_pending_distributions,
    query_current_epoch_revenue,
    query_epoch_countdown,
    query_bad_debt_active,
    query_revenue_split,
    query_reserve_balance,
};
use crate::reply::{
    DISTRIBUTION_REPLY_ID,
    REVENUE_DESTINATION_REPLY_ID,
    TAKE_REVENUE_REPLY_ID,
    handle_distribution_reply,
    handle_revenue_destination_reply,
    handle_take_revenue_reply,
};
use crate::state::{
    CONFIG,
    DISTRIBUTION_PROP,
    FAILED_DISTRIBUTIONS,
    PROMISES,
    PER_ASSET_REVENUE_ATTRIBUTION,
    LAST_DISTRIBUTION_TIME,
    EPOCH_REVENUE_ACCUMULATION,
    BAD_DEBT_ACTIVE,
    RESERVE_ACCUMULATION,
};

use cw2::set_contract_version;


// version info for migration info
const CONTRACT_NAME: &str = "crates.io:revenue-distributor";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");


///NOTES:
/// The promises work through the CDP contract by it sending promises for the affiliates 
/// and then sending the per-asset distribution ratio as ltv_distributions. 
/// The revenue contract then uses the ltv_Distributions to send the revenue to the Disco ONLY IF its added as a destination.
/// Otherwise it does the normal splits to the other destinations. The Disco simply has special logic to handle its distributions.

/// Contract instantiation
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let owner = deps.api.addr_validate(&msg.owner)?;

    // Validate revenue split ratios sum to 1.0
    msg.revenue_split.validate().map_err(ContractError::Std)?;

    let config = Config {
        owner,
        canonical_asset: msg.canonical_asset,
        revenue_split: msg.revenue_split,
        ltv_disco: deps.api.addr_validate(&msg.ltv_disco)?,
        transmuter_vault: membrane::types::VaultInfo {
            vault_addr: deps.api.addr_validate(&msg.transmuter_vault.vault_addr)?,
            deposit_token: msg.transmuter_vault.deposit_token,
            vault_token: msg.transmuter_vault.vault_token,
        },
        points_system_contract: msg.points_system_contract
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        cdp_contract: msg.cdp_contract
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        revenue_dispersal_window: msg.revenue_dispersal_window.or(Some(7)),
        acquisition_contract: msg.acquisition_contract
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        ltv_disco_contract: msg.ltv_disco_contract
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        auction_contract: msg.auction_contract
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
    };

    CONFIG.save(deps.storage, &config)?;
    PROMISES.save(deps.storage, &vec![])?;
    DISTRIBUTION_PROP.save(deps.storage, &vec![])?;
    BAD_DEBT_ACTIVE.save(deps.storage, &false)?;
    RESERVE_ACCUMULATION.save(deps.storage, &Uint128::zero())?;
    
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::new().add_attribute("method", "instantiate"))
}

/// Contract execution
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::SetPromises { promises, per_asset_distribution } => set_promises(deps, env, info, promises, per_asset_distribution),
        ExecuteMsg::DistributePromises { limit } => distribute_promises(deps, env, info, limit),
        ExecuteMsg::UpdateConfig {
            revenue_split,
            ltv_disco,
            transmuter_vault,
            points_system_contract,
            cdp_contract,
            revenue_dispersal_window,
            acquisition_contract,
            ltv_disco_contract,
            auction_contract,
        } => update_config(deps, env, info, revenue_split, ltv_disco, transmuter_vault, points_system_contract, cdp_contract, revenue_dispersal_window, acquisition_contract, ltv_disco_contract, auction_contract),
        ExecuteMsg::TakeRevenueFromBasket {} => {
            take_revenue_from_basket(deps, env, info)
        }
        ExecuteMsg::ExecuteRevenueDistribution {} => execute_revenue_distribution(deps, env, info),
        ExecuteMsg::ClearFailedDistributions {} => clear_failed_distributions(deps, env, info),
        ExecuteMsg::ClearPendingDistributions {} => clear_pending_distributions(deps, env, info),
        ExecuteMsg::RetryFailedDistribute { limit } => retry_failed_distribute(deps, env, info, limit),
        ExecuteMsg::AddNonCdtRevenue { per_asset_distribution } => add_non_cdt_revenue(deps, env, info, per_asset_distribution),
        ExecuteMsg::ToggleBadDebtFulfillment { toggle } => toggle_bad_debt_fulfillment(deps, info, toggle),
        ExecuteMsg::WithdrawReserve { amount, recipient } => withdraw_reserve(deps, env, info, amount, recipient),
    }
}

/// Toggle bad debt fulfillment mode.
/// When active, all CDT revenue is routed to CDP's FulfillBadDebt instead of normal distribution.
/// Only callable by the CDP contract.
fn toggle_bad_debt_fulfillment(
    deps: DepsMut,
    info: MessageInfo,
    toggle: bool,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only CDP contract can toggle
    let cdp_contract = config.cdp_contract
        .ok_or_else(|| ContractError::Unauthorized {})?;
    if info.sender != cdp_contract {
        return Err(ContractError::Unauthorized {});
    }

    BAD_DEBT_ACTIVE.save(deps.storage, &toggle)?;

    Ok(Response::new()
        .add_attribute("method", "toggle_bad_debt_fulfillment")
        .add_attribute("active", toggle.to_string()))
}

/// Set revenue promises for distribution
/// Validates that total promised amount <= sent amount
/// Any excess amount will be distributed via RevenueSplit
pub fn set_promises(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    promises: Vec<RevenuePromise>,
    per_asset_distribution: Option<Vec<Asset>>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut saved_promises = PROMISES.load(deps.storage)?;

    // Validate that we received exactly one asset type and it matches canonical asset
    if info.funds.len() != 1 {
        return Err(ContractError::Std(cosmwasm_std::StdError::generic_err(
            "Must send exactly one asset type",
        )));
    }

    let sent_asset = &info.funds[0];
    let sent_amount = sent_asset.amount;

    // Validate that sent asset matches canonical asset
    if sent_asset.denom != config.canonical_asset.info.to_string() {
        return Err(ContractError::Std(cosmwasm_std::StdError::generic_err(
            format!(
                "Sent asset {} doesn't match canonical asset {}",
                sent_asset.denom,
                config.canonical_asset.info.to_string()
            ),
        )));
    }

    // Calculate total promised amount
    let mut total_promised = Uint128::zero();
    for promise in &promises {
        total_promised += promise.amount;
    }

    // Validate that promised amount <= sent amount
    if total_promised > sent_amount {
        return Err(ContractError::InvalidAmount {
            sent: sent_amount.u128(),
            promised: total_promised.u128(),
        });
    }
    // If per-asset ratios are provided, accumulate into shared attribution map.
    // This tracks which collateral assets generated the revenue for per-asset routing
    // to Disco, Junior, and Senior tranches at distribution time.
    if let Some(distributions) = per_asset_distribution {
        for asset in distributions.iter() {
            let key = asset.info.to_string();
            let current = PER_ASSET_REVENUE_ATTRIBUTION.load(deps.storage, key.clone()).unwrap_or_else(|_| Uint128::zero());
            PER_ASSET_REVENUE_ATTRIBUTION.save(deps.storage, key.clone(), &(current + asset.amount))?;

            // Accumulate revenue in current epoch
            let epoch_current = EPOCH_REVENUE_ACCUMULATION
                .may_load(deps.storage, key.clone())?
                .unwrap_or_else(Uint128::zero);
            EPOCH_REVENUE_ACCUMULATION.save(deps.storage, key, &(epoch_current + asset.amount))?;
        }
    }

    // Aggregate new promises to existing promises
    for promise in promises {
        if let Some(existing_promise) = saved_promises.iter_mut().find(|p| p.address == promise.address) {
            existing_promise.amount += promise.amount;
        } else {
            saved_promises.push(promise);
        }
    }
    // Save promises
    PROMISES.save(deps.storage, &saved_promises)?;

    Ok(Response::new()
        .add_attribute("method", "set_promises")
        .add_attribute("promises_count", saved_promises.len().to_string())
        .add_attribute("total_promised", total_promised.to_string())
        .add_attribute("sent_amount", sent_amount.to_string()))
}

/// Distribute all current promises and clear them.
/// Uses reply_on_error for failed distributions to continue with others.
/// Checks revenue dispersal window if configured.
///
/// Flow:
/// 1. Distribute affiliate promises (BankMsg::Send) — always, no window check
/// 2. Accumulate reserve share into RESERVE_ACCUMULATION (no window check)
/// 3. Check window for batched distribution (Disco/Junior/Senior)
/// 4. If window passed: distribute remainder via RevenueSplit to Disco, Junior, Senior
pub fn distribute_promises(
    mut deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    limit: Option<u32>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Check if bad debt fulfillment mode is active
    let bad_debt_active = BAD_DEBT_ACTIVE.may_load(deps.storage)?.unwrap_or(false);
    if bad_debt_active {
        // Only redirect to bad debt mode if there's actually pending bad debt
        if let Some(result) = try_distribute_bad_debt_mode(&mut deps, &env, &config)? {
            return Ok(result);
        }
        // If pending_bad_debt is zero, fall through to normal distribution
    }

    // Load current promises
    let promises = PROMISES.load(deps.storage)?;
    let limit = limit.unwrap_or(promises.len() as u32);
    let mut messages: Vec<SubMsg> = vec![];
    let mut total_distributed = Uint128::zero();

    // Track promises for reply handling
    let mut affiliate_promises: Vec<RevenuePromise> = vec![];
    let mut ltv_promises_to_remove: Vec<RevenuePromise> = vec![];

    // Get balance of canonical asset, excluding reserve
    let canonical_asset_amount = deps.querier.query_balance(
        env.contract.address.clone(),
        config.canonical_asset.info.to_string(),
    )?.amount;
    let reserve_balance = RESERVE_ACCUMULATION.may_load(deps.storage)?.unwrap_or(Uint128::zero());
    let available_balance = canonical_asset_amount.checked_sub(reserve_balance).unwrap_or(Uint128::zero());

    // Step 1: Distribute affiliate promises (always, no window check)
    for promise in &promises[..limit as usize] {
        // LTV Disco should not be used as a promise, only as a split destination
        let is_ltv_disco = promise.address == config.ltv_disco.to_string();
        if is_ltv_disco {
            ltv_promises_to_remove.push(promise.clone());
            continue;
        }

        affiliate_promises.push(promise.clone());

        let asset_coin = cosmwasm_std::Coin {
            denom: config.canonical_asset.info.to_string(),
            amount: promise.amount,
        };

        // All promises are affiliate fees — use Bank send
        messages.push(SubMsg::reply_always(CosmosMsg::Bank(BankMsg::Send {
            to_address: promise.address.clone(),
            amount: vec![asset_coin],
        }), DISTRIBUTION_REPLY_ID));

        total_distributed += promise.amount;
    }

    // Save affiliate promises for reply handling
    DISTRIBUTION_PROP.save(deps.storage, &affiliate_promises)?;

    let promises_count = promises.len();

    // Remove LTV Disco promises that were enqueued
    if !ltv_promises_to_remove.is_empty() {
        let mut all_promises = PROMISES.load(deps.storage)?;
        for rem in ltv_promises_to_remove {
            if let Some(idx) = all_promises.iter().position(|p| p.address == rem.address && p.amount == rem.amount) {
                all_promises.remove(idx);
            }
        }
        PROMISES.save(deps.storage, &all_promises)?;
    }

    // Step 2: Calculate remaining amount after promises (excluding reserve)
    let remaining_amount = available_balance.checked_sub(total_distributed).unwrap_or(Uint128::zero());

    if !remaining_amount.is_zero() {
        // Step 3: Check window for batched distribution (all split recipients including reserve)
        if let Some(window_days) = config.revenue_dispersal_window {
            let current_time = env.block.time.seconds();
            let last_distribution = LAST_DISTRIBUTION_TIME.may_load(deps.storage)?.unwrap_or(0u64);
            let window_seconds = window_days * 24 * 60 * 60;

            if last_distribution > 0 && current_time < last_distribution + window_seconds {
                // Window hasn't passed — split deferred (affiliate promises still sent)
                return Ok(Response::new()
                    .add_submessages(messages)
                    .add_attribute("method", "distribute_promises")
                    .add_attribute("status", "window_not_passed")
                    .add_attribute("promises_distributed", promises_count.to_string())
                    .add_attribute("current_time", current_time.to_string())
                    .add_attribute("next_distribution", (last_distribution + window_seconds).to_string()));
            }
        }

        // Step 4: Window passed — accumulate reserve and distribute batched amount
        // Reserve is accumulated here (not pre-window) to avoid double-counting
        // when distribute_promises is called multiple times within an epoch.
        let reserve_amount = decimal_multiplication(
            Decimal::from_ratio(remaining_amount, Uint128::one()),
            config.revenue_split.reserve_ratio,
        )?.to_uint_floor();
        if !reserve_amount.is_zero() {
            let current_reserve = RESERVE_ACCUMULATION.may_load(deps.storage)?.unwrap_or(Uint128::zero());
            RESERVE_ACCUMULATION.save(deps.storage, &(current_reserve + reserve_amount))?;
        }

        let batched_amount = remaining_amount - reserve_amount;
        if !batched_amount.is_zero() {
            let split_msgs = distribute_revenue_split(&mut deps, &config, batched_amount)?;
            for msg in split_msgs {
                messages.push(SubMsg::reply_always(msg, REVENUE_DESTINATION_REPLY_ID));
            }

            // Clear per-asset attribution map after all three recipients processed
            clear_per_asset_attribution(deps.storage)?;
        }
    }

    // Update last distribution time if we distributed batched amounts
    if !messages.is_empty() {
        LAST_DISTRIBUTION_TIME.save(deps.storage, &env.block.time.seconds())?;

        // Clear epoch revenue accumulation for new epoch
        let all_assets: Vec<String> = EPOCH_REVENUE_ACCUMULATION
            .keys(deps.storage, None, None, cosmwasm_std::Order::Ascending)
            .map(|kv| kv.unwrap())
            .collect();
        for asset_key in all_assets {
            EPOCH_REVENUE_ACCUMULATION.remove(deps.storage, asset_key);
        }
    }

    Ok(Response::new()
        .add_submessages(messages)
        .add_attribute("method", "distribute_promises")
        .add_attribute("promises_distributed", promises_count.to_string())
        .add_attribute("total_distributed", total_distributed.to_string())
        .add_attribute("remaining_distributed", remaining_amount.to_string()))
}

/// Distribute remaining amount according to RevenueSplit ratios (Disco/Junior/Senior).
/// Reserve is already carved out before this is called.
fn distribute_revenue_split(
    deps: &mut DepsMut,
    config: &Config,
    batched_amount: Uint128,
) -> Result<Vec<CosmosMsg>, ContractError> {
    let mut msgs: Vec<CosmosMsg> = vec![];
    let split = &config.revenue_split;

    // Compute effective ratios for the 3 batched recipients (normalized after removing reserve)
    let batched_total_ratio = split.disco_ratio + split.junior_ratio + split.senior_ratio;
    if batched_total_ratio.is_zero() {
        return Ok(msgs);
    }

    // Disco share: batched_amount * (disco_ratio / batched_total_ratio)
    let disco_share_ratio = Decimal::from_ratio(
        split.disco_ratio.atomics(),
        batched_total_ratio.atomics(),
    );
    let disco_amount = decimal_multiplication(
        Decimal::from_ratio(batched_amount, Uint128::one()),
        disco_share_ratio,
    )?.to_uint_floor();

    // Junior share: batched_amount * (junior_ratio / batched_total_ratio)
    let junior_share_ratio = Decimal::from_ratio(
        split.junior_ratio.atomics(),
        batched_total_ratio.atomics(),
    );
    let junior_amount = decimal_multiplication(
        Decimal::from_ratio(batched_amount, Uint128::one()),
        junior_share_ratio,
    )?.to_uint_floor();

    // Senior gets the rest to avoid rounding dust
    let senior_amount = batched_amount - disco_amount - junior_amount;

    // Send to Disco
    if !disco_amount.is_zero() {
        let disco_promise = RevenuePromise {
            address: config.ltv_disco.to_string(),
            amount: disco_amount,
        };
        let disco_msgs = ltv_disco_add_revenue_msgs(deps, config, &disco_promise)?;
        msgs.extend(disco_msgs);
    }

    // Send to Junior tranches
    if !junior_amount.is_zero() {
        let junior_msgs = tranche_add_revenue_msgs(deps, config, junior_amount, true)?;
        msgs.extend(junior_msgs);
    }

    // Send to Senior tranches
    if !senior_amount.is_zero() {
        let senior_msgs = tranche_add_revenue_msgs(deps, config, senior_amount, false)?;
        msgs.extend(senior_msgs);
    }

    Ok(msgs)
}

/// Try to handle bad debt fulfillment mode.
/// Returns Some(Response) if bad debt was fulfilled, None if pending_bad_debt is zero (fall through to normal).
fn try_distribute_bad_debt_mode(
    deps: &mut DepsMut,
    env: &Env,
    config: &Config,
) -> Result<Option<Response>, ContractError> {
    let cdp_addr = config.cdp_contract.clone()
        .ok_or_else(|| ContractError::Std(cosmwasm_std::StdError::generic_err("CDP contract not configured")))?;

    let basket: membrane::types::Basket = deps.querier.query(&QueryRequest::Wasm(
        WasmQuery::Smart {
            contract_addr: cdp_addr.to_string(),
            msg: to_json_binary(&CDPQueryMsg::GetBasket {})?,
        },
    ))?;

    let canonical_balance = deps.querier.query_balance(
        env.contract.address.clone(),
        config.canonical_asset.info.to_string(),
    )?.amount;
    let reserve_balance = RESERVE_ACCUMULATION.may_load(deps.storage)?.unwrap_or(Uint128::zero());
    let available_balance = canonical_balance.checked_sub(reserve_balance).unwrap_or(Uint128::zero());

    if !basket.pending_bad_debt.is_zero() && !available_balance.is_zero() {
        let fulfill_amount = std::cmp::min(available_balance, basket.pending_bad_debt);
        let remainder = available_balance - fulfill_amount;

        let mut messages: Vec<SubMsg> = vec![];

        // Send fulfill_amount to CDP FulfillBadDebt
        messages.push(SubMsg::reply_always(
            CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: cdp_addr.to_string(),
                msg: to_json_binary(&CDPExecuteMsg::FulfillBadDebt {})?,
                funds: vec![Coin {
                    denom: config.canonical_asset.info.to_string(),
                    amount: fulfill_amount,
                }],
            }),
            DISTRIBUTION_REPLY_ID,
        ));

        // Distribute remainder via RevenueSplit
        if !remainder.is_zero() {
            // Accumulate reserve share
            let reserve_amount = decimal_multiplication(
                Decimal::from_ratio(remainder, Uint128::one()),
                config.revenue_split.reserve_ratio,
            )?.to_uint_floor();
            if !reserve_amount.is_zero() {
                let current_reserve = RESERVE_ACCUMULATION.may_load(deps.storage)?.unwrap_or(Uint128::zero());
                RESERVE_ACCUMULATION.save(deps.storage, &(current_reserve + reserve_amount))?;
            }

            let batched_amount = remainder - reserve_amount;
            if !batched_amount.is_zero() {
                let split_msgs = distribute_revenue_split(deps, config, batched_amount)?;
                for msg in split_msgs {
                    messages.push(SubMsg::reply_always(msg, REVENUE_DESTINATION_REPLY_ID));
                }
            }

            // Clear per-asset attribution
            clear_per_asset_attribution(deps.storage)?;
        }

        // Update last distribution time and clear epoch accumulation
        LAST_DISTRIBUTION_TIME.save(deps.storage, &env.block.time.seconds())?;
        let all_assets: Vec<String> = EPOCH_REVENUE_ACCUMULATION
            .keys(deps.storage, None, None, cosmwasm_std::Order::Ascending)
            .map(|kv| kv.unwrap())
            .collect();
        for asset_key in all_assets {
            EPOCH_REVENUE_ACCUMULATION.remove(deps.storage, asset_key);
        }

        // Clear promises since we're redirecting
        PROMISES.save(deps.storage, &vec![])?;

        return Ok(Some(Response::new()
            .add_submessages(messages)
            .add_attribute("method", "distribute_promises")
            .add_attribute("mode", "bad_debt_fulfillment")
            .add_attribute("fulfill_amount", fulfill_amount)
            .add_attribute("remainder_distributed", remainder)));
    }

    // If pending_bad_debt is zero, return None to fall through to normal distribution
    Ok(None)
}

/// Clear the per-asset revenue attribution map after all split recipients are processed
fn clear_per_asset_attribution(storage: &mut dyn Storage) -> Result<(), ContractError> {
    let all_assets: Vec<String> = PER_ASSET_REVENUE_ATTRIBUTION
        .keys(storage, None, None, cosmwasm_std::Order::Ascending)
        .map(|kv| kv.unwrap())
        .collect();
    for asset_key in all_assets {
        PER_ASSET_REVENUE_ATTRIBUTION.remove(storage, asset_key);
    }
    Ok(())
}

/// Update contract configuration (admin only)
pub fn update_config(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    revenue_split: Option<RevenueSplit>,
    ltv_disco: Option<String>,
    transmuter_vault: Option<RDVaultInfoMessage>,
    points_system_contract: Option<String>,
    cdp_contract: Option<String>,
    revenue_dispersal_window: Option<u64>,
    acquisition_contract: Option<String>,
    ltv_disco_contract: Option<String>,
    auction_contract: Option<String>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;

    // Only owner can update config
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    // Update fields if provided
    if let Some(split) = revenue_split {
        split.validate().map_err(ContractError::Std)?;
        config.revenue_split = split;
    }
    if let Some(ltv) = ltv_disco { config.ltv_disco = deps.api.addr_validate(&ltv)?; }
    if let Some(v) = transmuter_vault { 
        config.transmuter_vault = membrane::types::VaultInfo {
            vault_addr: deps.api.addr_validate(&v.vault_addr)?,
            deposit_token: v.deposit_token,
            vault_token: v.vault_token,
        };
    }
    if let Some(pts) = points_system_contract {
        config.points_system_contract = Some(deps.api.addr_validate(&pts)?);
    }
    if let Some(cdp) = cdp_contract {
        config.cdp_contract = Some(deps.api.addr_validate(&cdp)?);
    }
    if let Some(window) = revenue_dispersal_window {
        update_dispersal_window(deps.storage, _env, info, window)?;
    }
    if let Some(lockdrop) = acquisition_contract {
        config.acquisition_contract = Some(deps.api.addr_validate(&lockdrop)?);
    }
    if let Some(disco) = ltv_disco_contract {
        config.ltv_disco_contract = Some(deps.api.addr_validate(&disco)?);
    }
    if let Some(auction) = auction_contract {
        config.auction_contract = Some(deps.api.addr_validate(&auction)?);
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("method", "update_config"))
}

/// Clear failed distributions (admin only)
pub fn clear_failed_distributions(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only owner can clear failed distributions
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    // Clear all failed distributions
    let failed_distributions: Vec<String> = FAILED_DISTRIBUTIONS
        .keys(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .map(|item| item.unwrap())
        .collect();

    for key in failed_distributions {
        FAILED_DISTRIBUTIONS.remove(deps.storage, key);
    }

    Ok(Response::new()
        .add_attribute("method", "clear_failed_distributions"))
}

/// Clear pending distributions (admin only)
pub fn clear_pending_distributions(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only owner can clear pending distributions
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    // Clear pending distributions
    DISTRIBUTION_PROP.save(deps.storage, &vec![])?;

    Ok(Response::new()
        .add_attribute("method", "clear_pending_distributions"))
}

/// Retry failed distributions (admin only)
pub fn retry_failed_distribute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    limit: Option<u32>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only owner can retry failed distributions
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    // Get all failed distributions
    let failed_distributions: Vec<(String, u128)> = FAILED_DISTRIBUTIONS
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .map(|item| item.unwrap())
        .collect();

    if failed_distributions.is_empty() {
        return Ok(Response::new()
            .add_attribute("method", "retry_failed_distribute")
            .add_attribute("status", "no_failed_distributions"));
    }

    let limit = limit.unwrap_or(failed_distributions.len() as u32);
    let mut messages: Vec<SubMsg> = vec![];
    let mut retry_promises = vec![];

    // Convert failed distributions back to promises for retry
    for (address, amount) in failed_distributions.iter().take(limit as usize) {
        let address_clone = address.clone();
        let promise = RevenuePromise {
            address: address_clone.clone(),
            amount: Uint128::from(*amount),
        };
        retry_promises.push(promise.clone());

        // Create the distribution message (all failed distributions are affiliate sends)
        let asset_coin = cosmwasm_std::Coin {
            denom: config.canonical_asset.info.to_string(),
            amount: Uint128::from(*amount),
        };

        let msg = CosmosMsg::Bank(BankMsg::Send {
            to_address: address_clone.clone(),
            amount: vec![asset_coin.clone()],
        });

        // Use reply_always to handle both success and failure
        messages.push(SubMsg::reply_always(msg, DISTRIBUTION_REPLY_ID));
    }

    // Store retry promises in DISTRIBUTION_PROP
    DISTRIBUTION_PROP.save(deps.storage, &retry_promises)?;

    // Remove the retried failed distributions
    for (address, _) in failed_distributions.into_iter().take(limit as usize) {
        FAILED_DISTRIBUTIONS.remove(deps.storage, address);
    }

    Ok(Response::new()
        .add_submessages(messages)
        .add_attribute("method", "retry_failed_distribute")
        .add_attribute("retry_count", limit.to_string())
        .add_attribute("status", "retrying"))
}

/// Route non-CDT revenue (collateral fees) to auction
/// Accepts any non-CDT asset and sends it to the auction contract via StartAuction
pub fn add_non_cdt_revenue(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    per_asset_distribution: Vec<Asset>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Validate auction contract is configured
    let auction_contract = config.auction_contract
        .ok_or_else(|| ContractError::Std(cosmwasm_std::StdError::generic_err(
            "Auction contract not configured",
        )))?;

    // Validate that funds are provided
    if info.funds.is_empty() {
        return Err(ContractError::Std(cosmwasm_std::StdError::generic_err(
            "No funds provided",
        )));
    }

    if info.funds.len() > 1 {
        return Err(ContractError::Std(cosmwasm_std::StdError::generic_err(
            "Only one asset type allowed per call",
        )));
    }

    let coin = &info.funds[0];
    
    // Validate it's not CDT (canonical asset)
    if coin.denom == config.canonical_asset.info.to_string() {
        return Err(ContractError::Std(cosmwasm_std::StdError::generic_err(
            "CDT revenue should use SetPromises instead",
        )));
    }

    // Send to auction via StartAuction
    // The auction will initialize a FeeAuction for this asset
    let msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: auction_contract.to_string(),
        msg: to_json_binary(&AuctionExecuteMsg::StartAuction {
            repayment_position_info: None,
            send_to: None,
            auction_asset: Asset {
                info: AssetInfo::NativeToken { 
                    denom: coin.denom.clone() 
                },
                amount: coin.amount,
            },
            per_asset_distribution: Some(per_asset_distribution),
        })?,
        funds: vec![coin.clone()],
    });

    Ok(Response::new()
        .add_message(msg)
        .add_attribute("method", "add_non_cdt_revenue")
        .add_attribute("asset", coin.denom.clone())
        .add_attribute("amount", coin.amount.to_string())
        .add_attribute("routed_to_auction", "true"))
}

/// Directly send CDT to LTV Disco via AddRevenue (no vault entry needed).
/// Uses shared PER_ASSET_REVENUE_ATTRIBUTION map for per-asset routing.
/// Does NOT clear the map — caller is responsible for clearing after all recipients are processed.
fn ltv_disco_add_revenue_msgs(
    deps: &mut DepsMut,
    config: &Config,
    promise: &RevenuePromise,
) -> Result<Vec<CosmosMsg>, ContractError> {
    // Read current per-asset attribution map and compute total
    let all_assets: Vec<(String, Uint128)> = PER_ASSET_REVENUE_ATTRIBUTION
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .map(|kv| kv.unwrap())
        .collect();
    let total: Uint128 = all_assets.iter().fold(Uint128::zero(), |acc, (_, v)| acc + *v);

    if total.is_zero() || all_assets.is_empty() {
        // No per-asset attribution data — nothing to route
        return Ok(vec![]);
    }

    // Build AddRevenue messages for each asset, sending CDT directly
    // Only send revenue if there are deposits in the disco for that asset
    let mut msgs: Vec<CosmosMsg> = vec![];
    for (asset_denom, asset_total) in all_assets {
        let portion: Uint128 = decimal_multiplication(
            Decimal::from_ratio(promise.amount, Uint128::one()),
            Decimal::from_ratio(asset_total, total)
        )?.to_uint_floor();
        if portion.is_zero() { continue; }

        // Check if there are deposits in the disco for this asset
        let has_deposits = match deps.querier.query_wasm_smart::<AssetQueueResponse>(
            config.ltv_disco.clone(),
            &LTVDiscoQueryMsg::GetAssetQueue {
                assets: vec![asset_denom.clone()],
                limit: None,
                start_after: None,
            },
        ) {
            Ok(queue_response) => {
                queue_response.queues.first()
                    .map(|(_, queue)| {
                        let total_deposits: Uint128 = queue.slots
                            .iter()
                            .map(|slot| slot.total_deposit_tokens)
                            .sum();
                        !total_deposits.is_zero()
                    })
                    .unwrap_or(false)
            },
            Err(_) => false
        };

        if !has_deposits { continue; }

        let add_revenue = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.ltv_disco.to_string(),
            msg: to_json_binary(&membrane::ltv_disco::ExecuteMsg::AddRevenue {
                asset: asset_denom.clone(),
            })?,
            funds: vec![Coin {
                denom: config.canonical_asset.info.to_string(),
                amount: portion
            }],
        });
        msgs.push(add_revenue);
    }

    Ok(msgs)
}

/// Send CDT to transmuter's AddRevenue for junior or senior tranches.
/// Uses shared PER_ASSET_REVENUE_ATTRIBUTION map for per-asset routing.
fn tranche_add_revenue_msgs(
    deps: &mut DepsMut,
    config: &Config,
    total_amount: Uint128,
    junior: bool,
) -> Result<Vec<CosmosMsg>, ContractError> {
    let all_assets: Vec<(String, Uint128)> = PER_ASSET_REVENUE_ATTRIBUTION
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .map(|kv| kv.unwrap())
        .collect();
    let total: Uint128 = all_assets.iter().fold(Uint128::zero(), |acc, (_, v)| acc + *v);

    if total.is_zero() || all_assets.is_empty() {
        // No per-asset attribution data — nothing to route
        return Ok(vec![]);
    }

    let mut msgs: Vec<CosmosMsg> = vec![];
    for (asset_denom, asset_total) in all_assets {
        let portion: Uint128 = decimal_multiplication(
            Decimal::from_ratio(total_amount, Uint128::one()),
            Decimal::from_ratio(asset_total, total)
        )?.to_uint_floor();
        if portion.is_zero() { continue; }

        let add_revenue = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.transmuter_vault.vault_addr.to_string(),
            msg: to_json_binary(&TransmuterExecuteMsg::AddRevenue {
                collateral_denom: asset_denom,
                junior,
            })?,
            funds: vec![Coin {
                denom: config.canonical_asset.info.to_string(),
                amount: portion,
            }],
        });
        msgs.push(add_revenue);
    }

    Ok(msgs)
}

/// Take revenue from CDP Basket's pending_revenue
/// Always takes ALL available revenue and maintains per-asset attribution
fn take_revenue_from_basket(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    
    // Get CDP contract address from config
    let cdp_addr = config.cdp_contract
        .ok_or_else(|| ContractError::Std(cosmwasm_std::StdError::generic_err(
            "CDP contract address must be configured",
        )))?;
    
    // Save current CDT balance before calling TakeRevenue
    let current_balance = deps.querier.query_balance(
        env.contract.address.clone(),
        config.canonical_asset.info.to_string(),
    )?.amount;
    crate::state::PRE_TAKE_REVENUE_BALANCE.save(deps.storage, &current_balance)?;
    
    // Query CDP for basket to get per_asset_rev before calling TakeRevenue
    let basket: membrane::types::Basket = deps.querier.query(&QueryRequest::Wasm(
        WasmQuery::Smart {
            contract_addr: cdp_addr.to_string(),
            msg: to_json_binary(&CDPQueryMsg::GetBasket {})?,
        }
    ))?;
    
    // Save per_asset_rev before calling TakeRevenue
    let per_asset_rev = basket.pending_revenue.per_asset_rev.clone();
    crate::state::PRE_TAKE_REVENUE_PER_ASSET.save(deps.storage, &per_asset_rev)?;
    
    // Call CDP's TakeRevenue (which takes all available revenue)
    let take_revenue_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: cdp_addr.to_string(),
        msg: to_json_binary(&CDPExecuteMsg::TakeRevenue {})?,
        funds: vec![],
    });
    
    Ok(Response::new()
        .add_submessage(SubMsg::reply_on_success(take_revenue_msg, TAKE_REVENUE_REPLY_ID))
        .add_attribute("method", "take_revenue_from_basket")
        .add_attribute("pre_balance", current_balance.to_string()))
}

/// Permissionless execution to pull revenue and distribute within window
/// Checks if window has passed, then calls TakeRevenueFromBasket and DistributePromises
pub fn execute_revenue_distribution(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let contract_addr = env.contract.address.clone();
    let current_time = env.block.time.seconds();
    
    // Check if window has passed if window is configured
    if let Some(window_days) = config.revenue_dispersal_window {
        let last_distribution = LAST_DISTRIBUTION_TIME.may_load(deps.storage)?
            .unwrap_or(0u64);
        let window_seconds = window_days * 24 * 60 * 60; // Convert days to seconds
        
        // Only check window if we've distributed before (last_distribution > 0)
        // First distribution should always be allowed
        // Use <= to ensure we block if exactly at the window boundary
        if last_distribution > 0 && current_time <= last_distribution + window_seconds {
            // Window hasn't passed yet
            return Ok(Response::new()
                .add_attribute("method", "execute_revenue_distribution")
                .add_attribute("status", "window_not_passed")
                .add_attribute("current_time", current_time.to_string())
                .add_attribute("last_distribution", last_distribution.to_string())
                .add_attribute("window_seconds", window_seconds.to_string())
                .add_attribute("next_distribution", (last_distribution + window_seconds + 1).to_string()));
        }
    }
    
    // Window has passed (or not configured), proceed with revenue pull and distribution
    // First call TakeRevenueFromBasket, which will trigger DistributePromises via reply
    take_revenue_from_basket(deps, env, MessageInfo {
        sender: contract_addr,
        funds: vec![],
    })
}

/// Update dispersal window and synchronize with lockdrop and disco (admin only)
/// Updates: revenue_dispersal_window, lockdrop minimum_lock_days and periods, disco dispersal_window
pub fn update_dispersal_window(
    storage: &mut dyn Storage,
    _env: Env,
    info: MessageInfo,
    window_days: u64,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(storage)?;
    
    // Only owner can update
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }
    
    if window_days == 0 {
        return Err(ContractError::Std(cosmwasm_std::StdError::generic_err(
            "Window days must be greater than zero",
        )));
    }
    
    let mut msgs: Vec<CosmosMsg> = vec![];
    
    // Update self: revenue_dispersal_window
    let mut updated_config = config.clone();
    updated_config.revenue_dispersal_window = Some(window_days);
    CONFIG.save(storage, &updated_config)?;
    
    // Update LTV Disco: dispersal_window (convert days to hours)
    if let Some(disco_addr) = &config.ltv_disco_contract {
        let disco_window_hours = window_days * 24;
        let update_disco_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: disco_addr.to_string(),
                msg: to_json_binary(&membrane::ltv_disco::ExecuteMsg::UpdateConfig {
                    owner: None,
                    cdp_contract: None,
                    deposit_denom: None,
                    cdt_denom: None,
                    minimum_deposit: None,
                    unstaking_period: Some(disco_window_hours),
                    oracle_contract: None,
                    chain_proxy_contract: None,
                    emissions_voting_contract: None,
                    affiliate_fee: None,
                    max_management_fee: None,
                    points_system_contract: None,
                    revenue_distributor: None,
                    auction_contract: None,
                    mbrn_denom: None,
                    liquidation_engine_contract: None,
                })?,
            funds: vec![],
        });
        msgs.push(update_disco_msg);
    }
    
    // Update Lockdrop: minimum_lock_days and periods (using 5:2 ratio)
    if let Some(lockdrop_addr) = &config.acquisition_contract {
        let deposit_period = (window_days * 5) / 7;
        let withdrawal_period = (window_days * 2) / 7;
        
        // Ensure at least 1 day for each period
        let deposit_period = deposit_period.max(1);
        let withdrawal_period = withdrawal_period.max(1);
        
        let update_lockdrop_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: lockdrop_addr.to_string(),
            msg: to_json_binary(&membrane::acquisition::ExecuteMsg::UpdateConfig {
                owner: None,
                transmuter_contract: None,
                neutron_proxy: None,
                cdp_contract: None,
                deposit_period_days: Some(deposit_period),
                withdrawal_period_days: Some(withdrawal_period),
                deposit_token: None,
                minimum_deposit: None,
                mbrn_denom: None,
                staking_contract: None,
                mars_mirror_contract: None,
                ltv_disco_contract: None,
                discounts_contract: None,
                emissions_voting_contract: None,
                cliff_period_days: Some(window_days),
                acquisition_config: None,
            })?,
            funds: vec![],
        });
        msgs.push(update_lockdrop_msg);
    }
    
    Ok(Response::new()
        .add_messages(msgs)
        .add_attribute("method", "update_dispersal_window")
        .add_attribute("window_days", window_days.to_string())
        .add_attribute("disco_window_hours", (window_days * 24).to_string()))
}

/// Withdraw accumulated reserve funds (admin only)
fn withdraw_reserve(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    amount: Option<Uint128>,
    recipient: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let recipient_addr = deps.api.addr_validate(&recipient)?;
    let current_reserve = RESERVE_ACCUMULATION.may_load(deps.storage)?.unwrap_or(Uint128::zero());

    let withdraw_amount = amount.unwrap_or(current_reserve);
    if withdraw_amount > current_reserve {
        return Err(ContractError::Std(cosmwasm_std::StdError::generic_err(
            format!("Withdraw amount {} exceeds reserve balance {}", withdraw_amount, current_reserve)
        )));
    }
    if withdraw_amount.is_zero() {
        return Err(ContractError::Std(cosmwasm_std::StdError::generic_err("Nothing to withdraw")));
    }

    RESERVE_ACCUMULATION.save(deps.storage, &(current_reserve - withdraw_amount))?;

    let send_msg = CosmosMsg::Bank(BankMsg::Send {
        to_address: recipient_addr.to_string(),
        amount: vec![Coin {
            denom: config.canonical_asset.info.to_string(),
            amount: withdraw_amount,
        }],
    });

    Ok(Response::new()
        .add_message(send_msg)
        .add_attribute("method", "withdraw_reserve")
        .add_attribute("amount", withdraw_amount.to_string())
        .add_attribute("recipient", recipient))
}

/// Contract queries
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: cosmwasm_std::Deps, env: Env, msg: QueryMsg) -> StdResult<cosmwasm_std::Binary> {
    match msg {
        QueryMsg::Config {} => query_config(deps),
        QueryMsg::Promises {} => query_promises(deps),
        QueryMsg::FailedDistributions {} => query_failed_distributions(deps),
        QueryMsg::PendingDistributions {} => query_pending_distributions(deps),
        QueryMsg::CurrentEpochRevenue {} => query_current_epoch_revenue(deps),
        QueryMsg::EpochCountdown {} => query_epoch_countdown(deps, env),
        QueryMsg::BadDebtActive {} => query_bad_debt_active(deps),
        QueryMsg::RevenueSplit {} => query_revenue_split(deps),
        QueryMsg::ReserveBalance {} => query_reserve_balance(deps),
    }
}

/// Handle replies from sub-messages
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> Result<Response, ContractError> {
    match msg.id {
        DISTRIBUTION_REPLY_ID => handle_distribution_reply(deps, env, msg),
        REVENUE_DESTINATION_REPLY_ID => handle_revenue_destination_reply(deps, env, msg),
        TAKE_REVENUE_REPLY_ID => handle_take_revenue_reply(deps, env, msg),
        _ => Err(ContractError::Std(cosmwasm_std::StdError::generic_err(
            "Unknown reply ID",
        ))),
    }
}

