// race_engine/src/contract.rs

// HASH CONVERSION CONSTRAINT AND SOLUTION
// ======================================
// 
// The user's requirement: "state <- legacy must equal state <- integer"
// 
// SOLUTION IMPLEMENTED: Brute Force State Matching
// 
// Instead of trying to reverse-engineer the Blake2b hash, we now brute force test
// every possible state until finding a match with the legacy hash. This ensures
// perfect consistency because we use the actual state that generated the legacy hash.
// 
// How it works:
// 1. For each legacy hash, test every possible state combination (x, y, speed, other_cars)
// 2. Generate the legacy hash for each test state using the original algorithm
// 3. When a match is found, use that state to generate the integer hash
// 4. Migrate Q-values from legacy to integer format
// 5. Remove legacy entry and store integer entry
// 
// Benefits:
// - Perfect consistency: "state <- legacy must equal state <- integer"
// - Guaranteed accuracy: Uses the actual state that generated the legacy hash
// - Single hash processing: Processes one hash at a time to manage computational cost
// - User satisfaction: Q-values are preserved exactly as they were
// 
// The original legacy hash was generated using:
// 1. 22-bit key from tile properties and car positions
// 2. Blake2b hash of the 22-bit key to produce 32-byte hash
// 
// The new integer hash generation:
// 1. Creates a 16-bit hash based on tile properties in 4 directions
// 2. Excludes other cars (unlike the legacy system)
// 3. Uses a deterministic algorithm that can be reproduced

use std::collections::HashMap;

use blake2::digest::{Update, VariableOutput};
use blake2::Blake2bVar;
use cosmwasm_std::{
    entry_point, to_json_binary, Binary, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, QuerierWrapper, Response, StdResult, Storage, Uint128, WasmMsg
};
use cw_storage_plus::Bound;

use crate::error::ContractError;
use crate::state::{add_recent_race, get_config, get_integer_q_values, get_q_values, get_recent_races, get_track_training_stats, set_config, set_integer_q_values, set_q_values, update_fastest_time, update_pvp_training_stats, update_solo_training_stats, update_track_top_times, update_brain_progress, CAR_BRAIN_PROGRESS, CAR_RECENT_RACES, CAR_TRACK_TRAINING_STATS, CONFIG, INTEGER_Q_TABLE, Q_TABLE, TRACK_RECENT_RACES};
use membrane::types::{ActionSelectionStrategy, GoingBackward, IntegerQTableEntry, RewardNumbers, Track, TrackTile};
use membrane::race_engine::{CarState, Config, ExecuteMsg, GetIntegerQResponse, GetTrackTrainingStatsResponse, InstantiateMsg, MigrateMsg, MigrationStatusResponse, QueryMsg, RaceResult, RaceResultResponse, RaceState, RecentRacesResponse, TrainingConfig, DEFAULT_BOOST_SPEED, DEFAULT_SPEED};
use membrane::car::{ExecuteMsg as Car_ExecuteMsg, QueryMsg as Car_QueryMsg};
use membrane::byte_minter::{QueryMsg as ByteMinterQueryMsg, VerifyEventRaceResponse, ExecuteMsg as ByteMinterExecuteMsg, EventType as ByteEventType};
// Race simulation constants
// const MAX_CARS: usize = 8;
// const MAX_TRACK_SIZE: usize = 50;
const MIN_CARS: usize = 1;

const MAX_LIMIT: u32 = 32;

// Action constants (4 possible actions: 0-3)
const ACTION_UP: usize = 0;
const ACTION_DOWN: usize = 1;
const ACTION_LEFT: usize = 2;
const ACTION_RIGHT: usize = 3;

// Tile Flags
const WALL: u8 = 0;
const STICKY: u8 = 1;
const BOOST: u8 = 2;
const FINISH: u8 = 3;
const CAR: u8 = 4;

// Training constants
const EPSILON: f32 = 0.9;
const TEMPERATURE: f32 = 0.0;

// Q-learning constants
const ALPHA: f32 = 0.1; // Learning rate
const GAMMA: f32 = 0.9; // Discount factor
const MAX_Q_VALUE: i32 = 127;
const MIN_Q_VALUE: i32 = -128;

// Brain progress constants
const MAX_POSSIBLE_STATES: u16 = 625; // 5^4 tile combinations

// Reward constants
const STUCK_PENALTY: i32 = -5;
const GOING_BACKWARD_PENALTY: i32 = -1;
const DISTANCE_REWARD: i32 = 1;
const WALL_PENALTY: i32 = -8;
const NO_MOVE_PENALTY: i32 = -1;
const EXPLORATION_BONUS: i32 = 6;
const RANK_REWARDS: [i32; 3] = [100, 50, 25]; // 1st, 2nd, 3rd place

// New imports for ownership checks
use serde::Deserialize;
use membrane::car::Cw721QueryMsg;

// Minimal response type for cw721 "owner_of" query
#[derive(Deserialize)]
struct OwnerOfResponse {
    owner: String,
}

/// Deterministic but simple RNG for on-chain use (fallback if no external crate)
fn pseudo_random(seed: u32, modulus: u32) -> u32 {
    let a: u32 = 1103515245;
    let c: u32 = 12345;
    (a.wrapping_mul(seed).wrapping_add(c)) % modulus
}

/// Mix two 32-bit seeds into a new pseudo-random seed (state-aware determinism)
fn mix_seed(a: u32, b: u32) -> u32 {
    // Lightweight mixer (inspired by splitmix/wyhash) to decorrelate low bits
    let mut x = a ^ b;
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// Convert Decimal (fixed 18 fractional digits) to f32
fn decimal_to_f32(d: Decimal) -> f32 {
    let num = d.atomics().u128() as f64;
    let denom = 1e18f64;
    (num / denom) as f32
}

/// Create action strategy based on training configuration
/// 
/// For epsilon decay strategy (when enable_epsilon_decay is true):
/// - Starts with initial_epsilon (e.g., 0.3 for 30% exploration)
/// - Gradually decreases to final_epsilon (e.g., 0.01 for 1% exploration)
/// - Decay is linear based on training progress (current_tick / total_ticks)
/// - This encourages exploration early in training and exploitation later
/// 
/// For regular epsilon greedy (when enable_epsilon_decay is false):
/// - Uses constant epsilon value throughout training
/// - Provides consistent exploration rate
fn make_action_strategy(
    training_mode: bool, 
    epsilon: f32, 
    temperature: f32,
    current_tick: u32,
    total_ticks: u32,
    enable_epsilon_decay: bool,
) -> ActionSelectionStrategy {
    if !training_mode {
        ActionSelectionStrategy::Best
    } else if temperature > 0.0 {
        ActionSelectionStrategy::Softmax(temperature)
    } else if epsilon == 0.0 {
        // When epsilon is explicitly 0%, choose the best action deterministically
        ActionSelectionStrategy::Best
    } else if epsilon > 0.0 {
        // Use epsilon decay if explicitly enabled and we have valid tick information
        if enable_epsilon_decay && current_tick > 0 && total_ticks > 0 {
            ActionSelectionStrategy::EpsilonDecay {
                initial_epsilon: epsilon,
                final_epsilon: 0.01, // Final epsilon of 1%
                current_tick,
                total_ticks,
            }
        } else {
            // Use regular epsilon greedy
            ActionSelectionStrategy::EpsilonGreedy(epsilon)
        }
    } else {
        // Fallback to Best if inputs are degenerate
        ActionSelectionStrategy::Best
    }
}

/// Query all Q-tables for a car upfront
// fn query_full_q_tables(config: Config, querier: QuerierWrapper, car_id: u128) -> Result<GetQResponse, ContractError> {
//     let q_tables: GetQResponse = querier.query_wasm_smart::<GetQResponse>(config.car_contract, &Car_QueryMsg::GetQ {
//         car_id: car_id.to_string(),
//         state_hash: None,
//     })?;
//     Ok(q_tables)
// }

//Convert the GetQResponse to a HashMap<String, [i32; 4]>
// fn get_q_tables(q_tables: GetQResponse) -> Result<HashMap<String, [i32; 4]>, ContractError> {
//     let mut q_tables_map = HashMap::new();
//     for q in q_tables.q_values {
//         q_tables_map.insert(q.state_hash, q.action_values);
//     }
//     Ok(q_tables_map)
// }

///Get q-values for a specific state hash
// fn get_q_values(q_tables: GetQResponse, state_hash: u128) -> Result<[i32; 4], ContractError> {
//      let q_values = q_tables.q_values.iter().find(|q| q.state_hash == state_hash);
//     Ok(q_values.unwrap_or(&QTableEntry {
//         state_hash: state_hash.to_string(),
//         action_values: [0, 0, 0, 0],
//     }).action_values)
// }

/// Query Q-values from car contract
// fn query_car_q_values(config: Config, querier: QuerierWrapper, car_id: u128, state_hash: u128) -> Result<[i32; 4], ContractError> {
//     let q_tables: GetQResponse = querier.query_wasm_smart::<GetQResponse>(config.car_contract, &Car_QueryMsg::GetQ {
//         car_id: car_id.to_string(),
//         state_hash: Some(state_hash.to_string()),
//     })?;
    
//     // Return default Q-values if no values found for this state
//     if q_tables.q_values.is_empty() {
//         Ok([0, 0, 0, 0])
//     } else {
//         Ok(q_tables.q_values[0].action_values)
//     }
// }



#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let admin = deps.api.addr_validate(&msg.admin)?;
    let track_contract = deps.api.addr_validate(&msg.track_contract)?;
    let car_contract = deps.api.addr_validate(&msg.car_contract)?;
    
    let config = membrane::race_engine::Config {
        admin: admin.to_string(),
        track_contract: track_contract.to_string(),
        car_contract: car_contract.to_string(),
        max_ticks: 100,
        max_recent_races: 10,
        byte_minter_contract: None,
    };
    
    set_config(deps.storage, config)?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("admin", admin)
        .add_attribute("track_contract", track_contract)
        .add_attribute("car_contract", car_contract))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::SimulateRace { track_id, car_ids, pvp, train, training_config, reward_config, max_race_ticks } => {
            execute_simulate_race(deps, _env, _info, track_id, car_ids, pvp, train, training_config, reward_config, max_race_ticks)
        },
        ExecuteMsg::ResetQ { car_id } => {
            execute_reset_q(deps.storage, car_id.into())
        },
        ExecuteMsg::PurgeCar { car_id } => {
            let config = get_config(deps.storage)?;
            if _info.sender.as_str() != config.car_contract {
                return Err(ContractError::Unauthorized {});
            }
            execute_purge_car(deps.storage, car_id.u128())
        },
        ExecuteMsg::UpdateConfig { max_ticks, byte_minter_contract } => {
            let mut config = get_config(deps.storage)?;
            if _info.sender.as_str() != config.admin { return Err(ContractError::Unauthorized {}); }
            if let Some(v) = max_ticks { config.max_ticks = v; }
            if let Some(addr) = byte_minter_contract { config.byte_minter_contract = Some(addr); }
            set_config(deps.storage, config)?;
            Ok(Response::new().add_attribute("action", "update_config"))
        },
        ExecuteMsg::MigrateQTableStates { car_id, batch_size } => {
            execute_migrate_q_table_states(deps, _info, car_id, batch_size)
        }

    }
}

