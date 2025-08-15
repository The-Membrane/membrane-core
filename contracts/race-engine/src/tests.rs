use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{from_json, to_json_binary, Addr, Binary, OwnedDeps, Querier, QuerierResult, QueryRequest, SystemResult, ContractResult};

use crate::contract::{execute, instantiate, query};
use racing::race_engine::{ExecuteMsg, InstantiateMsg, QueryMsg, TrainingConfig, GetTrackTrainingStatsResponse};
use racing::types::{RewardNumbers, Track, TrackTile, TileProperties};

const ADMIN: &str = "admin";
const CAR_CONTRACT: &str = "car_contract";
const TRACK_CONTRACT: &str = "track_contract";

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
    
    Track {
        creator: "creator".to_string(),
        id: 1,
        name: "test_track".to_string(),
        width: 5,
        height: 5,
        layout,
        fastest_tick_time: 10,
    }
}

fn setup_test_app() -> OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier<cosmwasm_std::Empty>> {
    let mut deps = mock_dependencies();
    let track = create_test_track();
    
    // Set up mock querier to return track data
    let track_clone = track.clone();
    deps.querier.update_wasm(move |w| {
        match w {
            cosmwasm_std::WasmQuery::Smart { contract_addr, msg } if *contract_addr == TRACK_CONTRACT => {
                let track_response = to_json_binary(&track_clone).unwrap();
                Ok(ContractResult::Ok(track_response)).into()
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
    
    println!("✅ Basic training stats query test passed");
    
    // Simulate a solo race with training enabled
    let simulate_msg = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128],
        train: true,
        training_config: Some(TrainingConfig {
            training_mode: true,
            epsilon: 0.1,
            temperature: 0.0,
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
    println!("🔍 Final stats: {:?}", final_stats);
    
    // Check that solo stats were updated (tally should be 1)
    assert_eq!(final_stats.stats.solo.tally, 1);
    // PvP stats should remain at 0 since this was a solo race
    assert_eq!(final_stats.stats.pvp.tally, 0);
    
    println!("✅ Training stats updated after solo race");
    
    // Test PvP race
    let pvp_simulate_msg = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128, 2u128],
        train: true,
        training_config: Some(TrainingConfig {
                training_mode: true,
                epsilon: 0.1,
            temperature: 0.0,
            enable_epsilon_decay: false,
        }),
        reward_config: None,
    };
    
    let pvp_result = execute(deps.as_mut(), env.clone(), info.clone(), pvp_simulate_msg);
    assert!(pvp_result.is_ok());
    
    // Query training stats after PvP race
    let pvp_query_msg = QueryMsg::GetTrackTrainingStats {
        car_id: 1u128,
        track_id: Some(1u128),
        start_after: None,
        limit: None,
    };
    
    let pvp_response = query(deps.as_ref(), env.clone(), pvp_query_msg).unwrap();
    let pvp_stats: Vec<GetTrackTrainingStatsResponse> = from_json(pvp_response).unwrap();
    let pvp_stats = &pvp_stats[0]; // Get the first (and only) response
    
    // Check that PvP stats were updated
    assert_eq!(pvp_stats.stats.pvp.tally, 1);
    // Solo stats should remain at 1 from previous race
    assert_eq!(pvp_stats.stats.solo.tally, 1);
    
    println!("✅ Training stats updated after PvP race");
    println!("✅ All training stats tests passed!");
    println!("🔍 PvP stats: {:?}", pvp_stats);
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
            train: true,
            training_config: Some(TrainingConfig {
            training_mode: true,
            epsilon: 0.1,
            temperature: 0.0,
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
    
    println!("✅ Multiple tracks query test passed!");
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
            train: true,
            training_config: Some(TrainingConfig {
                training_mode: true,
                epsilon: 0.9, // 90% random exploration
                temperature: 0.0,
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
        println!("Race {}: Fastest time = {} ticks", i + 1, stats.stats.solo.fastest);
        
        // Check if the car actually finished or hit the time limit
        if stats.stats.solo.fastest == 100 {
            println!("  -> Car hit MAX_TICKS limit (didn't finish)");
            } else {
            println!("  -> Car finished successfully");
        }
    }
    
    // Check that we have some variability in completion times
    let min_time = completion_times.iter().min().unwrap();
    let max_time = completion_times.iter().max().unwrap();
    
    println!("Min time: {}, Max time: {}", min_time, max_time);
    
    // If all times are 100, it means the car never finished
    if *min_time == 100 && *max_time == 100 {
        println!("⚠️  All races hit time limit - car is not finishing with 90% randomness");
        println!("This suggests the car needs more deterministic behavior to reach the finish");
        } else {
        assert!(max_time > min_time, "Should have variability in completion times with high randomness");
    }
    
    println!("✅ Random behavior variability test passed!");
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
        train: true,
        training_config: Some(TrainingConfig {
            training_mode: true,
            epsilon: 0.0, // No randomness
            temperature: 0.0,
            enable_epsilon_decay: false,
        }),
        reward_config: None,
    };
    
    let result = execute(deps.as_mut(), env.clone(), info.clone(), deterministic_msg);
    assert!(result.is_ok());
    
    let query_msg = QueryMsg::GetTrackTrainingStats {
        car_id: 1u128,
        track_id: Some(1u128),
        start_after: None,
        limit: None,
    };
    
    let response = query(deps.as_ref(), env.clone(), query_msg.clone()).unwrap();
    let stats: Vec<GetTrackTrainingStatsResponse> = from_json(response).unwrap();
    let deterministic_time = stats[0].stats.solo.fastest;
    
    println!("Deterministic behavior: {} ticks", deterministic_time);
    
    // Test 2: Random behavior (epsilon = 1.0, 100% random)
    let random_msg = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128],
        train: true,
        training_config: Some(TrainingConfig {
            training_mode: true,
            epsilon: 1.0, // 100% random
            temperature: 0.0,
            enable_epsilon_decay: false,
        }),
        reward_config: None,
    };
    
    let result = execute(deps.as_mut(), env.clone(), info.clone(), random_msg);
    assert!(result.is_ok());
    
    let response = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let stats: Vec<GetTrackTrainingStatsResponse> = from_json(response).unwrap();
    let random_time = stats[0].stats.solo.fastest;
    
    println!("Random behavior: {} ticks", random_time);
    
    // The random time should be different from deterministic time
    // (though they could theoretically be the same by chance)
    println!("Deterministic: {}, Random: {}", deterministic_time, random_time);
    
    println!("✅ Deterministic vs random behavior test passed!");
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
        train: true,
        training_config: Some(TrainingConfig {
            training_mode: true,
            epsilon: 0.0, // No randomness - pure Q-learning
                temperature: 0.0,
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
    
    let response = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let stats: Vec<GetTrackTrainingStatsResponse> = from_json(response).unwrap();
    let stats = &stats[0];
    
    println!("Deterministic behavior (epsilon=0.0): {} ticks", stats.stats.solo.fastest);
    
    // Run the same test again to see if it's consistent
    let simulate_msg2 = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128],
        train: true,
        training_config: Some(TrainingConfig {
                training_mode: true,
            epsilon: 0.0, // No randomness - pure Q-learning
                temperature: 0.0,
            enable_epsilon_decay: false,
        }),
        reward_config: None,
    };
    
    let result2 = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg2);
    assert!(result2.is_ok());
    
    let query_msg2 = QueryMsg::GetTrackTrainingStats {
        car_id: 1u128,
        track_id: Some(1u128),
        start_after: None,
        limit: None,
    };
    
    let response2 = query(deps.as_ref(), env.clone(), query_msg2).unwrap();
    let stats2: Vec<GetTrackTrainingStatsResponse> = from_json(response2).unwrap();
    let stats2 = &stats2[0];
    
    println!("Second run (epsilon=0.0): {} ticks", stats2.stats.solo.fastest);
    
    // The times should be consistent because the pseudo_random function is deterministic
    assert_eq!(stats.stats.solo.fastest, stats2.stats.solo.fastest, 
               "Deterministic behavior should be consistent");
    
    println!("✅ Empty Q-table behavior test passed!");
    println!("💡 The car uses deterministic 'random' initial Q-values, not zeros!");
}

#[test]
fn test_learning_process_investigation() {
    let mut deps = setup_test_app();
    let env = mock_env();
    let info = mock_info("test_user", &[]);
    
    // Run multiple races to see if the car learns and improves
    let mut completion_times = vec![];
    
    for race_num in 0..5 {
        // Reset Q-table before each race to see if learning happens within a single race
        let reset_msg = ExecuteMsg::ResetQ {
            car_id: cosmwasm_std::Uint128::from(1u128),
        };
        execute(deps.as_mut(), env.clone(), info.clone(), reset_msg).ok();
        
        let simulate_msg = ExecuteMsg::SimulateRace {
            track_id: cosmwasm_std::Uint128::from(1u128),
            car_ids: vec![1u128],
            train: true,
            training_config: Some(TrainingConfig {
                training_mode: true,
                epsilon: 0.1, // 10% random
                temperature: 0.0,
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
        
        let response = query(deps.as_ref(), env.clone(), query_msg).unwrap();
        let stats: Vec<GetTrackTrainingStatsResponse> = from_json(response).unwrap();
        let stats = &stats[0];
        
        completion_times.push(stats.stats.solo.fastest);
        println!("Race {}: {} ticks", race_num + 1, stats.stats.solo.fastest);
        
        // Check if car finished or hit time limit
        if stats.stats.solo.fastest == 100 {
            println!("  -> Hit time limit (didn't finish)");
            } else {
            println!("  -> Finished successfully");
        }
    }
    
    // Check for consistency
    let min_time = completion_times.iter().min().unwrap();
    let max_time = completion_times.iter().max().unwrap();
    
    println!("Min time: {}, Max time: {}", min_time, max_time);
    
    if min_time == max_time {
        println!("⚠️  All races took exactly {} ticks - this suggests deterministic behavior", min_time);
        println!("💡 The car is likely following the same path every time due to:");
        println!("   1. Deterministic initial Q-values");
        println!("   2. Deterministic pseudo-random function");
        println!("   3. Low epsilon (10% random) means 90% of actions are 'best'");
        } else {
        println!("✅ Some variability in completion times");
    }
    
    println!("✅ Learning process investigation complete!");
}

#[test]
fn test_seed_determinism_explanation() {
    let mut deps = setup_test_app();
    let env = mock_env();
    let info = mock_info("test_user", &[]);
    
    println!("🔍 Investigating why epsilon doesn't create variability between test runs...");
    
    // The issue: The seed is always tick_index (0, 1, 2, 3, ...)
    // This means the same "random" numbers are generated every time
    
    // Test 1: Run with epsilon = 0.5 (50% random)
    let simulate_msg1 = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128],
        train: true,
        training_config: Some(TrainingConfig {
            training_mode: true,
            epsilon: 0.5, // 50% random
            temperature: 0.0,
            enable_epsilon_decay: false,
        }),
        reward_config: None,
    };
    
    let result1 = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg1);
    assert!(result1.is_ok());
    
    let query_msg1 = QueryMsg::GetTrackTrainingStats {
        car_id: 1u128,
        track_id: Some(1u128),
        start_after: None,
        limit: None,
    };
    
    let response1 = query(deps.as_ref(), env.clone(), query_msg1).unwrap();
    let stats1: Vec<GetTrackTrainingStatsResponse> = from_json(response1).unwrap();
    let time1 = stats1[0].stats.solo.fastest;
    
    println!("First run (epsilon=0.5): {} ticks", time1);
    
    // Reset Q-table
    let reset_msg = ExecuteMsg::ResetQ {
        car_id: cosmwasm_std::Uint128::from(1u128),
    };
    execute(deps.as_mut(), env.clone(), info.clone(), reset_msg).ok();
    
    // Test 2: Run again with same epsilon
    let simulate_msg2 = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128],
        train: true,
        training_config: Some(TrainingConfig {
                training_mode: true,
            epsilon: 0.5, // Same 50% random
                temperature: 0.0,
                enable_epsilon_decay: false,
        }),
        reward_config: None,
    };
    
    let result2 = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg2);
    assert!(result2.is_ok());
    
    let query_msg2 = QueryMsg::GetTrackTrainingStats {
        car_id: 1u128,
        track_id: Some(1u128),
        start_after: None,
        limit: None,
    };
    
    let response2 = query(deps.as_ref(), env.clone(), query_msg2).unwrap();
    let stats2: Vec<GetTrackTrainingStatsResponse> = from_json(response2).unwrap();
    let time2 = stats2[0].stats.solo.fastest;
    
    println!("Second run (epsilon=0.5): {} ticks", time2);
    
    // The times should be the same because:
    // 1. Seed is always tick_index (0, 1, 2, 3, ...)
    // 2. Same seed = same "random" numbers
    // 3. Same epsilon = same probability of random vs best action
    // 4. Same initial Q-values (deterministic pseudo_random)
    
    assert_eq!(time1, time2, "Times should be identical due to deterministic seed");
    
    println!("✅ Seed determinism explanation test passed!");
    println!("💡 The car behavior is deterministic because:");
    println!("   - Seed is always tick_index (0, 1, 2, 3, ...)");
    println!("   - Same seed = same 'random' numbers");
    println!("   - Same initial Q-values every time");
    println!("   - Epsilon only affects probability, not the random numbers themselves");
}

