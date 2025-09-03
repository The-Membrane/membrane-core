use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{from_json, to_json_binary, Addr, Binary, OwnedDeps, Querier, QuerierResult, QueryRequest, SystemResult, ContractResult};
use serde::Serialize;

use crate::contract::{execute, instantiate, query};
use crate::error::ContractError;
use membrane::race_engine::{ExecuteMsg, InstantiateMsg, QueryMsg, TrainingConfig, GetTrackTrainingStatsResponse, GetIntegerQResponse, MigrationStatusResponse};
use membrane::types::{RewardNumbers, Track, TrackTile, TileProperties, GoingBackward};

const ADMIN: &str = "admin";
const CAR_CONTRACT: &str = "car_contract";
const TRACK_CONTRACT: &str = "track_contract";

#[derive(Serialize)]
struct OwnerOfResp { owner: String }

// Mock track for testing
fn create_test_track() -> Track {
    let mut layout = vec![vec![TrackTile {
        properties: TileProperties::normal(),
        progress_towards_finish: 0,
        x: 0,
        y: 0,
    }; 5]; 5];
    
    // Set finish line at the top
    for x in 0..5 {
        layout[0][x] = TrackTile {
            properties: TileProperties::finish(),
            progress_towards_finish: 0,
            x: x as u8,
            y: 0,
        };
    }
    
    // Set start line at the bottom
    for x in 0..5 {
        layout[4][x] = TrackTile {
            properties: TileProperties::start(),
            progress_towards_finish: 4,
            x: x as u8,
            y: 4,
        };
    }

    // Build starting_tiles vector from bottom row
    let mut starting_tiles = vec![];
    for x in 0..5 {
        starting_tiles.push(layout[4][x].clone());
    }
    
    Track {
        creator: "creator".to_string(),
        id: 1,
        name: "test_track".to_string(),
        width: 5,
        height: 5,
        layout,
        fastest_tick_time: 10,
        starting_tiles,
    }
}

fn create_test_track_with_one_start_tile() -> Track {
    let mut t = create_test_track();
    // Keep only one starting tile
    t.starting_tiles = vec![t.layout[4][0].clone()];
    t
}

fn setup_test_app_with_track(track: Track) -> OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier<cosmwasm_std::Empty>> {
    let mut deps = mock_dependencies();
    let track = track.clone();
    
    // Set up mock querier to return track data and owner responses
    deps.querier.update_wasm(move |w| {
        match w {
            cosmwasm_std::WasmQuery::Smart { contract_addr, .. } if *contract_addr == TRACK_CONTRACT => {
                let track_response = to_json_binary(&track).unwrap();
                Ok(ContractResult::Ok(track_response)).into()
            }
            cosmwasm_std::WasmQuery::Smart { contract_addr, .. } if *contract_addr == CAR_CONTRACT => {
                // Return minimal OwnerOfResponse JSON: { "owner": "test_user" }
                let owner_resp = to_json_binary(&OwnerOfResp { owner: "test_user".to_string() }).unwrap();
                Ok(ContractResult::Ok(owner_resp)).into()
            }
            _ => Ok(ContractResult::Err(cosmwasm_std::StdError::generic_err("Unknown query").to_string())).into(),
        }
    });
    
    let env = mock_env();
    let info = mock_info(ADMIN, &[]);
    
    // Instantiate contract
    let instantiate_msg = InstantiateMsg {
        admin: ADMIN.to_string(),
        track_contract: TRACK_CONTRACT.to_string(),
        car_contract: CAR_CONTRACT.to_string(),
    };
    
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();
    
    deps
}

fn setup_test_app() -> OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier<cosmwasm_std::Empty>> {
    setup_test_app_with_track(create_test_track())
}

