// race_engine/src/contract.rs

use std::collections::HashMap;

use cosmwasm_std::{
    entry_point, to_json_binary, Binary, CosmosMsg, Deps, DepsMut, Env, MessageInfo, QuerierWrapper, Response, StdResult, Storage, Uint128, from_json
};
use cw_storage_plus::Bound;

use crate::error::ContractError;
use crate::state::{CAR_TRACK_TRAINING_STATS, add_recent_race, get_config, get_q_values, get_recent_races, set_config, set_q_values, CONFIG, MAX_TICKS, Q_TABLE, update_solo_training_stats, update_pvp_training_stats, get_track_training_stats};
use racing::types::{ActionSelectionStrategy, QTableEntry, RewardNumbers, Track, TrackTile};
use racing::race_engine::{CarState, Config, ConfigResponse, ExecuteMsg, GetQResponse, GetTrackTrainingStatsResponse, InstantiateMsg, QueryMsg, RaceResult, RaceResultResponse, RaceState, RecentRacesResponse, TrainingConfig, DEFAULT_BOOST_SPEED, DEFAULT_SPEED};
use racing::car::{ExecuteMsg as Car_ExecuteMsg, QueryMsg as Car_QueryMsg};
// Race simulation constants
const MAX_CARS: usize = 8;
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
const MAX_Q_VALUE: i32 = 100;
const MIN_Q_VALUE: i32 = -100;

// Reward constants
const STUCK_PENALTY: i32 = -5;
const WALL_PENALTY: i32 = -8;
const NO_MOVE_PENALTY: i32 = 0;
const EXPLORATION_BONUS: i32 = 6;
const RANK_REWARDS: [i32; 3] = [100, 50, 25]; // 1st, 2nd, 3rd place

/// Deterministic but simple RNG for on-chain use (fallback if no external crate)
fn pseudo_random(seed: u32, modulus: u32) -> u32 {
    let a: u32 = 1103515245;
    let c: u32 = 12345;
    (a.wrapping_mul(seed).wrapping_add(c)) % modulus
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
        ActionSelectionStrategy::Random
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

/// Parse through Vec to update Q-values in storage
fn batch_update_car_q_values(storage: &mut dyn Storage, car_id: u128, state_updates: &Vec<QTableEntry>, msgs: &mut Vec<CosmosMsg>, config: &Config) -> Result<(), ContractError> {
   //For each QTableEntry, update the Q-values in storage
   for update in state_updates {
        set_q_values(storage, car_id, &update.state_hash, update.action_values)?;
   }
   
    Ok(())
}

/// Apply batched Q-learning updates to car contract
/// 
/// This function applies multiple Q-learning updates in a single call to the car contract,
/// which is more efficient than individual updates.
fn apply_batched_q_updates(
    storage: &mut dyn Storage,
    car: &CarState,
    updates: Vec<( [u8; 32], u8, i32, Option< [u8; 32]>)>, // (state_hash, action, reward, next_state_hash)
    config: Config,
    querier: QuerierWrapper,
) -> Result<(), ContractError> {
    // In a real implementation, this would:
    // 1. Use pre-loaded Q-values from car state (no need to re-query)
    // 2. Apply Q-learning updates for each (state, action, reward, next_state)
    // 3. Send all updated Q-values back to the car contract in a single transaction
    
    let mut msgs = vec![];
    
    // Collect all unique state hashes that need to be updated
    let mut state_updates: HashMap< [u8; 32], QTableEntry> = HashMap::new();
    
    // First pass: collect all current Q-values from pre-loaded Q-tables for states that need updates
    for (state_hash, _, _, _) in &updates {
        if !state_updates.contains_key(state_hash) {
            if let Some(cached_values) = car.q_table.iter().find(|q| q.state_hash == *state_hash) {
                state_updates.insert(state_hash.clone(), cached_values.clone());
            } else {
                // Initialize with default Q-values if not found in cache
                state_updates.insert(state_hash.clone(), QTableEntry {
                    state_hash: state_hash.clone(),
                    action_values: [0, 0, 0, 0],
                });
            }
        }
    }
    
    // Second pass: apply Q-learning updates to collected Q-values
    for (state_hash, action, reward, next_state_hash) in updates {
        // Validate action index (4 possible actions: 0-3)
        if action >= 4 {
            return Err(ContractError::InvalidAction { action: action as usize });
        }

        // Get current Q-values for this state
        let q_values = state_updates.get_mut(&state_hash).unwrap();
        
        // Get max Q-value for next state (for Q-learning update)
        let max_next_q = if let Some(next_hash) = &next_state_hash {
            let next_q_values = if let Some(cached_values) = car.q_table.iter().find(|q| q.state_hash == *next_hash) {
                cached_values.action_values
            } else {
                // Fallback to query if not in pre-loaded Q-tables
                 [0, 0, 0, 0]
            };
            next_q_values.iter().max().cloned().unwrap_or(0)
        } else {
            0 // No next state, so no future reward
        };
        
        // Q-learning update formula: Q(s,a) = Q(s,a) + α[r + γ max Q(s',a') - Q(s,a)]
        let old_value = q_values.action_values[action as usize];
        let new_value = ((1.0 - ALPHA) * (old_value as f32) + 
                        ALPHA * ((reward as f32) + (GAMMA * (max_next_q as f32)))).round() as i32;
        
        // Clamp the value to prevent explosion
        q_values.action_values[action as usize] = new_value.clamp(MIN_Q_VALUE, MAX_Q_VALUE);
    }
    
    // Third pass: send all updated Q-values to car contract in a single batch
    let state_updates_vec: Vec<QTableEntry> = state_updates.into_values().collect();
    batch_update_car_q_values(storage, car.car_id, &state_updates_vec, &mut msgs, &config)?;
    
    Ok(())
}

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
    
    let config = racing::race_engine::Config {
        admin: admin.to_string(),
        track_contract: track_contract.to_string(),
        car_contract: car_contract.to_string(),
        max_ticks: MAX_TICKS,
        max_recent_races: 10,
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
        ExecuteMsg::SimulateRace { track_id, car_ids, train, training_config, reward_config } => {
            execute_simulate_race(deps, _env, track_id, car_ids, train, training_config, reward_config)
        },
        ExecuteMsg::ResetQ { car_id } => {
            execute_reset_q(deps.storage, car_id.into())
        },
    }
}

