use cosmwasm_schema::cw_serde;
use cosmwasm_std::{StdError, StdResult, Storage, Timestamp, Uint128};
use cw_storage_plus::Item;

use membrane::transmuter::{Config, VolumeWindow};

pub const CONFIG: Item<Config> = Item::new("config");
pub const VAULT_TOKEN_SUPPLY: Item<Uint128> = Item::new("vault_token_supply");

pub const TRANSMUTE_HISTORY: Item<Vec<TransmuteSnapshot>> = Item::new("transmute_history");
pub const VOLUME_HISTORY: Item<Vec<VolumeWindow>> = Item::new("volume_history");
pub const VOLUME_WINDOW: Item<VolumeWindow> = Item::new("volume_window");

#[cw_serde]
pub struct TransmuteSnapshot {
    pub offered_asset: String,
    pub offered_amount: Uint128,
    pub received_asset: String,
    pub received_amount: Uint128,
    pub block_time: Timestamp,
}


pub fn new_volume_window(now: Timestamp) -> VolumeWindow {
    VolumeWindow {
        asset_a_swapped: Uint128::zero(),
        asset_a_received: Uint128::zero(),
        asset_b_swapped: Uint128::zero(),
        asset_b_received: Uint128::zero(),
        block_time: now,
    }
}

pub fn apply_volume_update(
    window: &mut VolumeWindow,
    asset_a_swapped: Uint128,
    asset_a_received: Uint128,
    asset_b_swapped: Uint128,
    asset_b_received: Uint128,
) {
    window.asset_a_swapped += asset_a_swapped;
    window.asset_a_received += asset_a_received;
    window.asset_b_swapped += asset_b_swapped;
    window.asset_b_received += asset_b_received;
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