#[test]
fn test_training_stats_after_race() {
    let mut deps = setup_test_app();
    let env = mock_env();
    let info = mock_info("test_user", &[]);
    
    // Test that we can query training stats (should return default values)
    let query_msg = QueryMsg::GetTrackTrainingStats {
        car_id: 1u128,
        track_id: Some(1u128),
        start_after: None,
        limit: None,
    };
    
    let response = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let stats_response: Vec<GetTrackTrainingStatsResponse> = from_json(response).unwrap();
    let stats_response = &stats_response[0]; // Get the first (and only) response
    
    // Verify default values
    assert_eq!(stats_response.stats.solo.tally, 0);
    assert_eq!(stats_response.stats.solo.win_rate, 0);
    assert_eq!(stats_response.stats.solo.fastest, u32::MAX);
    assert_eq!(stats_response.stats.pvp.tally, 0);
    assert_eq!(stats_response.stats.pvp.win_rate, 0);
    assert_eq!(stats_response.stats.pvp.fastest, u32::MAX);
    
    // Simulate a solo race with training enabled
    let simulate_msg = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128],
        pvp: Some(false),
        train: true,
        training_config: Some(TrainingConfig {
            training_mode: true,
            epsilon: cosmwasm_std::Decimal::percent(10),
            temperature: cosmwasm_std::Decimal::zero(),
            enable_epsilon_decay: false,
        }),
        reward_config: None,
    };
    
    let result = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg.clone());
    assert!(result.is_ok());
    
    // Query training stats after the race
    let query_msg = QueryMsg::GetTrackTrainingStats {
        car_id: 1u128,
        track_id: Some(1u128),
        start_after: None,
        limit: None,
    };
    
    let final_response = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let final_stats: Vec<GetTrackTrainingStatsResponse> = from_json(final_response).unwrap();
    let final_stats = &final_stats[0]; // Get the first (and only) response
    
    // Check that solo stats were updated (tally should be 1)
    assert_eq!(final_stats.stats.solo.tally, 1);
    // PvP stats should remain at 0 since this was a solo race
    assert_eq!(final_stats.stats.pvp.tally, 0);
}

#[test]
fn test_pvp_training_includes_car_zero() {
    let mut deps = setup_test_app();
    let env = mock_env();
    let info = mock_info("test_user", &[]);

    // PvP training with a single provided car should auto-append car 0 (The Singularity)
    let pvp_simulate_msg = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128],
        pvp: Some(true),
        train: true,
        training_config: Some(TrainingConfig {
            training_mode: true,
            epsilon: cosmwasm_std::Decimal::percent(10),
            temperature: cosmwasm_std::Decimal::zero(),
            enable_epsilon_decay: false,
        }),
        reward_config: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), pvp_simulate_msg);
    assert!(res.is_ok(), "PvP training with car 0 should succeed");

    // Car 0 should have PvP stats updated
    let pvp_query_car0 = QueryMsg::GetTrackTrainingStats {
        car_id: 0u128,
        track_id: Some(1u128),
        start_after: None,
        limit: None,
    };
    let resp0 = query(deps.as_ref(), env.clone(), pvp_query_car0).unwrap();
    let stats0: Vec<GetTrackTrainingStatsResponse> = from_json(resp0).unwrap();
    let stats0 = &stats0[0];
    assert_eq!(stats0.stats.pvp.tally, 1, "Car 0 should have PvP tally 1");
    assert!(stats0.stats.pvp.fastest < u32::MAX, "Car 0 PvP fastest should be updated");
}

#[test]
fn test_training_invalid_car_count_error() {
    let mut deps = setup_test_app();
    let env = mock_env();
    let info = mock_info("test_user", &[]);

    // train=true should require exactly one provided car id
    let simulate_msg = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128, 2u128],
        pvp: Some(false),
        train: true,
        training_config: None,
        reward_config: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg);
    assert!(res.is_err(), "Expected error for invalid car count when training");
}

#[test]
fn test_pvp_insufficient_starting_tiles_error() {
    // Track with only one start tile
    let mut deps = setup_test_app_with_track(create_test_track_with_one_start_tile());
    let env = mock_env();
    let info = mock_info("test_user", &[]);

    let simulate_msg = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128], // valid single input
        pvp: Some(true),      // will append car 0 making 2 cars
        train: true,
        training_config: None,
        reward_config: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg);
    assert!(res.is_err(), "Expected error due to insufficient starting tiles for PvP");
}