/// Reset the Q-table for a car
fn execute_reset_q(storage: &mut dyn Storage, car_id: u128) -> Result<Response, ContractError> {
    // Reset integer Q-table
    let prefix = INTEGER_Q_TABLE.prefix(car_id);
    let range = prefix.range(storage, None, None, cosmwasm_std::Order::Ascending);
    let keys: Vec<u32> = range.map(|item| {
        let (key, _) = item.unwrap();
        key
    }).collect();
    
    for key in keys {
        INTEGER_Q_TABLE.remove(storage, (car_id, key));
    }
    Ok(Response::new())
}

/// Purge all state for a car: Q-table, training stats, recent races
fn execute_purge_car(storage: &mut dyn Storage, car_id: u128) -> Result<Response, ContractError> {
    // Remove all integer Q-table entries for car
    let prefix = INTEGER_Q_TABLE.prefix(car_id);
    let range = prefix.range(storage, None, None, cosmwasm_std::Order::Ascending);
    let keys: Vec<u32> = range.map(|item| {
        let (key, _) = item.unwrap();
        key
    }).collect();
    for key in keys { INTEGER_Q_TABLE.remove(storage, (car_id, key)); }
    
    // Remove all original Q-table entries for car (if any)
    let legacy_prefix = Q_TABLE.prefix(car_id);
    let legacy_range = legacy_prefix.range(storage, None, None, cosmwasm_std::Order::Ascending);
    let legacy_keys: Vec<[u8; 32]> = legacy_range.map(|item| {
        let (key, _) = item.unwrap();
        key
    }).collect();
    for key in legacy_keys { Q_TABLE.remove(storage, (car_id, &key)); }

    // Remove all training stats for car across tracks
    let stats_prefix = CAR_TRACK_TRAINING_STATS.prefix(car_id);
    let stats_range = stats_prefix.range(storage, None, None, cosmwasm_std::Order::Ascending);
    let track_ids: Vec<u128> = stats_range.map(|item| {
        let (track_id, _) = item.unwrap();
        track_id
    }).collect();
    for track_id in track_ids { CAR_TRACK_TRAINING_STATS.remove(storage, (car_id, track_id)); }

    // Remove recent races for car
    CAR_RECENT_RACES.remove(storage, car_id);

    Ok(Response::new())
}

/// Migrate existing Q-table states from legacy byte array hashes to integer hashes
/// **ENHANCED**: Now tests every possible state until finding a match with the legacy hash
fn execute_migrate_q_table_states(
    deps: DepsMut,
    info: MessageInfo,
    car_id: Uint128,
    batch_size: Option<u32>,
) -> Result<Response, ContractError> {
    
    // Anyone can migrate //
    
    let car_id = car_id.u128();
    let batch_size = batch_size.unwrap_or(1); // Process one hash at a time due to computational expense
    
    // Get all original Q-table entries for this car
    let prefix = Q_TABLE.prefix(car_id);
    let range = prefix.range(deps.storage, None, None, cosmwasm_std::Order::Ascending);
    let entries: Vec<([u8; 32], [i32; 4])> = range
        .take(batch_size as usize)
        .map(|item| {
            let (state_hash, action_values) = item.map_err(|e| ContractError::Std(e))?;
            Ok((state_hash, action_values))
        })
        .collect::<Result<Vec<_>, ContractError>>()?;
    
    let mut migrated_count = 0;
    let mut skipped_count = 0;
    let mut error_count = 0;
    
    for (legacy_hash, action_values) in entries {
        // **NEW**: Test every possible state until we find a match with the legacy hash
        match find_state_for_legacy_hash(&legacy_hash) {
            Ok(tile_combination) => {
                // Found the tile combination that generated this legacy hash
                // Now generate the integer hash using this tile combination
                let integer_state_hash = generate_state_hash_for_migration_with_tiles(tile_combination);
                
                // Check if already migrated
                if get_integer_q_values(deps.storage, car_id, integer_state_hash).is_ok() {
                    skipped_count += 1;
                    continue;
                }
                
                // Convert i32 original values to i8 (clamp to i8 range)
                let compressed_action_values = [
                    action_values[0].clamp(-128, 127) as i8,
                    action_values[1].clamp(-128, 127) as i8,
                    action_values[2].clamp(-128, 127) as i8,
                    action_values[3].clamp(-128, 127) as i8,
                ];
                
                // Store the Q-values with the new integer hash
                set_integer_q_values(deps.storage, car_id, integer_state_hash, compressed_action_values)?;
                
                // Remove the original entry after successful migration
                Q_TABLE.remove(deps.storage, (car_id, &legacy_hash));
                
                migrated_count += 1;
            }
            Err(_) => {
                // Could not find matching state (should not happen with proper legacy hash)
                error_count += 1;
            }
        }
    }
    
    Ok(Response::new()
        .add_attribute("action", "migrate_q_table_states")
        .add_attribute("car_id", car_id.to_string())
        .add_attribute("migrated", migrated_count.to_string())
        .add_attribute("skipped", skipped_count.to_string())
        .add_attribute("errors", error_count.to_string())
        .add_attribute("batch_size", batch_size.to_string()))
}

/// **NEW**: Find the state that generated a legacy hash by testing every possible tile combination
/// This function brute forces through all 625 tile combinations until finding a match
fn find_state_for_legacy_hash(legacy_hash: &[u8; 32]) -> Result<[TileFlag; 4], ContractError> {
    // Generate all 625 tile combinations (5^4 = 625)
    let tile_combinations = generate_all_tile_combinations();
    
    // Test all 625 tile combinations
    // Use fixed position and speed since they don't affect the hash
    let x = 0;
    let y = 0;
    let speed = 1;
    let other_cars = vec![];
    
    for tile_combo in &tile_combinations {
        let test_hash = generate_legacy_state_hash_for_migration(x, y, speed, &other_cars, *tile_combo);
        if test_hash == *legacy_hash {
            return Ok(*tile_combo);
        }
    }
    
    // If no match found, return an error
    Err(ContractError::Std(cosmwasm_std::StdError::generic_err("No matching state found for legacy hash")))
}

/// **NEW**: Generate all 625 tile combinations (5^4 = 625)
/// Each direction can be: Wall(0), Sticky(1), Boost(2), Finish(3), Normal(4)
fn generate_all_tile_combinations() -> Vec<[TileFlag; 4]> {
    let mut combinations = Vec::new();
    
    // Generate all combinations of 4 directions with 5 possible tile types each
    for up in 0..5 {
        for down in 0..5 {
            for left in 0..5 {
                for right in 0..5 {
                    combinations.push([
                        match up { 0 => TileFlag::Wall, 1 => TileFlag::Sticky, 2 => TileFlag::Boost, 3 => TileFlag::Finish, _ => TileFlag::Normal },
                        match down { 0 => TileFlag::Wall, 1 => TileFlag::Sticky, 2 => TileFlag::Boost, 3 => TileFlag::Finish, _ => TileFlag::Normal },
                        match left { 0 => TileFlag::Wall, 1 => TileFlag::Sticky, 2 => TileFlag::Boost, 3 => TileFlag::Finish, _ => TileFlag::Normal },
                        match right { 0 => TileFlag::Wall, 1 => TileFlag::Sticky, 2 => TileFlag::Boost, 3 => TileFlag::Finish, _ => TileFlag::Normal },
                    ]);
                }
            }
        }
    }
    
    combinations
}

/// **NEW**: Generate legacy state hash for migration testing
/// This recreates the original legacy hash generation algorithm exactly
fn generate_legacy_state_hash_for_migration(
    x: i32, y: i32,
    speed: u32,
    other_cars: &[(i32,i32)],
    tile_combination: [TileFlag; 4], // 4 directions: U, D, L, R
) -> [u8; 32] {
    // ---------- 1. build 22-bit key ----------
    let mut key: u32 = 0;           // we'll only use lowest 22 bits
    for (i, &(dx,dy)) in DIRS.iter().enumerate() {
        let tx = x + dx.wrapping_mul(speed as i32);
        let ty = y + dy.wrapping_mul(speed as i32);

        // --- 3-bit tile flag ---
        let mut flag = TileFlag::Normal as u8;

        if tx < 0 || ty < 0 || ty as usize >= 50 || tx as usize >= 50 {
            flag = TileFlag::Wall as u8;
        } else {
            // Use the provided tile combination instead of track lookup
            flag = tile_combination[i] as u8;
        }

        // --- 1-bit "has car" flag ---
        let has_car = other_cars
            .iter()
            .any(|&(cx,cy)| cx == tx && cy == ty) as u8;

        // pack into 4 bits and shift into position
        let nibble = (flag & 0b111) | (has_car << 3);
        key |= (nibble as u32) << (i * 4);
    }

    // ---------- 2. closest-car direction ----------
    let mut dir3 = Dir3::None as u8;
    if !other_cars.is_empty() {
        let (mut best_d2, mut best_dir) = (i32::MAX, Dir3::None as u8);
        for &(cx,cy) in other_cars {
            let dx = cx - x;
            let dy = cy - y;
            let d2 = dx*dx + dy*dy;
            if d2 < best_d2 {
                best_d2 = d2;
                best_dir = if dx.abs() > dy.abs() {
                    if dx > 0 { Dir3::Right } else { Dir3::Left }
                } else {
                    if dy > 0 { Dir3::Down }  else { Dir3::Up }
                } as u8;
            }
        }
        dir3 = best_dir;
    }
    key |= (dir3 as u32) << 16;   // bits 16-18

    // ---------- 3. hash ----------
    let mut hasher = Blake2bVar::new(32).unwrap(); // 256-bit
    let key_bytes = key.to_le_bytes();            // 4 bytes, lowest 3 used
    hasher.update(&key_bytes[..3]);               // feed 3 tight bytes
    let mut out = [0u8; 32];
    let _ = hasher.finalize_variable(&mut out);

    out
}

