use cosmwasm_std::{Decimal, Deps, StdResult, StdError, Uint128, Env};
use std::str::FromStr;
use membrane::ltv_disco::{
    Config, AssetQueue, AssetQueueResponse, BackingDepositResponse, BackingDepositsByUserResponse,
    RevenueTrackingEntry, PendingClaimsResponse, DepositPendingClaim,
    UserLifetimeRevenueEntry, RevenueEvent, AssetsResponse, DailyTVLResponse,
    UserTotalDepositsResponse, ManagedDepositKeysResponse, ManagerFeeResponse,
    AllUserDepositsResponse, UserDepositInfo, DailyDepositResponse, UnstakeRequestsResponse,
    SlotWeightsResponse, AverageLTVsResponse,
};
use membrane::stability_pool_vault::{calculate_base_tokens, calculate_vault_tokens};
use membrane::types::AssetInfo;
use membrane::oracle::{QueryMsg as Oracle_QueryMsg, PriceResponse};
use membrane::math::decimal_multiplication;

use crate::state::{
    CONFIG, ASSET_QUEUES, REVENUE_TRACKING, REVENUE_EVENTS, USER_LIFETIME_REVENUE,
    BACKING_DEPOSITS, USER_DEPOSITS, DAILY_TVL_TRACKER, DAILY_DEPOSIT_TRACKER,
    USER_TOTAL_DEPOSITS, MANAGED_DEPOSITS, MANAGER_FEE, UNSTAKE_REQUESTS, USER_UNSTAKE_REQUESTS,
};
use crate::execute::{calculate_slot_weights, ltv_to_pct, is_active_slot};

const MAX_LIMIT: u32 = 32;

/// Query contract configuration
pub fn query_config(deps: Deps) -> StdResult<Config> {
    CONFIG.load(deps.storage)
}

/// Query asset queue(s)
/// If `assets` is non-empty, returns queues for those specific assets.
/// If `assets` is empty, returns all queues (paginated with `limit`/`start_after`).
pub fn query_asset_queue(
    deps: Deps,
    assets: Vec<String>,
    limit: Option<u32>,
    start_after: Option<String>,
) -> StdResult<AssetQueueResponse> {
    use cw_storage_plus::Bound;

    let queues: Vec<(String, AssetQueue)> = if !assets.is_empty() {
        assets.into_iter()
            .filter_map(|asset| {
                ASSET_QUEUES.load(deps.storage, asset.clone())
                    .ok()
                    .map(|queue| (asset, queue))
            })
            .collect()
    } else {
        let limit = limit.unwrap_or(MAX_LIMIT).min(MAX_LIMIT) as usize;
        let start = start_after.as_deref().map(Bound::exclusive);

        ASSET_QUEUES
            .range(deps.storage, start, None, cosmwasm_std::Order::Ascending)
            .take(limit)
            .collect::<StdResult<Vec<_>>>()?
    };

    Ok(AssetQueueResponse { queues })
}

/// Query backing deposit by user, asset, slot, and deposit_id
pub fn query_backing_deposit(
    deps: Deps,
    user: String,
    asset: String,
    slot: u8,
    deposit_id: Uint128,
) -> StdResult<BackingDepositResponse> {
    let user_addr = deps.api.addr_validate(&user)?;
    let deposit_key = crate::execute::make_deposit_key(&asset, slot, &user_addr.to_string(), &deposit_id);

    let deposit = BACKING_DEPOSITS.load(deps.storage, deposit_key)
        .map_err(|_| StdError::not_found("Deposit not found"))?;

    Ok(BackingDepositResponse { deposit })
}

/// Query backing deposits by user for a specific asset
pub fn query_backing_deposits_by_user(
    deps: Deps,
    user: String,
    asset: String,
    limit: Option<u32>,
    _start_after: Option<Uint128>,
) -> StdResult<BackingDepositsByUserResponse> {
    let user_addr = deps.api.addr_validate(&user)?;
    let limit = limit.unwrap_or(MAX_LIMIT) as usize;

    let deposit_keys = USER_DEPOSITS
        .may_load(deps.storage, (user_addr, asset))?
        .unwrap_or_default();

    let mut deposits = Vec::new();
    for key in deposit_keys.iter().take(limit) {
        if let Ok(deposit) = BACKING_DEPOSITS.load(deps.storage, key.clone()) {
            deposits.push(deposit);
        }
    }

    Ok(BackingDepositsByUserResponse { deposits })
}

