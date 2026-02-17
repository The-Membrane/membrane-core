use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, Int128, StdError, StdResult, Storage, Timestamp, Uint128};
use cw_storage_plus::{Item, Map};

use membrane::transmuter::{Config, VolumeWindow, RateHistoryEntry};

pub const CONFIG: Item<Config> = Item::new("config");
pub const DEPOSIT_TOTAL: Item<Uint128> = Item::new("deposit_total");
pub const TOKEN_RATE_ASSURANCE: Item<TokenRateAssurance> = Item::new("token_rate_assurance");
pub const CUMULATIVE_VOLUME: Item<Uint128> = Item::new("cumulative_volume");

pub const TRANSMUTE_HISTORY: Item<Vec<TransmuteSnapshot>> = Item::new("transmute_history");
pub const VOLUME_HISTORY: Item<Vec<VolumeWindow>> = Item::new("volume_history");
pub const VOLUME_WINDOW: Item<VolumeWindow> = Item::new("volume_window");

// Per-address sliding window entries for rate limiting (amounts denominated in Asset A base units)
pub const RATE_LIMIT_FLOWS: Map<String, Vec<FlowEntry>> = Map::new("rate_limit_flows");

// Whitelist set management stored in config.whitelist; this map can be used later for derived data if needed

// Tracks how much paired_asset is currently outstanding from allowlisted deployment venues
pub const DEPLOYED_PAIRED_ASSET: Item<Uint128> = Item::new("deployed_paired_asset");

// Global sliding window entries for all non-whitelisted addresses
pub const GLOBAL_RATE_LIMIT_FLOWS: Item<Vec<FlowEntry>> = Item::new("global_rate_limit_flows");

// Tracks accumulated fees (in paired_asset) that couldn't be converted to CDT yet
pub const PENDING_REVENUE: Item<Uint128> = Item::new("pending_revenue");

// ================= Affiliates =================
/// Affiliates map: user address -> Vec<AffiliateData>
pub const AFFILIATES: Map<String, Vec<membrane::types::AffiliateData>> = Map::new("affiliates");
/// Maximum number of affiliates per user
pub const AFFILIATE_LIMIT: usize = 10;

#[cw_serde]
pub struct TransmuteSnapshot {
    pub offered_asset: String,
    pub offered_amount: Uint128,
    pub received_asset: String,
    pub received_amount: Uint128,
    pub block_time: Timestamp,
}

#[cw_serde]
pub struct FlowEntry {
    /// Signed base amount in Asset A units; positive for asset_b->asset_a, negative for asset_a->asset_b
    pub amount_base: Int128,
    pub block_time: Timestamp,
}

#[cw_serde]
pub struct TokenRateAssurance {
    pub pre_deposit_total: Uint128,
}

/// Emissions event (similar to RevenueEvent in ltv_disco)
#[cw_serde]
pub struct EmissionsEvent {
    pub timestamp: u64,
    /// Emissions per 1 unit of retention weight
    pub amount_per_weight: Decimal,
    /// Remaining total to be claimed from this event
    pub amount_to_be_claimed: Uint128,
}

/// Time cliff representing a change in daily weight delta
#[cw_serde]
pub struct WeightTimeCliff {
    /// Timestamp when this cliff becomes active
    pub timestamp: u64,
    /// Daily delta change at this cliff (can be positive or negative)
    pub delta_change: Int128,
}

/// Tracks retention weight changes over time for a user
#[cw_serde]
pub struct RetentionWeightTracking {
    /// Base weight at reference_time
    pub base_weight: Uint128,
    /// Reference timestamp for base_weight and daily_delta calculations
    pub reference_time: u64,
    /// Daily delta at reference_time
    pub daily_delta: Int128,
    /// Time cliffs for this user (sorted by timestamp)
    pub time_cliffs: Vec<WeightTimeCliff>,
}

// ================= Rate History =================
pub const RATE_HISTORY: Item<Vec<RateHistoryEntry>> = Item::new("rate_history");
pub const LAST_RATE_UPDATE: Item<Timestamp> = Item::new("last_rate_update");

// ================= User Deposits for Discount Tracking =================
#[cw_serde]
pub struct UserDeposit {
    /// Deposit ID (unique identifier for this deposit)
    pub deposit_id: Uint128,
    /// Deposit amount (1:1 tracking, CDT + paired asset)
    pub amount: Uint128,
    /// Timestamp when deposit was made
    pub deposit_time: u64,
    /// Lock information (similar to ltv_disco structure)
    pub locked: Option<membrane::types::Locked>,
    /// Timestamp when deposit/lock was created (for discount curve calculation)
    pub start_time: u64,
}

/// User address -> Vec<UserDeposit>
pub const USER_DEPOSITS: Map<String, Vec<UserDeposit>> = Map::new("user_deposits");

// Constants for deposit management
pub const MIN_DEPOSIT_AMOUNT: Uint128 = Uint128::new(5);
pub const MAX_DEPOSITS_PER_USER: usize = 100;
pub const DEPOSIT_CONSOLIDATION_WINDOW_SECS: u64 = 604_800; // 1 week

// Deposit ID tracking
pub const CURRENT_DEPOSIT_ID: Item<Uint128> = Item::new("current_deposit_id");

