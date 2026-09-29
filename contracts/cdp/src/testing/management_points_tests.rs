#[cfg(test)]
mod tests {
    use cosmwasm_std::{
        testing::mock_dependencies, Addr, Decimal, Uint128,
    };
    use membrane::cdp::Config;
    use membrane::types::{
        cAsset, Asset, AssetInfo, Position, RateSegment, IRMConfig,
    };

    use crate::positions::{check_volatile_window_and_update, is_in_volatile_window};
    use crate::state::{CollateralVolatility, VOLATILITY};

    /// Helper to create a Config with optional points_contract
    fn mock_config(points_contract: Option<Addr>) -> Config {
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
            oracle_time_limit: 600,
            cpc_multiplier: Decimal::one(),
            debt_minimum: Uint128::new(100),
            base_debt_cap_multiplier: Uint128::new(10),
            rate_slope_multiplier: Decimal::one(),
            affiliate_fee_max: Decimal::percent(5),
            skip_credit_price_accrual: false,
            liquidation_stat_limit: 500,
            transmuter_addr: None,
            irm_config: IRMConfig {
                adjustment_speed: Decimal::percent(50),
                min_adaptive_rate: Decimal::permille(1),
                max_adaptive_rate: Decimal::one(),
            },
            points_contract,
        }
    }

    /// Helper to create a Position with the given debt and collateral denom
    fn mock_position(debt: u128, collateral_denom: &str) -> Position {
        Position {
            position_id: Uint128::one(),
            collateral_assets: vec![cAsset {
                asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: collateral_denom.to_string(),
                    },
                    amount: Uint128::new(1_000_000),
                },
                max_borrow_LTV: Decimal::percent(50),
                max_LTV: Decimal::percent(70),
                pool_info: None,
                rate_index: Decimal::one(),
                peg_rate_index: Decimal::one(),
                force_redemptions: None,
                max_threshold_to_delay: Decimal::percent(15),
            }],
            rate_segments: vec![RateSegment {
                amount: Uint128::new(debt),
                fixed_rate: None,
            }],
            deployed_to: vec![],
            pending_interest: Uint128::zero(),
            total_interest_accrued: Uint128::zero(),
            peg_rate_segments: vec![],
            vol_window_initial_debt: None,
        }
    }

    /// Helper to save volatile data to storage
    fn save_volatility(deps: &mut cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>, denom: &str, raw_list: Vec<Decimal>) {
        VOLATILITY
            .save(
                &mut deps.storage,
                denom.to_string(),
                &CollateralVolatility {
                    index: Decimal::zero(),
                    volatility_list: vec![],
                    raw_volatility_list: raw_list,
                },
            )
            .unwrap();
    }

    // ==================
    // is_in_volatile_window tests
    // ==================

    #[test]
    fn test_not_in_volatile_window_when_no_volatility_data() {
        let deps = mock_dependencies();
        let position = mock_position(1000, "atom");

        let result = is_in_volatile_window(&deps.storage, &position.collateral_assets);
        assert!(!result, "Should NOT be in volatile window with no volatility data");
    }

    #[test]
    fn test_not_in_volatile_window_when_empty_list() {
        let mut deps = mock_dependencies();
        save_volatility(&mut deps, "atom", vec![]);

        let position = mock_position(1000, "atom");
        let result = is_in_volatile_window(&deps.storage, &position.collateral_assets);
        assert!(!result, "Should NOT be in volatile window with empty volatility list");
    }

    #[test]
    fn test_in_volatile_window_when_current_above_average() {
        let mut deps = mock_dependencies();

        // Volatility list: [1, 1, 1, 5] -> avg = 2, current = 5 -> in window
        save_volatility(&mut deps, "atom", vec![
            Decimal::one(),
            Decimal::one(),
            Decimal::one(),
            Decimal::from_ratio(5u128, 1u128),
        ]);

        let position = mock_position(1000, "atom");
        let result = is_in_volatile_window(&deps.storage, &position.collateral_assets);
        assert!(result, "Should be in volatile window when current vol > average");
    }

    #[test]
    fn test_not_in_volatile_window_when_current_below_average() {
        let mut deps = mock_dependencies();

        // Volatility list: [5, 5, 5, 1] -> avg = 4, current = 1 -> NOT in window
        save_volatility(&mut deps, "atom", vec![
            Decimal::from_ratio(5u128, 1u128),
            Decimal::from_ratio(5u128, 1u128),
            Decimal::from_ratio(5u128, 1u128),
            Decimal::one(),
        ]);

        let position = mock_position(1000, "atom");
        let result = is_in_volatile_window(&deps.storage, &position.collateral_assets);
        assert!(!result, "Should NOT be in volatile window when current vol < average");
    }

    #[test]
    fn test_not_in_volatile_window_when_all_equal() {
        let mut deps = mock_dependencies();

        // All equal -> current == average -> NOT in window (requires strictly greater)
        save_volatility(&mut deps, "atom", vec![
            Decimal::from_ratio(3u128, 1u128),
            Decimal::from_ratio(3u128, 1u128),
            Decimal::from_ratio(3u128, 1u128),
        ]);

        let position = mock_position(1000, "atom");
        let result = is_in_volatile_window(&deps.storage, &position.collateral_assets);
        assert!(!result, "Should NOT be in volatile window when current == average");
    }

    // ==================
    // check_volatile_window_and_update tests
    // ==================

    #[test]
    fn test_entering_volatile_window_sets_initial_debt() {
        let mut deps = mock_dependencies();

        // Set up volatile window: current vol > average
        save_volatility(&mut deps, "atom", vec![
            Decimal::one(),
            Decimal::one(),
            Decimal::from_ratio(5u128, 1u128),
        ]);

        let config = mock_config(Some(Addr::unchecked("points_contract")));
        let mut position = mock_position(1000, "atom");
        let owner = Addr::unchecked("user");

        assert!(position.vol_window_initial_debt.is_none());

        let msgs = check_volatile_window_and_update(
            &deps.storage,
            &config,
            &mut position,
            &owner,
        )
        .unwrap();

        // Should set initial debt and return no messages
        assert_eq!(position.vol_window_initial_debt, Some(Uint128::new(1000)));
        assert!(msgs.is_empty(), "No messages should be sent while entering volatile window");
    }

    #[test]
    fn test_staying_in_volatile_window_preserves_initial_debt() {
        let mut deps = mock_dependencies();

        // Set up volatile window
        save_volatility(&mut deps, "atom", vec![
            Decimal::one(),
            Decimal::one(),
            Decimal::from_ratio(5u128, 1u128),
        ]);

        let config = mock_config(Some(Addr::unchecked("points_contract")));
        // Position already has initial debt tracked (entered volatile window earlier with 2000 debt)
        let mut position = mock_position(1000, "atom");
        position.vol_window_initial_debt = Some(Uint128::new(2000));
        let owner = Addr::unchecked("user");

        let msgs = check_volatile_window_and_update(
            &deps.storage,
            &config,
            &mut position,
            &owner,
        )
        .unwrap();

        // Should preserve original initial debt (2000), not overwrite with current (1000)
        assert_eq!(position.vol_window_initial_debt, Some(Uint128::new(2000)));
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_exiting_volatile_window_with_negative_delta_sends_points() {
        let mut deps = mock_dependencies();

        // NOT in volatile window (current vol < average)
        save_volatility(&mut deps, "atom", vec![
            Decimal::from_ratio(5u128, 1u128),
            Decimal::from_ratio(5u128, 1u128),
            Decimal::one(),
        ]);

        let config = mock_config(Some(Addr::unchecked("points_contract")));
        // Position had 2000 debt when entering, now has 1000 (repaid 1000 during volatility)
        let mut position = mock_position(1000, "atom");
        position.vol_window_initial_debt = Some(Uint128::new(2000));
        let owner = Addr::unchecked("user");

        let msgs = check_volatile_window_and_update(
            &deps.storage,
            &config,
            &mut position,
            &owner,
        )
        .unwrap();

        // Should clear the tracking
        assert!(position.vol_window_initial_debt.is_none());
        // Should send points message (negative delta = repaid)
        assert_eq!(msgs.len(), 1, "Should send points message for negative delta");
    }

    #[test]
    fn test_exiting_volatile_window_with_positive_delta_no_points() {
        let mut deps = mock_dependencies();

        // NOT in volatile window
        save_volatility(&mut deps, "atom", vec![
            Decimal::from_ratio(5u128, 1u128),
            Decimal::from_ratio(5u128, 1u128),
            Decimal::one(),
        ]);

        let config = mock_config(Some(Addr::unchecked("points_contract")));
        // Position had 1000 debt when entering, now has 2000 (increased debt during volatility)
        let mut position = mock_position(2000, "atom");
        position.vol_window_initial_debt = Some(Uint128::new(1000));
        let owner = Addr::unchecked("user");

        let msgs = check_volatile_window_and_update(
            &deps.storage,
            &config,
            &mut position,
            &owner,
        )
        .unwrap();

        // Should clear the tracking
        assert!(position.vol_window_initial_debt.is_none());
        // Should NOT send points message (positive delta = increased debt)
        assert!(msgs.is_empty(), "No points for positive delta");
    }

    #[test]
    fn test_exiting_volatile_window_with_equal_debt_no_points() {
        let mut deps = mock_dependencies();

        // NOT in volatile window
        save_volatility(&mut deps, "atom", vec![
            Decimal::from_ratio(5u128, 1u128),
            Decimal::from_ratio(5u128, 1u128),
            Decimal::one(),
        ]);

        let config = mock_config(Some(Addr::unchecked("points_contract")));
        // Same debt as initial - no change
        let mut position = mock_position(1000, "atom");
        position.vol_window_initial_debt = Some(Uint128::new(1000));
        let owner = Addr::unchecked("user");

        let msgs = check_volatile_window_and_update(
            &deps.storage,
            &config,
            &mut position,
            &owner,
        )
        .unwrap();

        assert!(position.vol_window_initial_debt.is_none());
        assert!(msgs.is_empty(), "No points when debt unchanged");
    }

    #[test]
    fn test_exiting_volatile_window_no_points_contract_configured() {
        let mut deps = mock_dependencies();

        // NOT in volatile window
        save_volatility(&mut deps, "atom", vec![
            Decimal::from_ratio(5u128, 1u128),
            Decimal::from_ratio(5u128, 1u128),
            Decimal::one(),
        ]);

        // No points contract configured
        let config = mock_config(None);
        let mut position = mock_position(1000, "atom");
        position.vol_window_initial_debt = Some(Uint128::new(2000));
        let owner = Addr::unchecked("user");

        let msgs = check_volatile_window_and_update(
            &deps.storage,
            &config,
            &mut position,
            &owner,
        )
        .unwrap();

        // Should clear tracking but no message since no points contract
        assert!(position.vol_window_initial_debt.is_none());
        assert!(msgs.is_empty(), "No messages without points contract");
    }

    #[test]
    fn test_no_volatile_window_no_tracking_no_messages() {
        let deps = mock_dependencies();

        // NOT in volatile window (no data)
        let config = mock_config(Some(Addr::unchecked("points_contract")));
        let mut position = mock_position(1000, "atom");
        // No vol_window_initial_debt set
        let owner = Addr::unchecked("user");

        let msgs = check_volatile_window_and_update(
            &deps.storage,
            &config,
            &mut position,
            &owner,
        )
        .unwrap();

        assert!(position.vol_window_initial_debt.is_none());
        assert!(msgs.is_empty(), "No messages when not tracking and not in window");
    }
}