/// Query if the contract can handle bad debt for an asset
pub fn query_can_handle_bad_debt(deps: Deps, asset: String, amount: Uint128) -> StdResult<bool> {
    let config = CONFIG.load(deps.storage)?;
    let queue = match ASSET_QUEUES.load(deps.storage, asset.clone()) {
        Ok(queue) => queue,
        Err(_) => return Ok(false),
    };

    // Sum total deposit tokens across all slots
    let total_deposit_tokens: Uint128 = queue.slots
        .iter()
        .map(|slot| slot.total_deposit_tokens)
        .sum();

    if total_deposit_tokens.is_zero() {
        return Ok(false);
    }

    // Query oracle for asset and CDT prices
    let asset_info = AssetInfo::NativeToken { denom: asset };
    let cdt_info = AssetInfo::NativeToken { denom: config.cdt_denom };
    let asset_infos = vec![asset_info, cdt_info];

    let price_response: Result<Vec<PriceResponse>, _> = deps.querier.query_wasm_smart(
        config.oracle_contract.to_string(),
        &Oracle_QueryMsg::Prices {
            asset_infos,
            twap_timeframe: 0,
            oracle_time_limit: 0,
        },
    );

    if let Ok(prices) = price_response {
        if let Ok(collateral_value) = prices[0].get_value(total_deposit_tokens) {
            if let Ok(cdt_amount) = prices[1].get_amount(collateral_value) {
                return Ok(cdt_amount >= amount);
            }
        }
    }

    Ok(false)
}

/// Query cumulative revenue with optional slot filter
pub fn query_cumulative_revenue(
    deps: Deps,
    asset: String,
    slot: Option<u8>,
) -> StdResult<Vec<RevenueTrackingEntry>> {
    match slot {
        Some(s) => {
            REVENUE_TRACKING
                .may_load(deps.storage, (asset, s.to_string()))?
                .map(Ok)
                .unwrap_or_else(|| Ok(vec![]))
        }
        None => {
            // Aggregate all slots — iterate queue slots instead of hardcoded range
            let mut all_entries: Vec<RevenueTrackingEntry> = vec![];
            let queue = match ASSET_QUEUES.may_load(deps.storage, asset.clone())? {
                Some(q) => q,
                None => return Ok(vec![]),
            };
            for slot in &queue.slots {
                let slot_key = ltv_to_pct(slot.max_ltv).to_string();
                if let Ok(Some(entries)) = REVENUE_TRACKING.may_load(
                    deps.storage,
                    (asset.clone(), slot_key),
                ) {
                    for entry in entries {
                        if let Some(existing) = all_entries.iter_mut().find(|e| e.timestamp == entry.timestamp) {
                            existing.total_revenue += entry.total_revenue;
                        } else {
                            all_entries.push(entry);
                        }
                    }
                }
            }
            all_entries.sort_by_key(|e| e.timestamp);
            Ok(all_entries)
        }
    }
}

/// Query pending claims for a user across all their deposits for an asset
pub fn query_pending_claims(
    deps: Deps,
    user: String,
    asset: String,
) -> StdResult<PendingClaimsResponse> {
    let user_addr = deps.api.addr_validate(&user)?;

    let deposit_keys = USER_DEPOSITS
        .may_load(deps.storage, (user_addr, asset.clone()))?
        .unwrap_or_default();

    let mut claims = Vec::new();

    for deposit_key_str in deposit_keys {
        if let Ok(deposit) = BACKING_DEPOSITS.load(deps.storage, deposit_key_str.clone()) {
            // Parse slot and deposit_id from key: "asset:slot:user:deposit_id"
            let parts: Vec<&str> = deposit_key_str.split(':').collect();
            if parts.len() < 4 {
                continue;
            }
            let slot = match u8::from_str(parts[1]) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let deposit_id = match Uint128::from_str(parts[3]) {
                Ok(id) => id,
                Err(_) => continue,
            };

            let pending = calculate_pending_revenue(
                deps.storage,
                &deposit,
                asset.clone(),
                slot,
            )?;

            if !pending.is_zero() {
                claims.push(DepositPendingClaim {
                    slot,
                    deposit_id,
                    pending_amount: pending,
                });
            }
        }
    }

    Ok(PendingClaimsResponse {
        user,
        asset,
        claims,
    })
}