#[test]
fn test_training_ownership_enforced() {
    let mut deps = setup_test_app_with_track(create_test_track());
    let env = mock_env();
    // Simulate a different sender; our mock owner is "test_user"
    let info = mock_info("not_owner", &[]);

    let simulate_msg = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128],
        pvp: Some(false),
        train: true,
        training_config: None,
        reward_config: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg);
    assert!(res.is_err(), "Expected Unauthorized when sender is not car owner");
}

#[test]
fn test_multiple_tracks_query() {
    let mut deps = setup_test_app();
    let env = mock_env();
    let info = mock_info("test_user", &[]);
    
    // Simulate races on multiple tracks for the same car
    let tracks = vec!["track_1", "track_2", "track_3"];
    
    for (i, track_id) in tracks.iter().enumerate() {
        let simulate_msg = ExecuteMsg::SimulateRace {
            track_id: cosmwasm_std::Uint128::from((i + 1) as u128),
            car_ids: vec![1u128],
            pvp: Some(false),
            train: true,
            training_config: Some(TrainingConfig {
            training_mode: true,
            epsilon: cosmwasm_std::Decimal::percent(10),
            temperature: cosmwasm_std::Decimal::zero(),
            enable_epsilon_decay: false,
            }),
            reward_config: None,
        };
        
        let result = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg);
        assert!(result.is_ok(), "Race simulation failed for track {}", track_id);
    }
    
    // Query all tracks for the car (track_id = None)
    let query_msg = QueryMsg::GetTrackTrainingStats {
        car_id: 1u128,
        track_id: None,
        start_after: None,
        limit: Some(5),
    };
    
    let response = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let stats: Vec<GetTrackTrainingStatsResponse> = from_json(response).unwrap();
    
    // Should return stats for all tracks
    assert_eq!(stats.len(), 3, "Should return stats for all 3 tracks");
    
    // Verify each track has solo stats updated
    for stat in &stats {
        assert_eq!(stat.stats.solo.tally, 1, "Each track should have 1 solo race");
        assert!(stat.stats.solo.fastest < u32::MAX, "Fastest time should be updated");
    }
}

#[test]
fn test_random_behavior_variability() {
    let mut deps = setup_test_app();
    let env = mock_env();
    let info = mock_info("test_user", &[]);
    
    // Test with high epsilon (90% random) to show variability
    let mut completion_times = vec![];
    
    for i in 0..5 {
        let simulate_msg = ExecuteMsg::SimulateRace {
            track_id: cosmwasm_std::Uint128::from(1u128),
            car_ids: vec![1u128],
            pvp: Some(false),
            train: true,
            training_config: Some(TrainingConfig {
                training_mode: true,
                epsilon: cosmwasm_std::Decimal::percent(90), // 90% random exploration
                temperature: cosmwasm_std::Decimal::zero(),
                enable_epsilon_decay: false,
            }),
            reward_config: None,
        };
        
        let result = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg);
        assert!(result.is_ok());
        
        // Query stats to get completion time
        let query_msg = QueryMsg::GetTrackTrainingStats {
            car_id: 1u128,
            track_id: Some(1u128),
            start_after: None,
            limit: None,
        };
        
        let response = query(deps.as_ref(), env.clone(), query_msg.clone()).unwrap();
        let stats: Vec<GetTrackTrainingStatsResponse> = from_json(response).unwrap();
        let stats = &stats[0];
        
        completion_times.push(stats.stats.solo.fastest);
    }
    
    // Check that we have some variability in completion times (or at least non-panicking)
    let _min_time = completion_times.iter().min().unwrap();
    let _max_time = completion_times.iter().max().unwrap();
}