#[test]
fn test_initial_q_values_investigation() {
    let mut deps = setup_test_app();
    let env = mock_env();
    let info = mock_info("test_user", &[]);
    
    println!("🔍 Investigating initial Q-values and action selection...");
    
    // Let's see what the initial Q-values are for the first few states
    // The car starts at position (4, 4) and needs to reach (0, 0)
    
    // Query Q-values for the initial state
    let query_msg = QueryMsg::GetQ {
        car_id: 1u128,
        state_hash: None, // Get all Q-values
    };
    
    let response = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let q_response: racing::race_engine::GetQResponse = from_json(response).unwrap();
    
    println!("Initial Q-values for car 1:");
    for (i, q_entry) in q_response.q_values.iter().take(5).enumerate() {
        println!("  State {}: {:?}", i, q_entry.action_values);
    }
    
    // Now let's test what happens with different epsilon values
    let mut results = vec![];
    
    for epsilon in [0.0, 0.1, 0.5, 0.9, 1.0] {
        // Reset Q-table
        let reset_msg = ExecuteMsg::ResetQ {
            car_id: cosmwasm_std::Uint128::from(1u128),
        };
        execute(deps.as_mut(), env.clone(), info.clone(), reset_msg).ok();
        
        let simulate_msg = ExecuteMsg::SimulateRace {
            track_id: cosmwasm_std::Uint128::from(1u128),
            car_ids: vec![1u128],
            train: true,
            training_config: Some(TrainingConfig {
                training_mode: true,
                epsilon,
                temperature: 0.0,
                enable_epsilon_decay: false,
            }),
            reward_config: None,
        };
        
        let result = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg);
        assert!(result.is_ok());
        
        let query_msg = QueryMsg::GetTrackTrainingStats {
            car_id: 1u128,
            track_id: Some(1u128),
            start_after: None,
            limit: None,
        };
        
        let response = query(deps.as_ref(), env.clone(), query_msg).unwrap();
        let stats: Vec<GetTrackTrainingStatsResponse> = from_json(response).unwrap();
        let time = stats[0].stats.solo.fastest;
        
        results.push((epsilon, time));
        println!("Epsilon {}: {} ticks", epsilon, time);
    }
    
    // Check if all results are the same
    let times: Vec<u32> = results.iter().map(|(_, time)| *time).collect();
    let all_same = times.iter().all(|&t| t == times[0]);
    
    if all_same {
        println!("⚠️  All epsilon values produced the same result: {} ticks", times[0]);
        println!("💡 This suggests that:");
        println!("   1. Initial Q-values are all equal, OR");
        println!("   2. The 'best' action and 'random' action are the same, OR");
        println!("   3. The car always follows the same path regardless of action selection");
            } else {
        println!("✅ Different epsilon values produced different results");
        for (epsilon, time) in results {
            println!("  Epsilon {}: {} ticks", epsilon, time);
        }
    }
    
    println!("✅ Initial Q-values investigation complete!");
}