/// Helper to calculate pending revenue for a single deposit
fn calculate_pending_revenue(
    storage: &dyn cosmwasm_std::Storage,
    deposit: &membrane::ltv_disco::BackingDeposit,
    asset: String,
    slot: u8,
) -> StdResult<Uint128> {
    let key = (asset, slot.to_string());
    let events = REVENUE_EVENTS
        .may_load(storage, key)?
        .unwrap_or_default();

    let mut pending = Uint128::zero();

    for event in events {
        if event.timestamp <= deposit.last_claimed {
            continue;
        }

        if event.amount_to_be_claimed.is_zero() {
            continue;
        }

        let user_share_decimal = decimal_multiplication(
            Decimal::from_ratio(deposit.vault_tokens, Uint128::one()),
            event.amount_per_vt,
        )?;
        let user_share = user_share_decimal.to_uint_floor();
        pending = pending.checked_add(user_share).unwrap_or(pending);
    }

    Ok(pending)
}

/// Query user lifetime revenue entries
pub fn query_user_lifetime_revenue(
    deps: Deps,
    user: String,
    asset: String,
) -> StdResult<Vec<UserLifetimeRevenueEntry>> {
    let user_addr = deps.api.addr_validate(&user)?;
    USER_LIFETIME_REVENUE
        .may_load(deps.storage, (user_addr, asset))?
        .ok_or_else(|| StdError::not_found("User lifetime revenue"))
}

/// Query revenue events for a specific slot
pub fn query_revenue_events(
    deps: Deps,
    asset: String,
    slot: u8,
) -> StdResult<Vec<RevenueEvent>> {
    let key = (asset, slot.to_string());
    REVENUE_EVENTS
        .may_load(deps.storage, key)?
        .ok_or_else(|| StdError::not_found("Revenue events"))
}

/// Query all assets that have queues
pub fn query_assets(deps: Deps) -> StdResult<AssetsResponse> {
    let assets: Vec<String> = ASSET_QUEUES
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .map(|item| {
            let (key, _) = item?;
            Ok(key)
        })
        .collect::<StdResult<Vec<String>>>()?;

    Ok(AssetsResponse { assets })
}

/// Query daily TVL tracker history
pub fn query_daily_tvl(deps: Deps) -> StdResult<DailyTVLResponse> {
    let entries = DAILY_TVL_TRACKER.may_load(deps.storage)?.unwrap_or_default();
    Ok(DailyTVLResponse { entries })
}

/// Query daily deposit tracker history for an asset
pub fn query_daily_deposits(deps: Deps, asset: String) -> StdResult<DailyDepositResponse> {
    let entries = DAILY_DEPOSIT_TRACKER
        .may_load(deps.storage, asset)?
        .unwrap_or_default();
    Ok(DailyDepositResponse { entries })
}

/// Query user's total deposits
pub fn query_user_total_deposits(deps: Deps, user: String) -> StdResult<UserTotalDepositsResponse> {
    deps.api.addr_validate(&user)?;

    let total_deposits = USER_TOTAL_DEPOSITS
        .may_load(deps.storage, user)?
        .unwrap_or(Uint128::zero());

    Ok(UserTotalDepositsResponse { total_deposits })
}

/// Query all user deposits across all assets
pub fn query_all_user_deposits(
    deps: Deps,
    user: String,
) -> StdResult<AllUserDepositsResponse> {
    let user_addr = deps.api.addr_validate(&user)?;

    let mut all_deposits = Vec::new();

    // Range through all USER_DEPOSITS entries for this user
    let user_deposits_prefix = USER_DEPOSITS.prefix(user_addr);
    let entries = user_deposits_prefix
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .collect::<Result<Vec<_>, _>>()?;

    for (asset, deposit_keys) in entries {
        for deposit_key in deposit_keys {
            // Parse deposit key: "asset:slot:user:deposit_id"
            let parts: Vec<&str> = deposit_key.split(':').collect();
            if parts.len() < 4 {
                continue;
            }

            let slot = match u8::from_str(parts[1]) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let deposit_id = match Uint128::from_str(parts[3]) {
                Ok(id) => id,
                Err(_) => continue,
            };

            if let Ok(deposit) = BACKING_DEPOSITS.load(deps.storage, deposit_key.clone()) {
                let deposit_tokens = match convert_vault_tokens_to_deposit_tokens(
                    deps,
                    asset.clone(),
                    slot,
                    deposit.vault_tokens,
                ) {
                    Ok(tokens) => tokens,
                    Err(_) => deposit.vault_tokens,
                };

                all_deposits.push(UserDepositInfo {
                    asset: asset.clone(),
                    slot,
                    deposit_id,
                    deposit,
                    deposit_tokens,
                });
            }
        }
    }

    Ok(AllUserDepositsResponse {
        deposits: all_deposits,
    })
}