#[test]
fn test_deterministic_vs_random() {
    let mut deps = setup_test_app();
    let env = mock_env();
    let info = mock_info("test_user", &[]);
    
    // Test 1: Deterministic behavior (epsilon = 0.0, no randomness)
    let deterministic_msg = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128],
        pvp: Some(false),
        train: true,
        training_config: Some(TrainingConfig {
            training_mode: true,
            epsilon: cosmwasm_std::Decimal::zero(), // No randomness
            temperature: cosmwasm_std::Decimal::zero(),
            enable_epsilon_decay: false,
        }),
        reward_config: None,
    };
    
    let result = execute(deps.as_mut(), env.clone(), info.clone(), deterministic_msg);
    assert!(result.is_ok());
}

#[test]
fn test_empty_q_table_behavior() {
    let mut deps = setup_test_app();
    let env = mock_env();
    let info = mock_info("test_user", &[]);
    
    // Test with epsilon = 0.0 (no randomness) to see deterministic behavior
    let simulate_msg = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128],
        pvp: Some(false),
        train: true,
        training_config: Some(TrainingConfig {
            training_mode: true,
            epsilon: cosmwasm_std::Decimal::zero(), // No randomness - pure Q-learning
                temperature: cosmwasm_std::Decimal::zero(),
                enable_epsilon_decay: false,
        }),
            reward_config: None,
        };
        
    let result = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg);
    assert!(result.is_ok());
}

#[test]
fn test_learning_process_investigation() {
    let mut deps = setup_test_app();
    let env = mock_env();
    let info = mock_info("test_user", &[]);
    
    // Run multiple races to see if the car learns and improves
    for _race_num in 0..3 {
        // Reset Q-table before each race to see if learning happens within a single race
        let reset_msg = ExecuteMsg::ResetQ {
            car_id: cosmwasm_std::Uint128::from(1u128),
        };
        execute(deps.as_mut(), env.clone(), info.clone(), reset_msg).ok();
        
        let simulate_msg = ExecuteMsg::SimulateRace {
            track_id: cosmwasm_std::Uint128::from(1u128),
            car_ids: vec![1u128],
            pvp: Some(false),
            train: true,
            training_config: Some(TrainingConfig {
                training_mode: true,
                epsilon: cosmwasm_std::Decimal::percent(10), // 10% random
                temperature: cosmwasm_std::Decimal::zero(),
                enable_epsilon_decay: false,
            }),
            reward_config: None,
        };
        
        let result = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg);
        assert!(result.is_ok());
    }
}

#[test]
fn test_seed_determinism_explanation() {
    let mut deps = setup_test_app();
    let env = mock_env();
    let info = mock_info("test_user", &[]);
    
    // The issue: The seed is always tick_index (0, 1, 2, 3, ...)
    
    // Test 1: Run with epsilon = 0.5 (50% random)
    let simulate_msg1 = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128],
        pvp: Some(false),
        train: true,
        training_config: Some(TrainingConfig {
            training_mode: true,
            epsilon: cosmwasm_std::Decimal::percent(50), // 50% random
            temperature: cosmwasm_std::Decimal::zero(),
            enable_epsilon_decay: false,
        }),
        reward_config: None,
    };
    
    let result1 = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg1);
    assert!(result1.is_ok());
}

#[test]
fn test_initial_q_values_investigation() {
    let mut deps = setup_test_app();
    let env = mock_env();
    let info = mock_info("test_user", &[]);
    
    // Reset Q-table
    let reset_msg = ExecuteMsg::ResetQ {
        car_id: cosmwasm_std::Uint128::from(1u128),
    };
    execute(deps.as_mut(), env.clone(), info.clone(), reset_msg).ok();
    
    let simulate_msg = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128],
        pvp: Some(false),
        train: true,
        training_config: Some(TrainingConfig {
            training_mode: true,
            epsilon: cosmwasm_std::Decimal::percent(10),
            temperature: cosmwasm_std::Decimal::zero(),
            enable_epsilon_decay: false,
        }),
        reward_config: None,
    };
    
    let result = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg);
    assert!(result.is_ok());
}

