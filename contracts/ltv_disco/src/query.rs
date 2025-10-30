use cosmwasm_std::{Decimal, Deps, StdResult, StdError, Uint128};
use std::str::FromStr;
use membrane::ltv_disco::{
    Config, LTVQueue, AverageLTVsResponse,
    LTVQueueResponse, BackingDepositResponse, BackingDepositsByUserResponse, 
    RevenueTrackingEntry, PendingClaimsResponse, DepositPendingClaim, 
    UserLifetimeRevenueEntry, RevenueEvent, BackingDeposit, AssetsResponse, DailyTVLResponse,
    UserTotalDepositsResponse
};
use membrane::types::AssetInfo;
use membrane::oracle::{QueryMsg as Oracle_QueryMsg, PriceResponse};

use crate::state::{CONFIG, LTV_QUEUES, REVENUE_TRACKING, REVENUE_EVENTS, USER_LIFETIME_REVENUE, BACKING_DEPOSITS, USER_DEPOSITS, DISPERSAL, DAILY_TVL_TRACKER, USER_TOTAL_DEPOSITS};

const MAX_LIMIT: u32 = 32;

/// Query contract configuration
pub fn query_config(deps: Deps) -> StdResult<Config> {
    CONFIG.load(deps.storage)
}

/// Query LTV queue for an asset
pub fn query_ltv_queue(deps: Deps, asset: String) -> StdResult<LTVQueueResponse> {
    let queue = LTV_QUEUES.load(deps.storage, asset)?;
    Ok(LTVQueueResponse { queue })
}

/// Query backing deposit by user and group
pub fn query_backing_deposit(
    deps: Deps, 
    user: String, 
    asset: String, 
    ltv: Decimal, 
    max_borrow_ltv: Decimal
) -> StdResult<BackingDepositResponse> {
    let user_addr = deps.api.addr_validate(&user)?;
    let deposit_key = format!("{}:{}:{}:{}", asset, ltv, max_borrow_ltv, user_addr);
    
    let deposit = BACKING_DEPOSITS.load(deps.storage, deposit_key)?;

    Ok(BackingDepositResponse { deposit })
}

/// Query backing deposits by user
pub fn query_backing_deposits_by_user(
    deps: Deps,
    user: String,
    asset: String,
    limit: Option<u32>,
    _start_after: Option<Uint128>,
) -> StdResult<BackingDepositsByUserResponse> {
    let user_addr = deps.api.addr_validate(&user)?;
    let limit = limit.unwrap_or(MAX_LIMIT) as usize;
    
    // Load deposit keys from USER_DEPOSITS
    let deposit_keys = USER_DEPOSITS
        .may_load(deps.storage, (user_addr.clone(), asset))?
        .unwrap_or_else(Vec::new);
    
    let mut deposits = Vec::new();
    for key in deposit_keys.iter().take(limit) {
        if let Ok(deposit) = BACKING_DEPOSITS.load(deps.storage, key.clone()) {
            deposits.push(deposit);
        }
    }

    Ok(BackingDepositsByUserResponse { deposits })
}

/// Query average LTVs for assets
pub fn query_average_ltvs(deps: Deps, assets: Vec<String>) -> StdResult<AverageLTVsResponse> {
    let mut total_weighted_ltv = Decimal::zero();
    let mut total_weighted_borrow_ltv = Decimal::zero();
    let mut total_weight = Decimal::zero();
    let mut total_borrow_weight = Decimal::zero();

    for asset in assets {
        if let Ok(queue) = LTV_QUEUES.load(deps.storage, asset) {
            for slot in queue.slots {
                if !slot.total_deposit_tokens.is_zero() {
                    // Use total_deposit_tokens as weight for LTV
                    let weight = Decimal::from_ratio(slot.total_deposit_tokens.u128(), 1u128);
                    total_weighted_ltv += slot.ltv * weight;
                    total_weight += weight;
                }

                for group in slot.deposit_groups {
                    if !group.total_vault_tokens.is_zero() {
                        let borrow_weight = Decimal::from_ratio(group.total_vault_tokens.u128(), 1u128);
                        total_weighted_borrow_ltv += group.max_borrow_ltv * borrow_weight;
                        total_borrow_weight += borrow_weight;
                    }
                }
            }
        }
    }

    let average_max_ltv = if total_weight.is_zero() {
        Decimal::zero()
    } else {
        total_weighted_ltv / total_weight
    };

    let average_max_borrow_ltv = if total_borrow_weight.is_zero() {
        Decimal::zero()
    } else {
        total_weighted_borrow_ltv / total_borrow_weight
    };

    Ok(AverageLTVsResponse { 
        average_max_ltv,
        average_max_borrow_ltv 
    })
}