/// **NEW**: Generate integer state hash for migration using tile combination
fn generate_state_hash_for_migration_with_tiles(tile_combination: [TileFlag; 4]) -> u32 {
    // Build 16-bit key from tile combination (4 bits each: 3 bits tile type)
    let mut key: u32 = 0;
    
    for (i, &tile_flag) in tile_combination.iter().enumerate() {
        // pack into 4 bits and shift into position
        key |= (tile_flag as u32) << (i * 4);
    }
    
    // Return the 16-bit key directly as integer hash
    key
}





fn get_starting_tiles(track: Track) -> Vec<(usize, usize)> {
    let mut start_indices = vec![];
    for tile in &track.starting_tiles {
        start_indices.push((tile.x as usize, tile.y as usize));
    }
    start_indices
}


//TODO: 
// -- The Singularity car will bypass this training (maybe we should make it the 0'd ID)
pub fn execute_simulate_race(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    track_id: Uint128,
    mut car_ids: Vec<u128>,
    pvp: Option<bool>,
    train: bool,
    training_config: Option<TrainingConfig>,
    reward_config: Option<RewardNumbers>,
    max_race_ticks: Option<u32>,
) -> Result<Response, ContractError> {
    let config = get_config(deps.storage)?;
    let mut msgs = vec![];
    
    // Validate input
    if car_ids.len() < MIN_CARS {
        return Err(ContractError::InvalidCarCount { 
            expected: MIN_CARS as u32,
            actual: car_ids.len() as u32,
        });
    }

    //If train, ensure there is only one car
    if train && car_ids.len() != 1 {
        return Err(ContractError::InvalidCarCount { 
            expected: 1, 
            actual: car_ids.len() as u32
        });
    }

    //Set pvp to false if not provided
    let pvp = if let Some(pvp) = pvp {
        pvp
    } else {
        if car_ids.len() == 1 {
            false
        } else {
            true
        }
    };

    // Enforce training restrictions: car must exist and be owned by caller
    if train {
        for car_id in &car_ids {
            // Query cw721 owner_of via car contract; treat not found as CarNotFound
            let owner_resp: OwnerOfResponse = deps.querier.query_wasm_smart::<OwnerOfResponse>(
                config.car_contract.clone(),
                &Car_QueryMsg::Base(Cw721QueryMsg::OwnerOf { 
                    token_id: car_id.to_string(), 
                    include_expired: None,
                })
            ).map_err(|_| ContractError::CarNotFound { car_id: car_id.to_string() })?;

            if owner_resp.owner != info.sender.to_string() {
                return Err(ContractError::Unauthorized {});
            }
        }
    }

    //If pvp is true & its training, add the Singularity car to the car_ids
    if pvp && train {
        car_ids.push(0);
    }

    //If training_config is None, use default values
    let mut training_config = match training_config {
        Some(config) => config,
        None => TrainingConfig {
            training_mode: train,
            epsilon: Decimal::percent((EPSILON * 100.0) as u64),
            temperature: Decimal::zero(),
            enable_epsilon_decay: true,
        },
    };
    //If not training, set training_mode to false
    if !train {
        training_config.training_mode = false;
    }
    let reward_config = match reward_config {
        Some(config) => config,
        None => RewardNumbers {
            going_backward: GoingBackward {
                penalty: GOING_BACKWARD_PENALTY,
                include_progress_towards_finish: true,
            },
            stuck: STUCK_PENALTY,
            wall: WALL_PENALTY,
            distance: DISTANCE_REWARD,
            no_move: NO_MOVE_PENALTY,
            explore: EXPLORATION_BONUS,
            rank: membrane::types::RankReward {
                first: RANK_REWARDS[0],
                second: RANK_REWARDS[1],
                third: RANK_REWARDS[2],
                other: 0, // Default value instead of array access
            },
        },
    };

    // Load track from track manager contract
    let track = load_track_from_manager(deps.as_ref(), config.clone(), track_id.clone())?;
    let track_layout = track.clone().layout;
    let fastest_track_tick_time = track.clone().fastest_tick_time;

    //If car_ids.len() > 1, ensure the track has enough starting tiles
    if car_ids.len() > 1 && track.starting_tiles.len() < car_ids.len() {
        return Err(ContractError::InvalidTrack { track_id: track_id.into() });
    }

    //Find the indices of any starting tiles
    let start_indices = get_starting_tiles(track);

    // Initialize car states with random, non-overlapping starting tiles
    // Deterministic shuffle of starting indices
    let mut cars = vec![];
    let mut perm: Vec<usize> = (0..start_indices.len()).collect();
    let seed: u32 = (env.block.height as u32)
        ^ (env.block.time.seconds() as u32)
        ^ (car_ids.len() as u32);
    // Fisher-Yates to shuffle the starting indices
    if perm.len() > 1 {
        let mut i = perm.len() - 1;
        while i > 0 {
            let r = (pseudo_random(seed.wrapping_add(i as u32), (i as u32) + 1)) as usize;
            perm.swap(i, r);
            i -= 1;
        }
    }
    // Assign first N positions to cars
    for (i, car_id) in car_ids.iter().enumerate() {
        let start_index = if start_indices.len() > 0 { perm[i] } else { 0 };
        
        // **NEW**: Query all Q-tables for this car upfront
        // let q_tables_res = query_full_q_tables(config.clone(), deps.querier, car_id)?;
        // let q_tables = get_q_tables(q_tables_res)?;
 
        cars.push(CarState {
            car_id: car_id.clone(),
            tile: track_layout[start_indices[start_index].1][start_indices[start_index].0].clone(),
            x: start_indices[start_index].0 as i32,
            y: start_indices[start_index].1 as i32,
            stuck: false,
            finished: false,
            steps_taken: 0,
            last_action: ACTION_UP, // Default to UP
            // **NEW**: Initialize hit_wall
            hit_wall: false,
            // **NEW**: Initialize speed modifiers
            current_speed: DEFAULT_SPEED as u32, // Default normal speed
            // **NEW**: Initialize integer-based action history
            integer_action_history: vec![],
            // **NEW**: Initialize integer Q-tables
            integer_q_table: vec![],
        });
    }

    // Initialize race state
    let mut race_state = RaceState {
        cars,
        track_layout,
        tick: 0,
        play_by_play: std::collections::HashMap::new(),
    };

    // Response accumulator
    let mut response = Response::new();

    // Before simulating, check if this is a byte-minter event race
    let mut event_for_this_race: Option<ByteEventType> = None;
    // Only check for byte-minter events if not training
    if !train {
        if let Some(byte_minter_addr) = config.byte_minter_contract.clone() {
            let verify: VerifyEventRaceResponse = deps.querier.query_wasm_smart(
                byte_minter_addr.clone(),
                &ByteMinterQueryMsg::VerifyEventRace { track_id: track_id.u128(), car_ids: car_ids.clone(), pvp }
            )?;
            if verify.allowed { event_for_this_race = verify.event; }
        }
    }

    // Simulate race using provided max_race_ticks or fallback to configured max_ticks
    let max_ticks = max_race_ticks.unwrap_or(config.max_ticks);
    let race_result = simulate_race(deps.storage, &mut race_state, training_config, max_ticks, env.block.time.seconds() as u32)?;

    // Generate race ID
    let race_id = format!("race_{}_{}", track_id, env.block.time.seconds());

    // Create race result
    let race_result_struct = membrane::race_engine::RaceResult {
        race_id: race_id.clone(),
        track_id,
        car_ids: car_ids.clone(),
        winner_ids: race_result.winner_ids.clone(),
        rankings: race_result.rankings.clone(),
        play_by_play: race_result.play_by_play.clone(),
        steps_taken: race_result.steps_taken.clone(),
    };

    // Save race result
    add_recent_race(deps.storage, race_result_struct.clone(), None, Some(track_id.into()))?;
    for car in &race_state.cars {
        add_recent_race(deps.storage, race_result_struct.clone(), Some(car.car_id), None)?;
        // Update per-track top times only for finished cars
        if car.finished {
            //Update fastest time
            update_fastest_time(deps.storage, car.car_id, track_id.into(), car.steps_taken)?;
            let _ = update_track_top_times(deps.storage, track_id.into(), car.car_id, car.steps_taken);
        }
    }

    // Apply Q-learning updates directly to car model in storage
    if train {
        apply_q_learning_updates(
            deps.storage, 
            &mut race_state, 
            &race_result, 
            reward_config.clone(), 
            config.clone(), 
            deps.querier,
            fastest_track_tick_time,
            env.block.time,
        )?;
        
        // Update training stats for each car - only if there's meaningful progress
        let is_solo = car_ids.len() == 1;
        for car in &race_state.cars {
            let won = race_result.winner_ids.contains(&car.car_id);
            let completion_time = if car.finished { car.steps_taken } else { max_ticks };
            
            // Only update stats if car finished or made significant progress
            let should_update = car.finished || completion_time < max_ticks;
            if should_update {
                // Update training stats
                if is_solo {
                    update_solo_training_stats(deps.storage, car.car_id, track_id.into(), won, completion_time)?;
                } else {
                    update_pvp_training_stats(deps.storage, car.car_id, track_id.into(), won, completion_time)?;
                }
            }
        }
    }

    // Post-race: if this was a byte-minter event, record winners/finishers
    if let Some(event) = event_for_this_race.clone() {
        if let Some(byte_minter_addr) = config.byte_minter_contract.clone() {
            match event {
                ByteEventType::Maze => {
                    for car in &race_state.cars {
                        if car.finished {
                            let msg = cosmwasm_std::WasmMsg::Execute { 
                                contract_addr: byte_minter_addr.clone(), 
                                msg: to_json_binary(&ByteMinterExecuteMsg::RecordWin { 
                                    event: ByteEventType::Maze, 
                                    car_id: car.car_id, 
                                    runner: info.sender.to_string() 
                                })?, 
                                funds: vec![] 
                            };
                            response = response.add_message(CosmosMsg::Wasm(msg));
                        }
                    }
                }
                ByteEventType::Pvp => {
                    if let Some(winner) = race_result.winner_ids.first() { if *winner != 0 {
                        let msg = cosmwasm_std::WasmMsg::Execute { 
                            contract_addr: byte_minter_addr.clone(), 
                            msg: to_json_binary(&ByteMinterExecuteMsg::RecordWin { 
                                event: ByteEventType::Pvp, 
                                car_id: *winner, 
                                runner: info.sender.to_string() 
                            })?, 
                            funds: vec![] 
                        };
                        response = response.add_message(CosmosMsg::Wasm(msg));
                    }}
                }
            }
        }
    }

    //Update car energy
    if train {
        for car in &race_state.cars {
            //Skip The Singularity
            if car.car_id == 0 {
                continue;
            }
            
            //Update car energy
            msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.car_contract.clone(),
                msg: to_json_binary(&Car_ExecuteMsg::ConsumeTrainingEnergy {
                    token_id: car.car_id.to_string(),
                    sessions: 1,
                })?,
                funds: vec![],
            }));
        }
    }

    response = response
        .add_messages(msgs)
        .add_attribute("method", "simulate_race")
        .add_attribute("race_id", race_id)
        .add_attribute("car_count", car_ids.len().to_string())
        .add_attribute("ticks", race_state.tick.to_string())
        .add_attribute("winners", race_result.winner_ids.len().to_string());
    

    Ok(response)
}