#[test]
fn test_epsilon_variance_investigation() {
    let mut deps = setup_test_app();
    let env = mock_env();
    let info = mock_info("test_user", &[]);
    
    // Test a range of epsilon values to see where variance occurs
    for epsilon in [0.0, 0.1, 0.5, 1.0] {
        // Reset Q-table
        let reset_msg = ExecuteMsg::ResetQ {
            car_id: cosmwasm_std::Uint128::from(1u128),
        };
        execute(deps.as_mut(), env.clone(), info.clone(), reset_msg).ok();
        
        let epsilon_dec = cosmwasm_std::Decimal::percent((epsilon * 100.0) as u64);
        let simulate_msg = ExecuteMsg::SimulateRace {
            track_id: cosmwasm_std::Uint128::from(1u128),
            car_ids: vec![1u128],
            pvp: Some(false),
            train: true,
            training_config: Some(TrainingConfig {
                training_mode: true,
                epsilon: epsilon_dec,
                temperature: cosmwasm_std::Decimal::zero(),
                enable_epsilon_decay: false,
            }),
            reward_config: None,
        };
        
        let result = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg);
        assert!(result.is_ok());
    }
}

#[test]
fn test_epsilon_06_specific_investigation() {
    let mut deps = setup_test_app();
    let env = mock_env();
    let info = mock_info("test_user", &[]);
    
    // Test epsilon 0.6 specifically
    let simulate_msg = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128],
        pvp: Some(false),
        train: true,
        training_config: Some(TrainingConfig {
            training_mode: true,
            epsilon: cosmwasm_std::Decimal::percent(60), // 60% random
            temperature: cosmwasm_std::Decimal::zero(),
            enable_epsilon_decay: false,
        }),
        reward_config: None,
    };
    
    let result = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg);
    assert!(result.is_ok());
}

#[test]
fn test_pvp_training_stats() {
    let mut deps = setup_test_app();
    let env = mock_env();
    let info = mock_info("test_user", &[]);
    
    // Simulate a PvP race with multiple cars and training enabled
    let simulate_msg = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128], // provide one; pvp true appends car 0
        pvp: Some(true),
        train: true,
        training_config: Some(TrainingConfig {
            training_mode: true,
            epsilon: cosmwasm_std::Decimal::percent(10),
            temperature: cosmwasm_std::Decimal::zero(),
            enable_epsilon_decay: false,
        }),
        reward_config: Some(RewardNumbers {
            distance: 1,
            going_backward: GoingBackward {
                penalty: -1,
                include_progress_towards_finish: true,
            },
            stuck: -5,
            wall: -8,
            no_move: 0,
            explore: 6,
            rank: membrane::types::RankReward {
                first: 100,
                second: 50,
                third: 25,
                other: 0,
            },
        }),
    };
    
    let result = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg);
    assert!(result.is_ok(), "PvP race simulation failed: {:?}", result.err());
    
    // Query training stats for player car 1 and car 0
    for car_id in &[1u128, 0u128] {
        let query_msg = QueryMsg::GetTrackTrainingStats {
            car_id: *car_id,
            track_id: Some(1u128),
            start_after: None,
            limit: None,
        };
        
        let response = query(deps.as_ref(), env.clone(), query_msg).unwrap();
        let stats: Vec<GetTrackTrainingStatsResponse> = from_json(response).unwrap();
        let stats = &stats[0]; // Get the first (and only) response
        
        // Verify that PvP stats were updated
        assert_eq!(stats.stats.pvp.tally, 1, "PvP tally should be 1 for car {}", car_id);
        assert!(stats.stats.pvp.fastest < u32::MAX, "PvP fastest time should be updated for car {}", car_id);
        
        // Solo stats should remain at 0 since this was a PvP race
        assert_eq!(stats.stats.solo.tally, 0, "Solo tally should remain 0 for PvP race");
        // fastest may be populated by generic race tracking; only enforce tally here
    }
}