/// Query managed deposit keys for a manager (paginated)
pub fn query_managed_deposit_keys(
    deps: Deps,
    manager: String,
    limit: Option<u32>,
    start_after: Option<String>,
) -> StdResult<ManagedDepositKeysResponse> {
    let manager_addr = deps.api.addr_validate(&manager)?;
    let keys = MANAGED_DEPOSITS
        .may_load(deps.storage, manager_addr)?
        .unwrap_or_default();

    let total = keys.len() as u64;
    let max_limit = limit.unwrap_or(50).min(100) as usize;

    let start_index = if let Some(start_key) = start_after {
        keys.iter()
            .position(|k| k == &start_key)
            .map(|pos| pos + 1)
            .unwrap_or(0)
    } else {
        0
    };

    let end_index = (start_index + max_limit).min(keys.len());
    let result_keys = keys[start_index..end_index].to_vec();

    let next_start_after = if end_index < keys.len() {
        result_keys.last().cloned()
    } else {
        None
    };

    Ok(ManagedDepositKeysResponse {
        keys: result_keys,
        total,
        next_start_after,
    })
}

/// Helper function to convert vault tokens to deposit tokens for a slot
fn convert_vault_tokens_to_deposit_tokens(
    deps: Deps,
    asset: String,
    slot: u8,
    vault_tokens: Uint128,
) -> StdResult<Uint128> {
    let queue = ASSET_QUEUES.load(deps.storage, asset)?;

    let target = Decimal::percent(slot as u64);
    let s = queue.slots.iter().find(|s| s.max_ltv == target)
        .ok_or_else(|| StdError::generic_err(format!("Slot {}% not found", slot)))?;

    if s.total_deposit_tokens.is_zero() || s.total_vault_tokens.is_zero() {
        return Err(StdError::generic_err("Slot has no deposits"));
    }

    calculate_base_tokens(vault_tokens, s.total_deposit_tokens, s.total_vault_tokens)
}

/// Convert vault tokens to deposit tokens for a specific slot
pub fn query_vault_token_conversion(
    deps: Deps,
    asset: String,
    slot: u8,
    vault_tokens: Uint128,
) -> StdResult<Uint128> {
    convert_vault_tokens_to_deposit_tokens(deps, asset, slot, vault_tokens)
}

/// Convert deposit tokens to vault tokens for a specific slot
pub fn query_deposit_token_conversion(
    deps: Deps,
    asset: String,
    slot: u8,
    deposit_tokens: Uint128,
) -> StdResult<Uint128> {
    let queue = ASSET_QUEUES.load(deps.storage, asset)?;

    let target = Decimal::percent(slot as u64);
    let s = queue.slots.iter().find(|s| s.max_ltv == target)
        .ok_or_else(|| StdError::generic_err(format!("Slot {}% not found", slot)))?;

    if s.total_deposit_tokens.is_zero() || s.total_vault_tokens.is_zero() {
        return Err(StdError::generic_err("Slot has no deposits"));
    }

    calculate_vault_tokens(deposit_tokens, s.total_deposit_tokens, s.total_vault_tokens)
}

