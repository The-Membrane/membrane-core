#![allow(non_snake_case)]
//! Position-flow tests for the Debt contract.
//!
//! These tests exercise the position-level execute functions that need RATES
//! and other state items initialized. Uses `mock_dependencies()` for direct
//! storage access (since cw-multi-test can't easily bootstrap RATES).
//!
//! Covered functions:
//! - liquidation_debt_update
//! - accrue_and_validate
//! - accrue_and_return_debt
//! - close_position_callback
//! - set_active_deployment_venues (state helper)
//! - update_historical_interest_rates (state helper)

#[cfg(test)]
pub mod tests {
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env, mock_info},
        Addr, Decimal, MessageInfo, Uint128,
    };

    use membrane::debt::{AssetRateIndex, DebtPosition};
    use membrane::types::{
        FixedRate, FixedRateEnd, RateSegment, StringEntry,
    };

    use crate::positions::{
        accrue_and_return_debt, accrue_and_validate, close_position, close_position_callback,
        increase_debt, liquidation_debt_update,
    };
    use crate::rates::get_total_position_debt;
    use crate::state::{
        set_active_deployment_venues, update_historical_interest_rates,
        ACTIVE_DEPLOYMENT_VENUES, CDT_SUPPLY, CLOSE_POSITION_PROPAGATION,
        ClosePositionPropagation, COLLATERAL_REFRESH_REPLY_ID, CLOSE_POSITION_SELL_REPLY_ID,
        HISTORICAL_INTEREST_RATES, INCREASE_DEBT_PROPAGATION, MAX_RATE_HISTORY_ENTRIES,
        POSITIONS,
    };
    use crate::testing::test_helpers::helpers::{
        create_position_with_debt, init_basket, init_basket_rate_index, init_cdt_supply,
        init_collateral_types_cache, init_config, init_frozen_state, init_rates,
    };

    // ========================================================================
    // Local helpers
    // ========================================================================

    /// Initialize all state items needed for position operations.
    /// Aligns rates_last_accrued with mock_env() block time to prevent overflow.
    fn init_test_state(storage: &mut dyn cosmwasm_std::Storage) {
        let mut rates = init_rates(storage);
        // Align rates timestamps with mock_env() so time_elapsed is small
        let block_time = cosmwasm_std::testing::mock_env().block.time.seconds();
        rates.rates_last_accrued = block_time;
        rates.credit_last_accrued = block_time;
        crate::state::RATES.save(storage, &rates).unwrap();

        init_config(storage);
        // credit_price defaults to 1.0 from init_basket(), which is realistic.
        // The zero-price CPC guard in accrue_position() prevents division by zero
        // when oracle_contract=None sets credit_TWAP_price=0.
        init_basket(storage);

        init_cdt_supply(storage, Uint128::new(10_000_000_000));
        init_frozen_state(storage, false);
        init_basket_rate_index(storage, "uosmo");
        init_collateral_types_cache(storage, "uosmo", Decimal::percent(80));
    }

    /// Create and save a position with variable debt and collateral_rate_indices.
    fn create_position_with_indices(
        storage: &mut dyn cosmwasm_std::Storage,
        owner: Addr,
        position_id: Uint128,
        var_amount: u128,
        peg_var_amount: u128,
    ) -> DebtPosition {
        let mut position = create_position_with_debt(
            storage, owner.clone(), position_id, var_amount, peg_var_amount,
        );

        // Add collateral_rate_indices for "uosmo" to avoid bootstrapping inside accrue
        position.collateral_rate_indices = vec![AssetRateIndex {
            asset_denom: "uosmo".to_string(),
            rate_index: Decimal::one(),
            peg_rate_index: Decimal::one(),
            ratio: Decimal::one(),
        }];

        // Re-save position with indices
        let mut positions = POSITIONS
            .may_load(storage, owner.clone())
            .unwrap()
            .unwrap_or_default();
        if let Some(pos) = positions
            .iter_mut()
            .find(|p| p.position_id == position_id)
        {
            *pos = position.clone();
        }
        POSITIONS.save(storage, owner, &positions).unwrap();

        position
    }

    /// Build a MessageInfo for the liquidation contract (authorized caller).
    /// Must match the address stored by `init_config` -> `Addr::unchecked("liquidation_contract")`.
    fn liquidation_info() -> MessageInfo {
        mock_info("liquidation_contract", &[])
    }

    /// Build a MessageInfo for the collateral contract (authorized caller).
    /// Must match the address stored by `init_config` -> `Addr::unchecked("collateral_contract")`.
    fn collateral_info() -> MessageInfo {
        mock_info("collateral_contract", &[])
    }

    // ========================================================================
    // liquidation_debt_update tests
    // ========================================================================

    #[test]
    fn liq_debt_update_reduces_variable_debt_proportionally() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        // Position: 1000 variable, 0 peg
        create_position_with_indices(deps.as_mut().storage, owner.clone(), Uint128::one(), 1000, 0);

        let info = liquidation_info();
        let res = liquidation_debt_update(
            deps.as_mut(),
            env,
            info,
            Uint128::one(),
            owner.to_string(),
            Uint128::new(400),
        )
        .unwrap();

        // Verify debt reduced
        let (_, pos) = crate::state::get_target_position(
            deps.as_ref().storage,
            owner,
            Uint128::one(),
        )
        .unwrap();
        let total_debt = get_total_position_debt(&pos);
        assert_eq!(total_debt, Uint128::new(600));

        // Verify attributes
        assert!(res.attributes.iter().any(|a| a.key == "debt_after" && a.value == "600"));
        assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "liquidation_debt_update"));
    }

    #[test]
    fn liq_debt_update_reduces_mixed_segments() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        // Manually create position with variable + fixed segments
        let position = DebtPosition {
            position_id: Uint128::new(2),
            rate_segments: vec![
                RateSegment {
                    amount: Uint128::new(600),
                    fixed_rate: None,
                },
                RateSegment {
                    amount: Uint128::new(400),
                    fixed_rate: Some(FixedRate {
                        rate: Decimal::percent(5),
                        end: FixedRateEnd {
                            end_time: 99999999,
                            rollover: false,
                            duration_months: 3,
                        },
                    }),
                },
            ],
            peg_rate_segments: vec![],
            deployed_to: vec![],
            pending_interest: Uint128::zero(),
            total_interest_accrued: Uint128::zero(),
            vol_window_initial_debt: None,
            collateral_rate_indices: vec![AssetRateIndex {
                asset_denom: "uosmo".to_string(),
                rate_index: Decimal::one(),
                peg_rate_index: Decimal::one(),
                ratio: Decimal::one(),
            }],
        };
        POSITIONS
            .save(deps.as_mut().storage, owner.clone(), &vec![position])
            .unwrap();

        let info = liquidation_info();
        let _res = liquidation_debt_update(
            deps.as_mut(),
            env,
            info,
            Uint128::new(2),
            owner.to_string(),
            Uint128::new(500),
        )
        .unwrap();

        let (_, pos) = crate::state::get_target_position(
            deps.as_ref().storage,
            owner,
            Uint128::new(2),
        )
        .unwrap();
        let total_debt = get_total_position_debt(&pos);
        // Total was 1000, repay 500 -> 500 left
        assert_eq!(total_debt, Uint128::new(500));
    }

    #[test]
    fn liq_debt_update_both_pools_reduced_proportionally() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        // Position: 600 regular variable + 400 peg variable = 1000 total
        create_position_with_indices(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::new(3),
            600,
            400,
        );

        let info = liquidation_info();
        let _res = liquidation_debt_update(
            deps.as_mut(),
            env,
            info,
            Uint128::new(3),
            owner.to_string(),
            Uint128::new(500),
        )
        .unwrap();

        let (_, pos) = crate::state::get_target_position(
            deps.as_ref().storage,
            owner,
            Uint128::new(3),
        )
        .unwrap();

        let total_debt = get_total_position_debt(&pos);
        assert_eq!(total_debt, Uint128::new(500));

        // Regular pool had 600/1000 = 60% of debt -> repays 300
        let regular_total: Uint128 = pos.rate_segments.iter().map(|s| s.amount).sum();
        assert_eq!(regular_total, Uint128::new(300));

        // Peg pool had 400/1000 = 40% of debt -> repays 200
        let peg_total: Uint128 = pos.peg_rate_segments.iter().map(|s| s.amount).sum();
        assert_eq!(peg_total, Uint128::new(200));
    }

    #[test]
    fn liq_debt_update_full_wipeout() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        // Position: 500 variable
        create_position_with_indices(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::new(4),
            500,
            0,
        );

        let info = liquidation_info();
        let res = liquidation_debt_update(
            deps.as_mut(),
            env,
            info,
            Uint128::new(4),
            owner.to_string(),
            Uint128::new(1000), // repay more than total
        )
        .unwrap();

        let (_, pos) = crate::state::get_target_position(
            deps.as_ref().storage,
            owner,
            Uint128::new(4),
        )
        .unwrap();
        let total_debt = get_total_position_debt(&pos);
        assert_eq!(total_debt, Uint128::zero());
        assert!(res.attributes.iter().any(|a| a.key == "debt_after" && a.value == "0"));
    }

    #[test]
    fn liq_debt_update_cdt_supply_decreases() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        let initial_supply = CDT_SUPPLY.load(deps.as_ref().storage).unwrap();
        create_position_with_indices(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::new(5),
            1000,
            0,
        );

        let info = liquidation_info();
        liquidation_debt_update(
            deps.as_mut(),
            env,
            info,
            Uint128::new(5),
            owner.to_string(),
            Uint128::new(300),
        )
        .unwrap();

        let new_supply = CDT_SUPPLY.load(deps.as_ref().storage).unwrap();
        assert_eq!(new_supply, initial_supply - Uint128::new(300));
    }

    #[test]
    fn liq_debt_update_unauthorized_caller() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        create_position_with_indices(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::new(6),
            1000,
            0,
        );

        // Use a random unauthorized sender
        let info = mock_info("random_caller", &[]);
        let err = liquidation_debt_update(
            deps.as_mut(),
            env,
            info,
            Uint128::new(6),
            owner.to_string(),
            Uint128::new(100),
        )
        .unwrap_err();

        assert!(
            matches!(err, crate::ContractError::Unauthorized {}),
            "Expected Unauthorized, got: {:?}",
            err
        );
    }

    // ========================================================================
    // accrue_and_validate tests
    // ========================================================================

    #[test]
    fn accrue_and_validate_zero_debt_passes() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        // Position with zero debt
        create_position_with_indices(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::new(10),
            0,
            0,
        );

        let info = collateral_info();
        let res = accrue_and_validate(
            deps.as_mut(),
            env,
            info,
            owner.to_string(),
            Uint128::new(10),
            Decimal::from_ratio(1000u128, 1u128), // $1000 collateral
            Decimal::percent(80),                   // avg_max_borrow_ltv
            Decimal::percent(90),                   // avg_max_ltv
        )
        .unwrap();

        assert!(res.attributes.iter().any(|a| a.key == "total_debt" && a.value == "0"));
        assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "accrue_and_validate"));
    }

    #[test]
    fn accrue_and_validate_unauthorized_caller() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        create_position_with_indices(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::new(11),
            0,
            0,
        );

        let info = mock_info("random_caller", &[]);
        let err = accrue_and_validate(
            deps.as_mut(),
            env,
            info,
            owner.to_string(),
            Uint128::new(11),
            Decimal::from_ratio(1000u128, 1u128),
            Decimal::percent(80),
            Decimal::percent(90),
        )
        .unwrap_err();

        assert!(matches!(err, crate::ContractError::Unauthorized {}));
    }

    #[test]
    fn accrue_and_validate_position_under_limits() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        // Position with 100 debt (well under limits)
        // Credit price = $1, so debt_value = $0.0001 (100 / 10^6)
        // Collateral value = $1000
        // LTV = 0.0001/1000 = 0.00001% - well under any limit
        create_position_with_indices(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::new(12),
            100,
            0,
        );

        let info = collateral_info();
        let res = accrue_and_validate(
            deps.as_mut(),
            env,
            info,
            owner.to_string(),
            Uint128::new(12),
            Decimal::from_ratio(1000u128, 1u128), // collateral value
            Decimal::percent(80),                   // avg_max_borrow_ltv
            Decimal::percent(90),                   // avg_max_ltv
        )
        .unwrap();

        // Should succeed with correct attributes
        assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "accrue_and_validate"));
        assert!(res.attributes.iter().any(|a| a.key == "position_id" && a.value == "12"));
    }

    #[test]
    fn accrue_and_validate_saves_position_after_accrual() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        create_position_with_indices(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::new(13),
            1_000_000,
            0,
        );

        let info = collateral_info();
        let _res = accrue_and_validate(
            deps.as_mut(),
            env,
            info,
            owner.to_string(),
            Uint128::new(13),
            Decimal::from_ratio(10_000_000u128, 1u128), // collateral value >> debt
            Decimal::percent(80),
            Decimal::percent(90),
        )
        .unwrap();

        // Verify position is saved (can be loaded)
        let (_, pos) = crate::state::get_target_position(
            deps.as_ref().storage,
            owner,
            Uint128::new(13),
        )
        .unwrap();
        // Position should have collateral_rate_indices populated after accrual
        // (bootstrapped if they were initially empty)
        assert!(!pos.collateral_rate_indices.is_empty() || pos.rate_segments.is_empty());
    }

    #[test]
    fn accrue_and_validate_large_debt_with_adequate_collateral_passes() {
        // With the CPC zero-price guard, accrue_position safely skips CPC when
        // oracle_contract = None (credit_TWAP_price = 0). credit_price = 1.0 gives
        // realistic debt values so LTV checks work correctly.
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        create_position_with_indices(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::new(14),
            10_000_000_000,
            0,
        );

        let info = collateral_info();
        let res = accrue_and_validate(
            deps.as_mut(),
            env,
            info,
            owner.to_string(),
            Uint128::new(14),
            Decimal::from_ratio(100_000_000_000u128, 1u128), // collateral value >> debt
            Decimal::percent(80),    // avg_max_borrow_ltv
            Decimal::percent(90),    // avg_max_ltv
        )
        .unwrap();

        // debt_value = 10_000_000_000 * credit_price(1.0) = 10_000_000_000
        assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "accrue_and_validate"));
    }

    // ========================================================================
    // accrue_and_return_debt tests
    // ========================================================================

    #[test]
    fn accrue_and_return_debt_returns_correct_total() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        create_position_with_indices(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::new(20),
            5000,
            3000,
        );

        let info = liquidation_info();
        let res = accrue_and_return_debt(
            deps.as_mut(),
            env,
            info,
            owner.to_string(),
            Uint128::new(20),
        )
        .unwrap();

        // total_debt should be at least 8000 (5000 regular + 3000 peg)
        // It may be slightly higher due to accrual, but with rates_last_accrued = 1000
        // and env.block.time = mock_env default (~1571797419), there could be accrual.
        assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "accrue_and_return_debt"));
        assert!(res.attributes.iter().any(|a| a.key == "position_id" && a.value == "20"));

        // total_debt attribute should be present and > 0
        let total_debt_attr = res
            .attributes
            .iter()
            .find(|a| a.key == "total_debt")
            .expect("total_debt attribute missing");
        let total_debt: Uint128 = total_debt_attr.value.parse().unwrap();
        assert!(total_debt >= Uint128::new(8000));
    }

    #[test]
    fn accrue_and_return_debt_unauthorized() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        create_position_with_indices(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::new(21),
            5000,
            0,
        );

        // Use collateral contract (wrong authorized caller - should be liquidation)
        let info = collateral_info();
        let err = accrue_and_return_debt(
            deps.as_mut(),
            env,
            info,
            owner.to_string(),
            Uint128::new(21),
        )
        .unwrap_err();

        assert!(matches!(err, crate::ContractError::Unauthorized {}));
    }

    #[test]
    fn accrue_and_return_debt_both_pools() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        create_position_with_indices(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::new(22),
            3000,
            7000,
        );

        let info = liquidation_info();
        let res = accrue_and_return_debt(
            deps.as_mut(),
            env,
            info,
            owner.to_string(),
            Uint128::new(22),
        )
        .unwrap();

        // debt_response attribute should be present (serialized JSON)
        assert!(
            res.attributes.iter().any(|a| a.key == "debt_response"),
            "debt_response attribute missing from response"
        );

        let total_debt_attr = res
            .attributes
            .iter()
            .find(|a| a.key == "total_debt")
            .unwrap();
        let total_debt: Uint128 = total_debt_attr.value.parse().unwrap();
        // At minimum the original 10,000 (may have accrued)
        assert!(total_debt >= Uint128::new(10_000));
    }

    // ========================================================================
    // close_position_callback tests
    // ========================================================================

    #[test]
    fn close_position_callback_cleans_up_propagation() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        // Save propagation state that the callback should clean up
        CLOSE_POSITION_PROPAGATION
            .save(
                deps.as_mut().storage,
                &ClosePositionPropagation {
                    position_info: membrane::types::UserInfo {
                        position_id: Uint128::new(30),
                        position_owner: owner.to_string(),
                    },
                    close_percentage: Decimal::one(),
                    max_spread: Decimal::percent(5),
                    send_to: None,
                },
            )
            .unwrap();

        let res = close_position_callback(
            deps.as_mut(),
            env,
            Uint128::new(30),
            owner.to_string(),
        )
        .unwrap();

        // Propagation state should be removed
        assert!(CLOSE_POSITION_PROPAGATION
            .may_load(deps.as_ref().storage)
            .unwrap()
            .is_none());

        assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "close_position_callback"));
    }

    #[test]
    fn close_position_callback_has_correct_attributes() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();

        let res = close_position_callback(
            deps.as_mut(),
            env,
            Uint128::new(42),
            "some_owner".to_string(),
        )
        .unwrap();

        assert!(res.attributes.iter().any(|a| a.key == "position_id" && a.value == "42"));
        assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "close_position_callback"));
    }

    // ========================================================================
    // set_active_deployment_venues tests
    // ========================================================================

    #[test]
    fn set_active_deployment_venues_add() {
        let mut deps = mock_dependencies();

        // Initialize empty
        ACTIVE_DEPLOYMENT_VENUES
            .save(deps.as_mut().storage, &vec![])
            .unwrap();

        set_active_deployment_venues(
            deps.as_mut().storage,
            vec![
                StringEntry {
                    entry: "venue_a".to_string(),
                    remove: false,
                },
                StringEntry {
                    entry: "venue_b".to_string(),
                    remove: false,
                },
            ],
        )
        .unwrap();

        let venues = ACTIVE_DEPLOYMENT_VENUES
            .load(deps.as_ref().storage)
            .unwrap();
        assert_eq!(venues.len(), 2);
        assert!(venues.contains(&"venue_a".to_string()));
        assert!(venues.contains(&"venue_b".to_string()));

        // Adding duplicate should not create a second entry
        set_active_deployment_venues(
            deps.as_mut().storage,
            vec![StringEntry {
                entry: "venue_a".to_string(),
                remove: false,
            }],
        )
        .unwrap();

        let venues = ACTIVE_DEPLOYMENT_VENUES
            .load(deps.as_ref().storage)
            .unwrap();
        assert_eq!(venues.len(), 2); // still 2, no duplicate
    }

    #[test]
    fn set_active_deployment_venues_remove() {
        let mut deps = mock_dependencies();

        ACTIVE_DEPLOYMENT_VENUES
            .save(
                deps.as_mut().storage,
                &vec!["venue_a".to_string(), "venue_b".to_string(), "venue_c".to_string()],
            )
            .unwrap();

        set_active_deployment_venues(
            deps.as_mut().storage,
            vec![StringEntry {
                entry: "venue_b".to_string(),
                remove: true,
            }],
        )
        .unwrap();

        let venues = ACTIVE_DEPLOYMENT_VENUES
            .load(deps.as_ref().storage)
            .unwrap();
        assert_eq!(venues.len(), 2);
        assert!(!venues.contains(&"venue_b".to_string()));
        assert!(venues.contains(&"venue_a".to_string()));
        assert!(venues.contains(&"venue_c".to_string()));

        // Remove non-existent venue - should be no-op
        set_active_deployment_venues(
            deps.as_mut().storage,
            vec![StringEntry {
                entry: "nonexistent".to_string(),
                remove: true,
            }],
        )
        .unwrap();

        let venues = ACTIVE_DEPLOYMENT_VENUES
            .load(deps.as_ref().storage)
            .unwrap();
        assert_eq!(venues.len(), 2);
    }

    // ========================================================================
    // update_historical_interest_rates tests
    // ========================================================================

    #[test]
    fn update_historical_interest_rates_trims_at_max() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let asset = "uosmo".to_string();

        // Add MAX_RATE_HISTORY_ENTRIES entries
        for i in 0..MAX_RATE_HISTORY_ENTRIES {
            let mut env_i = env.clone();
            env_i.block.time = cosmwasm_std::Timestamp::from_seconds(i as u64);
            update_historical_interest_rates(
                deps.as_mut().storage,
                env_i,
                asset.clone(),
                Decimal::percent(5),
            )
            .unwrap();
        }

        let history = HISTORICAL_INTEREST_RATES
            .load(deps.as_ref().storage, asset.clone())
            .unwrap();
        assert_eq!(history.len(), MAX_RATE_HISTORY_ENTRIES);

        // Add one more -> should trim oldest
        let mut env_new = env.clone();
        env_new.block.time = cosmwasm_std::Timestamp::from_seconds(99999);
        update_historical_interest_rates(
            deps.as_mut().storage,
            env_new,
            asset.clone(),
            Decimal::percent(7),
        )
        .unwrap();

        let history = HISTORICAL_INTEREST_RATES
            .load(deps.as_ref().storage, asset)
            .unwrap();
        assert_eq!(history.len(), MAX_RATE_HISTORY_ENTRIES);
        // Last entry should be the new one
        assert_eq!(history.last().unwrap().timestamp, 99999);
        assert_eq!(history.last().unwrap().rate, "0.07");
        // First entry should NOT be timestamp 0 (trimmed)
        assert_ne!(history.first().unwrap().timestamp, 0);
    }

    // ========================================================================
    // increase_debt: happy path (saves propagation, sends SubMsg)
    // ========================================================================

    #[test]
    fn increase_debt_happy_path_saves_propagation_and_sends_submsg() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);

        let owner = Addr::unchecked("user1");
        create_position_with_indices(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::one(),
            500_000, // 500K variable debt
            0,
        );

        let env = mock_env();
        let info = mock_info("user1", &[]);

        let res = increase_debt(
            deps.as_mut(),
            env,
            info,
            Uint128::one(),
            Some(Uint128::new(100_000)),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        assert!(res.is_ok(), "increase_debt should succeed: {:?}", res.err());
        let response = res.unwrap();

        // Should have a SubMsg to Collateral.RefreshAndAssess
        assert_eq!(response.messages.len(), 1);
        assert_eq!(response.messages[0].id, COLLATERAL_REFRESH_REPLY_ID);

        // Should have correct attributes
        let method = response.attributes.iter().find(|a| a.key == "method").unwrap();
        assert_eq!(method.value, "increase_debt");

        // Propagation state should be saved
        let prop = INCREASE_DEBT_PROPAGATION.load(deps.as_ref().storage).unwrap();
        assert_eq!(prop.position_owner, "user1");
        assert_eq!(prop.position_id, Uint128::one());
        assert_eq!(prop.amount, Some(Uint128::new(100_000)));
        assert!(!prop.peg_debt);
    }

    #[test]
    fn increase_debt_with_ltv_saves_ltv_in_propagation() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);

        let owner = Addr::unchecked("user1");
        create_position_with_indices(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::one(),
            500_000,
            0,
        );

        let env = mock_env();
        let info = mock_info("user1", &[]);

        let res = increase_debt(
            deps.as_mut(),
            env,
            info,
            Uint128::one(),
            None,
            Some(Decimal::percent(50)),
            Some("recipient".to_string()),
            None,
            None,
            None,
            Some(true), // peg_debt
        ).unwrap();

        assert_eq!(res.messages.len(), 1);

        let prop = INCREASE_DEBT_PROPAGATION.load(deps.as_ref().storage).unwrap();
        assert_eq!(prop.ltv, Some(Decimal::percent(50)));
        assert!(prop.amount.is_none());
        assert!(prop.peg_debt);
    }

    // ========================================================================
    // close_position: happy path (saves propagation, sends SubMsg)
    // ========================================================================

    #[test]
    fn close_position_happy_path_saves_propagation_and_sends_submsg() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);

        let owner = Addr::unchecked("user1");
        create_position_with_indices(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::one(),
            500_000,
            0,
        );

        let env = mock_env();
        let info = mock_info("user1", &[]);

        let res = close_position(
            deps.as_mut(),
            env,
            info,
            Uint128::one(),
            None, // full close
            Decimal::percent(5),
            None,
        );

        assert!(res.is_ok(), "close_position should succeed: {:?}", res.err());
        let response = res.unwrap();

        // Should have a SubMsg to Collateral.AuthorizedSellCollateral
        assert_eq!(response.messages.len(), 1);
        assert_eq!(response.messages[0].id, CLOSE_POSITION_SELL_REPLY_ID);

        // Should have correct attributes
        let method = response.attributes.iter().find(|a| a.key == "method").unwrap();
        assert_eq!(method.value, "close_position");
        let close_pct = response.attributes.iter().find(|a| a.key == "close_percentage").unwrap();
        assert_eq!(close_pct.value, "1");

        // Propagation state should be saved
        let prop = CLOSE_POSITION_PROPAGATION.load(deps.as_ref().storage).unwrap();
        assert_eq!(prop.position_info.position_owner, "user1");
        assert_eq!(prop.position_info.position_id, Uint128::one());
        assert_eq!(prop.close_percentage, Decimal::one());
        assert_eq!(prop.max_spread, Decimal::percent(5));
        assert!(prop.send_to.is_none());
    }

    #[test]
    fn close_position_partial_with_send_to() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);

        let owner = Addr::unchecked("user1");
        create_position_with_indices(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::one(),
            500_000,
            0,
        );

        let env = mock_env();
        let info = mock_info("user1", &[]);

        let res = close_position(
            deps.as_mut(),
            env,
            info,
            Uint128::one(),
            Some(Decimal::percent(50)),
            Decimal::percent(3),
            Some("recipient_addr".to_string()),
        ).unwrap();

        let prop = CLOSE_POSITION_PROPAGATION.load(deps.as_ref().storage).unwrap();
        assert_eq!(prop.close_percentage, Decimal::percent(50));
        assert_eq!(prop.max_spread, Decimal::percent(3));
        assert_eq!(prop.send_to, Some("recipient_addr".to_string()));
    }

    // ========================================================================
    // query_collateral_interest tests
    // ========================================================================

    #[test]
    fn query_collateral_interest_returns_rates() {
        let mut deps = mock_dependencies();

        // Initialize RATES with 2 collateral rates: 5% and 3%
        let rates = membrane::types::Rates {
            current_adaptive_rate: vec![Decimal::percent(3)],
            peg_current_adaptive_rate: vec![Decimal::percent(3)],
            lastest_collateral_rates: vec![
                membrane::types::Rate {
                    rate: Decimal::percent(5),
                    last_time_updated: 1000u64,
                },
                membrane::types::Rate {
                    rate: Decimal::percent(3),
                    last_time_updated: 1000u64,
                },
            ],
            base_interest_rate: Decimal::percent(2),
            fixed_rate_caps: membrane::types::FixedRateCaps {
                one_month: membrane::types::FixedRateCap {
                    cap: Decimal::percent(20),
                    amount: Uint128::zero(),
                    multiplier: Decimal::one(),
                },
                three_month: membrane::types::FixedRateCap {
                    cap: Decimal::percent(20),
                    amount: Uint128::zero(),
                    multiplier: Decimal::one(),
                },
                six_month: membrane::types::FixedRateCap {
                    cap: Decimal::percent(10),
                    amount: Uint128::zero(),
                    multiplier: Decimal::one(),
                },
            },
            rates_last_accrued: 1000u64,
            credit_last_accrued: 1000u64,
            cpc_margin_of_error: Decimal::one(),
            negative_rates: false,
            acquisition_bump_rate: Decimal::zero(),
        };
        crate::state::RATES.save(deps.as_mut().storage, &rates).unwrap();

        // Call query_collateral_interest
        let binary = crate::query::query_collateral_interest(deps.as_ref()).unwrap();

        // Deserialize result as Vec<Decimal>
        let collateral_rates: Vec<Decimal> = cosmwasm_std::from_json(binary).unwrap();

        assert_eq!(collateral_rates.len(), 2);
        assert_eq!(collateral_rates[0], Decimal::percent(5));
        assert_eq!(collateral_rates[1], Decimal::percent(3));
    }

    #[test]
    fn query_collateral_interest_empty_rates() {
        let mut deps = mock_dependencies();

        // Initialize RATES with empty lastest_collateral_rates
        let rates = membrane::types::Rates {
            current_adaptive_rate: vec![Decimal::percent(3)],
            peg_current_adaptive_rate: vec![Decimal::percent(3)],
            lastest_collateral_rates: vec![],
            base_interest_rate: Decimal::percent(2),
            fixed_rate_caps: membrane::types::FixedRateCaps {
                one_month: membrane::types::FixedRateCap {
                    cap: Decimal::percent(20),
                    amount: Uint128::zero(),
                    multiplier: Decimal::one(),
                },
                three_month: membrane::types::FixedRateCap {
                    cap: Decimal::percent(20),
                    amount: Uint128::zero(),
                    multiplier: Decimal::one(),
                },
                six_month: membrane::types::FixedRateCap {
                    cap: Decimal::percent(10),
                    amount: Uint128::zero(),
                    multiplier: Decimal::one(),
                },
            },
            rates_last_accrued: 1000u64,
            credit_last_accrued: 1000u64,
            cpc_margin_of_error: Decimal::one(),
            negative_rates: false,
            acquisition_bump_rate: Decimal::zero(),
        };
        crate::state::RATES.save(deps.as_mut().storage, &rates).unwrap();

        // Call query_collateral_interest
        let binary = crate::query::query_collateral_interest(deps.as_ref()).unwrap();

        // Deserialize result as Vec<Decimal>
        let collateral_rates: Vec<Decimal> = cosmwasm_std::from_json(binary).unwrap();

        assert!(collateral_rates.is_empty(), "Expected empty vec, got: {:?}", collateral_rates);
    }

    // ========================================================================
    // query_full_positions tests
    // ========================================================================

    #[test]
    fn query_full_positions_no_collateral_contract() {
        use membrane::debt::FullPosition;

        let mut deps = mock_dependencies();

        // Set CONFIG with collateral_contract = None
        let config = membrane::debt::Config {
            owner: Addr::unchecked("admin"),
            collateral_contract: None,
            liquidation_contract: Some(Addr::unchecked("liquidation_contract")),
            staking_contract: None,
            chain_proxy: None,
            debt_auction: None,
            oracle_contract: None,
            liquidity_contract: None,
            discounts_contract: None,
            revenue_distributor: None,
            transmuter_addr: None,
            points_contract: None,
            liq_fee: Decimal::percent(1),
            credit_twap_timeframe: 60u64,
            cpc_multiplier: Decimal::one(),
            debt_minimum: Uint128::new(100_000u128),
            base_debt_cap_multiplier: Uint128::new(10u128),
            rate_slope_multiplier: Decimal::one(),
            affiliate_fee_max: Decimal::percent(5),
            skip_credit_price_accrual: true,
            liquidation_stat_limit: 100u64,
            irm_config: membrane::types::IRMConfig {
                adjustment_speed: Decimal::from_ratio(50u128, 1u128),
                min_adaptive_rate: Decimal::permille(1),
                max_adaptive_rate: Decimal::from_ratio(2u128, 1u128),
            },
            acquisition_contract: None,
        };
        crate::state::CONFIG.save(deps.as_mut().storage, &config).unwrap();

        // Call query_full_positions with a user_info
        let binary = crate::query::query_full_positions(
            deps.as_ref(),
            None,
            None,
            Some(membrane::types::UserInfo {
                position_id: Uint128::one(),
                position_owner: "user1".to_string(),
            }),
            None,
        )
        .unwrap();

        // Should return empty vec since collateral_contract is None
        let positions: Vec<FullPosition> = cosmwasm_std::from_json(binary).unwrap();
        assert!(positions.is_empty(), "Expected empty positions when collateral_contract is None");

        // Also test with user param instead of user_info
        let binary = crate::query::query_full_positions(
            deps.as_ref(),
            None,
            None,
            None,
            Some("user1".to_string()),
        )
        .unwrap();

        let positions: Vec<FullPosition> = cosmwasm_std::from_json(binary).unwrap();
        assert!(positions.is_empty(), "Expected empty positions when collateral_contract is None (user param)");

        // Also test with no params (paginated path)
        let binary = crate::query::query_full_positions(
            deps.as_ref(),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let positions: Vec<FullPosition> = cosmwasm_std::from_json(binary).unwrap();
        assert!(positions.is_empty(), "Expected empty positions when collateral_contract is None (no params)");
    }

    // ========================================================================
    // accrue_and_return_debt: venue recall tests
    // ========================================================================

    #[test]
    fn accrue_and_return_debt_no_venues_zero_recall() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        // Position with debt but NO deployed_to venues (default from create_position_with_debt)
        create_position_with_indices(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::new(100),
            5000,
            3000,
        );

        // Verify position has empty deployed_to
        let (_, pos) = crate::state::get_target_position(
            deps.as_ref().storage,
            owner.clone(),
            Uint128::new(100),
        )
        .unwrap();
        assert!(
            pos.deployed_to.is_empty(),
            "Position should have no deployment venues"
        );

        let info = liquidation_info();
        let res = accrue_and_return_debt(
            deps.as_mut(),
            env,
            info,
            owner.to_string(),
            Uint128::new(100),
        )
        .unwrap();

        // venue_recall_amount attribute should be "0"
        let venue_recall_attr = res
            .attributes
            .iter()
            .find(|a| a.key == "venue_recall_amount")
            .expect("venue_recall_amount attribute missing");
        assert_eq!(
            venue_recall_attr.value, "0",
            "venue_recall_amount should be 0 when no venues are deployed"
        );

        // Parse debt_response from attribute.
        // The attribute value is Binary.to_string() (base64-encoded JSON).
        // Decode base64 back to Binary, then parse the JSON.
        let debt_response_attr = res
            .attributes
            .iter()
            .find(|a| a.key == "debt_response")
            .expect("debt_response attribute missing");
        let debt_binary = cosmwasm_std::Binary::from_base64(&debt_response_attr.value).unwrap();
        let debt_response: membrane::debt::AccrueDebtResponse =
            cosmwasm_std::from_json(&debt_binary).unwrap();
        assert_eq!(
            debt_response.venue_recall_amount,
            Uint128::zero(),
            "AccrueDebtResponse.venue_recall_amount should be zero with no venues"
        );

        // No venue recall SubMsgs should be present
        assert!(
            res.messages.is_empty(),
            "No SubMsgs expected when there are no deployment venues"
        );

        // Total debt should still be correct (at least 8000 = 5000 + 3000)
        let total_debt_attr = res
            .attributes
            .iter()
            .find(|a| a.key == "total_debt")
            .expect("total_debt attribute missing");
        let total_debt: Uint128 = total_debt_attr.value.parse().unwrap();
        assert!(
            total_debt >= Uint128::new(8000),
            "total_debt should be at least 8000 (5000 var + 3000 peg), got: {}",
            total_debt
        );
    }

    #[test]
    fn accrue_and_return_debt_with_venues_but_no_mock_querier_returns_zero_recall() {
        // When deployed_to has venues but the mock querier cannot answer
        // RetrievableCDT queries, the unwrap_or(Uint128::zero()) fallback
        // ensures venue_recall_amount stays at zero and no SubMsgs are built.
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        // Create position with debt
        let mut position = create_position_with_indices(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::new(101),
            10_000,
            0,
        );

        // Manually add a deployment venue to the position
        position.deployed_to = vec![membrane::types::DeploymentVenue {
            address: Addr::unchecked("venue_contract_1"),
            deployed_debt_amount: Uint128::new(5000),
            failed_liquidation: false,
        }];

        // Re-save position with the venue
        let mut positions = POSITIONS
            .load(deps.as_ref().storage, owner.clone())
            .unwrap();
        if let Some(pos) = positions
            .iter_mut()
            .find(|p| p.position_id == Uint128::new(101))
        {
            *pos = position;
        }
        POSITIONS.save(deps.as_mut().storage, owner.clone(), &positions).unwrap();

        let info = liquidation_info();
        let res = accrue_and_return_debt(
            deps.as_mut(),
            env,
            info,
            owner.to_string(),
            Uint128::new(101),
        )
        .unwrap();

        // Since mock querier can't handle RetrievableCDT, it returns error
        // which is caught by unwrap_or(Uint128::zero()), so recall = 0
        let venue_recall_attr = res
            .attributes
            .iter()
            .find(|a| a.key == "venue_recall_amount")
            .expect("venue_recall_amount attribute missing");
        assert_eq!(
            venue_recall_attr.value, "0",
            "venue_recall_amount should be 0 when querier cannot answer RetrievableCDT"
        );

        // No SubMsgs since retrievable was 0
        assert!(
            res.messages.is_empty(),
            "No SubMsgs expected when venue query returns zero"
        );

        // Parse debt_response to confirm (base64-encoded JSON attribute)
        let debt_response_attr = res
            .attributes
            .iter()
            .find(|a| a.key == "debt_response")
            .expect("debt_response attribute missing");
        let debt_binary = cosmwasm_std::Binary::from_base64(&debt_response_attr.value).unwrap();
        let debt_response: membrane::debt::AccrueDebtResponse =
            cosmwasm_std::from_json(&debt_binary).unwrap();
        assert_eq!(
            debt_response.venue_recall_amount,
            Uint128::zero(),
            "AccrueDebtResponse.venue_recall_amount should be zero when query fails"
        );
    }

    #[test]
    fn accrue_and_return_debt_skips_failed_liquidation_venues() {
        // Venues with failed_liquidation=true should be skipped during recall
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        let mut position = create_position_with_indices(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::new(102),
            10_000,
            0,
        );

        // Add a venue that has failed_liquidation=true
        position.deployed_to = vec![membrane::types::DeploymentVenue {
            address: Addr::unchecked("failed_venue"),
            deployed_debt_amount: Uint128::new(5000),
            failed_liquidation: true, // Should be skipped
        }];

        let mut positions = POSITIONS
            .load(deps.as_ref().storage, owner.clone())
            .unwrap();
        if let Some(pos) = positions
            .iter_mut()
            .find(|p| p.position_id == Uint128::new(102))
        {
            *pos = position;
        }
        POSITIONS.save(deps.as_mut().storage, owner.clone(), &positions).unwrap();

        let info = liquidation_info();
        let res = accrue_and_return_debt(
            deps.as_mut(),
            env,
            info,
            owner.to_string(),
            Uint128::new(102),
        )
        .unwrap();

        // Failed venue is skipped, so no recall
        let venue_recall_attr = res
            .attributes
            .iter()
            .find(|a| a.key == "venue_recall_amount")
            .expect("venue_recall_amount attribute missing");
        assert_eq!(
            venue_recall_attr.value, "0",
            "venue_recall_amount should be 0 when all venues have failed_liquidation=true"
        );

        // No SubMsgs since venue was skipped
        assert!(
            res.messages.is_empty(),
            "No SubMsgs expected when venue has failed_liquidation=true"
        );
    }
}