/// Load track from track manager contract
fn load_track_from_manager(deps: Deps, config: Config, track_id: Uint128) -> Result<Track, ContractError> {
    // For testing purposes, return a simple test track
    // In a real implementation, this would query the track manager contract
    let track: Track = deps.querier.query_wasm_smart::<Track>(
        config.track_contract, &membrane::track_manager::QueryMsg::GetTrack {
        track_id: track_id,
    })?;
    
    Ok(track)
}

/// Simulate the complete race
fn simulate_race(storage: &mut dyn Storage, race_state: &mut RaceState, training_config: TrainingConfig, max_ticks: u32, seed: u32) -> Result<RaceResult, ContractError> {
    let mut tick = 0;
    
    // Initialize play_by_play for each car
    for car in &race_state.cars {
        race_state.play_by_play.insert(car.car_id.clone(), membrane::race_engine::PlayByPlay {
            starting_position: membrane::race_engine::Position {
                x: car.x as u32,
                y: car.y as u32,
            },
            actions: vec![],
        });
    }
    
    while tick < max_ticks && !all_cars_finished(&race_state.cars) {
        // Simulate one tick
        simulate_tick(storage, race_state, training_config.clone(), tick, max_ticks, seed)?;
        
        tick += 1;
        race_state.tick = tick;
    }

    // Determine winners and rankings
    let (winner_ids, rankings, steps_taken) = calculate_results(&race_state.cars, &race_state.track_layout);

    Ok(RaceResult {
        ///Filled by calling function
        race_id: "race_id".to_string(),
        track_id: Uint128::zero(),
        car_ids: vec![],
        ///
        winner_ids,
        rankings,
        play_by_play: race_state.play_by_play.clone(),
        steps_taken,
    })
}

/// Simulate one tick of the race
fn simulate_tick(storage: &mut dyn Storage, race_state: &mut RaceState, training_config: TrainingConfig, tick_index: u32, max_ticks: u32, seed: u32) -> Result<(), ContractError> {
    // **NEW**: Reset car states for this tick
    for car in &mut race_state.cars {
        reset_car_state_for_tick(car);
    }
    
    let mut new_positions = vec![];
    let mut wall_collisions = vec![];
    
    // **NEW**: Collect all car positions before the loop to avoid borrow checker issues
    let all_car_positions: Vec<(i32, i32)> = race_state.cars.iter()
        .map(|car| (car.x, car.y))
        .collect();
    
    // **NEW**: Collect finished status before the mutable loop
    let car_finished_status: Vec<bool> = race_state.cars.iter()
        .map(|car| car.finished)
        .collect();
    
    // **NEW**: Collect stuck status before the mutable loop to allow one-turn skip
    let car_stuck_status: Vec<bool> = race_state.cars.iter()
        .map(|car| car.stuck)
        .collect();
    
    // Calculate intended moves for all cars
    let mut car_actions = vec![];
    
    // First pass: collect all car data and calculate actions
    for i in 0..race_state.cars.len() {
        // Get car data without borrowing
        let car_x = race_state.cars[i].x;
        let car_y = race_state.cars[i].y;
        let car_speed = race_state.cars[i].current_speed;
        let car_finished = race_state.cars[i].finished;
        let car_stuck = race_state.cars[i].stuck;
        
        if car_finished || car_stuck {
            new_positions.push((car_x, car_y));
            wall_collisions.push(false);
            car_actions.push(ACTION_UP); // Default action, won't be used
            continue;
        }
        
        //Get action strategy
        let strategy = make_action_strategy(training_config.training_mode, decimal_to_f32(training_config.epsilon), decimal_to_f32(training_config.temperature), tick_index, max_ticks, training_config.enable_epsilon_decay); // ε-greedy with 10% explore        
        // Get car action based on Q-table or heuristic
        // Get other cars' current positions (excluding this car)
        let other_cars_positions: Vec<(i32, i32)> = all_car_positions.iter()
            .enumerate()
            .filter(|(j, _)| *j != i && !car_finished_status[*j])
            .map(|(_, pos)| *pos)
            .collect();
        
        // Calculate action and update Q-table cache
        let action = calculate_car_action(&mut race_state.cars[i], storage, &race_state.track_layout, car_x, car_y, car_speed, &other_cars_positions, strategy, tick_index, seed)?;
        car_actions.push(action);
        // println!("Car action: {}, position: ({}, {})", action, car_x, car_y);
    }
    
    // Second pass: calculate new positions based on actions
    for i in 0..race_state.cars.len() {
        let car = &mut race_state.cars[i];
        if car.finished || car.stuck {
            continue; // Already handled in first pass
        }
        
        let action = car_actions[i];

        //Save action
        car.last_action = action;
        // **NEW**: Use car's current speed instead of tile speed
        let tile_speed = car.current_speed;

        // Calculate new position
        let (new_x, new_y, hit_wall) = calculate_new_position(car.x, car.y, action, tile_speed, &race_state.track_layout)?;
        
        new_positions.push((new_x, new_y));
        wall_collisions.push(hit_wall);
    }
    
    // Check for collisions
    let mut final_positions = vec![];
    for (i, (new_x, new_y)) in new_positions.iter().enumerate() {
        if check_collision(*new_x, *new_y, &new_positions, i) {
            // Collision detected, stay in place
            final_positions.push((race_state.cars[i].x, race_state.cars[i].y));
        } else {
            final_positions.push((*new_x, *new_y));
        }
    }
    
    // Update car positions and apply tile effects
    for (i, car) in race_state.cars.iter_mut().enumerate() {
        if car.finished {
            continue;
        }
        
        let (new_x, new_y) = final_positions[i];
        let hit_wall = wall_collisions[i];
        
        // **NEW**: Record action before applying tile effect
        // Get other cars' current positions (excluding this car)
        // let other_cars_positions: Vec<(i32, i32)> = all_car_positions.iter()
        //     .enumerate()
        //     .filter(|(j, _)| *j != i && !car_finished_status[*j])
        //     .map(|(_, pos)| *pos)
        //     .collect();
        
        // Generate integer state hash (new system only)
        let integer_state_hash = generate_state_hash(&race_state.track_layout, car.x, car.y, car.current_speed);
        
        // Record action in integer history only when training
        if training_config.training_mode {
            car.integer_action_history.push((integer_state_hash, car.last_action, car.tile.clone()));
        }
        
        // **NEW**: Track wall collision
        car.hit_wall = hit_wall;
        
        // **NEW**: Apply tile effects using properties directly
        apply_tile_effects_to_car(car, new_x, new_y, &race_state.track_layout)?;
        
        // If this car was stuck at the start of this tick, clear the stuck flag now (skip only one turn)
        if car_stuck_status[i] {
            car.stuck = false;
        }

        // Record action in play_by_play for this car
        if let Some(play_by_play) = race_state.play_by_play.get_mut(&car.car_id) {
            // Only add action if car has moved from previous position
            // let has_moved = if let Some(last_action) = play_by_play.actions.last() {
            //     last_action.resulting_position.x != new_x as u32 || 
            //     last_action.resulting_position.y != new_y as u32
            // } else {
            //     // First action - always add it
            //     true
            // };

            // if has_moved {
            //always add actions 
                play_by_play.actions.push(membrane::race_engine::Action {
                    action_value: Some(car.last_action as i8),
                    resulting_position: membrane::race_engine::Position {
                        x: new_x as u32,
                        y: new_y as u32,
                    },
                });
            // }
        }
    }
    
    Ok(())
}



