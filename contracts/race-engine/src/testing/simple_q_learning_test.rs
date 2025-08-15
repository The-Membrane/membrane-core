#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    /// Q-Learning parameters (same as in the contract)
    const ALPHA: f32 = 0.1; // Learning rate
    const GAMMA: f32 = 0.9; // Discount factor
    const MAX_Q_VALUE: i32 = 100;
    const MIN_Q_VALUE: i32 = -100;

    #[test]
    fn test_simple_q_learning() {
        println!("\n=== SIMPLE Q-LEARNING TEST ===");
        println!("This test demonstrates how Q-learning updates work in the racing system");
        
        // Simulate a car's Q-table for a specific state
        let mut q_values = [0i32; 4]; // [UP, DOWN, LEFT, RIGHT]
        let action_names = ["UP", "DOWN", "LEFT", "RIGHT"];
        
        println!("Initial Q-values: {:?}", q_values);
        
        // Simulate a series of learning experiences
        let learning_experiences = vec![
            (0, 5, Some([10, 5, 3, 8])),   // UP action, reward 5, next state Q-values
            (1, -3, Some([8, 12, 4, 6])),  // DOWN action, reward -3, next state Q-values
            (2, 2, Some([7, 9, 15, 5])),   // LEFT action, reward 2, next state Q-values
            (3, 8, Some([12, 6, 8, 18])),  // RIGHT action, reward 8, next state Q-values
            (0, 6, Some([15, 8, 7, 12])),  // UP action again, reward 6, next state Q-values
        ];
        
        for (action, reward, next_state_q_values) in &learning_experiences {
            println!("\n--- Learning Experience ---");
            println!("Action: {} (index {})", action_names[*action], action);
            println!("Reward: {}", reward);
            
            // Get max Q-value for next state
            let max_next_q = if let Some(next_q) = next_state_q_values {
                let max_q = next_q.iter().max().cloned().unwrap_or(0);
                println!("Next state Q-values: {:?}", next_q);
                println!("Max next Q-value: {}", max_q);
                max_q
            } else {
                println!("No next state (terminal state)");
                0
            };
            
            // Apply Q-learning update formula: Q(s,a) = Q(s,a) + α[r + γ max Q(s',a') - Q(s,a)]
            let old_value = q_values[*action];
            let new_value = ((1.0 - ALPHA) * (old_value as f32) + 
                            ALPHA * ((*reward as f32) + (GAMMA * (max_next_q as f32)))).round() as i32;
            
            // Clamp the value to prevent explosion
            let clamped_value = new_value.clamp(MIN_Q_VALUE, MAX_Q_VALUE);
            
            println!("Q-learning update:");
            println!("  Old Q-value: {}", old_value);
            println!("  New Q-value: {}", clamped_value);
            println!("  Change: {}", clamped_value - old_value);
            
            q_values[*action] = clamped_value;
            
            println!("Updated Q-values: {:?}", q_values);
            
            // Find the best action after this update
            let best_action = q_values.iter().enumerate()
                .max_by_key(|(_, &val)| val)
                .map(|(idx, _)| idx)
                .unwrap_or(0);
            
            println!("Best action after update: {} (Q-value: {})", 
                    action_names[best_action], q_values[best_action]);
        }
        
        // Final analysis
        println!("\n=== FINAL Q-LEARNING ANALYSIS ===");
        println!("Final Q-values: {:?}", q_values);
        
        // Find the best action
        let best_action = q_values.iter().enumerate()
            .max_by_key(|(_, &val)| val)
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        
        println!("Best learned action: {} (Q-value: {})", 
                action_names[best_action], q_values[best_action]);
        
        // Analyze Q-value distribution
        let max_q = q_values.iter().max().unwrap_or(&0);
        let min_q = q_values.iter().min().unwrap_or(&0);
        let q_spread = max_q - min_q;
        
        println!("Q-value analysis:");
        println!("  Max Q-value: {}", max_q);
        println!("  Min Q-value: {}", min_q);
        println!("  Q-value spread: {}", q_spread);
        
        if q_spread > 10 {
            println!("  ✅ Strong action preference learned");
        } else if q_spread > 5 {
            println!("  📈 Moderate action preference learned");
        } else {
            println!("  ⚠️  Weak action preference (may need more training)");
        }
        
        // Check if the best action makes sense given the rewards
        let total_reward_for_best = learning_experiences.iter()
            .filter(|(action, _, _)| *action == best_action)
            .map(|(_, reward, _)| reward)
            .sum::<i32>();
        
        println!("Total reward for best action ({}): {}", 
                action_names[best_action], total_reward_for_best);
        
        if total_reward_for_best > 0 {
            println!("  ✅ Best action has positive total reward");
        } else {
            println!("  ⚠️  Best action has negative total reward");
        }
        
        println!("\n🎯 Q-Learning Test Complete!");
        println!("This demonstrates how Q-values are updated during learning.");
        println!("In the actual racing system, these updates would be applied to the car's Q-table.");
        
        // Assert that learning was successful
        assert!(q_spread > 5, "Q-learning should show some action preference");
        assert!(total_reward_for_best > 0, "Best action should have positive total reward");
    }

    #[test]
    fn test_q_learning_formula() {
        println!("\n=== Q-LEARNING FORMULA TEST ===");
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
        
        println!("✅ Q-learning formula test passed!");
    }
} 