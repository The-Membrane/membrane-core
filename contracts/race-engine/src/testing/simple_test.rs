// Simple test to verify the contract structure works
#[cfg(test)]
mod tests {
    use crate::state::{Config, RaceResult, CarPosition};
    use cosmwasm_std::Addr;

    #[test]
    fn test_basic_structure() {
        // Test that we can create basic types
        let config = Config {
            car_nft_contract: Addr::unchecked("car_nft"),
            trainer_contract: Addr::unchecked("trainer"),
            track_manager_contract: Addr::unchecked("track_manager"),
        };
        
        assert_eq!(config.car_nft_contract, Addr::unchecked("car_nft"));
        assert_eq!(config.trainer_contract, Addr::unchecked("trainer"));
        assert_eq!(config.track_manager_contract, Addr::unchecked("track_manager"));
    }

    #[test]
    fn test_race_result_creation() {
        let result = RaceResult {
            race_id: "race_123".to_string(),
            track_id: "track_1".to_string(),
            winner_ids: vec!["car_1".to_string()],
            rankings: vec![("car_1".to_string(), 10), ("car_2".to_string(), 15)],
            play_by_play: vec!["Tick 1: Car 1 moved".to_string()],
            timestamp: 1234567890,
        };
        
        assert_eq!(result.race_id, "race_123");
        assert_eq!(result.winner_ids.len(), 1);
        assert_eq!(result.rankings.len(), 2);
    }

    #[test]
    fn test_car_position() {
        let position = CarPosition {
            car_id: "car_1".to_string(),
            x: 5,
            y: 10,
            stuck: false,
        };
        
        assert_eq!(position.car_id, "car_1");
        assert_eq!(position.x, 5);
        assert_eq!(position.y, 10);
        assert_eq!(position.stuck, false);
    }
} 


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_epsilon_decay_calculation() {
        // Test epsilon decay calculation
        let initial_epsilon = 0.3;
        let final_epsilon = 0.01;
        let total_ticks = 100;
        
        // At 0% progress (tick 0)
        let progress_0 = 0.0;
        let epsilon_0 = initial_epsilon - (initial_epsilon - final_epsilon) * progress_0;
        assert_eq!(epsilon_0, 0.3);
        
        // At 50% progress (tick 50)
        let progress_50 = 0.5;
        let epsilon_50 = initial_epsilon - (initial_epsilon - final_epsilon) * progress_50;
        assert_eq!(epsilon_50, 0.155); // 0.3 - (0.3 - 0.01) * 0.5 = 0.3 - 0.145 = 0.155
        
        // At 100% progress (tick 100)
        let progress_100 = 1.0;
        let epsilon_100 = initial_epsilon - (initial_epsilon - final_epsilon) * progress_100;
        assert_eq!(epsilon_100, 0.01);
    }
}

/// Test epsilon decay calculation independently
pub fn test_epsilon_decay() {
    let initial_epsilon = 0.3;
    let final_epsilon = 0.01;
    let total_ticks = 100;
    
    println!("Testing Epsilon Decay vs Regular Epsilon Greedy");
    println!("===============================================");
    println!("Initial epsilon: {}", initial_epsilon);
    println!("Final epsilon: {}", final_epsilon);
    println!("Total ticks: {}", total_ticks);
    println!();
    
    // Test epsilon decay at different progress points
    for tick in (0..=total_ticks).step_by(20) {
        let progress = tick as f32 / total_ticks as f32;
        let decay_epsilon = initial_epsilon - (initial_epsilon - final_epsilon) * progress;
        
        println!("Tick {} ({:.0}%): Decay={:.3}, Regular={:.3}", 
                tick, progress * 100.0, decay_epsilon, initial_epsilon);
    }
    
    println!();
    println!("Epsilon Decay Strategy:");
    println!("- Early training: High exploration (30% → 21%)");
    println!("- Mid training: Balanced (21% → 9%)");
    println!("- Late training: Low exploration (9% → 1%)");
    println!();
    println!("Regular Epsilon Greedy:");
    println!("- Constant exploration rate (30%) throughout training");
    println!();
    println!("Use enable_epsilon_decay=true for decay, false for constant epsilon");
    
    // Verify the calculations
    let progress_50 = 0.5;
    let epsilon_50 = initial_epsilon - (initial_epsilon - final_epsilon) * progress_50;
    assert!((epsilon_50 - 0.155_f32).abs() < 0.001_f32);
    
    println!("All calculations verified!");
}