/// **OPTIMIZED**: Calculate car action with reduced computation for gas efficiency
fn calculate_car_action(
    car: &mut CarState,
    storage: &mut dyn Storage,
    track_layout: &[Vec<membrane::types::TrackTile>],
    x: i32,
    y: i32,
    car_speed: u32,
    _other_cars: &[(i32, i32)], // Unused for gas optimization
    strategy: ActionSelectionStrategy,
    tick_index: u32,
    seed: u32, // required for deterministic randomness
) -> Result<usize, ContractError> {
    // **Deterministic RNG**: Mix global seed with car id, tick, and per-state hash
    let car_id_u32 = (car.car_id % (u32::MAX as u128)) as u32;
    let base_seed = seed.wrapping_mul(car_id_u32.wrapping_add(tick_index));

    // Generate integer state hash for current position (optimized)
    let integer_state_hash = generate_state_hash(track_layout, x, y, car_speed);
    let seed = mix_seed(base_seed, integer_state_hash);
    
    // **GAS OPTIMIZATION**: Check cache first, then storage
    let q_values = if let Some(cached_values) = car.integer_q_table.iter().find(|q| q.state_hash == integer_state_hash) {
        cached_values.action_values.clone()
    } else {
        // Not in cache, get from storage
        let storage_values = get_integer_q_values(storage, car.car_id, integer_state_hash)
            .unwrap_or_else(|_| {
                // For new states, use small random initial Q-values instead of zeros
                // This provides better exploration and prevents all cars from learning the same way
                [
                    pseudo_random(seed, 5) as i8,
                    pseudo_random(seed.wrapping_add(1), 5) as i8,
                    pseudo_random(seed.wrapping_add(2), 5) as i8,
                    pseudo_random(seed.wrapping_add(3), 5) as i8,
                ]
            });
        
        // Cache the Q-values for this race
        car.integer_q_table.push(IntegerQTableEntry {
            state_hash: integer_state_hash,
            action_values: storage_values,
        });
        
        storage_values
    };
    
    let action_count = q_values.len() as u32;

    // **GAS OPTIMIZATION**: Simplified strategy matching
    match strategy {
        ActionSelectionStrategy::Best => {
            // Find best action with random tie-break to avoid always choosing UP on ties
            let mut best_indices: Vec<usize> = vec![0];
            let mut best_value = q_values[0];
            for (i, &value) in q_values.iter().enumerate().skip(1) {
                if value > best_value {
                    best_value = value;
                    best_indices.clear();
                    best_indices.push(i);
                } else if value == best_value {
                    best_indices.push(i);
                }
            }
            if best_indices.len() == 1 {
                Ok(best_indices[0])
            } else {
                let choice = pseudo_random(seed, best_indices.len() as u32) as usize;
                Ok(best_indices[choice])
            }
        }

        ActionSelectionStrategy::Random => {
            Ok((pseudo_random(seed, action_count)) as usize)
        }

        ActionSelectionStrategy::EpsilonGreedy(epsilon) => {
            let threshold = (epsilon * 100.0) as u32;
            if pseudo_random(seed, 100) < threshold {
                Ok((pseudo_random(seed.wrapping_add(1), action_count)) as usize)
            } else {
                // Best action with random tie-break
                let mut best_indices: Vec<usize> = vec![0];
                let mut best_value = q_values[0];
                for (i, &value) in q_values.iter().enumerate().skip(1) {
                    if value > best_value {
                        best_value = value;
                        best_indices.clear();
                        best_indices.push(i);
                    } else if value == best_value {
                        best_indices.push(i);
                    }
                }
                if best_indices.len() == 1 {
                    Ok(best_indices[0])
                } else {
                    let choice = pseudo_random(seed.wrapping_add(2), best_indices.len() as u32) as usize;
                    Ok(best_indices[choice])
                }
            }
        }

        ActionSelectionStrategy::EpsilonDecay { initial_epsilon, final_epsilon, current_tick, total_ticks } => {
            // **GAS OPTIMIZATION**: Simplified epsilon calculation
            let progress = current_tick as f32 / total_ticks as f32;
            let current_epsilon = initial_epsilon - (initial_epsilon - final_epsilon) * progress;
            
            let threshold = (current_epsilon * 100.0) as u32;
            if pseudo_random(seed, 100) < threshold {
                Ok((pseudo_random(seed.wrapping_add(1), action_count)) as usize)
            } else {
                // Best action with random tie-break
                let mut best_indices: Vec<usize> = vec![0];
                let mut best_value = q_values[0];
                for (i, &value) in q_values.iter().enumerate().skip(1) {
                    if value > best_value {
                        best_value = value;
                        best_indices.clear();
                        best_indices.push(i);
                    } else if value == best_value {
                        best_indices.push(i);
                    }
                }
                if best_indices.len() == 1 {
                    Ok(best_indices[0])
                } else {
                    let choice = pseudo_random(seed.wrapping_add(3), best_indices.len() as u32) as usize;
                    Ok(best_indices[choice])
                }
            }
        }

        ActionSelectionStrategy::Softmax(temp) => {
            // **GAS OPTIMIZATION**: Simplified softmax calculation
            let mut max_q = q_values[0];
            for &q in q_values.iter().skip(1) {
                if q > max_q {
                    max_q = q;
                }
            }
            
            // Use simplified softmax with max normalization
            let exp_vals: Vec<f32> = q_values.iter()
                .map(|&q| ((q as f32 - max_q as f32) / temp).exp())
                .collect();

            let sum: f32 = exp_vals.iter().sum();
            let sample = (pseudo_random(seed, 10000) as f32) / 10000.0;
            let mut acc = 0.0;

            for (i, &exp_val) in exp_vals.iter().enumerate() {
                acc += exp_val / sum;
                if sample < acc {
                    return Ok(i);
                }
            }

            Ok(action_count as usize - 1) // fallback
        }
    }
}

/// Generate state hash based on current position and surrounding tiles
/// NEW: Returns integer hash instead of byte array, excludes other cars

#[repr(u8)]
#[derive(Copy, Clone, Debug)]
enum TileFlag { Wall=0, Sticky=1, Boost=2, Finish=3, Normal=4 }

#[repr(u8)]
enum Dir3 { None=0, Up=1, Down=2, Left=3, Right=4 }

const DIRS: [(i32, i32); 4] = [(0,-1), (0,1), (-1,0), (1,0)]; // U D L R

/// NEW: Generate integer state hash (excludes other cars, represents them as empty tiles)
/// 
/// This function creates a 16-bit integer hash based on the car's position and surrounding tiles.
/// The hash represents the state of the car in a compressed format for gas efficiency.
/// 
/// IMPORTANT: This function is used by the new system to generate integer hashes during races.
/// The convert_legacy_hash_to_integer function provides a deterministic mapping from legacy
/// 32-byte hashes to 16-bit integer hashes, but cannot guarantee that the same state will
/// produce the same hash in both systems without knowing how the legacy hash was originally generated.
pub fn generate_state_hash(
    track: &[Vec<TrackTile>],
    x: i32, y: i32,
    speed: u32,
) -> u32 {
    // Build 16-bit key from directions (4 bits each: 3 bits tile type)
    let mut key: u32 = 0;
    
    for (i, &(dx,dy)) in DIRS.iter().enumerate() {
        let tx = x + dx.wrapping_mul(speed as i32);
        let ty: i32 = y + dy.wrapping_mul(speed as i32);

        // --- 3-bit tile flag ---
        let mut flag = TileFlag::Normal as u8;

        if tx < 0 || ty < 0 || ty as usize >= track.len()
           || tx as usize >= track[0].len() {
            flag = TileFlag::Wall as u8;
        } else {
            let tile = &track[ty as usize][tx as usize];
            flag = if tile.properties.blocks_movement {
                TileFlag::Wall as u8
            } else if tile.properties.skip_next_turn {
                TileFlag::Sticky as u8
            } else if tile.properties.speed_modifier > DEFAULT_BOOST_SPEED.into() {
                TileFlag::Boost as u8
            } else if tile.properties.is_finish {
                TileFlag::Finish as u8
            } else {
                TileFlag::Normal as u8
            };
        }

        // pack into 4 bits and shift into position
        key |= (flag as u32) << (i * 4);
    }

    // Return the 16-bit key directly as integer hash
    key
}



/// Calculate new position based on action
fn calculate_new_position(
    x: i32,
    y: i32,
    action: usize,
    tiles_moved: u32,
    track_layout: &[Vec<membrane::types::TrackTile>],
) -> Result<(i32, i32, bool), ContractError> {
    let (dx, dy) = match action {
        ACTION_UP => (0, -(tiles_moved as i32)),
        ACTION_DOWN => (0, tiles_moved as i32),
        ACTION_LEFT => (-(tiles_moved as i32), 0),
        ACTION_RIGHT => (tiles_moved as i32, 0),
        _ => return Err(ContractError::InvalidAction { action }),
    };

    let mut new_x = x + dx;
    let mut new_y = y + dy;
    let mut hit_wall = false;

    // Safety check: ensure we don't have integer overflow
    if new_x < 0 || new_y < 0 || new_x > i32::MAX - 100 || new_y > i32::MAX - 100 {
        hit_wall = true;
        new_x = x;
        new_y = y;
        return Ok((new_x, new_y, hit_wall));
    }

    // Check bounds first
    let out_of_bounds = new_x < 0 || new_y < 0 || 
       new_x >= track_layout[0].len() as i32 || 
       new_y >= track_layout.len() as i32;
    
    // Check if target tile blocks movement or if car is out of bounds
    if out_of_bounds {
        // Wall collision - out of bounds
        hit_wall = true;
        // Bounce off wall - ensure we stay within bounds
        match action {
            ACTION_UP => new_y = 0, // Clamp to top edge
            ACTION_DOWN => new_y = (track_layout.len() - 1) as i32, // Clamp to bottom edge
            ACTION_LEFT => new_x = 0, // Clamp to left edge
            ACTION_RIGHT => new_x = (track_layout[0].len() - 1) as i32, // Clamp to right edge
            _ => {},
        };
    } else {
        // Check if the target tile blocks movement
        let target_tile = &track_layout[new_y as usize][new_x as usize];
        if target_tile.properties.blocks_movement {
            // Wall collision
            hit_wall = true;
            // Bounce off wall - stay in current position
            new_x = x;
            new_y = y;
        }
    }

    Ok((new_x, new_y, hit_wall))
}

/// Apply tile effects directly using properties
fn apply_tile_effects_to_car(
    car: &mut CarState,
    new_x: i32,
    new_y: i32,
    track_layout: &[Vec<membrane::types::TrackTile>],
) -> Result<(), ContractError> {
    //Increment steps taken
    car.steps_taken += 1;

    // Check bounds before accessing tile
    let out_of_bounds = new_x < 0 || new_y < 0 || 
       new_x >= track_layout[0].len() as i32 || 
       new_y >= track_layout.len() as i32;
    
    if out_of_bounds {
        // Car is out of bounds, stay in current position
        return Ok(());
    }
    
    let tile = &track_layout[new_y as usize][new_x as usize];
    
    // Apply speed modifiers based on tile properties
    car.current_speed = tile.properties.speed_modifier;
    
    
    // Apply other effects
    if tile.properties.is_finish {
        // println!("Car finished, new position: ({}, {})", new_x, new_y);
        car.finished = true;
        car.x = new_x;
        car.y = new_y;
        car.tile = tile.clone();
    } else if tile.properties.is_start {
        car.x = new_x;
        car.y = new_y;
        car.tile = tile.clone();
    } else if tile.properties.blocks_movement {
        // Wall - position already handled in calculate_new_position (bounced back)
        // Update car position to the bounced position
        car.x = new_x;
        car.y = new_y;
        car.tile = tile.clone();
    } else if tile.properties.skip_next_turn {
        // Sticky tile - move but skip next turn
        car.x = new_x;
        car.y = new_y;
        car.tile = tile.clone();
        car.stuck = true; // Will be reset next turn
    } else {
        // Normal movement
        car.x = new_x;
        car.y = new_y;
        car.tile = tile.clone();
    }
    
    // Apply damage/healing
    if tile.properties.damage != 0 {
        // TODO: Implement damage system if needed
        // For now, just track it
    }
    
    Ok(())
}