/// Reset the Q-table for a car
fn execute_reset_q(storage: &mut dyn Storage, car_id: u128) -> Result<Response, ContractError> {
    let prefix = Q_TABLE.prefix(car_id);
    let range = prefix.range(storage, None, None, cosmwasm_std::Order::Ascending);
    let keys: Vec<[u8; 32]> = range.map(|item| {
        let (key, _) = item.unwrap();
        key
    }).collect();
    
    for key in keys {
        Q_TABLE.remove(storage, (car_id, &key));
    }
    Ok(Response::new())
}

fn find_start_indices(track_layout: &[Vec<racing::types::TrackTile>]) -> Vec<(usize, usize)> {
    let mut start_indices = vec![];
    for (y, row) in track_layout.iter().enumerate() {
        for (x, tile) in row.iter().enumerate() {
            if tile.properties.is_start {   
                start_indices.push((x, y));
            }
        }
    }
    start_indices
}



pub fn execute_simulate_race(
    deps: DepsMut,
    env: Env,
    track_id: Uint128,
    car_ids: Vec<u128>,
    train: bool,
    training_config: Option<TrainingConfig>,
    reward_config: Option<RewardNumbers>,
) -> Result<Response, ContractError> {
    let config = get_config(deps.storage)?;
    // Validate input
    if car_ids.len() < MIN_CARS || car_ids.len() > MAX_CARS {
        return Err(ContractError::InvalidCarCount { 
            expected: MIN_CARS as u32, 
            actual: car_ids.len() as u32
        });
    }

    //If training_config is None, use default values
    let training_config = match training_config {
        Some(config) => config,
        None => TrainingConfig {
            training_mode: true,
            epsilon: EPSILON,
            temperature: TEMPERATURE,
            enable_epsilon_decay: true,
        },
    };
    let reward_config = match reward_config {
        Some(config) => config,
        None => RewardNumbers {
            stuck: STUCK_PENALTY,
            wall: WALL_PENALTY,
            distance: 1,
            no_move: NO_MOVE_PENALTY,
            explore: EXPLORATION_BONUS,
            rank: racing::types::RankReward {
                first: RANK_REWARDS[0],
                second: RANK_REWARDS[1],
                third: RANK_REWARDS[2],
                other: 0, // Default value instead of array access
            },
        },
    };

    // Load track from track manager contract
    let track = load_track_from_manager(deps.as_ref(), config.clone(), track_id.clone())?;
    let track_layout = track.layout;
    let fastest_track_tick_time = track.fastest_tick_time;

    //Find the indices of any starting tiles
    let start_indices = find_start_indices(&track_layout);

    // Initialize car states
    let mut cars = vec![];
    for (i, car_id) in car_ids.iter().enumerate() {
        //if there are multiple starting tiles, choose car ID mod start_indices.len()
        let start_index = if start_indices.len() > 1 {
            (i % start_indices.len()) as usize
        } else {
            0
        };
        
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
            // **NEW**: Initialize action history
            action_history: vec![],
            // **NEW**: Initialize hit_wall
            hit_wall: false,
            // **NEW**: Initialize speed modifiers
            current_speed: DEFAULT_SPEED as u32, // Default normal speed
            // **NEW**: Initialize Q-tables with pre-queried values
            q_table: vec![],
        });
    }

    // Initialize race state
    let mut race_state = RaceState {
        cars,
        track_layout,
        tick: 0,
        play_by_play: std::collections::HashMap::new(),
    };

    // Simulate race
    let race_result = simulate_race(deps.storage, &mut race_state, training_config)?;

    // Generate race ID
    let race_id = format!("race_{}_{}", track_id, env.block.time.seconds());

    // Create race result
    let race_result_struct = racing::race_engine::RaceResult {
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
    for car_id in car_ids.clone() {
        add_recent_race(deps.storage, race_result_struct.clone(), Some(car_id), None)?;
    }

    // **NEW**: Apply Q-learning updates directly to car model in storage
    if train {
        apply_q_learning_updates(
            deps.storage, 
            &race_state, 
            &race_result, 
            reward_config.clone(), 
            config.clone(), 
            deps.querier,
            fastest_track_tick_time
        )?;
        
        // **NEW**: Update training stats for each car
        let is_solo = car_ids.len() == 1;
        for car in &race_state.cars {
            let won = race_result.winner_ids.contains(&car.car_id);
            let completion_time = if car.finished { car.steps_taken } else { MAX_TICKS };
            
            // Update training stats
            if is_solo {
                update_solo_training_stats(deps.storage, car.car_id, track_id.into(), won, completion_time)?;
            } else {
                update_pvp_training_stats(deps.storage, car.car_id, track_id.into(), won, completion_time)?;
            }
        }
    }

    let mut response = Response::new()
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
        config.track_contract, &racing::track_manager::QueryMsg::GetTrack {
        track_id: track_id,
    })?;
    
    Ok(track)
}

