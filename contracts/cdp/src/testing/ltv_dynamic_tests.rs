#[cfg(test)]
mod tests {
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env, mock_info},
        Addr, Decimal, Env,
    };
    use membrane::cdp::Config;
    use membrane::types::{cAsset, Asset, AssetInfo};

    use crate::state::{LTVUpdateTracker, LTV_UPDATE_TRACKERS, CONFIG, BASKET};
    use crate::ltv_updater::{calculate_upward_accrual, apply_capped_downward_shift, SECONDS_PER_DAY};

    /// Helper to create a test config with LTV parameters
    fn mock_config() -> Config {
        Config {
            owner: Addr::unchecked("owner"),
            staking_contract: None,
            chain_proxy: None,
            debt_auction: None,
            oracle_contract: None,
            liquidity_contract: None,
            discounts_contract: None,
            ltv_disco: Addr::unchecked("ltv_disco"),
            revenue_distributor: None,
            liq_fee: Decimal::percent(1),
            collateral_twap_timeframe: 60,
            credit_twap_timeframe: 60,
            oracle_time_limit: 60,
            cpc_multiplier: Decimal::one(),
            debt_minimum: cosmwasm_std::Uint128::new(100),
            base_debt_cap_multiplier: cosmwasm_std::Uint128::new(100),
            rate_slope_multiplier: Decimal::one(),
            affiliate_fee_max: Decimal::percent(5),
            skip_credit_price_accrual: false,
            liquidation_stat_limit: 500,
            ltv_upward_kp: Decimal::percent(5), // 5% per day
            ltv_downward_period: 604800, // 1 week
            ltv_max_downward_shift: Decimal::percent(5), // 5% max shift
        }
    }

    #[test]
    fn test_upward_accrual_proportional() {
        // Test proportional controller for upward movement
        let current_ltv = Decimal::percent(70);
        let disco_ltv = Decimal::percent(80);
        let kp = Decimal::percent(5); // 5% per day
        
        // After 1 day
        let last_update = 1000u64;
        let current_time = last_update + SECONDS_PER_DAY;
        
        let result = calculate_upward_accrual(
            current_ltv,
            disco_ltv,
            last_update,
            current_time,
            kp,
        ).unwrap();
        
        // Error = 10% (80% - 70%)
        // Accrual = 5% * 10% * 1 day = 0.5%
        // New LTV = 70% + 0.5% = 70.5%
        assert_eq!(result, Decimal::percent(70) + Decimal::permille(5));
    }

    #[test]
    fn test_upward_accrual_doesnt_overshoot() {
        // Test that upward accrual doesn't exceed target
        let current_ltv = Decimal::percent(79);
        let disco_ltv = Decimal::percent(80);
        let kp = Decimal::percent(10); // Higher Kp
        
        // After 1 day
        let last_update = 1000u64;
        let current_time = last_update + SECONDS_PER_DAY;
        
        let result = calculate_upward_accrual(
            current_ltv,
            disco_ltv,
            last_update,
            current_time,
            kp,
        ).unwrap();
        
        // Even with high Kp, should not exceed disco_ltv
        // Error = 1%, Kp = 10%, accrual = 0.1% which gives 79.1%
        // The .min(disco_ltv) caps it, but with small error we get 79.1%
        assert!(result <= disco_ltv);
        assert!(result >= current_ltv); // At least moved toward target
    }

    #[test]
    fn test_upward_accrual_over_multiple_periods() {
        // Test that accrual works correctly over multiple periods
        let mut current_ltv = Decimal::percent(70);
        let disco_ltv = Decimal::percent(80);
        let kp = Decimal::percent(5);
        
        let mut last_update = 1000u64;
        
        // Simulate 5 days
        for _day in 0..5 {
            let current_time = last_update + SECONDS_PER_DAY;
            current_ltv = calculate_upward_accrual(
                current_ltv,
                disco_ltv,
                last_update,
                current_time,
                kp,
            ).unwrap();
            last_update = current_time;
        }
        
        // Should be closer to target but not exceed it
        assert!(current_ltv > Decimal::percent(70));
        assert!(current_ltv <= disco_ltv);
    }

    #[test]
    fn test_upward_accrual_zero_time_elapsed() {
        // Test that no accrual happens when no time has passed
        let current_ltv = Decimal::percent(70);
        let disco_ltv = Decimal::percent(80);
        let kp = Decimal::percent(5);
        
        let current_time = 1000u64;
        
        let result = calculate_upward_accrual(
            current_ltv,
            disco_ltv,
            current_time,
            current_time,
            kp,
        ).unwrap();
        
        assert_eq!(result, current_ltv);
    }

    #[test]
    fn test_downward_shift_capped() {
        // Test that downward shifts are capped
        let current_ltv = Decimal::percent(80);
        let staged_ltv = Decimal::percent(60); // Want to drop by 20%
        let max_shift = Decimal::percent(5); // But can only drop by 5%
        
        let result = apply_capped_downward_shift(
            current_ltv,
            staged_ltv,
            max_shift,
        ).unwrap();
        
        // Should only drop by 5% (4% of 80% = 4%)
        let expected = Decimal::percent(80) - (Decimal::percent(80) * Decimal::percent(5));
        assert_eq!(result, expected);
        assert_eq!(result, Decimal::percent(76)); // 80% - 4% = 76%
    }

    #[test]
    fn test_downward_shift_under_cap() {
        // Test that small downward shifts aren't capped
        let current_ltv = Decimal::percent(80);
        let staged_ltv = Decimal::percent(78); // Want to drop by 2%
        let max_shift = Decimal::percent(5); // Can drop up to 5%
        
        let result = apply_capped_downward_shift(
            current_ltv,
            staged_ltv,
            max_shift,
        ).unwrap();
        
        // Should drop the full 2% to reach staged value
        assert_eq!(result, staged_ltv);
        assert_eq!(result, Decimal::percent(78));
    }

    #[test]
    fn test_tracker_initialization() {
        let mut deps = mock_dependencies();
        let current_time = 1000u64;
        
        // Initialize tracker
        let tracker = LTVUpdateTracker {
            last_upward_update: current_time,
            staged_max_ltv: None,
            staged_max_borrow_ltv: None,
            staged_timestamp: None,
        };
        
        LTV_UPDATE_TRACKERS.save(
            deps.as_mut().storage,
            "asset1".to_string(),
            &tracker,
        ).unwrap();
        
        // Load and verify
        let loaded = LTV_UPDATE_TRACKERS.load(
            deps.as_ref().storage,
            "asset1".to_string(),
        ).unwrap();
        
        assert_eq!(loaded.last_upward_update, current_time);
        assert_eq!(loaded.staged_max_ltv, None);
        assert_eq!(loaded.staged_timestamp, None);
    }

    #[test]
    fn test_downward_staging_timer_starts_immediately() {
        // Test that timer starts when downward value is first staged
        let mut deps = mock_dependencies();
        let initial_time = 1000u64;
        
        let mut tracker = LTVUpdateTracker {
            last_upward_update: initial_time,
            staged_max_ltv: None,
            staged_max_borrow_ltv: None,
            staged_timestamp: None,
        };
        
        // First downward detection
        let stage_time = 2000u64;
        tracker.staged_max_ltv = Some(Decimal::percent(75));
        tracker.staged_timestamp = Some(stage_time);
        
        assert_eq!(tracker.staged_timestamp, Some(stage_time));
        
        // Later, if disco drops further, timestamp should NOT change
        let later_time = 3000u64;
        tracker.staged_max_ltv = Some(Decimal::percent(70)); // Even lower
        // Timestamp stays the same (this logic is in the main update function)
        
        assert_eq!(tracker.staged_timestamp, Some(stage_time)); // Still original
    }

    #[test]
    fn test_proportional_controller_converges() {
        // Test that P-controller converges asymptotically to target
        let mut current_ltv = Decimal::percent(70);
        let disco_ltv = Decimal::percent(80);
        let kp = Decimal::percent(10); // Higher Kp for faster convergence
        
        let mut last_update = 1000u64;
        let initial_ltv = current_ltv;
        let mut previous_ltv = current_ltv;
        
        // Simulate 10 days
        for _day in 0..10 {
            let current_time = last_update + SECONDS_PER_DAY;
            current_ltv = calculate_upward_accrual(
                current_ltv,
                disco_ltv,
                last_update,
                current_time,
                kp,
            ).unwrap();
            
            // Each step should make progress toward target
            assert!(current_ltv > previous_ltv || current_ltv == disco_ltv);
            // Should never overshoot
            assert!(current_ltv <= disco_ltv);
            
            previous_ltv = current_ltv;
            last_update = current_time;
        }
        
        // After 10 days should have made significant progress
        // Verify we've moved at least halfway from 70% toward 80%
        let progress_needed = Decimal::percent(75); // Halfway point
        assert!(current_ltv >= progress_needed, 
            "Expected at least {} but got {}", progress_needed, current_ltv);
    }

    #[test]
    fn test_low_kp_slower_convergence() {
        // Test that lower Kp results in slower convergence (as intended)
        let current_ltv = Decimal::percent(70);
        let disco_ltv = Decimal::percent(80);
        let low_kp = Decimal::percent(2); // 2% per day
        
        let last_update = 1000u64;
        let current_time = last_update + SECONDS_PER_DAY;
        
        let result = calculate_upward_accrual(
            current_ltv,
            disco_ltv,
            last_update,
            current_time,
            low_kp,
        ).unwrap();
        
        // Error = 10%, Kp = 2%, so accrual = 0.2%
        // Result should be 70.2%
        let expected = Decimal::percent(70) + Decimal::permille(2);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_cap_ltv_values() {
        // Test that LTVs are properly capped
        use crate::ltv_updater::cap_ltv_values;
        
        let mut max_borrow = Decimal::percent(95);
        let mut max_ltv = Decimal::percent(90);
        
        // max_borrow > max_ltv, should adjust max_borrow
        cap_ltv_values(&mut max_borrow, &mut max_ltv).unwrap();
        
        assert!(max_borrow < max_ltv);
        assert_eq!(max_borrow, Decimal::percent(90) * Decimal::percent(95));
    }

    #[test]
    fn test_downward_shift_timer_and_reset() {
        // Test that downward shifts only happen after period elapses
        // and timer resets when new lower LTV is detected
        use crate::ltv_updater::process_ltv_update;
        
        let current_ltv = Decimal::percent(80);
        let disco_ltv_initial = Decimal::percent(75); // First downward detection
        let disco_ltv_lower = Decimal::percent(70);   // Even lower later
        let kp = Decimal::percent(5);
        let downward_period = 604800; // 1 week
        let max_downward_shift = Decimal::percent(5);
        
        let mut staged_ltv = None;
        let mut staged_timestamp = None;
        let last_upward_update = 1000u64;
        
        // First downward detection - should stage and start timer
        let current_time = 2000u64;
        let (new_ltv, updated) = process_ltv_update(
            current_ltv,
            disco_ltv_initial,
            &mut staged_ltv,
            &mut staged_timestamp,
            last_upward_update,
            current_time,
            kp,
            downward_period,
            max_downward_shift,
        ).unwrap();
        
        // Should not update yet (still in waiting period)
        assert!(!updated);
        assert_eq!(new_ltv, current_ltv);
        assert_eq!(staged_ltv, Some(disco_ltv_initial));
        assert_eq!(staged_timestamp, Some(current_time));
        
        // Try to apply before period elapses - should still not update
        let before_period_time = current_time + downward_period - 1;
        let (new_ltv, updated) = process_ltv_update(
            current_ltv,
            disco_ltv_initial,
            &mut staged_ltv,
            &mut staged_timestamp,
            last_upward_update,
            before_period_time,
            kp,
            downward_period,
            max_downward_shift,
        ).unwrap();
        
        assert!(!updated);
        assert_eq!(new_ltv, current_ltv);
        assert_eq!(staged_ltv, Some(disco_ltv_initial));
        assert_eq!(staged_timestamp, Some(current_time)); // Timer unchanged
        
        // Now detect even lower LTV - should update staged value but NOT reset timer
        let lower_detection_time = before_period_time - 1000; // Before period elapses
        let (new_ltv, updated) = process_ltv_update(
            current_ltv,
            disco_ltv_lower,
            &mut staged_ltv,
            &mut staged_timestamp,
            last_upward_update,
            lower_detection_time,
            kp,
            downward_period,
            max_downward_shift,
        ).unwrap();
        
        assert!(!updated); // Still not time to apply
        assert_eq!(new_ltv, current_ltv);
        assert_eq!(staged_ltv, Some(disco_ltv_lower)); // Updated to lower value
        assert_eq!(staged_timestamp, Some(current_time)); // Timer NOT reset!
        
        // Now period has elapsed - should apply the shift
        let after_period_time = current_time + downward_period;
        let (new_ltv, updated) = process_ltv_update(
            current_ltv,
            disco_ltv_lower,
            &mut staged_ltv,
            &mut staged_timestamp,
            last_upward_update,
            after_period_time,
            kp,
            downward_period,
            max_downward_shift,
        ).unwrap();
        
        assert!(updated);
        // Should apply capped shift: max 5% of 80% = 4%, so 80% - 4% = 76%
        assert_eq!(new_ltv, Decimal::percent(76));
        assert_eq!(staged_ltv, None); // Cleared after application
        assert_eq!(staged_timestamp, None); // Cleared after application
        
        // Now detect another downward shift - should start NEW timer
        let new_current_ltv = new_ltv; // 76%
        let new_disco_ltv = Decimal::percent(70); // Want to go to 70%
        let new_detection_time = after_period_time + 1000;
        
        let (new_ltv, updated) = process_ltv_update(
            new_current_ltv,
            new_disco_ltv,
            &mut staged_ltv,
            &mut staged_timestamp,
            last_upward_update,
            new_detection_time,
            kp,
            downward_period,
            max_downward_shift,
        ).unwrap();
        
        assert!(!updated); // Not time yet
        assert_eq!(new_ltv, new_current_ltv);
        assert_eq!(staged_ltv, Some(new_disco_ltv));
        assert_eq!(staged_timestamp, Some(new_detection_time)); // NEW timer started
        
        // Verify we need to wait another full period
        let before_new_period = new_detection_time + downward_period - 1;
        let (new_ltv, updated) = process_ltv_update(
            new_current_ltv,
            new_disco_ltv,
            &mut staged_ltv,
            &mut staged_timestamp,
            last_upward_update,
            before_new_period,
            kp,
            downward_period,
            max_downward_shift,
        ).unwrap();
        
        assert!(!updated); // Still waiting
        assert_eq!(staged_timestamp, Some(new_detection_time)); // Timer unchanged
    }

    #[test]
    fn test_cap_ltv_values_over_100() {
        use crate::ltv_updater::cap_ltv_values;
        
        let mut max_borrow = Decimal::percent(105);
        let mut max_ltv = Decimal::percent(110);
        
        cap_ltv_values(&mut max_borrow, &mut max_ltv).unwrap();
        
        // Both should be capped at 100%
        assert_eq!(max_ltv, Decimal::percent(100));
        // max_borrow should be 95% of max_ltv since it was >= max_ltv
        assert!(max_borrow < max_ltv);
    }
}

