use cosmwasm_std::{StdError, StdResult, Storage};
use cw_storage_plus::{Item, Map};
use serde::{Deserialize, Serialize};

use racing::race_engine::{Config, RaceResult};
use racing::types::{TrackTrainingStats, TrainingStats};

pub const CONFIG: Item<Config> = Item::new("config");
pub const CAR_RECENT_RACES: Map<u128, Vec<RaceResult>> = Map::new("car_recent_races");
pub const TRACK_RECENT_RACES: Map<u128, Vec<RaceResult>> = Map::new("track_recent_races");

// Constants
pub const MAX_CAR_RECENT_RACES: usize = 9;
pub const MAX_TRACK_RECENT_RACES: usize = 32;
pub const MAX_TICKS: u32 = 100;


// Q-table storage: (car_id, state_hash) -> [i32; 4] action values
pub const Q_TABLE: Map<(u128, &[u8; 32]), [i32; 4]> = Map::new("q_table");

// Training stats storage: (car_id, track_id) -> TrackTrainingStats
pub const CAR_TRACK_TRAINING_STATS: Map<(u128, u128), TrackTrainingStats> = Map::new("car_track_training_stats");

pub fn get_q_values(storage: &dyn Storage, car_id: u128, state_hash: & [u8; 32]) -> StdResult<[i32; 4]> {
    Q_TABLE.load(storage, (car_id, state_hash))
}

pub fn set_q_values(
    storage: &mut dyn Storage,
    car_id: u128,
    state_hash: &[u8; 32],
    q_values: [i32; 4],
) -> StdResult<()> {
    Q_TABLE.save(storage, (car_id, state_hash), &q_values)
}


pub fn get_config(storage: &dyn cosmwasm_std::Storage) -> StdResult<Config> {
    CONFIG.load(storage)
}

pub fn set_config(storage: &mut dyn cosmwasm_std::Storage, config: Config) -> StdResult<()> {
    CONFIG.save(storage, &config)
}

pub fn get_recent_races(storage: &dyn cosmwasm_std::Storage, car_id: Option<u128>, track_id: Option<u128>) -> StdResult<Vec<RaceResult>> {
    if let Some(car_id) = car_id {
        CAR_RECENT_RACES.load(storage, car_id)
    } else if let Some(track_id) = track_id {
        TRACK_RECENT_RACES.load(storage, track_id)
    } else {
        return Err(StdError::generic_err("No car or track ID provided"));
    }
}

pub fn add_recent_race(storage: &mut dyn cosmwasm_std::Storage, race_result: RaceResult, car_id: Option<u128>, track_id: Option<u128>) -> StdResult<()> {
    let mut races = if let Some(car_id) = car_id.clone() {
        CAR_RECENT_RACES.load(storage, car_id).unwrap_or_default()
    } else if let Some(track_id) = track_id.clone() {
        TRACK_RECENT_RACES.load(storage, track_id).unwrap_or_default()
    } else {
        return Err(StdError::generic_err("No car or track ID provided"));
    };
    
    races.push(race_result);

    //Set max length
    let max: usize = if let Some(_) = car_id {
        MAX_CAR_RECENT_RACES
    } else if let Some(_) = track_id {
        MAX_TRACK_RECENT_RACES
    } else {
        return Err(StdError::generic_err("No car or track ID provided"));
    };
    
    
    // Keep only the most recent races
    if races.len() > max {
        races = races.into_iter().rev().take(max).collect();
        races.reverse();
    }
    
    if let Some(car_id) = car_id {
        CAR_RECENT_RACES.save(storage, car_id, &races)?;
    } else if let Some(track_id) = track_id {
        TRACK_RECENT_RACES.save(storage, track_id, &races)?;
    } else {
        return Err(StdError::generic_err("No car or track ID provided"));
    }
    
    Ok(())
}

// Training stats functions
pub fn get_track_training_stats(storage: &dyn Storage, car_id: u128, track_id: u128) -> StdResult<TrackTrainingStats> {
    CAR_TRACK_TRAINING_STATS.load(storage, (car_id, track_id))
}

pub fn set_track_training_stats(
    storage: &mut dyn Storage,
    car_id: u128,
    track_id: u128,
    stats: TrackTrainingStats,
) -> StdResult<()> {
    CAR_TRACK_TRAINING_STATS.save(storage, (car_id, track_id), &stats)
}

pub fn update_solo_training_stats(
    storage: &mut dyn Storage,
    car_id: u128,
    track_id: u128,
    won: bool,
    completion_time: u32,
) -> StdResult<TrackTrainingStats> {
    let mut stats = CAR_TRACK_TRAINING_STATS.load(storage, (car_id, track_id))
        .unwrap_or_else(|_| TrackTrainingStats {
            solo: TrainingStats {
                tally: 0,
                win_rate: 0,
                fastest: u32::MAX,
            },
            pvp: TrainingStats {
                tally: 0,
                win_rate: 0,
                fastest: u32::MAX,
            },
        });
    
    // Update solo stats
    stats.solo.tally += 1;
    
    // Calculate new win rate
    let total_wins = (stats.solo.win_rate * (stats.solo.tally - 1)) / 100;
    let new_wins = if won { total_wins + 1 } else { total_wins };
    stats.solo.win_rate = (new_wins * 100) / stats.solo.tally;
    
    // Update fastest time if this run was faster
    if completion_time < stats.solo.fastest {
        stats.solo.fastest = completion_time;
    }
    
    CAR_TRACK_TRAINING_STATS.save(storage, (car_id, track_id), &stats)?;
    Ok(stats)
}

pub fn update_pvp_training_stats(
    storage: &mut dyn Storage,
    car_id: u128,
    track_id: u128,
    won: bool,
    completion_time: u32,
) -> StdResult<TrackTrainingStats> {
    let mut stats = CAR_TRACK_TRAINING_STATS.load(storage, (car_id, track_id))
        .unwrap_or_else(|_| TrackTrainingStats {
            solo: TrainingStats {
                tally: 0,
                win_rate: 0,
                fastest: u32::MAX,
            },
            pvp: TrainingStats {
                tally: 0,
                win_rate: 0,
                fastest: u32::MAX,
            },
        });
    
    // Update PvP stats
    stats.pvp.tally += 1;
    
    // Calculate new win rate
    let total_wins = (stats.pvp.win_rate * (stats.pvp.tally - 1)) / 100;
    let new_wins = if won { total_wins + 1 } else { total_wins };
    stats.pvp.win_rate = (new_wins * 100) / stats.pvp.tally;
    
    // Update fastest time if this run was faster
    if completion_time < stats.pvp.fastest {
        stats.pvp.fastest = completion_time;
    }
    
    CAR_TRACK_TRAINING_STATS.save(storage, (car_id, track_id), &stats)?;
    Ok(stats)
}