/// Simulate the complete race
fn simulate_race(storage: &mut dyn Storage, race_state: &mut RaceState, training_config: TrainingConfig) -> Result<RaceResult, ContractError> {
    let mut tick = 0;
    
    // Initialize play_by_play for each car
    for car in &race_state.cars {
        race_state.play_by_play.insert(car.car_id.clone(), racing::race_engine::PlayByPlay {
            starting_position: racing::race_engine::Position {
                car_id: car.car_id.clone(),
                x: car.x as u32,
                y: car.y as u32,
            },
            actions: vec![],
        });
    }
    
    while tick < MAX_TICKS && !all_cars_finished(&race_state.cars) {
        // Simulate one tick
        simulate_tick(storage, race_state, training_config.clone(), tick)?;
        
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
fn simulate_tick(storage: &mut dyn Storage, race_state: &mut RaceState, training_config: TrainingConfig, tick_index: u32) -> Result<(), ContractError> {
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
        let strategy = make_action_strategy(training_config.training_mode, training_config.epsilon, training_config.temperature, tick_index, MAX_TICKS, training_config.enable_epsilon_decay); // ε-greedy with 10% explore        
        // Get car action based on Q-table or heuristic
        // Get other cars' current positions (excluding this car)
        let other_cars_positions: Vec<(i32, i32)> = all_car_positions.iter()
            .enumerate()
            .filter(|(j, _)| *j != i && !car_finished_status[*j])
            .map(|(_, pos)| *pos)
            .collect();
        
        // Calculate action and update Q-table cache
        let action = calculate_car_action(&mut race_state.cars[i], storage, &race_state.track_layout, car_x, car_y, car_speed, &other_cars_positions, strategy, tick_index)?;
        car_actions.push(action);
        // println!("Car action: {}, position: ({}, {})", action, car_x, car_y);
    }
    
    // Second pass: calculate new positions based on actions
    for i in 0..race_state.cars.len() {
        let car = &race_state.cars[i];
        if car.finished || car.stuck {
            continue; // Already handled in first pass
        }
        
        let action = car_actions[i];
        
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
        let other_cars_positions: Vec<(i32, i32)> = all_car_positions.iter()
            .enumerate()
            .filter(|(j, _)| *j != i && !car_finished_status[*j])
            .map(|(_, pos)| *pos)
            .collect();
        
        let state_hash = generate_state_hash(&race_state.track_layout, car.x, car.y, car.current_speed, &other_cars_positions);
        let action = if car.x != new_x || car.y != new_y { 
            // Determine action based on movement
            if car.x < new_x { ACTION_RIGHT }
            else if car.x > new_x { ACTION_LEFT }
            else if car.y < new_y { ACTION_DOWN }
            else if car.y > new_y { ACTION_UP }
            else { ACTION_RIGHT } // Default to right if no movement
        } else { 
            ACTION_RIGHT // Default to right if no movement
        };
        
        // Record action in history
        car.action_history.push((state_hash, action, car.tile.clone()));
        
        // **NEW**: Track wall collision
        car.hit_wall = hit_wall;
        
        // **NEW**: Apply tile effects using properties directly
        apply_tile_effects_to_car(car, new_x, new_y, &race_state.track_layout)?;
        
        car.last_action = action;
        
        // Record action in play_by_play for this car
        if let Some(play_by_play) = race_state.play_by_play.get_mut(&car.car_id) {
            play_by_play.actions.push(racing::race_engine::Action {
                action: action.to_string(),
                resulting_position: racing::race_engine::Position {
                    car_id: car.car_id.clone(),
                    x: new_x as u32,
                    y: new_y as u32,
                },
            });
        }
    }
    
    Ok(())
}

/// Calculate car action using pre-loaded Q-tables
fn calculate_car_action(
    car: &mut CarState,
    storage: &mut dyn Storage,
    track_layout: &[Vec<racing::types::TrackTile>],
    x: i32,
    y: i32,
    car_speed: u32,
    other_cars: &[(i32, i32)],
    strategy: ActionSelectionStrategy,
    seed: u32, // required for deterministic randomness
) -> Result<usize, ContractError> {
    //Set seed.
    // - Allows for deterministic randomness for each car to be different
    let seed = seed * car.car_id as u32;
    // Generate state hash for current position
    let state_hash = generate_state_hash(track_layout, x, y, car_speed, other_cars);
    
    // Get Q-values from storage
    let q_values = if let Ok(stored_values) = Q_TABLE.load(storage, (car.car_id, &state_hash)) {
        stored_values
    } 
    //If Q-table is not stored, check if it exists in car state
    else if let Some(cached_values) = car.q_table.iter().find(|q| q.state_hash == state_hash) {
        cached_values.action_values.clone()
    } else {
        // For new states, use small random initial Q-values instead of zeros
        // This provides better exploration and prevents all cars from learning the same way
        let random_q_values = [
            pseudo_random(seed, 5) as i32,
            pseudo_random(seed + 1, 5) as i32,
            pseudo_random(seed + 2, 5) as i32,
            pseudo_random(seed + 3, 5) as i32,
        ];
        random_q_values
    };
    //Store Q-values in car state
    car.q_table.push(QTableEntry {
        state_hash: state_hash.clone(),
        action_values: q_values,
    });
    
    let action_count = q_values.len() as u32;

    match strategy {
        ActionSelectionStrategy::Best => {
            Ok(q_values.iter().enumerate()
                .max_by_key(|(_, &val)| val)
                .map(|(idx, _)| idx)
                .unwrap_or(0))
        }

        ActionSelectionStrategy::Random => {
            Ok((pseudo_random(seed, action_count)) as usize)
        }

        ActionSelectionStrategy::EpsilonGreedy(epsilon) => {
            let threshold = (epsilon * 100.0) as u32;
            if pseudo_random(seed, 100) < threshold {
                Ok((pseudo_random(seed + 1, action_count)) as usize)
            } else {
                Ok(q_values.iter().enumerate()
                    .max_by_key(|(_, &val)| val)
                    .map(|(idx, _)| idx)
                    .unwrap_or(0))
            }
        }

        ActionSelectionStrategy::EpsilonDecay { initial_epsilon, final_epsilon, current_tick, total_ticks } => {
            // Calculate current epsilon based on training progress
            // Linear decay: epsilon = initial - (initial - final) * progress
            let progress = current_tick as f32 / total_ticks as f32;
            let current_epsilon = initial_epsilon - (initial_epsilon - final_epsilon) * progress;
            
            let threshold = (current_epsilon * 100.0) as u32;
            if pseudo_random(seed, 100) < threshold {
                Ok((pseudo_random(seed + 1, action_count)) as usize)
            } else {
                Ok(q_values.iter().enumerate()
                    .max_by_key(|(_, &val)| val)
                    .map(|(idx, _)| idx)
                    .unwrap_or(0))
            }
        }

        ActionSelectionStrategy::Softmax(temp) => {
            let exp_vals: Vec<f32> = q_values.iter()
                .map(|&q| ((q as f32) / temp).exp())
                .collect();

            let sum: f32 = exp_vals.iter().sum();
            let probs: Vec<f32> = exp_vals.iter().map(|&v| v / sum).collect();

            let mut acc = 0.0;
            let sample = (pseudo_random(seed, 10000) as f32) / 10000.0;

            for (i, &p) in probs.iter().enumerate() {
                acc += p;
                if sample < acc {
                    return Ok(i);
                }
            }

            Ok(action_count as usize - 1) // fallback
        }
    }
}

// /// Query Q-table from car contract
// fn query_car_q_table(
//     car_id: u128,
//     track_layout: &[Vec<racing::types::TrackTile>],
//     x: i32,
//     y: i32,
//     car_speed: u32,
//     other_cars: &[(i32, i32)],
// ) -> Result<[i32; 4], ContractError> {
//     // Generate state hash based on current position and surrounding tiles
//     let state_hash = generate_state_hash(track_layout, x, y, car_speed, other_cars);
    
//     // In a real implementation, this would query the car contract
//     // For now, return default Q-values for 4 actions
//     Ok([0, 0, 0, 0])
// }

/// Generate state hash based on current position and surrounding tiles
use blake2::{
    digest::{Update, VariableOutput},
    Blake2bVar,
};

#[repr(u8)]
enum TileFlag { Wall=0, Sticky=1, Boost=2, Finish=3, Normal=4 }

#[repr(u8)]
enum Dir3 { None=0, Up=1, Down=2, Left=3, Right=4 }

const DIRS: [(i32, i32); 4] = [(0,-1), (0,1), (-1,0), (1,0)]; // U D L R

pub fn generate_state_hash(
    track: &[Vec<TrackTile>],
    x: i32, y: i32,
    speed: u32,
    other_cars: &[(i32,i32)],
) -> [u8; 32] {

    // ---------- 1. build 22-bit key ----------
    let mut key: u32 = 0;           // we’ll only use lowest 22 bits
    for (i, &(dx,dy)) in DIRS.iter().enumerate() {
        let tx = x + dx * speed as i32;
        let ty = y + dy * speed as i32;

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

        // --- 1-bit “has car” flag ---
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
    hasher.finalize_variable(&mut out);

    out
}

/// Calculate new position based on action
fn calculate_new_position(
    x: i32,
    y: i32,
    action: usize,
    tiles_moved: u32,
    track_layout: &[Vec<racing::types::TrackTile>],
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

    // Check bounds first
    let out_of_bounds = new_x < 0 || new_y < 0 || 
       new_x >= track_layout[0].len() as i32 || 
       new_y >= track_layout.len() as i32;
    
    // Check if target tile blocks movement or if car is out of bounds
    if out_of_bounds {
        // Wall collision - out of bounds
        hit_wall = true;
        // Bounce off wall
            match action {
                ACTION_UP => new_y -= 1,
                ACTION_DOWN => new_y += 1,
                ACTION_LEFT => new_x += 1,
                ACTION_RIGHT => new_x -= 1,
                _ => {},
            };
    } else {
        // Check if the target tile blocks movement
        let target_tile = &track_layout[new_y as usize][new_x as usize];
        if target_tile.properties.blocks_movement {
            // Wall collision
            hit_wall = true;
            // Bounce off wall
            match action {
                ACTION_UP => new_y -= 1,
                ACTION_DOWN => new_y += 1,
                ACTION_LEFT => new_x += 1,
                ACTION_RIGHT => new_x -= 1,
                _ => {},
            };
        }
    }

    Ok((new_x, new_y, hit_wall))
}

/// Apply tile effects directly using properties
fn apply_tile_effects_to_car(
    car: &mut CarState,
    new_x: i32,
    new_y: i32,
    track_layout: &[Vec<racing::types::TrackTile>],
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
        println!("Car finished, new position: ({}, {})", new_x, new_y);
        car.finished = true;
        car.x = new_x;
        car.y = new_y;
        car.tile = tile.clone();
    } else if tile.properties.is_start {
        car.x = new_x;
        car.y = new_y;
        car.tile = tile.clone();
    } else if tile.properties.blocks_movement {
        // Wall - stay in place
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

/// Calculate race results using progress_towards_finish from tile properties
fn calculate_results(cars: &[CarState], track_layout: &[Vec<racing::types::TrackTile>]) -> (Vec<u128>, Vec<racing::race_engine::Rank>, Vec<racing::race_engine::Step>) {
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
        rankings.push(racing::race_engine::Rank {
            car_id: car.car_id.clone(),
            rank: rank as u32,
        });
    }
    for (rank, car) in unfinished_cars.iter().enumerate() {
        rankings.push(racing::race_engine::Rank {
            car_id: car.car_id.clone(),
            rank: (finished_cars.len() + rank) as u32,
        });
    }
    
    // Steps taken for each car
    let steps_taken = cars.iter()
        .map(|car| racing::race_engine::Step {
            car_id: car.car_id.clone(),
            steps_taken: car.steps_taken,
        })
        .collect();
    
    (winner_ids, rankings, steps_taken)
}

/// Create a test track for development
fn create_test_track() -> Vec<Vec<racing::types::TrackTile>> {
    let width = 10;
    let height = 10;
    
    let mut track = vec![vec![racing::types::TrackTile {
        properties: racing::types::TileProperties::normal(),
        progress_towards_finish: 0,
        x: 0,
        y: 0,
    }; width]; height];
    
    // Set finish line at the top
    for x in 0..width {
        track[0][x] = racing::types::TrackTile {
            properties: racing::types::TileProperties::finish(),
            progress_towards_finish: 0,
            x: x as u8,
            y: 0,
        };
    }
    
    // Set start line at the bottom
    for x in 0..width {
        track[height-1][x] = racing::types::TrackTile {
            properties: racing::types::TileProperties::start(),
            progress_towards_finish: height as u16 - 1,
            x: x as u8,
            y: (height-1) as u8,
        };
    }
    
    // Add some obstacles
    track[5][5] = racing::types::TrackTile {
        properties: racing::types::TileProperties::wall(),
        progress_towards_finish: 5,
        x: 5,
        y: 5,
    };
    
    track[3][3] = racing::types::TrackTile {
        properties: racing::types::TileProperties::sticky(),
        progress_towards_finish: 3,
        x: 3,
        y: 3,
    };
    
    track[7][7] = racing::types::TrackTile {
        properties: racing::types::TileProperties::boost(DEFAULT_BOOST_SPEED as u32),
        progress_towards_finish: 7,
        x: 7,
        y: 7,
    };
    
    //No more slow tiles 
    track[2][2] = racing::types::TrackTile {
        properties: racing::types::TileProperties::normal(),
        progress_towards_finish: 2,
        x: 2,
        y: 2,
    };
    
    track[4][4] = racing::types::TrackTile {
        properties: racing::types::TileProperties::normal(),
        progress_towards_finish: 4,
        x: 4,
        y: 4,
    };
    
    track[6][6] = racing::types::TrackTile {
        properties: racing::types::TileProperties::normal(),
        progress_towards_finish: 6,
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
        QueryMsg::GetQ { car_id, state_hash } => to_json_binary(&query_q_values(deps, car_id, state_hash).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::GetTrackTrainingStats { car_id, track_id, start_after, limit } => to_json_binary(&query_track_training_stats(deps, car_id, track_id, start_after, limit).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
    }
}




pub fn query_q_values(
    deps: Deps,
    car_id: u128,
    state_hash: Option<[u8; 32]>,
) -> Result<GetQResponse, ContractError> {
    // Check if car exists
    // get_car_info(deps.storage, &car_id)?;
    
    let q_values = match state_hash {
        Some(hash) => {
            // Return single Q-table entry
            let action_values = get_q_values(deps.storage, car_id, &hash).unwrap_or([0; 4]);
            vec![QTableEntry {
                state_hash: hash,
                action_values,
            }]
        }
        None => {
            // Return all Q-table entries for this car
            let mut entries = vec![];
            let range = Q_TABLE.prefix(car_id).range(deps.storage, None, None, cosmwasm_std::Order::Ascending);
            for item in range {
                let (state_hash, action_values) = item.map_err(|e| ContractError::Std(e))?;
                entries.push(QTableEntry {
                    state_hash,
                    action_values,
                });
            }
            entries
        }
    };
    
    Ok(GetQResponse {
        car_id,
        q_values,
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
                .unwrap_or_else(|_| racing::types::TrackTrainingStats {
                    solo: racing::types::TrainingStats {
                        tally: 0,
                        win_rate: 0,
                        fastest: u32::MAX,
                    },
                    pvp: racing::types::TrainingStats {
                        tally: 0,
                        win_rate: 0,
                        fastest: u32::MAX,
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
// CONTINUE BUILDING REWARD FUNCTION INTO THE RACING CONTRACT.
// WE'RE MOVING THE REWARD FUNCTION INTO THIS CONTRACT & MAKING IT DO THE TRAINING (I.E. THE Q TABLE UPDATES)
// - migrate the q-table updates from the trainer contract to here
// = update table per tick or tick batch (see trainer contract) (it updates per tick but we can group them & batch update)
// - save the q-table to the car contract post-training
// - test that it doesn't get stuck 
// 
/// Apply Q-learning updates directly to car contracts based on race results and car actions
fn apply_q_learning_updates(
    storage: &mut dyn Storage,
    race_state: &RaceState,
    race_result: &RaceResult,
    reward_config: RewardNumbers,
    config: Config,
    querier: QuerierWrapper,
    fastest_track_tick_time: u64,
) -> Result<(), ContractError> {
    
    // Collect all Q-updates for each car
    let mut car_updates: std::collections::HashMap<u128, Vec<( [u8; 32], u8, i32, Option< [u8; 32]>)>> = std::collections::HashMap::new();
    
    for car in &race_state.cars {
        let mut updates = vec![];
        
        // Process each action in the car's history
        for (i, (state_hash, action, tile)) in car.action_history.iter().enumerate() {
            // Calculate reward for this specific action
            let action_reward = calculate_action_reward(
                car,
                race_result,
                *action,
                match i {
                    0 => car.tile.clone(),
                    _ => car.action_history[i - 1].2.clone(),
                },
                tile.clone(),
                i,
                car.action_history.len(),
                reward_config.clone(),
                fastest_track_tick_time,
            )?;
            
            // Determine next state hash (if not the last action)
            let next_state_hash = if i < car.action_history.len() - 1 {
                Some(car.action_history[i + 1].0.clone())
            } else {
                None
            };
            
            // Collect update: (state_hash, action, reward, next_state_hash)
            updates.push((state_hash.clone(), *action as u8, action_reward, next_state_hash));
        }
        
        car_updates.insert(car.car_id.clone(), updates);
    }
    
    // Apply batched updates to each car's model in storage
    for car in &race_state.cars {
        if let Some(updates) = car_updates.get(&car.car_id) {
            apply_batched_q_updates(storage, car, updates.clone(), config.clone(), querier.clone())?;
        }
    }
    
    Ok(())
}

/// Calculate reward for a specific action
fn calculate_action_reward(
    car: &CarState,
    race_result: &RaceResult,
    action: usize,
    last_tile: racing::types::TrackTile,
    tile: racing::types::TrackTile,
    action_index: usize,
    total_actions: usize,
    reward_config: RewardNumbers,
    fastest_track_tick_time: u64,
) -> Result<i32, ContractError> {

    let mut rank = 0;
    let mut reward = 0i32;
    // Check if car finished
    if car.finished {
        // Check if car is a winner
        if race_result.winner_ids.contains(&car.car_id) {
            rank = 0;
        } else {
            // Find car's ranking
            let ranking = race_result.rankings.iter()
                .position(|rank| rank.car_id == car.car_id)
                .unwrap_or(race_result.rankings.len());
            
            rank = ranking as u8;
        }

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
    } else {
        reward += reward_config.distance * delta;
    } 
    if delta > 0 {
        reward += reward_config.distance * tile.progress_towards_finish as i32;
    }
    println!("Reward: {}", reward);
    Ok(reward)
}
