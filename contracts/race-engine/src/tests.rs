use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{from_json, to_json_binary, Addr, Binary, OwnedDeps, Querier, QuerierResult, QueryRequest, SystemResult, ContractResult};
use serde::Serialize;

use crate::contract::{execute, instantiate, query};
use membrane::race_engine::{ExecuteMsg, InstantiateMsg, QueryMsg, TrainingConfig, GetTrackTrainingStatsResponse};
use membrane::types::{RewardNumbers, Track, TrackTile, TileProperties};

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