// ================= Retention Emissions =================
/// RevenueEvent system (similar to ltv_disco)
pub const EMISSIONS_EVENTS: Item<Vec<EmissionsEvent>> = Item::new("emissions_events");

/// Time-based weight tracking for accurate denominators
/// Key: user address
pub const RETENTION_WEIGHT_TRACKING: Map<String, RetentionWeightTracking> = Map::new("retention_weight_tracking");

/// Global retention weight tracking (aggregates all users, similar to GroupLVTTracking in ltv_disco)
pub const GLOBAL_RETENTION_WEIGHT_TRACKING: Item<RetentionWeightTracking> = Item::new("global_retention_weight_tracking");

/// Track last emissions distribution to ensure events are created before claims
pub const LAST_EMISSIONS_DISTRIBUTION: Item<Timestamp> = Item::new("last_emissions_distribution");

/// User's last claimed timestamp per deposit (for emissions)
/// Key: (user_address, deposit_index)
pub const USER_EMISSIONS_LAST_CLAIMED: Map<(String, u64), u64> = Map::new("user_emissions_last_claimed");


pub fn new_volume_window(now: Timestamp, cumulative_volume: Uint128) -> VolumeWindow {
    VolumeWindow {
        cdt_swapped: Uint128::zero(),
        cdt_received: Uint128::zero(),
        paired_asset_swapped: Uint128::zero(),
        paired_asset_received: Uint128::zero(),
        block_time: now,
        cumulative_volume,
    }
}

pub fn apply_volume_update(
    store: &mut dyn Storage,
    window: &mut VolumeWindow,
    cdt_swapped: Uint128,
    cdt_received: Uint128,
    paired_asset_swapped: Uint128,
    paired_asset_received: Uint128,
) -> StdResult<()> {
    // Calculate the volume delta being added in this update
    let update_delta = cdt_swapped
        .checked_add(cdt_received)?
        .checked_add(paired_asset_swapped)?
        .checked_add(paired_asset_received)?;
    
    // Update window fields
    window.cdt_swapped += cdt_swapped;
    window.cdt_received += cdt_received;
    window.paired_asset_swapped += paired_asset_swapped;
    window.paired_asset_received += paired_asset_received;
    
    // Get current cumulative volume and add the delta
    let current_cumulative = CUMULATIVE_VOLUME.may_load(store)?.unwrap_or(Uint128::zero());
    let new_cumulative = current_cumulative.checked_add(update_delta)?;
    
    // Update cumulative volume tracker
    CUMULATIVE_VOLUME.save(store, &new_cumulative)?;
    
    // Set window's cumulative_volume to the current cumulative total
    window.cumulative_volume = new_cumulative;
    
    Ok(())
}

pub fn init_history(store: &mut dyn Storage) -> StdResult<()> {
    TRANSMUTE_HISTORY.save(store, &Vec::new())?;
    VOLUME_HISTORY.save(store, &Vec::new())?;
    Ok(())
}

pub fn append_transmute_snapshot(
    store: &mut dyn Storage,
    cap: u32,
    snapshot: TransmuteSnapshot,
) -> StdResult<()> {
    if cap == 0 {
        return Err(StdError::generic_err("transmute history cap cannot be zero"));
    }

    let mut history = TRANSMUTE_HISTORY.may_load(store)?.unwrap_or_default();
    //Push first
    history.push(snapshot);
    //Then trim if needed
    if history.len() > cap as usize {
        let remove = history.len() - cap as usize;
        history.drain(0..remove);
    }

    TRANSMUTE_HISTORY.save(store, &history)?;
    Ok(())
}

pub fn append_volume_window(
    store: &mut dyn Storage,
    cap: u32,
    window: VolumeWindow,
) -> StdResult<()> {
    if cap == 0 {
        return Err(StdError::generic_err("volume history cap cannot be zero"));
    }

    let mut history = VOLUME_HISTORY.may_load(store)?.unwrap_or_default();
    //Push first
    history.push(window);

    //Then trim if needed
    if history.len() > cap as usize {
        let remove = history.len() - cap as usize;
        history.drain(0..remove);
    }

    VOLUME_HISTORY.save(store, &history)
}

pub fn history_slice<T: Clone>(
    history: &[T],
    start_after: Option<u64>,
    limit: Option<u32>,
) -> (Vec<T>, usize) {
    let total = history.len();
    if total == 0 {
        return (vec![], 0);
    }

    let start_index = start_after
        .and_then(|idx| idx.checked_add(1))
        .unwrap_or(0)
        .min(total as u64) as usize;

    let max = limit.unwrap_or(50).min(100) as usize;
    let end_index = (start_index + max).min(total);

    (history[start_index..end_index].to_vec(), start_index)
}

pub fn history_total<T>(history: &[T]) -> u64 {
    history.len() as u64
}

pub fn append_rate_history_entry(
    store: &mut dyn Storage,
    cap: u32,
    entry: RateHistoryEntry,
) -> StdResult<()> {
    if cap == 0 {
        return Err(StdError::generic_err("rate history cap cannot be zero"));
    }

    let mut history = RATE_HISTORY.may_load(store)?.unwrap_or_default();
    //Push first
    history.push(entry);

    //Then trim if needed
    if history.len() > cap as usize {
        let remove = history.len() - cap as usize;
        history.drain(0..remove);
    }

    RATE_HISTORY.save(store, &history)
}