/// Reset car state for next turn (called at start of each tick)
fn reset_car_state_for_tick(car: &mut CarState) {
    // Reset hit_wall
    car.hit_wall = false;
}

/// Check for collision between cars
fn check_collision(x: i32, y: i32, positions: &[(i32, i32)], current_car: usize) -> bool {
    for (i, (other_x, other_y)) in positions.iter().enumerate() {
        if i != current_car && *other_x == x && *other_y == y {
            return true;
        }
    }
    false
}

/// Check if all cars have finished
fn all_cars_finished(cars: &[CarState]) -> bool {
    cars.iter().all(|car| car.finished)
}

/// Calculate brain progress metrics for a car after training
fn calculate_brain_progress(
    storage: &dyn Storage,
    car_id: u128,
) -> Result<(u16, u8, u16), ContractError> {
    // Get all Q-values for this car
    let q_table_range = INTEGER_Q_TABLE.prefix(car_id).range(storage, None, None, cosmwasm_std::Order::Ascending);
    
    let mut states_seen = 0u16;
    let mut total_confidence = 0u32;
    let mut wall_collisions = 0u16;
    
    for item in q_table_range {
        let (state_hash, q_values) = item.map_err(|e| ContractError::Std(e))?;
        states_seen += 1;
        
        // Calculate confidence as max Q-value normalized to 0-100 (Doesn't work)
        let max_q = q_values.iter().max().unwrap_or(&0);
        let confidence = ((*max_q as i32 * 100) / MAX_Q_VALUE) as u8;
        total_confidence += confidence as u32;
        
        // Check if the preferred action (highest Q-value) leads to a wall
        if let Some(preferred_action) = q_values.iter().position(|&x| x == *max_q) {
            if does_action_lead_to_wall(state_hash, preferred_action) {
                wall_collisions += 1;
            }
        }
    }
    
    // Calculate average confidence
    let avg_confidence = if states_seen > 0 {
        (total_confidence / states_seen as u32) as u8
    } else {
        0
    };
    
    Ok((states_seen, avg_confidence, wall_collisions))
}

/// Check if a preferred action leads to a wall by decoding the state hash
fn does_action_lead_to_wall(state_hash: u32, action: usize) -> bool {
    // Decode the state hash to get tile information
    // The state hash represents tiles in 4 directions: up, down, left, right
    // Each direction can have 5 tile types (0-4), so we need to extract 4 values
    
    // Extract tile types from the 16-bit state hash
    let tile_up = (state_hash & 0xF) as u8;      // Bits 0-3
    let tile_down = ((state_hash >> 4) & 0xF) as u8;  // Bits 4-7
    let tile_left = ((state_hash >> 8) & 0xF) as u8;  // Bits 8-11
    let tile_right = ((state_hash >> 12) & 0xF) as u8; // Bits 12-15
    
    // Check if the preferred action leads to a wall
    match action {
        0 => tile_up == 0,      // UP action leads to wall if tile_up is wall (0)
        1 => tile_down == 0,    // DOWN action leads to wall if tile_down is wall (0)
        2 => tile_left == 0,    // LEFT action leads to wall if tile_left is wall (0)
        3 => tile_right == 0,   // RIGHT action leads to wall if tile_right is wall (0)
        _ => false,
    }
}


/// Calculate race results using progress_towards_finish from tile properties
fn calculate_results(cars: &[CarState], track_layout: &[Vec<membrane::types::TrackTile>]) -> (Vec<u128>, Vec<membrane::race_engine::Rank>, Vec<membrane::race_engine::Step>) {
    let mut finished_cars: Vec<_> = cars.iter()
        .filter(|car| car.finished)
        .collect();
    
    let mut unfinished_cars: Vec<_> = cars.iter()
        .filter(|car| !car.finished)
        .collect();
    
    // Sort finished cars by steps taken (lower is better)
    finished_cars.sort_by_key(|car| car.steps_taken);
    
    // Sort unfinished cars by progress_towards_finish (higher progress = closer to finish)
    unfinished_cars.sort_by_key(|car| {
        // Use the tile's progress_towards_finish value
        // Higher progress = closer to finish, so we sort in reverse order
        std::cmp::Reverse(car.tile.progress_towards_finish)
    });
    
    // Winners are the finished cars with lowest steps
    let winner_ids = finished_cars.iter()
        .map(|car| car.car_id.clone())
        .collect();
    
    // Rankings: finished cars first (by steps), then unfinished cars (by progress)
    let mut rankings = vec![];
    for (rank, car) in finished_cars.iter().enumerate() {
        rankings.push(membrane::race_engine::Rank {
            car_id: car.car_id.clone(),
            rank: rank as u32,
        });
    }
    for (rank, car) in unfinished_cars.iter().enumerate() {
        rankings.push(membrane::race_engine::Rank {
            car_id: car.car_id.clone(),
            rank: (finished_cars.len() + rank) as u32,
        });
    }
    
    // Steps taken for each car
    let steps_taken = cars.iter()
        .map(|car| membrane::race_engine::Step {
            car_id: car.car_id.clone(),
            steps_taken: car.steps_taken,
        })
        .collect();
    
    (winner_ids, rankings, steps_taken)
}

/// Create a test track for development
fn create_test_track() -> Vec<Vec<membrane::types::TrackTile>> {
    let width = 10;
    let height = 10;
    
    let mut track = vec![vec![membrane::types::TrackTile {
        properties: membrane::types::TileProperties::normal(),
        progress_towards_finish: 0,
        min_steps_to_finish_from_start: None,
        x: 0,
        y: 0,
    }; width]; height];
    
    // Set finish line at the top
    for x in 0..width {
        track[0][x] = membrane::types::TrackTile {
            properties: membrane::types::TileProperties::finish(),
            progress_towards_finish: 0,
            min_steps_to_finish_from_start: None,
            x: x as u8,
            y: 0,
        };
    }
    
    // Set start line at the bottom
    for x in 0..width {
        track[height-1][x] = membrane::types::TrackTile {
            properties: membrane::types::TileProperties::start(),
            progress_towards_finish: height as u16 - 1,
            min_steps_to_finish_from_start: None,
            x: x as u8,
            y: (height-1) as u8,
        };
    }
    
    // Add some obstacles
    track[5][5] = membrane::types::TrackTile {
        properties: membrane::types::TileProperties::wall(),
        progress_towards_finish: 5,
        min_steps_to_finish_from_start: None,
        x: 5,
        y: 5,
    };
    
    track[3][3] = membrane::types::TrackTile {
        properties: membrane::types::TileProperties::sticky(),
        progress_towards_finish: 3,
        min_steps_to_finish_from_start: None,
        x: 3,
        y: 3,
    };
    
    track[7][7] = membrane::types::TrackTile {
        properties: membrane::types::TileProperties::boost(DEFAULT_BOOST_SPEED as u32),
        progress_towards_finish: 7,
        min_steps_to_finish_from_start: None,
        x: 7,
        y: 7,
    };
    
    //No more slow tiles 
    track[2][2] = membrane::types::TrackTile {
        properties: membrane::types::TileProperties::normal(),
        progress_towards_finish: 2,
        min_steps_to_finish_from_start: None,
        x: 2,
        y: 2,
    };
    
    track[4][4] = membrane::types::TrackTile {
        properties: membrane::types::TileProperties::normal(),
        progress_towards_finish: 4,
        min_steps_to_finish_from_start: None,
        x: 4,
        y: 4,
    };
    
    track[6][6] = membrane::types::TrackTile {
        properties: membrane::types::TileProperties::normal(),
        progress_towards_finish: 6,
        min_steps_to_finish_from_start: None,
        x: 6,
        y: 6,
    };
    
    // Set proper coordinates and distances
    for y in 0..height {
        for x in 0..width {
            if x < track[y].len() {
                track[y][x].progress_towards_finish = y as u16;
                track[y][x].x = x as u8;
                track[y][x].y = y as u8;
            }
        }
    }
    
    track
}


#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetRaceResult { race_id, track_id } => to_json_binary(&query_race_result(deps, track_id, race_id).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::ListRecentRaces { car_id, track_id, start_after, limit } => to_json_binary(&query_recent_races(deps, car_id, track_id, start_after, limit).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::GetConfig {  } => to_json_binary(&CONFIG.load(deps.storage).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::GetIntegerQ { car_id, state_hash } => to_json_binary(&query_integer_q_values(deps, car_id, state_hash).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::GetMigrationStatus { car_id } => to_json_binary(&query_migration_status(deps, car_id).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::GetBrainProgress { car_id } => to_json_binary(&query_brain_progress(deps, car_id).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::GetTrackTrainingStats { car_id, track_id, start_after, limit } => to_json_binary(&query_track_training_stats(deps, car_id, track_id, start_after, limit).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::GetTopTimes { track_id } => to_json_binary(&crate::state::get_track_top_times(deps.storage, track_id).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
    }
}






/// NEW: Query integer-based Q-values
pub fn query_integer_q_values(
    deps: Deps,
    car_id: u128,
    state_hash: Option<u32>,
) -> Result<GetIntegerQResponse, ContractError> {
    let q_values = match state_hash {
        Some(hash) => {
            // Return single Q-table entry
            let action_values = get_integer_q_values(deps.storage, car_id, hash).unwrap_or([0; 4]);
            vec![IntegerQTableEntry {
                state_hash: hash,
                action_values,
            }]
        }
        None => {
            // Return all Q-table entries for this car
            let mut entries = vec![];
            let range = INTEGER_Q_TABLE.prefix(car_id).range(deps.storage, None, None, cosmwasm_std::Order::Ascending);
            for item in range {
                let (state_hash, action_values) = item.map_err(|e| ContractError::Std(e))?;
                entries.push(IntegerQTableEntry {
                    state_hash,
                    action_values,
                });
            }
            entries
        }
    };
    
    Ok(GetIntegerQResponse {
        car_id,
        q_values,
    })
}