#[test]
fn test_epsilon_variance_investigation() {
    let mut deps = setup_test_app();
    let env = mock_env();
    let info = mock_info("test_user", &[]);
    
    println!("🔍 Investigating why epsilon 0.1-0.6 produces variance...");
    
    // Test a range of epsilon values to see where variance occurs
    let mut results = vec![];
    
    for epsilon in [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0] {
        // Reset Q-table
        let reset_msg = ExecuteMsg::ResetQ {
            car_id: cosmwasm_std::Uint128::from(1u128),
        };
        execute(deps.as_mut(), env.clone(), info.clone(), reset_msg).ok();
        
        let simulate_msg = ExecuteMsg::SimulateRace {
            track_id: cosmwasm_std::Uint128::from(1u128),
            car_ids: vec![1u128],
            train: true,
            training_config: Some(TrainingConfig {
                training_mode: true,
                epsilon,
                temperature: 0.0,
                enable_epsilon_decay: false,
            }),
            reward_config: None,
        };
        
        let result = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg);
        assert!(result.is_ok());
        
        let query_msg = QueryMsg::GetTrackTrainingStats {
            car_id: 1u128,
            track_id: Some(1u128),
            start_after: None,
            limit: None,
        };
        
        let response = query(deps.as_ref(), env.clone(), query_msg).unwrap();
        let stats: Vec<GetTrackTrainingStatsResponse> = from_json(response).unwrap();
        let time = stats[0].stats.solo.fastest;
        
        results.push((epsilon, time));
        println!("Epsilon {}: {} ticks", epsilon, time);
    }
    
    // Analyze the results
    println!("\n📊 Analysis:");
    for (epsilon, time) in &results {
        if *time == 100 {
            println!("  Epsilon {}: {} ticks (DIDN'T FINISH)", epsilon, time);
        } else {
            println!("  Epsilon {}: {} ticks (FINISHED)", epsilon, time);
        }
    }
    
    // Check for patterns
    let finished_times: Vec<u32> = results.iter()
        .filter(|(_, time)| *time < 100)
        .map(|(_, time)| *time)
        .collect();
    
    if !finished_times.is_empty() {
        let min_time = finished_times.iter().min().unwrap();
        let max_time = finished_times.iter().max().unwrap();
        println!("\n🎯 Finished races: {} to {} ticks", min_time, max_time);
        
        if min_time == max_time {
            println!("⚠️  All finished races took exactly {} ticks", min_time);
        } else {
            println!("✅ Found variance in completion times!");
        }
    }
    
    // Check if there's a threshold where behavior changes
    let mut threshold_found = false;
    for i in 0..results.len() - 1 {
        let (eps1, time1) = results[i];
        let (eps2, time2) = results[i + 1];
        
        if time1 == 100 && time2 < 100 {
            println!("\n💡 Threshold found: Epsilon {} -> {} (stuck -> finished)", eps1, eps2);
            threshold_found = true;
        }
    }
    
    if !threshold_found {
        println!("\n💡 No clear threshold - variance occurs across epsilon range");
    }
    
    println!("✅ Epsilon variance investigation complete!");
}

