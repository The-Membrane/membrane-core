use cosmwasm_std::{Storage, StdResult, Uint128};
use cw_storage_plus::{Item, Map};
use membrane::byte_minter::{Config, DifficultyAdjustmentConfig};
use serde::{Deserialize, Serialize};

pub const CONFIG: Item<Config> = Item::new("config");

// Current event windows start time (seconds since epoch)
pub const MAZE_WINDOW_START: Item<u64> = Item::new("maze_window_start");
pub const PVP_WINDOW_START: Item<u64> = Item::new("pvp_window_start");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MazeEventInfo {
    pub track_id: Option<u128>,
    pub set_ts: Option<u64>,
}

// Selected track IDs and timestamp for current maze window
pub const MAZE_EVENT_INFO: Item<MazeEventInfo> = Item::new("maze_event_info");
// Selected PvP track id for current window
pub const PVP_EVENT_TRACK_ID: Item<Option<u128>> = Item::new("pvp_event_track_id");

// Winners set per window
pub const MAZE_WINNERS: Map<(u64, u128), bool> = Map::new("maze_winners");
pub const PVP_WINNERS: Map<(u64, u128), bool> = Map::new("pvp_winners");

// Win count tracking for difficulty adjustment
pub const MAZE_WIN_COUNT: Item<u32> = Item::new("maze_win_count");
pub const PVP_WIN_COUNT: Item<u32> = Item::new("pvp_win_count");

// Historical win averages for difficulty adjustment (rolling window of last N windows)
pub const MAZE_WIN_HISTORY: Item<Vec<u32>> = Item::new("maze_win_history");
pub const PVP_WIN_HISTORY: Item<Vec<u32>> = Item::new("pvp_win_history");

// Difficulty adjustment configuration
pub const DIFFICULTY_ADJUSTMENT_CONFIG: Item<DifficultyAdjustmentConfig> = Item::new("difficulty_adjustment_config");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CarLifetimeTracker {
    pub lifetime_rewards: Uint128,
    pub mazes_completed: u32,
    pub pvp_wins: u32,
}

// Car lifetime tracking: car_id -> CarLifetimeTracker
pub const CAR_LIFETIME_TRACKERS: Map<u128, CarLifetimeTracker> = Map::new("car_lifetime_trackers");

pub fn get_config(storage: &dyn Storage) -> StdResult<Config> { CONFIG.load(storage) }
pub fn set_config(storage: &mut dyn Storage, config: Config) -> StdResult<()> { CONFIG.save(storage, &config) } 