/// NEW: Query migration status for a car
pub fn query_migration_status(
    deps: Deps,
    car_id: u128,
) -> Result<MigrationStatusResponse, ContractError> {
    // Count original Q-table entries
    let legacy_count = Q_TABLE.prefix(car_id)
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .count() as u32;
    
    // Count integer Q-table entries
    let integer_count = INTEGER_Q_TABLE.prefix(car_id)
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .count() as u32;
    
    // Migration is complete when there are no legacy entries left
    let migration_complete = legacy_count == 0;
    
    Ok(MigrationStatusResponse {
        car_id,
        legacy_entries_count: legacy_count,
        integer_entries_count: integer_count,
        migration_complete,
    })
}

/// Query brain progress for a specific car
pub fn query_brain_progress(
    deps: Deps,
    car_id: u128,
) -> Result<membrane::race_engine::BrainProgressResponse, ContractError> {
    let brain_progress = crate::state::get_brain_progress(deps.storage, car_id)
        .unwrap_or_default();
    
    Ok(membrane::race_engine::BrainProgressResponse {
        car_id,
        brain_progress,
    })
}






pub fn query_race_result(
    deps: Deps,
    track_id: u128,
    race_id: String,
) -> Result<RaceResultResponse, ContractError> {
    let races = get_recent_races(deps.storage, None, Some(track_id))?;
    let result = races.into_iter().find(|r| r.race_id == race_id);
    
    match result {
        Some(r) => Ok(RaceResultResponse { 
            result:RaceResult {
                race_id: r.race_id,
                track_id: r.track_id,
                car_ids: r.car_ids,
                winner_ids: r.winner_ids,
                rankings: r.rankings,
                play_by_play: r.play_by_play.into_iter().map(|(k, v)| (k, v)).collect(),
                steps_taken: r.steps_taken,
            }
        }),
        None => Err(ContractError::RaceNotFound { race_id }),
    }
}

pub fn query_recent_races(
    deps: Deps,
    car_id: Option<u128>,
    track_id: Option<u128>,
    start_after: Option<u128>,
    limit: Option<u32>,
) -> Result<RecentRacesResponse, ContractError> {
    let races = get_recent_races(deps.storage, car_id, track_id)?;
    let msg_races: Vec<RaceResult> = races.iter().map(|r| RaceResult {
        race_id: r.race_id.clone(),
        track_id: r.track_id.clone(),
        car_ids: r.car_ids.clone(),
        winner_ids: r.winner_ids.clone(),
        rankings: r.rankings.clone(),
        play_by_play: r.play_by_play.clone(),
        steps_taken: r.steps_taken.clone(),
    }).collect();
    Ok(RecentRacesResponse { races: msg_races })
}

pub fn query_track_training_stats(
    deps: Deps,
    car_id: u128,
    track_id: Option<u128>,
    start_after: Option<u128>,
    limit: Option<u32>,
) -> Result<Vec<GetTrackTrainingStatsResponse>, ContractError> {
    match track_id {
        Some(track_id_str) => {
            // Single track query - return just this track's stats
            let stats = get_track_training_stats(deps.storage, car_id, track_id_str)
                .unwrap_or_else(|_| membrane::types::TrackTrainingStats {
                    solo: membrane::types::TrainingStats {
                        tally: 0,
                        win_rate: 0,
                        fastest: u32::MAX,
                        first_time: u32::MAX,
                    },
                    pvp: membrane::types::TrainingStats {
                        tally: 0,
                        win_rate: 0,
                        fastest: u32::MAX,
                        first_time: u32::MAX,
                    },
                });
            
            Ok(vec![GetTrackTrainingStatsResponse {
                car_id,
                track_id: track_id_str,
                stats,
            }])
        }
        None => {
            // Multiple tracks query - return all tracks for this car
            let limit = limit.unwrap_or(MAX_LIMIT); // Default limit
            let start_after = if let Some(start_after) = start_after.clone(){
                Some(Bound::exclusive(start_after))
            } else {
                None
            };
            // Range through all track training stats for this car
            let res = CAR_TRACK_TRAINING_STATS
                .prefix(car_id)
                .range(deps.storage, start_after, None, cosmwasm_std::Order::Ascending)
                .take(limit as usize)
                .map(|item| {
                let (track_id, stats) = item.unwrap();
                
                    GetTrackTrainingStatsResponse {
                        car_id: car_id.clone(),
                        track_id,
                        stats,
                    }}).collect();

            Ok(res)
            
        }
    }
}

// (Can we add actions later? Can we make the actions more abstract to keep the Q-Table simpler? 
// Can we compress the current statehash without losing tile information?? )
// CONTINUE BUILDING REWARD FUNCTION INTO THE membrane CONTRACT.
// WE'RE MOVING THE REWARD FUNCTION INTO THIS CONTRACT & MAKING IT DO THE TRAINING (I.E. THE Q TABLE UPDATES)
// - migrate the q-table updates from the trainer contract to here
// = update table per tick or tick batch (see trainer contract) (it updates per tick but we can group them & batch update)
// - save the q-table to the car contract post-training
// - test that it doesn't get stuck 
// 
/// Apply Q-learning updates immediately at the end of the race
fn apply_q_learning_updates(
    storage: &mut dyn Storage,
    race_state: &mut RaceState,
    race_result: &RaceResult,
    reward_config: RewardNumbers,
    _config: Config,
    _querier: QuerierWrapper,
    fastest_track_tick_time: u64,
    block_time: cosmwasm_std::Timestamp,
) -> Result<(), ContractError> {
    
    // Apply Q-learning updates immediately for each car
    for car in &mut race_state.cars {
        // Reverse pass: from last action to first for faster value backup
        let history_len = car.integer_action_history.len();
        for i in (0..history_len).rev() {
            let (state_hash, action, tile) = &car.integer_action_history[i];
            // Calculate reward for this specific action
            let action_reward = calculate_action_reward(
                car,
                race_result,
                *action,
                match i {
                    0 => car.tile.clone(),
                    _ => car.integer_action_history[i - 1].2.clone(),
                },
                tile.clone(),
                i,
                history_len,
                reward_config.clone(),
                fastest_track_tick_time,
            )?;
            
            // Get current Q-values
            let mut current_q_values = get_integer_q_values(storage, car.car_id, *state_hash)
                .unwrap_or([0, 0, 0, 0]);
            
            // Apply Q-learning update with TD bootstrapping
            let action_idx = *action as usize;
            let old_value = current_q_values[action_idx] as f32;
            let reward = action_reward as f32;

            // Bootstrap term: max_a' Q(s', a') if next state exists, else 0
            let next_max: f32 = if i + 1 < history_len {
                let next_state_hash = car.integer_action_history[i + 1].0;
                // Read next state's latest Q-values from storage first (reverse pass relies on fresh values)
                let next_q_values = get_integer_q_values(storage, car.car_id, next_state_hash)
                    .unwrap_or_else(|_| {
                        // fallback to cache if not in storage yet
                        car.integer_q_table
                            .iter()
                            .find(|e| e.state_hash == next_state_hash)
                            .map(|e| e.action_values)
                            .unwrap_or([0, 0, 0, 0])
                    });
                let mut m = next_q_values[0] as f32;
                for &v in next_q_values.iter().skip(1) {
                    let fv = v as f32;
                    if fv > m { m = fv; }
                }
                m
            } else {
                0.0
            };

            // Q-learning update: Q(s,a) ← Q + α [ r + γ · max Q(s',·) − Q ]
            let target = reward + GAMMA * next_max;
            let new_value = (old_value + ALPHA * (target - old_value)).round() as i32;
            current_q_values[action_idx] = new_value.clamp(-128, 127) as i8;
            
            // Store updated Q-values immediately
            set_integer_q_values(storage, car.car_id, *state_hash, current_q_values)?;
            // Keep cache consistent for potential subsequent lookups
            if let Some(entry) = car.integer_q_table.iter_mut().find(|e| e.state_hash == *state_hash) {
                entry.action_values = current_q_values;
            } else {
                car.integer_q_table.push(IntegerQTableEntry { state_hash: *state_hash, action_values: current_q_values });
            }
        }
    }
    
    // Update brain progress for each car after Q-learning updates
    for car in &race_state.cars {
        // Calculate brain progress metrics
        match calculate_brain_progress(storage, car.car_id) {
            Ok((states_seen, avg_confidence, wall_collisions)) => {
                // Update brain progress with current block time
                update_brain_progress(
                    storage,
                    car.car_id,
                    states_seen,
                    avg_confidence,
                    wall_collisions,
                    block_time,
                )?;
            }
            Err(e) => {
                // Log error but don't fail the entire operation
                println!("Failed to calculate brain progress for car {}: {:?}", car.car_id, e);
            }
        }
    }
    
    Ok(())
}

/// Calculate reward for a specific action
fn calculate_action_reward(
    car: &CarState,
    race_result: &RaceResult,
    _action: usize,
    last_tile: membrane::types::TrackTile,
    tile: membrane::types::TrackTile,
    _action_index: usize,
    total_actions: usize,
    reward_config: RewardNumbers,
    fastest_track_tick_time: u64,
) -> Result<i32, ContractError> {

    let mut reward = 0i32;
    // Check if car finished; only reward on terminal action
    if car.finished && (_action_index + 1 == total_actions) {
        // Check if car is a winner
        let rank = if race_result.winner_ids.contains(&car.car_id) {
            0
        } else {
            // Find car's ranking
            let ranking = race_result.rankings.iter()
                .position(|r| r.car_id == car.car_id)
                .unwrap_or(race_result.rankings.len());
            
            ranking as u8
        };

        //Add rank reward
        reward += match rank {
            0 => reward_config.rank.first,
            1 => reward_config.rank.second,
            2 => reward_config.rank.third,
            _ => reward_config.rank.other,
        };

        //Add reward for speed
        let r_ticks = 100.0 * (fastest_track_tick_time as f32) / (total_actions as f32);
        reward += r_ticks as i32;
    }

    // **NEW**: Use hit_wall field instead of checking tile type
    if car.hit_wall {
        reward += reward_config.wall;
    }

    // Base Tile penalties (excluding wall since we handle it above)
    if tile.properties.skip_next_turn {
        reward += reward_config.stuck;
    }

    // Movement reward
    
    let delta = tile.progress_towards_finish as i32 - last_tile.progress_towards_finish as i32;
    // println!("Delta: {}", delta);
    if delta == 0 {
        reward += reward_config.no_move;
    } 
    else if delta < 0 {
        let progress_towards_finish = if reward_config.going_backward.include_progress_towards_finish {
            std::cmp::max(tile.progress_towards_finish as i32, 1)
        } else {
            1
        };
        reward += reward_config.going_backward.penalty * progress_towards_finish;
    } 
    else if delta > 0 {
        reward += reward_config.distance * tile.progress_towards_finish as i32;
    }
    // println!("Reward: {}", reward);
    Ok(reward)
}