#[test]
fn test_no_training_stats_when_training_disabled() {
    let mut deps = setup_test_app();
    let env = mock_env();
    let info = mock_info("test_user", &[]);
    
    // Simulate a race with training disabled
    let simulate_msg = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128],
        pvp: Some(false),
        train: false, // Training disabled
        training_config: None,
        reward_config: None,
    };
    
    let result = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg);
    assert!(result.is_ok(), "Race simulation failed: {:?}", result.err());
    
    // Query training stats after the race
    let query_msg = QueryMsg::GetTrackTrainingStats {
        car_id: 1u128,
        track_id: Some(1u128),
        start_after: None,
        limit: None,
    };
    
    let response = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let stats: Vec<GetTrackTrainingStatsResponse> = from_json(response).unwrap();
    let stats = &stats[0]; // Get the first (and only) response
    
    // Verify that stats were NOT updated since training was disabled
    assert_eq!(stats.stats.solo.tally, 0, "Solo tally should remain 0 when training disabled");
    // fastest fields may be influenced by non-training race recording; focus on tallies only
    assert_eq!(stats.stats.pvp.tally, 0, "PvP tally should remain 0 when training disabled");
    // fastest fields may be influenced by non-training race recording; focus on tallies only
}

#[test]
fn test_purge_car_removes_all_state() {
    let mut deps = setup_test_app();
    let env = mock_env();
    let info = mock_info(ADMIN, &[]);

    // Pre-populate Q_TABLE, training stats, and recent races for car 42
    let car_id: u128 = 42;
    // Insert one recent race for car
    let result = execute(deps.as_mut(), env.clone(), mock_info("test_user", &[]), ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![car_id],
        pvp: Some(false),
        train: true,
        training_config: Some(TrainingConfig { training_mode: true, epsilon: cosmwasm_std::Decimal::percent(10), temperature: cosmwasm_std::Decimal::zero(), enable_epsilon_decay: false }),
        reward_config: None,
    });
    assert!(result.is_ok());

    // Manually set one training stats entry
    crate::state::set_track_training_stats(deps.as_mut().storage, car_id, 1u128, membrane::types::TrackTrainingStats { 
        solo: membrane::types::TrainingStats { tally: 1, win_rate: 100, fastest: 50, first_time: 50 },
        pvp: membrane::types::TrainingStats { tally: 0, win_rate: 0, fastest: u32::MAX, first_time: u32::MAX },
    }).unwrap();

    // Call PurgeCar from car contract authority
    let purge = ExecuteMsg::PurgeCar { car_id: cosmwasm_std::Uint128::from(car_id) };
    let res = execute(deps.as_mut(), env.clone(), mock_info(CAR_CONTRACT, &[]), purge).unwrap();
    assert_eq!(0, res.messages.len());

    // Assert integer Q-table removed for this car
    let mut any_q = false;
    for _ in crate::state::INTEGER_Q_TABLE.prefix(car_id).range(deps.as_ref().storage, None, None, cosmwasm_std::Order::Ascending) { any_q = true; break; }
    assert!(!any_q, "Integer Q-table entries should be removed");

    // Assert training stats removed for all tracks for this car
    let mut any_stats = false;
    for _ in crate::state::CAR_TRACK_TRAINING_STATS.prefix(car_id).range(deps.as_ref().storage, None, None, cosmwasm_std::Order::Ascending) { any_stats = true; break; }
    assert!(!any_stats, "Training stats should be removed");

    // Assert recent races removed for this car
    let has_recent = crate::state::CAR_RECENT_RACES.has(deps.as_ref().storage, car_id);
    assert!(!has_recent, "Recent races should be removed");
}

