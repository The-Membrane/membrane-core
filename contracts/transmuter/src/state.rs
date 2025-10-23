use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Int128, StdError, StdResult, Storage, Timestamp, Uint128};
use cw_storage_plus::{Item, Map};

use membrane::transmuter::{Config, VolumeWindow};

pub const CONFIG: Item<Config> = Item::new("config");
pub const VAULT_TOKEN_SUPPLY: Item<Uint128> = Item::new("vault_token_supply");
pub const TOKEN_RATE_ASSURANCE: Item<TokenRateAssurance> = Item::new("token_rate_assurance");

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
    pub pre_btokens_per_one: Uint128,
}


pub fn new_volume_window(now: Timestamp) -> VolumeWindow {
    VolumeWindow {
        cdt_swapped: Uint128::zero(),
        cdt_received: Uint128::zero(),
        paired_asset_swapped: Uint128::zero(),
        paired_asset_received: Uint128::zero(),
        block_time: now,
    }
}

pub fn apply_volume_update(
    window: &mut VolumeWindow,
    cdt_swapped: Uint128,
    cdt_received: Uint128,
    paired_asset_swapped: Uint128,
    paired_asset_received: Uint128,
) {
    window.cdt_swapped += cdt_swapped;
    window.cdt_received += cdt_received;
    window.paired_asset_swapped += paired_asset_swapped;
    window.paired_asset_received += paired_asset_received;
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