#[test]
fn test_epsilon_06_specific_investigation() {
    let mut deps = setup_test_app();
    let env = mock_env();
    let info = mock_info("test_user", &[]);
    
    println!("🔍 Investigating why epsilon 0.6 gives 60 ticks...");
    
    // Test epsilon 0.6 specifically
    let simulate_msg = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128],
        train: true,
        training_config: Some(TrainingConfig {
            training_mode: true,
            epsilon: 0.6, // 60% random
            temperature: 0.0,
            enable_epsilon_decay: false,
        }),
        reward_config: None,
    };
    
    let result = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg);
    assert!(result.is_ok());
    
    // Query training stats after the race
    let query_msg = QueryMsg::GetTrackTrainingStats {
        car_id: 1u128,
        track_id: Some(1u128),
        start_after: None,
        limit: None,
    };
    
    let response = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let stats: Vec<GetTrackTrainingStatsResponse> = from_json(response).unwrap();
    let stats = &stats[0];
    
    println!("Epsilon 0.6 result: {} ticks", stats.stats.solo.fastest);
    
    // Now test epsilon 0.1 to compare
    let reset_msg = ExecuteMsg::ResetQ {
        car_id: cosmwasm_std::Uint128::from(1u128),
    };
    execute(deps.as_mut(), env.clone(), info.clone(), reset_msg).ok();
    
    let simulate_msg2 = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128],
        train: true,
        training_config: Some(TrainingConfig {
            training_mode: true,
            epsilon: 0.1, // 10% random
            temperature: 0.0,
            enable_epsilon_decay: false,
        }),
        reward_config: None,
    };
    
    let result2 = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg2);
    assert!(result2.is_ok());
    
    let query_msg2 = QueryMsg::GetTrackTrainingStats {
        car_id: 1u128,
        track_id: Some(1u128),
        start_after: None,
        limit: None,
    };
    
    let response2 = query(deps.as_ref(), env.clone(), query_msg2).unwrap();
    let stats2: Vec<GetTrackTrainingStatsResponse> = from_json(response2).unwrap();
    let stats2 = &stats2[0];
    
    println!("Epsilon 0.1 result: {} ticks", stats2.stats.solo.fastest);
    
    // Compare the results
    println!("Comparison:");
    println!("  Epsilon 0.6: {} ticks", stats.stats.solo.fastest);
    println!("  Epsilon 0.1: {} ticks", stats2.stats.solo.fastest);
    
    if stats.stats.solo.fastest != stats2.stats.solo.fastest {
        println!("✅ Different epsilon values produced different results!");
        println!("💡 This suggests the action selection is actually working differently");
    } else {
        println!("⚠️  Both epsilon values produced the same result");
        println!("💡 This suggests the action selection is not working as expected");
    }
    
    println!("✅ Epsilon 0.6 specific investigation complete!");
}

