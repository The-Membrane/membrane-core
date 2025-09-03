use cosmwasm_std::{StdError, StdResult, Storage};
use cw_storage_plus::{Item, Map};
use serde::{Deserialize, Serialize};

use membrane::race_engine::{Config, RaceResult};
use membrane::types::{TrackTrainingStats, TrainingStats, TopTimes, TopTimeEntry, IntegerQTableEntry, StateHashConversion, PendingQUpdate};

pub const CONFIG: Item<Config> = Item::new("config");
pub const CAR_RECENT_RACES: Map<u128, Vec<RaceResult>> = Map::new("car_recent_races");
pub const TRACK_RECENT_RACES: Map<u128, Vec<RaceResult>> = Map::new("track_recent_races");

// Constants
pub const MAX_CAR_RECENT_RACES: usize = 9;
pub const MAX_TRACK_RECENT_RACES: usize = 32;
pub const MAX_TICKS: u32 = 100;


// Original Q-table storage: (car_id, state_hash) -> [i32; 4] action values (for migration)
pub const Q_TABLE: Map<(u128, &[u8; 32]), [i32; 4]> = Map::new("q_table");

// Integer Q-table storage: (car_id, state_hash) -> [i8; 4] action values (compressed)
pub const INTEGER_Q_TABLE: Map<(u128, u32), [i8; 4]> = Map::new("integer_q_table");

// **NEW**: Pending Q-table updates storage: (car_id, update_id) -> PendingQUpdate
pub const PENDING_Q_UPDATES: Map<(u128, u64), PendingQUpdate> = Map::new("pending_q_updates");

// **NEW**: Car update counter: car_id -> next_update_id
pub const CAR_UPDATE_COUNTER: Map<u128, u64> = Map::new("car_update_counter");

// Training stats storage: (car_id, track_id) -> TrackTrainingStats
pub const CAR_TRACK_TRAINING_STATS: Map<(u128, u128), TrackTrainingStats> = Map::new("car_track_training_stats");


pub const MAX_TOP_TIMES: usize = 100;
pub const TRACK_TOP_TIMES: Map<u128, TopTimes> = Map::new("track_top_times");

