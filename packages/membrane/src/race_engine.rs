use std::collections::HashMap;

use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Uint128, Decimal};

use crate::types::{IntegerQTableEntry, RewardNumbers, Track, TrackTile, TrackTrainingStats, TopTimes, BrainProgress};

pub const DEFAULT_SPEED: u8 = 1;
pub const DEFAULT_BOOST_SPEED: u8 = 3;

#[cw_serde]
pub struct InstantiateMsg {
    pub admin: String,
    pub track_contract: String,
    pub car_contract: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    SimulateRace {
        track_id: Uint128,
        car_ids: Vec<u128>,
        pvp: Option<bool>,
        train: bool,
        training_config: Option<TrainingConfig>,
        reward_config: Option<RewardNumbers>,
        max_race_ticks: Option<u32>,
    },
    /// Reset the Q-table for a car
    /// Must be called by the owner of the car in the car contract
    ResetQ {
        car_id: Uint128,
    },
    /// Purge all state for a car (Q-table and training stats). Only callable by car contract
    PurgeCar {
        car_id: Uint128,
    },
    /// Update contract configuration (admin only)
    UpdateConfig {
        max_ticks: Option<u32>,
        byte_minter_contract: Option<String>,
        rps_engine_contract: Option<String>,
        brain_progress_entry_limit: Option<u32>,
    },
    /// Migrate existing Q-table states from legacy byte array hashes to integer hashes
    MigrateQTableStates {
        car_id: Uint128,
        batch_size: Option<u32>,
    },

}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(RaceResultResponse)]
    GetRaceResult { 
        track_id: u128,
        race_id: String,
     },
    #[returns(RecentRacesResponse)]
    ListRecentRaces {
        ///Must provide one of the following////
        //Filter by car id 
        /// - If provided, return races for that car
        car_id: Option<u128>,
        //Filter by track id
        /// - If provided, return races for that track
        track_id: Option<u128>,
        //Start after a specific race id
        start_after: Option<u128>,
        limit: Option<u32>,
    },
    #[returns(ConfigResponse)]
    GetConfig {},

    #[returns(Vec<GetTrackTrainingStatsResponse>)]
    GetTrackTrainingStats { 
        car_id: u128, 
        track_id: Option<u128>,
        start_after: Option<u128>,
        limit: Option<u32>,
    },
    #[returns(TopTimes)]
    GetTopTimes { track_id: u128 },
    // NEW: Integer-based Q-table queries
    #[returns(GetIntegerQResponse)]
    GetIntegerQ { car_id: u128, state_hash: Option<u32> },
    // NEW: Migration status queries
    #[returns(MigrationStatusResponse)]
    GetMigrationStatus { car_id: u128 },
    // NEW: Brain progress queries
    #[returns(BrainProgressResponse)]
    GetBrainProgress { car_id: u128 },

}

#[cw_serde]
pub struct RaceResultResponse {
    pub result: RaceResult,
}


/// NEW: Response for integer-based Q-table queries
#[cw_serde]
pub struct GetIntegerQResponse {
    pub car_id: u128,
    pub q_values: Vec<IntegerQTableEntry>,
}

/// NEW: Response for migration status queries
#[cw_serde]
pub struct MigrationStatusResponse {
    pub car_id: u128,
    pub legacy_entries_count: u32,
    pub integer_entries_count: u32,
    pub migration_complete: bool,
}



#[cw_serde]
pub struct RecentRacesResponse {
    pub races: Vec<RaceResult>,
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

#[cw_serde]
pub struct GetTrackTrainingStatsResponse {
    pub car_id: u128,
    pub track_id: u128,
    pub stats: TrackTrainingStats,
}

#[cw_serde]
pub struct Rank {
    pub car_id: u128,
    pub rank: u32,
}


#[cw_serde]
pub struct Position {
    pub x: u32,
    pub y: u32,
}

#[cw_serde]
pub struct Action {
    pub action_value: Option<i8>,
    pub resulting_position: Position,
}

#[cw_serde]
pub struct PlayByPlay {
    pub starting_position: Position,
    pub actions: Vec<Action>,
}

#[cw_serde]
pub struct Step {
    pub car_id: u128,
    pub steps_taken: u32,
}

#[cw_serde]
pub struct RaceResult {
    pub race_id: String,
    pub track_id: Uint128,
    pub car_ids: Vec<u128>,
    pub winner_ids: Vec<u128>,
    pub rankings: Vec<Rank>,
    pub play_by_play: HashMap<u128, PlayByPlay>,
    pub steps_taken: Vec<Step>,
}



#[cw_serde]
pub struct CarState {
    pub car_id: u128,
    pub tile: TrackTile,
    pub x: i32,
    pub y: i32,
    pub stuck: bool,
    pub finished: bool,
    pub steps_taken: u32,
    pub last_action: usize,
    // **NEW**: Track wall collisions for reward calculation
    pub hit_wall: bool,
    // **NEW**: Track speed modifiers
    pub current_speed: u32,
    // **NEW**: Integer-based action history for compressed state representation
    pub integer_action_history: Vec<(u32, usize, TrackTile)>, // (integer_state_hash, action, tile)
    // **NEW**: Store used integer Q-table for this car
    pub integer_q_table: Vec<IntegerQTableEntry>, 
}

#[cw_serde]
pub struct RaceState {
    pub cars: Vec<CarState>,
    pub track_layout: Vec<Vec<TrackTile>>,
    pub tick: u32,
    pub play_by_play: std::collections::HashMap<u128, PlayByPlay>,
    pub rps_winners: Vec<u128>,
    pub rps_failures: u32,
}


#[cw_serde]
pub struct Config {
    pub admin: String,
    pub track_contract: String,
    pub car_contract: String,
    pub max_ticks: u32,
    pub max_recent_races: u32,
    pub byte_minter_contract: Option<String>,
    pub rps_engine_contract: Option<String>,
    /// Maximum number of brain progress entries to keep per car
    pub brain_progress_entry_limit: Option<u32>,
} 

#[cw_serde]
pub struct TrainingConfig {
    pub training_mode: bool,
    pub epsilon: Decimal,
    pub temperature: Decimal,
    pub enable_epsilon_decay: bool,
}


#[cw_serde]
pub struct MigrateMsg {}

/// Response for brain progress queries
#[cw_serde]
pub struct BrainProgressResponse {
    pub car_id: u128,
    pub brain_progress: BrainProgress,
}