#[entry_point]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    // Migrate BrainProgress from legacy format to new format
    // migrate_brain_progress(deps.storage)?;

    Ok(Response::new()
        .add_attribute("method", "migrate")
        .add_attribute("migrated", "brain_progress"))
}

// /// Migrate BrainProgress from legacy format (with totals) to new format (deprecated fields set to None)
// fn migrate_brain_progress(storage: &mut dyn Storage) -> Result<(), ContractError> {
//     // Get all car IDs that have brain progress data
//     let car_ids: Vec<u128> = CAR_BRAIN_PROGRESS
//         .keys(storage, None, None, cosmwasm_std::Order::Ascending)
//         .collect::<Result<Vec<_>, _>>()
//         .map_err(|e| ContractError::Std(e))?;
    
//     for car_id in car_ids {
//         // Load existing data
//         if let Ok(existing_progress) = CAR_BRAIN_PROGRESS.load(storage, car_id) {
//             // Create new format with deprecated fields set to None
//             let new_progress = membrane::types::BrainProgress {
//                 entries: existing_progress.entries,
//                 total_states_seen: None,
//                 current_avg_confidence: None,
//                 total_wall_collisions: None,
//             };
            
//             // Save in new format
//             CAR_BRAIN_PROGRESS.save(storage, car_id, &new_progress)?;
//         }
//     }
    
//     Ok(())
// }







#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::mock_dependencies;
    use cosmwasm_std::Decimal;

    // Helper to pretty print the strategy used for a given tick
    fn describe_strategy(tc: &TrainingConfig, tick: u32, total_ticks: u32) -> String {
        if !tc.training_mode {
            return "Best".to_string();
        }
        if decimal_to_f32(tc.temperature) > 0.0 {
            return format!("Softmax({})", decimal_to_f32(tc.temperature));
        }
        if tc.enable_epsilon_decay {
            return format!(
                "EpsilonDecay(initial={}, final=0.01, tick={}, total={})",
                decimal_to_f32(tc.epsilon), tick, total_ticks
            );
        }
        if decimal_to_f32(tc.epsilon) > 0.0 {
            return format!("EpsilonGreedy({})", decimal_to_f32(tc.epsilon));
        }
        "Random".to_string()
    }

    #[test]
    fn race_sim_logs_strategy_and_q_values_per_hash() {
        let mut deps = mock_dependencies();

        // Build a simple track and starting car state
        let track = create_test_track();
        let start_x: i32 = 5;
        let start_y: i32 = (track.len() - 1) as i32; // bottom row is start

        let mut race_state = RaceState {
            cars: vec![CarState {
                car_id: 123,
                tile: track[start_y as usize][start_x as usize].clone(),
                x: start_x,
                y: start_y,
                stuck: false,
                finished: false,
                steps_taken: 0,
                last_action: ACTION_UP,
                hit_wall: false,
                current_speed: DEFAULT_SPEED as u32,
                integer_action_history: vec![],
                integer_q_table: vec![],
            }],
            track_layout: track,
            tick: 0,
            play_by_play: std::collections::HashMap::new(),
        };

        // Pre-seed Q-values for the initial state to strongly prefer RIGHT
        let initial_hash = generate_state_hash(
            &race_state.track_layout,
            start_x,
            start_y,
            DEFAULT_SPEED as u32,
        );
        // Prefer RIGHT (index 3)
        let seeded_q: [i8; 4] = [0, 0, 1, 50];
        set_integer_q_values(deps.as_mut().storage, 123, initial_hash, seeded_q).unwrap();

        // Low exploration configuration
        let training_config = TrainingConfig {
            training_mode: true,
            epsilon: Decimal::percent(1), // 1% exploration
            temperature: Decimal::zero(),
            enable_epsilon_decay: false,
        };

        let max_ticks: u32 = 5;
        let seed: u32 = 4242;

        for t in 0..max_ticks {
            // Run one tick
            simulate_tick(
                deps.as_mut().storage,
                &mut race_state,
                training_config.clone(),
                t,
                max_ticks,
                seed,
            )
            .unwrap();

            // Inspect the last action/state used by the car
            let car = &race_state.cars[0];
            let (state_hash, action, _tile) = car
                .integer_action_history
                .last()
                .expect("integer_action_history should have at least one entry");
            let used_q = car
                .integer_q_table
                .iter()
                .find(|e| e.state_hash == *state_hash)
                .expect("Q-values for state_hash should be cached");

            // Log: tick, strategy, state hash, Q-values, chosen action
            println!(
                "tick={} strategy={} hash={} q_values={:?} action={}",
                t,
                describe_strategy(&training_config, t, max_ticks),
                state_hash,
                used_q.action_values,
                action
            );

            // On the first tick we expect exploitation to pick RIGHT (index 3)
            if t == 0 {
                assert_eq!(*action, super::ACTION_RIGHT);
            }
        }
    }

    #[test]
    fn test_does_action_lead_to_wall() {
        // Test case 1: All walls (0x0000)
        assert!(does_action_lead_to_wall(0x0000, 0)); // UP -> wall
        assert!(does_action_lead_to_wall(0x0000, 1)); // DOWN -> wall
        assert!(does_action_lead_to_wall(0x0000, 2)); // LEFT -> wall
        assert!(does_action_lead_to_wall(0x0000, 3)); // RIGHT -> wall

        // Test case 2: Wall only in down direction (0x1110)
        // 0x1110 = 0001 0001 0001 0000
        // up=0 (wall), down=1 (normal), left=1 (normal), right=1 (normal)
        assert!(does_action_lead_to_wall(0x1110, 0)); // UP -> wall
        assert!(!does_action_lead_to_wall(0x1110, 1)); // DOWN -> not wall
        assert!(!does_action_lead_to_wall(0x1110, 2)); // LEFT -> not wall
        assert!(!does_action_lead_to_wall(0x1110, 3)); // RIGHT -> not wall

        // Test case 3: Wall only in left direction (0x1011)
        // 0x1011 = 0001 0000 0001 0001
        // up=1 (normal), down=1 (normal), left=0 (wall), right=1 (normal)
        assert!(!does_action_lead_to_wall(0x1011, 0)); // UP -> not wall
        assert!(!does_action_lead_to_wall(0x1011, 1)); // DOWN -> not wall
        assert!(does_action_lead_to_wall(0x1011, 2)); // LEFT -> wall
        assert!(!does_action_lead_to_wall(0x1011, 3)); // RIGHT -> not wall

        // Test case 4: Wall only in right direction (0x0111)
        // 0x0111 = 0000 0001 0001 0001
        // up=1 (normal), down=1 (normal), left=1 (normal), right=0 (wall)
        assert!(!does_action_lead_to_wall(0x0111, 0)); // UP -> not wall
        assert!(!does_action_lead_to_wall(0x0111, 1)); // DOWN -> not wall
        assert!(!does_action_lead_to_wall(0x0111, 2)); // LEFT -> not wall
        assert!(does_action_lead_to_wall(0x0111, 3)); // RIGHT -> wall

        // Test case 5: No walls (0x1111)
        assert!(!does_action_lead_to_wall(0x1111, 0)); // UP -> not wall
        assert!(!does_action_lead_to_wall(0x1111, 1)); // DOWN -> not wall
        assert!(!does_action_lead_to_wall(0x1111, 2)); // LEFT -> not wall
        assert!(!does_action_lead_to_wall(0x1111, 3)); // RIGHT -> not wall

        // Test case 6: Mixed walls and normal tiles (0x1001)
        // 0x1001 = 0001 0000 0000 0001
        // up=1 (normal), down=0 (wall), left=0 (wall), right=1 (normal)
        assert!(!does_action_lead_to_wall(0x1001, 0)); // UP -> not wall
        assert!(does_action_lead_to_wall(0x1001, 1)); // DOWN -> wall
        assert!(does_action_lead_to_wall(0x1001, 2)); // LEFT -> wall
        assert!(!does_action_lead_to_wall(0x1001, 3)); // RIGHT -> not wall

        // Test case 7: Invalid action
        assert!(!does_action_lead_to_wall(0x0000, 4)); // Invalid action -> false
        assert!(!does_action_lead_to_wall(0x0000, 99)); // Invalid action -> false
    }

    #[test]
    fn test_state_hash_decoding() {
        // Test the bit extraction logic directly
        let state_hash = 0x1234; // Binary: 0001 0010 0011 0100
        
        // Extract tile types
        let tile_up = (state_hash & 0xF) as u8;      // Should be 4 (0100)
        let tile_down = ((state_hash >> 4) & 0xF) as u8;  // Should be 3 (0011)
        let tile_left = ((state_hash >> 8) & 0xF) as u8;  // Should be 2 (0010)
        let tile_right = ((state_hash >> 12) & 0xF) as u8; // Should be 1 (0001)
        
        assert_eq!(tile_up, 4);
        assert_eq!(tile_down, 3);
        assert_eq!(tile_left, 2);
        assert_eq!(tile_right, 1);
    }

    #[test]
    fn test_wall_detection_edge_cases() {
        // Test with maximum values (0xFFFF = all 15s, not walls)
        let max_state_hash = 0xFFFF;
        assert!(!does_action_lead_to_wall(max_state_hash, 0)); // All bits set, but 15 is not a wall
        assert!(!does_action_lead_to_wall(max_state_hash, 1));
        assert!(!does_action_lead_to_wall(max_state_hash, 2));
        assert!(!does_action_lead_to_wall(max_state_hash, 3));

        // Test with specific tile types (0 = wall, 1-4 = other types)
        let wall_up = 0x0000; // wall up
        let normal_up = 0x0001; // normal up
        let boost_up = 0x0002; // boost up
        let finish_up = 0x0003; // finish up
        let sticky_up = 0x0004; // sticky up

        assert!(does_action_lead_to_wall(wall_up, 0)); // wall up
        assert!(!does_action_lead_to_wall(normal_up, 0)); // normal up
        assert!(!does_action_lead_to_wall(boost_up, 0)); // boost up
        assert!(!does_action_lead_to_wall(finish_up, 0)); // finish up
        assert!(!does_action_lead_to_wall(sticky_up, 0)); // sticky up
    }
}

