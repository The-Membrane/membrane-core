use cosmwasm_std::{StdResult, Storage};
use cw_storage_plus::{Item, Map};
use serde::{Deserialize, Serialize};
use cosmwasm_schema::cw_serde;
use membrane::types::TickRecord;
// Minimal config for RPS engine
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Config {
    pub admin: String,
    pub car_contract: String,
    pub max_ticks: u32,
    pub match_history_limit: u32,
    pub tick_history_limit: u32,
}

pub const CONFIG: Item<Config> = Item::new("config");

// (car_id, state_id) -> [i8; 3] (R, P, S)
pub const Q_TABLE: Map<(u128, u8), [i8; 3]> = Map::new("rps_q_table");

// per-car rolling match results: 1 = win, 0 = loss
pub const MATCH_HISTORY: Map<u128, Vec<u8>> = Map::new("rps_match_history");


// per-car rolling play-by-play history (bounded by tick_history_limit)
pub const TICK_HISTORY: Map<u128, Vec<TickRecord>> = Map::new("rps_tick_history");

pub fn get_config(storage: &dyn Storage) -> StdResult<Config> {
    CONFIG.load(storage)
}

pub fn set_config(storage: &mut dyn Storage, cfg: Config) -> StdResult<()> {
    CONFIG.save(storage, &cfg)
}

pub fn get_q_values(storage: &dyn Storage, car_id: u128, state: u8) -> StdResult<[i8; 3]> {
    Q_TABLE.load(storage, (car_id, state))
}

pub fn set_q_values(
    storage: &mut dyn Storage,
    car_id: u128,
    state: u8,
    q_values: [i8; 3],
) -> StdResult<()> {
    Q_TABLE.save(storage, (car_id, state), &q_values)
}

pub fn push_match_result(storage: &mut dyn Storage, car_id: u128, won: bool) -> StdResult<()> {
    let mut history = MATCH_HISTORY.load(storage, car_id).unwrap_or_default();
    history.push(if won { 1 } else { 0 });

    let limit = CONFIG.load(storage)?.match_history_limit as usize;
    if history.len() > limit {
        let overflow = history.len() - limit;
        history.drain(0..overflow);
    }

    MATCH_HISTORY.save(storage, car_id, &history)
}

pub fn push_tick_records(storage: &mut dyn Storage, car_id: u128, mut ticks: Vec<TickRecord>) -> StdResult<()> {
    if ticks.is_empty() {
        return Ok(());
    }
    let mut history = TICK_HISTORY.load(storage, car_id).unwrap_or_default();
    history.append(&mut ticks);

    let limit = CONFIG.load(storage)?.tick_history_limit as usize;
    if history.len() > limit {
        let overflow = history.len() - limit;
        history.drain(0..overflow);
    }

    TICK_HISTORY.save(storage, car_id, &history)
}