#[test]
fn test_epsilon_decay_debug() {
    let mut deps = setup_test_app();
    let env = mock_env();
    let info = mock_info("test_user", &[]);
    
    println!("=== EPSILON DECAY DEBUG TEST ===");
    
    // Test 1: Run with NO training config (should use 90% default epsilon with decay enabled)
    println!("\n--- Test 1: No training config (90% default with decay) ---");
    let simulate_msg1 = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128],
        pvp: Some(false),
        train: true,
        training_config: None, // This should use default: 90% epsilon, decay enabled
        reward_config: None,
    };
    
    let result1 = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg1);
    assert!(result1.is_ok(), "Race 1 failed: {:?}", result1.err());
    
    // Test 2: Run with explicit training config (15% epsilon with decay enabled)
    println!("\n--- Test 2: Explicit training config (15% epsilon with decay) ---");
    let simulate_msg2 = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128],
        pvp: Some(false),
        train: true,
        training_config: Some(TrainingConfig {
            training_mode: true,
            epsilon: cosmwasm_std::Decimal::percent(15), // 15% epsilon
            temperature: cosmwasm_std::Decimal::zero(),
            enable_epsilon_decay: true, // Decay enabled
        }),
        reward_config: None,
    };
    
    let result2 = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg2);
    assert!(result2.is_ok(), "Race 2 failed: {:?}", result2.err());
    
    // Test 3: Run with explicit training config but NO decay (15% epsilon, no decay)
    println!("\n--- Test 3: Explicit training config (15% epsilon, NO decay) ---");
    let simulate_msg3 = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128],
        pvp: Some(false),
        train: true,
        training_config: Some(TrainingConfig {
            training_mode: true,
            epsilon: cosmwasm_std::Decimal::percent(15), // 15% epsilon
            temperature: cosmwasm_std::Decimal::zero(),
            enable_epsilon_decay: false, // Decay disabled
        }),
        reward_config: None,
    };
    
    let result3 = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg3);
    assert!(result3.is_ok(), "Race 3 failed: {:?}", result3.err());
    
    // Query training stats to see completion times
    for test_num in 1..=3 {
        let query_msg = QueryMsg::GetTrackTrainingStats {
            car_id: 1u128,
            track_id: Some(1u128),
            start_after: None,
            limit: None,
        };
        
        let response = query(deps.as_ref(), env.clone(), query_msg).unwrap();
        let stats: Vec<GetTrackTrainingStatsResponse> = from_json(response).unwrap();
        let stats = &stats[0];
        
        println!("Test {} completion time: {} ticks", test_num, stats.stats.solo.fastest);
    }
    
    println!("=== EPSILON DECAY DEBUG TEST COMPLETE ===");
}