/// Query if the LTV Disco can handle bad debt for an asset
/// Returns true if dispersals + deposit value (via oracle) >= requested CDT amount
pub fn query_can_handle_bad_debt(deps: Deps, asset: String, amount: Uint128) -> StdResult<bool> {
    let config = CONFIG.load(deps.storage)?;
    let queue: LTVQueue = match LTV_QUEUES.load(deps.storage, asset.clone()){
        Ok(queue) => queue,
        Err(_) => return Ok(false),
    };

    let mut total_available_cdt = Uint128::zero();

    // 1. Calculate available CDT from dispersals
    if let Ok(Some(dispersal)) = DISPERSAL.may_load(deps.storage, asset.clone()) {
        // Active dispersal: available = total_to_disperse - amount_dispersed
        if dispersal.active_dispersal.dispersal_start != 0 {
            let available_in_active = dispersal.total_to_disperse
                .checked_sub(dispersal.active_dispersal.amount_dispersed)
                .unwrap_or(Uint128::zero());
            total_available_cdt += available_in_active;
        }
        // Pending dispersal
        total_available_cdt += dispersal.pending_dispersal;
    }

    // 2. Calculate CDT value of deposit tokens via oracle
    // Sum total deposit tokens across all slots/groups
    let total_deposit_tokens: Uint128 = queue.slots
        .iter()
        .flat_map(|slot| &slot.deposit_groups)
        .map(|group| group.total_deposit_tokens)
        .sum();

    if !total_deposit_tokens.is_zero() {
        // Query oracle for asset and CDT prices
        let asset_info = AssetInfo::NativeToken { denom: asset.clone() };
        let cdt_info = AssetInfo::NativeToken { denom: config.cdt_denom.clone() };
        let asset_infos = vec![asset_info, cdt_info];

        // Query prices (handle error gracefully - return false if oracle fails)
        let price_response: Result<Vec<PriceResponse>, _> = deps.querier.query_wasm_smart(
            config.oracle_contract.to_string(),
            &Oracle_QueryMsg::Prices {
                asset_infos,
                twap_timeframe: 0,
                oracle_time_limit: 0,
            },
        );

        if let Ok(prices) = price_response {
            // Convert deposit tokens to CDT value
            // Collateral amount -> USD value -> CDT amount
            if let Ok(collateral_value) = prices[0].get_value(total_deposit_tokens) {
                if let Ok(cdt_amount) = prices[1].get_amount(collateral_value) {
                    total_available_cdt += cdt_amount;
                }
            }
        }
    }

    // 3. Check if total available CDT >= requested amount
    Ok(total_available_cdt >= amount)
}