// Original Q-table functions (for migration)
pub fn get_q_values(storage: &dyn Storage, car_id: u128, state_hash: &[u8; 32]) -> StdResult<[i32; 4]> {
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

// Integer Q-table functions
pub fn get_integer_q_values(storage: &dyn Storage, car_id: u128, state_hash: u32) -> StdResult<[i8; 4]> {
    INTEGER_Q_TABLE.load(storage, (car_id, state_hash))
}

pub fn set_integer_q_values(
    storage: &mut dyn Storage,
    car_id: u128,
    state_hash: u32,
    q_values: [i8; 4],
) -> StdResult<()> {
    INTEGER_Q_TABLE.save(storage, (car_id, state_hash), &q_values)
}

// **NEW**: Pending Q-table update functions
pub fn add_pending_q_update(
    storage: &mut dyn Storage,
    car_id: u128,
    mut update: PendingQUpdate,
) -> StdResult<u64> {
    // Get next update ID for this car
    let next_id = CAR_UPDATE_COUNTER.load(storage, car_id).unwrap_or(0) + 1;
    CAR_UPDATE_COUNTER.save(storage, car_id, &next_id)?;
    
    // Set the creation timestamp if not already set
    if update.created_at == 0 {
        // Use a simple timestamp based on the update ID for now
        // In a real implementation, this would use env.block.time.seconds()
        update.created_at = next_id as u32;
    }
    
    // Store the pending update
    PENDING_Q_UPDATES.save(storage, (car_id, next_id), &update)?;
    
    Ok(next_id)
}

pub fn get_pending_updates(
    storage: &dyn Storage,
    car_id: u128,
    limit: Option<u32>,
) -> StdResult<Vec<(u64, PendingQUpdate)>> {
    let limit = limit.unwrap_or(50); // Default batch size
    
    let updates: Vec<(u64, PendingQUpdate)> = PENDING_Q_UPDATES
        .prefix(car_id)
        .range(storage, None, None, cosmwasm_std::Order::Ascending)
        .take(limit as usize)
        .map(|item| {
            let (update_id, update) = item.map_err(|e| StdError::generic_err(e.to_string()))?;
            Ok((update_id, update))
        })
        .collect::<StdResult<Vec<_>>>()?;
    
    Ok(updates)
}

pub fn remove_pending_updates(
    storage: &mut dyn Storage,
    car_id: u128,
    update_ids: Vec<u64>,
) -> StdResult<()> {
    for update_id in update_ids {
        PENDING_Q_UPDATES.remove(storage, (car_id, update_id));
    }
    
    // Check if car still has pending updates
    let remaining_updates = PENDING_Q_UPDATES
        .prefix(car_id)
        .range(storage, None, None, cosmwasm_std::Order::Ascending)
        .next();
    
    if remaining_updates.is_none() {
        // CARS_WITH_PENDING_UPDATES.remove(storage, car_id); // This line is removed
    }
    
    Ok(())
}

pub fn has_pending_updates(storage: &dyn Storage, car_id: u128) -> StdResult<bool> {
    // Use range query to check if car has any pending updates
    let has_updates = PENDING_Q_UPDATES
        .prefix(car_id)
        .range(storage, None, None, cosmwasm_std::Order::Ascending)
        .next()
        .is_some();
    
    Ok(has_updates)
}

/// **NEW**: Batch process pending updates for gas efficiency
pub fn batch_process_pending_updates(
    storage: &mut dyn Storage,
    car_id: u128,
    batch_size: Option<u32>,
) -> StdResult<(u32, Vec<u64>)> {
    let batch_size = batch_size.unwrap_or(50);
    
    // Get pending updates
    let updates = get_pending_updates(storage, car_id, Some(batch_size))?;
    let mut processed_ids = vec![];
    
    // Process updates in memory first
    let mut q_updates: std::collections::HashMap<u32, [i8; 4]> = std::collections::HashMap::new();
    
    for (update_id, pending_update) in updates {
        // Get current Q-values
        let mut current_q_values = get_integer_q_values(storage, car_id, pending_update.state_hash)
            .unwrap_or([0, 0, 0, 0]);
        
        // Apply Q-learning update (simplified for gas efficiency)
        let action = pending_update.action as usize;
        let old_value = current_q_values[action] as f32;
        let reward = pending_update.reward as f32;
        
        // Simplified Q-learning update
        let new_value = (old_value * 0.9 + reward * 0.1).round() as i32;
        current_q_values[action] = new_value.clamp(-128, 127) as i8;
        
        // Store for batch write
        q_updates.insert(pending_update.state_hash, current_q_values);
        processed_ids.push(update_id);
    }
    
    // Batch write all Q-value updates
    for (state_hash, q_values) in q_updates {
        set_integer_q_values(storage, car_id, state_hash, q_values)?;
    }
    
    // Remove processed updates
    remove_pending_updates(storage, car_id, processed_ids.clone())?;
    
    Ok((processed_ids.len() as u32, processed_ids))
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
        races.remove(0);
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

// Helper: recompute the highest entry (the slowest time among current top entries)
fn recompute_highest_with_index(entries: &Vec<TopTimeEntry>) -> (Option<TopTimeEntry>, Option<u16>) {
    if entries.is_empty() { return (None, None); }
    let mut worst_idx: usize = 0;
    let mut worst_time: u16 = entries[0].time;
    for (i, e) in entries.iter().enumerate() {
        if e.time > worst_time {
            worst_time = e.time;
            worst_idx = i;
        }
    }
    (
        Some(TopTimeEntry { car_id: entries[worst_idx].car_id, time: entries[worst_idx].time }),
        Some(worst_idx as u16),
    )
}

pub fn get_track_top_times(storage: &dyn Storage, track_id: u128) -> StdResult<TopTimes> {
    TRACK_TOP_TIMES.load(storage, track_id)
}

/// Update the top-N times for a track with a new (car_id, time)
/// - Only one entry per car is allowed
/// - Keep at most MAX_TOP_TIMES entries
/// - Unordered storage; maintain a cached `highest` entry (slowest time) for quick thresholding
pub fn update_track_top_times(storage: &mut dyn Storage, track_id: u128, car_id: u128, time: u32) -> StdResult<TopTimes> {
    let mut top = TRACK_TOP_TIMES.load(storage, track_id).unwrap_or(TopTimes { entries: vec![], highest: None, highest_index: None, car_index: std::collections::BTreeMap::new() });
    let time_u16: u16 = time as u16;

    // If car already exists, update only if improved (lower time)
    if let Some(&idx_u16) = top.car_index.get(&car_id) {
        let idx = idx_u16 as usize;
        if let Some(existing) = top.entries.get_mut(idx) {
            if time_u16 < existing.time {
                existing.time = time_u16;
                // If this was the highest, we must recompute; otherwise no change to highest
                if top.highest_index.map(|i| i as usize) == Some(idx) {
                    let (new_highest, new_idx) = recompute_highest_with_index(&top.entries);
                    top.highest = new_highest;
                    top.highest_index = new_idx;
                }
                TRACK_TOP_TIMES.save(storage, track_id, &top)?;
            }
        }
        return Ok(top);
    }

    // Car not present: if capacity not full, push
    if top.entries.len() < MAX_TOP_TIMES {
        let new_idx = top.entries.len();
        top.entries.push(TopTimeEntry { car_id, time: time_u16 });
        top.car_index.insert(car_id, new_idx as u16);
        // Update highest caches: if empty or new time is slower than current highest
        match (&top.highest, top.highest_index) {
            (Some(h), Some(h_idx)) => {
                if time_u16 > h.time {
                    top.highest = Some(TopTimeEntry { car_id, time: time_u16 });
                    top.highest_index = Some(new_idx as u16);
                }
            }
            _ => {
                // First entry
                top.highest = Some(TopTimeEntry { car_id, time: time_u16 });
                top.highest_index = Some(new_idx as u16);
            }
        }
        TRACK_TOP_TIMES.save(storage, track_id, &top)?;
        return Ok(top);
    }

    // Capacity full: check against current worst (largest time)
    let should_insert = match &top.highest {
        Some(highest) => time_u16 < highest.time,
        None => true,
    };

    if should_insert {
        // Replace the current worst entry with the new one
        let worst_idx = top.highest_index.map(|i| i as usize)
            .unwrap_or_else(|| top.entries.iter().enumerate().max_by_key(|(_, e)| e.time).map(|(i, _)| i).unwrap_or(0));

        // Remove old car index mapping for the replaced entry
        let old_car_id = top.entries[worst_idx].car_id;
        top.car_index.remove(&old_car_id);

        // Insert new entry in place
        top.entries[worst_idx] = TopTimeEntry { car_id, time: time_u16 };
        top.car_index.insert(car_id, worst_idx as u16);

        // Recompute highest caches after replacement
        let (new_highest, new_idx) = recompute_highest_with_index(&top.entries);
        top.highest = new_highest;
        top.highest_index = new_idx;
        TRACK_TOP_TIMES.save(storage, track_id, &top)?;
    }

    Ok(top)
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
                first_time: u32::MAX,
            },
            pvp: TrainingStats {
                tally: 0,
                win_rate: 0,
                fastest: u32::MAX,
                first_time: u32::MAX,
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

    // Update first time completion
    if stats.solo.first_time == u32::MAX {
        stats.solo.first_time = completion_time;
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
                first_time: u32::MAX,
            },
            pvp: TrainingStats {
                tally: 0,
                win_rate: 0,
                fastest: u32::MAX,
                first_time: u32::MAX,
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

    // Update first time completion
    if stats.pvp.first_time == u32::MAX {
        stats.pvp.first_time = completion_time;
    }
    
    CAR_TRACK_TRAINING_STATS.save(storage, (car_id, track_id), &stats)?;
    Ok(stats)
}

pub fn update_fastest_time(storage: &mut dyn Storage, car_id: u128, track_id: u128, completion_time: u32) -> StdResult<()> {
    let mut stats = CAR_TRACK_TRAINING_STATS.load(storage, (car_id, track_id))
        .unwrap_or_else(|_| TrackTrainingStats {
            solo: TrainingStats {
                tally: 0,
                win_rate: 0,
                fastest: u32::MAX,
                first_time: u32::MAX,
            },
            pvp: TrainingStats {
                tally: 0,
                win_rate: 0,
                fastest: u32::MAX,
                first_time: u32::MAX,
            },
        });

    if completion_time < stats.solo.fastest {
        stats.solo.fastest = completion_time;
    }

    if completion_time < stats.pvp.fastest {
        stats.pvp.fastest = completion_time;
    }

    CAR_TRACK_TRAINING_STATS.save(storage, (car_id, track_id), &stats)?;
    Ok(())
}