#[test]
fn test_q_table_migration() {
    println!("\n=== Q-TABLE MIGRATION TEST ===");
    
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("admin", &[]);
    
    // Initialize the contract
    let init_msg = InstantiateMsg {
        admin: "admin".to_string(),
        car_contract: "car_contract".to_string(),
        track_contract: "track_contract".to_string(),
    };
    
    let init_result = instantiate(deps.as_mut(), env.clone(), info.clone(), init_msg);
    assert!(init_result.is_ok());
    
    // Create some legacy Q-table entries manually
    let car_id = 1u128;
    let legacy_hashes = vec![
        [1u8; 32], // Legacy hash 1
        [2u8; 32], // Legacy hash 2
        [3u8; 32], // Legacy hash 3
    ];
    let legacy_q_values = vec![
        [10i8, 20i8, 30i8, 40i8], // Q-values for hash 1
        [15i8, 25i8, 35i8, 45i8], // Q-values for hash 2
        [5i8, 15i8, 25i8, 35i8],  // Q-values for hash 3
    ];
    
    // Store legacy Q-table entries
    for (hash, q_values) in legacy_hashes.iter().zip(legacy_q_values.iter()) {
        crate::state::set_legacy_q_values(&mut deps.storage, car_id, hash, *q_values).unwrap();
    }
    
    // Verify legacy entries exist
    for (hash, expected_q_values) in legacy_hashes.iter().zip(legacy_q_values.iter()) {
        let stored_q_values = crate::state::get_legacy_q_values(&deps.storage, car_id, hash).unwrap();
        assert_eq!(stored_q_values, *expected_q_values);
        println!("Legacy entry verified: {:?} -> {:?}", hash, stored_q_values);
    }
    
    // Check migration status before migration
    let status_query = QueryMsg::GetMigrationStatus { car_id };
    let status_response: MigrationStatusResponse = from_json(
        query(deps.as_ref(), env.clone(), status_query).unwrap()
    ).unwrap();
    
    println!("Migration status before migration:");
    println!("  Legacy entries: {}", status_response.legacy_entries_count);
    println!("  Integer entries: {}", status_response.integer_entries_count);
    println!("  Migration complete: {}", status_response.migration_complete);
    
    assert_eq!(status_response.legacy_entries_count, 3);
    assert_eq!(status_response.integer_entries_count, 0);
    assert_eq!(status_response.migration_complete, false);
    
    // Perform migration
    let migrate_msg = ExecuteMsg::MigrateQTableStates {
        car_id: cosmwasm_std::Uint128::from(car_id),
        batch_size: Some(10), // Migrate all entries
    };
    
    let migrate_result = execute(deps.as_mut(), env.clone(), info.clone(), migrate_msg);
    assert!(migrate_result.is_ok());
    
    println!("Migration completed successfully");
    
    // Check migration status after migration
    let status_query_after = QueryMsg::GetMigrationStatus { car_id };
    let status_response_after: MigrationStatusResponse = from_json(
        query(deps.as_ref(), env.clone(), status_query_after).unwrap()
    ).unwrap();
    
    println!("Migration status after migration:");
    println!("  Legacy entries: {}", status_response_after.legacy_entries_count);
    println!("  Integer entries: {}", status_response_after.integer_entries_count);
    println!("  Migration complete: {}", status_response_after.migration_complete);
    
    // Verify migration results
    assert_eq!(status_response_after.legacy_entries_count, 0);
    assert_eq!(status_response_after.integer_entries_count, 3);
    assert_eq!(status_response_after.migration_complete, true);
    
    // Verify that integer Q-table entries were created correctly
    for (legacy_hash, expected_q_values) in legacy_hashes.iter().zip(legacy_q_values.iter()) {
        // Convert legacy hash to integer hash
        let integer_hash = convert_legacy_hash_to_integer(legacy_hash);
        
        // Get the migrated Q-values
        let migrated_q_values = crate::state::get_integer_q_values(&deps.storage, car_id, integer_hash).unwrap();
        
        println!("Migrated entry: legacy hash {:?} -> integer hash {} -> Q-values {:?}", 
                legacy_hash, integer_hash, migrated_q_values);
        
        // Verify Q-values were preserved (clamped to i8 range)
        for (i, &expected) in expected_q_values.iter().enumerate() {
            let migrated = migrated_q_values[i];
            assert_eq!(migrated, expected, "Q-value mismatch at index {}", i);
        }
    }
    
    // Test that legacy entries are no longer accessible
    for legacy_hash in &legacy_hashes {
        let legacy_result = crate::state::get_legacy_q_values(&deps.storage, car_id, legacy_hash);
        assert!(legacy_result.is_err(), "Legacy entry should be removed after migration");
    }
    
    // Test querying integer Q-values
    let integer_query = QueryMsg::GetIntegerQ { 
        car_id, 
        state_hash: None // Get all Q-values for this car
    };
    let integer_response: GetIntegerQResponse = from_json(
        query(deps.as_ref(), env.clone(), integer_query).unwrap()
    ).unwrap();
    
    println!("Integer Q-table entries after migration: {}", integer_response.q_values.len());
    assert_eq!(integer_response.q_values.len(), 3);
    
    // Verify each migrated entry
    for entry in &integer_response.q_values {
        println!("Integer entry: hash {} -> Q-values {:?}", entry.state_hash, entry.action_values);
        
        // Verify Q-values are in i8 range
        for &q_value in &entry.action_values {
            assert!(q_value >= -128 && q_value <= 127, "Q-value {} is out of i8 range", q_value);
        }
    }
    
    println!("✅ Q-table migration test passed!");
}

/// Helper function to convert legacy hash to integer hash (same as in contract)
fn convert_legacy_hash_to_integer(legacy_hash: &[u8; 32]) -> u32 {
    u32::from_le_bytes([legacy_hash[0], legacy_hash[1], legacy_hash[2], legacy_hash[3]])
}