/// Query total insurance (deposit totals)
/// Returns total in CDT if oracle available, otherwise returns separate per-asset values
pub fn query_total_insurance(
    deps: Deps,
    _env: Env,
) -> StdResult<membrane::ltv_disco::TotalInsuranceResponse> {
    let config = CONFIG.load(deps.storage)?;

    let mut total_insurance_cdt = Uint128::zero();
    let mut deposit_totals: Vec<(String, Uint128)> = vec![];
    let mut oracle_failed = false;

    let all_assets: Vec<String> = ASSET_QUEUES
        .keys(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .collect::<StdResult<Vec<String>>>()?;

    let cdt_info = AssetInfo::NativeToken {
        denom: config.cdt_denom.clone(),
    };

    for asset in &all_assets {
        let queue = match ASSET_QUEUES.load(deps.storage, asset.clone()) {
            Ok(q) => q,
            Err(_) => continue,
        };

        let total_deposit_tokens: Uint128 = queue.slots
            .iter()
            .map(|slot| slot.total_deposit_tokens)
            .sum();

        if total_deposit_tokens.is_zero() {
            continue;
        }

        deposit_totals.push((asset.clone(), total_deposit_tokens));

        let asset_info = AssetInfo::NativeToken {
            denom: asset.clone(),
        };
        let asset_infos = vec![asset_info, cdt_info.clone()];

        let price_response: Result<Vec<PriceResponse>, _> = deps.querier.query_wasm_smart(
            config.oracle_contract.to_string(),
            &Oracle_QueryMsg::Prices {
                asset_infos,
                twap_timeframe: 0,
                oracle_time_limit: 0,
            },
        );

        match price_response {
            Ok(prices) => {
                if let Ok(collateral_value) = prices[0].get_value(total_deposit_tokens) {
                    if let Ok(cdt_amount) = prices[1].get_amount(collateral_value) {
                        total_insurance_cdt += cdt_amount;
                    } else {
                        oracle_failed = true;
                    }
                } else {
                    oracle_failed = true;
                }
            }
            Err(_) => {
                oracle_failed = true;
            }
        }
    }

    if !oracle_failed {
        Ok(membrane::ltv_disco::TotalInsuranceResponse::WithOracle {
            total_insurance: total_insurance_cdt,
        })
    } else {
        Ok(membrane::ltv_disco::TotalInsuranceResponse::WithoutOracle {
            deposit_totals,
        })
    }
}

/// Query manager fee for a specific manager
pub fn query_manager_fee(deps: Deps, manager: String) -> StdResult<ManagerFeeResponse> {
    let manager_addr = deps.api.addr_validate(&manager)?;

    let fee = MANAGER_FEE
        .may_load(deps.storage, manager_addr)?
        .unwrap_or(Decimal::zero());

    Ok(ManagerFeeResponse { manager, fee })
}

/// Query pending unstake requests for a user
pub fn query_unstake_requests(
    deps: Deps,
    user: String,
    asset: String,
) -> StdResult<UnstakeRequestsResponse> {
    let user_addr = deps.api.addr_validate(&user)?;

    let request_keys = USER_UNSTAKE_REQUESTS
        .may_load(deps.storage, (user_addr, asset))?
        .unwrap_or_default();

    let mut requests = Vec::new();
    for key in request_keys {
        if let Ok(request) = UNSTAKE_REQUESTS.load(deps.storage, key) {
            requests.push(request);
        }
    }

    Ok(UnstakeRequestsResponse { requests })
}

/// Query computed revenue weights for all active slots
pub fn query_slot_weights(
    deps: Deps,
    asset: String,
) -> StdResult<SlotWeightsResponse> {
    let queue = ASSET_QUEUES.load(deps.storage, asset)?;
    let weights = calculate_slot_weights(&queue);
    let weights = weights?;
    Ok(SlotWeightsResponse { weights })
}

/// Query deposit-weighted average max LTV across active slots for the given assets.
/// Used by the Collateral contract's dynamic LTV updater.
pub fn query_average_ltvs(deps: Deps, assets: Vec<String>) -> StdResult<AverageLTVsResponse> {
    let mut weighted_sum = Decimal::zero();
    let mut total_deposits = Uint128::zero();

    for asset in &assets {
        let queue = match ASSET_QUEUES.may_load(deps.storage, asset.clone())? {
            Some(q) => q,
            None => continue,
        };

        for slot in &queue.slots {
            // Skip inactive slots
            if !is_active_slot(slot, &queue) {
                continue;
            }
            if slot.total_deposit_tokens.is_zero() {
                continue;
            }

            // weighted_sum += max_ltv * deposit_amount
            weighted_sum += slot.max_ltv * Decimal::from_ratio(slot.total_deposit_tokens, 1u128);
            total_deposits += slot.total_deposit_tokens;
        }
    }

    let average_max_ltv = if total_deposits.is_zero() {
        Decimal::zero()
    } else {
        // weighted_sum / total_deposits
        Decimal::from_ratio(weighted_sum.atomics(), Decimal::one().atomics())
            / Decimal::from_ratio(total_deposits, 1u128)
    };

    Ok(AverageLTVsResponse { average_max_ltv })
}
