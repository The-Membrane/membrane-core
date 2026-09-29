use cosmwasm_schema::cw_serde;
use cosmwasm_std::{StdError, StdResult, Storage, Timestamp, Uint128};
use cw_storage_plus::{Item, Map};

use membrane::transmuter::{Config, VolumeWindow, RateHistoryEntry, TrancheState, TrancheRateAssurance};

pub const CONFIG: Item<Config> = Item::new("config");

// ================= Tranche State =================
/// Per-user tranche allocations
/// Key: (user_addr, (collateral_denom, "j"|"s"))
/// Value: virtual vault token amount
pub const USER_TRANCHES: Map<(&str, (&str, &str)), Uint128> = Map::new("user_tranches");

/// Global tranche state per (collateral_denom, "j"|"s")
pub const TRANCHE_STATES: Map<(&str, &str), TrancheState> = Map::new("tranche_states");

/// Pre-operation rate snapshot for touched tranches (cleared after rate assurance)
pub const TRANCHE_RATE_ASSURANCE: Map<(&str, &str), TrancheRateAssurance> = Map::new("tranche_rate_assurance");

/// Sum of all tranche total_staked (for efficient global invariant checks)
pub const TOTAL_TRANCHE_STAKED: Item<Uint128> = Item::new("total_tranche_staked");

/// Accumulated swap fees not yet distributed to tranches
pub const UNDISTRIBUTED_FEES: Item<Uint128> = Item::new("undistributed_fees");

// ================= Acquisition =================
/// Tracks deposits from the acquisition contract separately.
pub const ACQUISITION_DEPOSIT_TOTAL: Item<Uint128> = Item::new("acquisition_deposit_total");

// ================= Volume & History =================
pub const CUMULATIVE_VOLUME: Item<Uint128> = Item::new("cumulative_volume");
pub const TRANSMUTE_HISTORY: Item<Vec<TransmuteSnapshot>> = Item::new("transmute_history");
pub const VOLUME_HISTORY: Item<Vec<VolumeWindow>> = Item::new("volume_history");
pub const VOLUME_WINDOW: Item<VolumeWindow> = Item::new("volume_window");

// Tracks how much paired_asset is currently outstanding from allowlisted deployment venues
pub const DEPLOYED_PAIRED_ASSET: Item<Uint128> = Item::new("deployed_paired_asset");

// Tracks accumulated fees (in paired_asset) that couldn't be converted to CDT yet
pub const PENDING_REVENUE: Item<Uint128> = Item::new("pending_revenue");

// ================= Affiliates =================
pub const AFFILIATES: Map<String, Vec<membrane::types::AffiliateData>> = Map::new("affiliates");
pub const AFFILIATE_LIMIT: usize = 10;

// ================= Rate History =================
pub const RATE_HISTORY: Item<Vec<RateHistoryEntry>> = Item::new("rate_history");
pub const LAST_RATE_UPDATE: Item<Timestamp> = Item::new("last_rate_update");

// ================= Snapshot Types =================
#[cw_serde]
pub struct TransmuteSnapshot {
    pub offered_asset: String,
    pub offered_amount: Uint128,
    pub received_asset: String,
    pub received_amount: Uint128,
    pub block_time: Timestamp,
}

// ================= Helper Functions =================

pub fn junior_str(junior: bool) -> &'static str {
    if junior { "j" } else { "s" }
}

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
    let update_delta = cdt_swapped
        .checked_add(cdt_received)?
        .checked_add(paired_asset_swapped)?
        .checked_add(paired_asset_received)?;

    window.cdt_swapped += cdt_swapped;
    window.cdt_received += cdt_received;
    window.paired_asset_swapped += paired_asset_swapped;
    window.paired_asset_received += paired_asset_received;

    let current_cumulative = CUMULATIVE_VOLUME.may_load(store)?.unwrap_or(Uint128::zero());
    let new_cumulative = current_cumulative.checked_add(update_delta)?;
    CUMULATIVE_VOLUME.save(store, &new_cumulative)?;
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
    history.push(snapshot);
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
    history.push(window);

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
    history.push(entry);

    if history.len() > cap as usize {
        let remove = history.len() - cap as usize;
        history.drain(0..remove);
    }

    RATE_HISTORY.save(store, &history)
}