/// Query cumulative revenue with optional aggregation
pub fn query_cumulative_revenue(
    deps: Deps,
    asset: String,
    max_ltv: Option<Decimal>,
    max_borrow_ltv: Option<Decimal>,
) -> StdResult<Vec<RevenueTrackingEntry>> {
    match (max_ltv, max_borrow_ltv) {
        // Both specified - return specific slot/group revenue
        (Some(ltv), Some(borrow_ltv)) => {
            REVENUE_TRACKING
                .may_load(deps.storage, (asset, ltv.to_string(), borrow_ltv.to_string()))?
                .map(|entries| Ok(entries))
                .unwrap_or_else(|| Ok(vec![]))
        }
        // Only max_ltv specified - aggregate all groups for that LTV
        (Some(ltv), None) => {
            let mut all_entries: Vec<RevenueTrackingEntry> = vec![];
            
            // Iterate through all possible borrow_ltv combinations for this asset/ltv
            // This requires loading the queue to get valid borrow_ltv values
            let queue = LTV_QUEUES.load(deps.storage, asset.clone())?;
            if let Some(slot) = queue.slots.iter().find(|s| s.ltv == ltv) {
                for group in &slot.deposit_groups {
                    if let Ok(Some(entries)) = REVENUE_TRACKING.may_load(
                        deps.storage,
                        (asset.clone(), ltv.to_string(), group.max_borrow_ltv.to_string())
                    ) {
                        // Merge entries by timestamp, summing revenues
                        for entry in entries {
                            if let Some(existing) = all_entries.iter_mut().find(|e| e.timestamp == entry.timestamp) {
                                existing.total_revenue += entry.total_revenue;
                            } else {
                                all_entries.push(entry);
                            }
                        }
                    }
                }
            }
            all_entries.sort_by_key(|e| e.timestamp);
            Ok(all_entries)
        }
        // Neither specified - aggregate all revenue for asset
        (None, None) => {
            let mut all_entries: Vec<RevenueTrackingEntry> = vec![];
            
            let queue = LTV_QUEUES.load(deps.storage, asset.clone())?;
            for slot in &queue.slots {
                for group in &slot.deposit_groups {
                    if let Ok(Some(entries)) = REVENUE_TRACKING.may_load(
                        deps.storage,
                        (asset.clone(), slot.ltv.to_string(), group.max_borrow_ltv.to_string())
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
            }
            all_entries.sort_by_key(|e| e.timestamp);
            Ok(all_entries)
        }
        // max_borrow_ltv without max_ltv is invalid
        (None, Some(_)) => {
            Err(cosmwasm_std::StdError::generic_err("max_borrow_ltv requires max_ltv to be specified"))
        }
    }
}

/// Query pending claims for a user across all their deposits
pub fn query_pending_claims(
    deps: Deps,
    user: String,
    asset: String,
) -> StdResult<PendingClaimsResponse> {
    let user_addr = deps.api.addr_validate(&user)?;
    
    // Use USER_DEPOSITS index for efficient lookup
    let deposit_keys = USER_DEPOSITS
        .may_load(deps.storage, (user_addr.clone(), asset.clone()))?
        .unwrap_or_else(Vec::new);
    
    let mut claims = Vec::new();
    
    // Iterate through user's deposit keys
    for deposit_key_str in deposit_keys {
        // deposit_key is "asset:ltv:max_borrow_ltv:user"
        if let Ok(deposit) = BACKING_DEPOSITS.load(deps.storage, deposit_key_str.clone()) {
            // Parse LTV values from key string
            let parts: Vec<&str> = deposit_key_str.split(':').collect();
            if parts.len() != 4 {
                continue;
            }
            let max_ltv = Decimal::from_str(parts[1])?;
            let max_borrow_ltv = Decimal::from_str(parts[2])?;
            
            let pending = calculate_pending_revenue(
                deps.storage,
                &deposit,
                asset.clone(),
                max_ltv,
                max_borrow_ltv,
            )?;
            
            if !pending.is_zero() {
                claims.push(DepositPendingClaim {
                    max_ltv,
                    max_borrow_ltv,
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
    deposit: &BackingDeposit,
    asset: String,
    max_ltv: Decimal,
    max_borrow_ltv: Decimal,
) -> StdResult<Uint128> {
    let key = (asset, max_ltv.to_string(), max_borrow_ltv.to_string());
    let events = REVENUE_EVENTS
        .may_load(storage, key)?
        .unwrap_or_else(Vec::new);
    
    let mut pending = Uint128::zero();
    
    for event in events {
        if event.timestamp <= deposit.last_claimed {
            continue;
        }
        
        if event.amount_to_be_claimed.is_zero() {
            continue;
        }
        
        let user_share = event.amount_per_vt * deposit.vault_tokens;
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

/// Query revenue events for a specific group
pub fn query_revenue_events(
    deps: Deps,
    asset: String,
    max_ltv: Decimal,
    max_borrow_ltv: Decimal,
) -> StdResult<Vec<RevenueEvent>> {
    let key = (asset, max_ltv.to_string(), max_borrow_ltv.to_string());
    REVENUE_EVENTS
        .may_load(deps.storage, key)?
        .ok_or_else(|| StdError::not_found("Revenue events"))
}

/// Query all assets that have LTV queues
pub fn query_assets(deps: Deps) -> StdResult<AssetsResponse> {
    let assets: Vec<String> = LTV_QUEUES
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
    let entries = DAILY_TVL_TRACKER.may_load(deps.storage)?.unwrap_or_else(Vec::new);
    Ok(DailyTVLResponse { entries })
}

/// Query user's total deposits
pub fn query_user_total_deposits(deps: Deps, user: String) -> StdResult<UserTotalDepositsResponse> {
    // Validate user address
    deps.api.addr_validate(&user)?;
    
    let total_deposits = USER_TOTAL_DEPOSITS
        .may_load(deps.storage, user)?
        .unwrap_or(Uint128::zero());
    
    Ok(UserTotalDepositsResponse { total_deposits })
}