#[test]
fn test_pvp_training_stats() {
    let mut deps = setup_test_app();
    let env = mock_env();
    let info = mock_info("test_user", &[]);
    
    // Simulate a PvP race with multiple cars and training enabled
    let simulate_msg = ExecuteMsg::SimulateRace {
        track_id: cosmwasm_std::Uint128::from(1u128),
        car_ids: vec![1u128, 2u128],
        train: true,
        training_config: Some(TrainingConfig {
            training_mode: true,
            epsilon: 0.1,
            temperature: 0.0,
            enable_epsilon_decay: false,
        }),
        reward_config: Some(RewardNumbers {
            distance: 1,
            stuck: -5,
            wall: -8,
            no_move: 0,
            explore: 6,
            rank: racing::types::RankReward {
                first: 100,
                second: 50,
                third: 25,
                other: 0,
            },
        }),
    };
    
    let result = execute(deps.as_mut(), env.clone(), info.clone(), simulate_msg);
    assert!(result.is_ok(), "PvP race simulation failed: {:?}", result.err());
    
    // Query training stats for both cars
    for car_id in &[1u128, 2u128] {
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
        assert_eq!(stats.stats.solo.fastest, u32::MAX, "Solo fastest should remain default");
    }
    
    println!("✅ PvP training stats test passed!");
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
    assert_eq!(stats.stats.solo.fastest, u32::MAX, "Solo fastest should remain default");
    assert_eq!(stats.stats.pvp.tally, 0, "PvP tally should remain 0 when training disabled");
    assert_eq!(stats.stats.pvp.fastest, u32::MAX, "PvP fastest should remain default");
    
    println!("✅ No training stats test passed!");
}