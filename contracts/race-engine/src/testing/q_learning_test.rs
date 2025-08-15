#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    /// Q-Learning parameters (same as in the contract)
    const ALPHA: f32 = 0.1; // Learning rate
    const GAMMA: f32 = 0.9; // Discount factor
    const MAX_Q_VALUE: i32 = 100;
    const MIN_Q_VALUE: i32 = -100;

    #[test]
    fn test_q_learning_demonstration() {
        println!("\n=== Q-LEARNING DEMONSTRATION ===");
        println!("This demonstrates how Q-learning works in the racing system");
        
        // Simulate a car learning to race
        println!("Simulating a car learning to race...");
        
        // Track Q-values for different racing situations
        let mut q_table: HashMap<String, [i32; 4]> = HashMap::new();
        
        // Initialize Q-values for different states
        let states = vec![
            "start_line".to_string(),
            "middle_track".to_string(),
            "near_finish".to_string(),
            "near_wall".to_string(),
            "on_boost".to_string(),
        ];
        
        for state in &states {
            q_table.insert(state.clone(), [0; 4]); // Initialize all actions to 0
        }
        
        // Simulate racing experiences
        let experiences = vec![
            // (state, action, reward, next_state)
            ("start_line", 0, 2, "middle_track"),      // UP from start: +2 reward
            ("middle_track", 0, 3, "near_finish"),     // UP from middle: +3 reward
            ("near_finish", 0, 10, "finished"),        // UP to finish: +10 reward
            ("middle_track", 3, -5, "near_wall"),      // RIGHT into wall: -5 reward
            ("near_wall", 2, 1, "middle_track"),       // LEFT away from wall: +1 reward
            ("middle_track", 0, 4, "near_finish"),     // UP again: +4 reward
            ("near_finish", 0, 12, "finished"),        // UP to finish again: +12 reward
            ("start_line", 0, 3, "middle_track"),      // UP from start again: +3 reward
            ("middle_track", 0, 5, "near_finish"),     // UP from middle again: +5 reward
            ("near_finish", 0, 15, "finished"),        // UP to finish again: +15 reward
        ];
        
        println!("Running {} learning experiences...", experiences.len());
        
        for (i, (state, action, reward, next_state)) in experiences.iter().enumerate() {
            println!("\n--- Experience {} ---", i + 1);
            println!("State: {}", state);
            println!("Action: {} ({})", action, ["UP", "DOWN", "LEFT", "RIGHT"][*action]);
            println!("Reward: {}", reward);
            println!("Next state: {}", next_state);
            
            // Get current Q-values for this state
            let mut current_q = q_table.get(*state).unwrap().clone();
            println!("Current Q-values: {:?}", current_q);
            
            // Get max Q-value for next state (if it exists in our table)
            let max_next_q = if let Some(next_q) = q_table.get(*next_state) {
                let max_q = next_q.iter().max().cloned().unwrap_or(0);
                println!("Next state Q-values: {:?}", next_q);
                println!("Max next Q-value: {}", max_q);
                max_q
            } else {
                println!("Next state not in Q-table (terminal state)");
                0
            };
            
            // Apply Q-learning update: Q(s,a) = Q(s,a) + α[r + γ max Q(s',a') - Q(s,a)]
            let old_q = current_q[*action];
            let new_q = ((1.0 - ALPHA) * (old_q as f32) + 
                        ALPHA * ((*reward as f32) + (GAMMA * (max_next_q as f32)))).round() as i32;
            
            // Clamp the value
            let clamped_q = new_q.clamp(MIN_Q_VALUE, MAX_Q_VALUE);
            
            println!("Q-learning update:");
            println!("  Old Q-value: {}", old_q);
            println!("  New Q-value: {}", clamped_q);
            println!("  Change: {}", clamped_q - old_q);
            
            // Update the Q-value
            current_q[*action] = clamped_q;
            q_table.insert(state.to_string(), current_q);
            
            println!("Updated Q-values: {:?}", current_q);
            
            // Find the best action for this state
            let best_action = current_q.iter().enumerate()
                .max_by_key(|(_, &val)| val)
                .map(|(idx, _)| idx)
                .unwrap_or(0);
            
            let action_names = ["UP", "DOWN", "LEFT", "RIGHT"];
            println!("Best action for this state: {} (Q-value: {})", 
                    action_names[best_action], current_q[best_action]);
        }
        
        // Analyze the learned Q-table
        println!("\n=== FINAL Q-TABLE ANALYSIS ===");
        
        for state in &states {
            let q_values = q_table.get(state).unwrap();
            let best_action = q_values.iter().enumerate()
                .max_by_key(|(_, &val)| val)
                .map(|(idx, _)| idx)
                .unwrap_or(0);
            
            let action_names = ["UP", "DOWN", "LEFT", "RIGHT"];
            println!("State '{}':", state);
            println!("  Q-values: {:?}", q_values);
            println!("  Best action: {} (Q-value: {})", 
                    action_names[best_action], q_values[best_action]);
            
            // Analyze learning effectiveness
            let max_q = q_values.iter().max().unwrap_or(&0);
            let min_q = q_values.iter().min().unwrap_or(&0);
            let q_spread = max_q - min_q;
            
            if q_spread > 8 {
                println!("  ✅ Strong action preference learned");
            } else if q_spread > 4 {
                println!("  📈 Moderate action preference learned");
            } else {
                println!("  ⚠️  Weak action preference");
            }
        }
        
        // Check if the learning makes sense
        println!("\n=== LEARNING VALIDATION ===");
        
        // Check if UP is preferred in most states (since it leads to finish)
        let up_preferences = states.iter().map(|state| {
            let q_values = q_table.get(state).unwrap();
            q_values[0] > q_values[1] && q_values[0] > q_values[2] && q_values[0] > q_values[3]
        }).collect::<Vec<bool>>();
        
        let sensible_up_count = up_preferences.iter().filter(|&&pref| pref).count();
        println!("States where UP is preferred: {}/{}", sensible_up_count, states.len());
        
        // Check if wall avoidance is learned
        let near_wall_q = q_table.get("near_wall").unwrap();
        let left_better_than_right = near_wall_q[2] > near_wall_q[3];
        println!("Wall avoidance learned (LEFT > RIGHT near wall): {}", left_better_than_right);
        
        // Overall assessment
        let learning_success = sensible_up_count >= 3 && left_better_than_right;
        println!("Overall learning success: {}", 
                if learning_success { "✅ SUCCESS" } else { "⚠️  NEEDS IMPROVEMENT" });
        
        println!("\n🎯 Q-Learning Demonstration Complete!");
        println!("This shows how cars learn optimal racing strategies through experience.");
        println!("In the actual system, these Q-values would be stored in the car contract.");
        println!("The car would use these values to choose the best action in each situation.");
        
        // Assert that learning was successful
        assert!(learning_success, "Q-learning should demonstrate successful learning");
    }

    #[test]
    fn test_q_learning_formula() {
        println!("\n=== Q-LEARNING FORMULA DEMONSTRATION ===");
        println!("Q(s,a) = Q(s,a) + α[r + γ max Q(s',a') - Q(s,a)]");
        println!("Where:");
        println!("  α (alpha) = learning rate = {}", ALPHA);
        println!("  γ (gamma) = discount factor = {}", GAMMA);
        println!("  r = immediate reward");
        println!("  max Q(s',a') = maximum Q-value for next state");
        
        // Example calculation
        let old_q_value = 5;
        let reward = 10;
        let max_next_q = 8;
        
        println!("\nExample calculation:");
        println!("  Old Q-value: {}", old_q_value);
        println!("  Reward: {}", reward);
        println!("  Max next Q-value: {}", max_next_q);
        
        let new_q_value = ((1.0 - ALPHA) * (old_q_value as f32) + 
                           ALPHA * ((reward as f32) + (GAMMA * (max_next_q as f32)))).round() as i32;
        
        println!("  New Q-value: {}", new_q_value);
        println!("  Change: {}", new_q_value - old_q_value);
        
        println!("\nThis formula allows the agent to learn from experience!");
        println!("Positive rewards increase Q-values, making actions more likely to be chosen.");
        println!("Negative rewards decrease Q-values, making actions less likely to be chosen.");
        
        // Verify the calculation is reasonable
        assert!(new_q_value > old_q_value, "Q-value should increase with positive reward");
        assert!(new_q_value <= MAX_Q_VALUE, "Q-value should be clamped to maximum");
    }
} 