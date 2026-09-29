#![allow(non_snake_case)]
//! Reply handler tests for the Debt contract.
//!
//! Tests all reply handlers in reply.rs:
//! - handle_collateral_refresh_reply (COLLATERAL_REFRESH_REPLY_ID = 1)
//! - handle_close_position_sell_reply (CLOSE_POSITION_SELL_REPLY_ID = 2)
//! - handle_close_position_withdraw_reply (CLOSE_POSITION_WITHDRAW_REPLY_ID = 3)
//! - handle_deployable_venue_reply (DEPLOYABLE_VENUE_REPLY_ID = 5)
//! - handle_revenue_reply (REVENUE_REPLY_ID = 6)
//! - handle_reply dispatcher (unknown ID)
//!
//! Uses mock_dependencies() and manually constructed Reply structs.

#[cfg(test)]
pub mod tests {
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        to_json_binary, Addr, Decimal, Uint128,
    };

    use membrane::collateral::CollateralAssessmentResponse;
    use membrane::debt::{AssetRateIndex, DebtPosition};
    use membrane::oracle::PriceResponse;
    use membrane::types::{
        cAsset, Asset, AssetInfo, DeploymentVenue, RateSegment,
    };

    use crate::rates::get_total_position_debt;
    use crate::reply::handle_reply;
    use crate::state::{
        CDT_SUPPLY, CLOSE_POSITION_PROPAGATION, ClosePositionPropagation,
        COLLATERAL_REFRESH_REPLY_ID, CLOSE_POSITION_SELL_REPLY_ID,
        CLOSE_POSITION_WITHDRAW_REPLY_ID, DEPLOYABLE_VENUE_PROPAGATION,
        DEPLOYABLE_VENUE_REPLY_ID, DeployableVenuePropagation,
        INCREASE_DEBT_PROPAGATION, IncreaseDebtPropagation, POSITIONS,
        REVENUE_REPLY_ID,
    };
    use crate::testing::test_helpers::helpers::{
        create_position_with_debt, init_basket, init_basket_rate_index,
        init_cdt_supply, init_collateral_types_cache, init_config,
        init_frozen_state, init_rates, make_error_reply, make_success_reply,
    };

    // ========================================================================
    // Local helpers
    // ========================================================================

    /// Initialize all state items needed for reply tests.
    /// Aligns rates_last_accrued with mock_env() block time to prevent overflow.
    fn init_test_state(storage: &mut dyn cosmwasm_std::Storage) {
        let mut rates = init_rates(storage);
        // Align rates timestamps with mock_env() so time_elapsed is small
        let block_time = cosmwasm_std::testing::mock_env().block.time.seconds();
        rates.rates_last_accrued = block_time;
        rates.credit_last_accrued = block_time;
        crate::state::RATES.save(storage, &rates).unwrap();

        init_config(storage);
        // Reply handlers do NOT call accrue_position, so credit_price = 1.0 is safe here
        // and allows LTV/solvency checks to work correctly.
        init_basket(storage);
        init_cdt_supply(storage, Uint128::new(10_000_000_000));
        init_frozen_state(storage, false);
        init_basket_rate_index(storage, "uosmo");
        init_collateral_types_cache(storage, "uosmo", Decimal::percent(80));
    }

    /// Save IncreaseDebtPropagation state for collateral refresh reply tests.
    fn save_increase_debt_propagation(
        storage: &mut dyn cosmwasm_std::Storage,
        owner: &str,
        position_id: Uint128,
        amount: Option<Uint128>,
        ltv: Option<Decimal>,
    ) {
        INCREASE_DEBT_PROPAGATION
            .save(
                storage,
                &IncreaseDebtPropagation {
                    position_owner: owner.to_string(),
                    position_id,
                    amount,
                    ltv,
                    mint_to_addr: None,
                    peg_debt: false,
                    debt_split: None,
                },
            )
            .unwrap();
    }

    /// Build a CollateralAssessmentResponse with given collateral value.
    fn mock_assessment(total_value: Decimal) -> CollateralAssessmentResponse {
        CollateralAssessmentResponse {
            collateral_assets: vec![cAsset {
                asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: "uosmo".to_string(),
                    },
                    // Price * amount should give total_value
                    // We'll set amount = total_value * 10^6 (since decimals=6 and price=1)
                    amount: (total_value * Decimal::from_ratio(1_000_000u128, 1u128)).to_uint_floor(),
                },
                max_borrow_LTV: Decimal::percent(80),
                max_LTV: Decimal::percent(90),
                rate_index: Decimal::one(),
                peg_rate_index: Decimal::one(),
                pool_info: None,
                force_redemptions: None,
            }],
            ratios: vec![Decimal::one()],
            prices: vec![PriceResponse {
                prices: vec![],
                price: Decimal::one(),
                decimals: 6,
            }],
            avg_max_ltv: Decimal::percent(90),
            avg_max_borrow_ltv: Decimal::percent(80),
            any_frozen: false,
            frozen_assets: vec![],
        }
    }

    /// Build a success reply containing a CollateralAssessmentResponse as an attribute.
    ///
    /// The reply handler parses the attribute value with `cosmwasm_std::from_json(value.as_bytes())`,
    /// so we must store the raw JSON string (not base64).
    fn collateral_refresh_success_reply(assessment: &CollateralAssessmentResponse) -> cosmwasm_std::Reply {
        let json_bytes = to_json_binary(assessment).unwrap();
        let json_str = String::from_utf8(json_bytes.to_vec()).unwrap();
        make_success_reply(
            COLLATERAL_REFRESH_REPLY_ID,
            vec![("assessment_response", &json_str)],
        )
    }

    /// Create a position with collateral_rate_indices for replay test usage.
    fn create_position_for_reply(
        storage: &mut dyn cosmwasm_std::Storage,
        owner: Addr,
        position_id: Uint128,
        var_amount: u128,
    ) -> DebtPosition {
        let mut position = create_position_with_debt(
            storage, owner.clone(), position_id, var_amount, 0,
        );

        // Add collateral_rate_indices
        position.collateral_rate_indices = vec![AssetRateIndex {
            asset_denom: "uosmo".to_string(),
            rate_index: Decimal::one(),
            peg_rate_index: Decimal::one(),
            ratio: Decimal::one(),
        }];

        // Re-save
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

    // ========================================================================
    // handle_collateral_refresh_reply tests
    // ========================================================================

    #[test]
    fn collateral_refresh_success_creates_segments_and_updates_state() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        // Create position with zero debt (first mint)
        create_position_for_reply(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::one(),
            0,
        );

        // Save propagation: mint 500_000 tokens
        save_increase_debt_propagation(
            deps.as_mut().storage,
            &owner.to_string(),
            Uint128::one(),
            Some(Uint128::new(500_000)),
            None,
        );

        // Build assessment with enough collateral ($1000 value)
        let assessment = mock_assessment(Decimal::from_ratio(1000u128, 1u128));
        let reply = collateral_refresh_success_reply(&assessment);

        let res = handle_reply(deps.as_mut(), env, reply).unwrap();

        // Verify response attributes
        assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "collateral_refresh_reply_success"));
        assert!(res.attributes.iter().any(|a| a.key == "mint_amount" && a.value == "500000"));

        // Verify CDT supply increased
        let cdt_supply = CDT_SUPPLY.load(deps.as_ref().storage).unwrap();
        assert_eq!(cdt_supply, Uint128::new(10_000_000_000) + Uint128::new(500_000));

        // Verify position has debt
        let (_, pos) = crate::state::get_target_position(
            deps.as_ref().storage,
            owner,
            Uint128::one(),
        )
        .unwrap();
        let total_debt = get_total_position_debt(&pos);
        assert_eq!(total_debt, Uint128::new(500_000));

        // Verify propagation state cleaned up
        assert!(INCREASE_DEBT_PROPAGATION
            .may_load(deps.as_ref().storage)
            .unwrap()
            .is_none());
    }

    #[test]
    fn collateral_refresh_frozen_asset_errors() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        create_position_for_reply(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::new(2),
            0,
        );

        save_increase_debt_propagation(
            deps.as_mut().storage,
            &owner.to_string(),
            Uint128::new(2),
            Some(Uint128::new(100_000)),
            None,
        );

        // Build assessment with frozen asset
        let mut assessment = mock_assessment(Decimal::from_ratio(1000u128, 1u128));
        assessment.any_frozen = true;
        assessment.frozen_assets = vec!["uosmo".to_string()];

        let reply = collateral_refresh_success_reply(&assessment);

        let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();
        assert!(
            matches!(err, crate::ContractError::Frozen {}),
            "Expected Frozen, got: {:?}",
            err
        );
    }

    #[test]
    fn collateral_refresh_below_debt_minimum_errors() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        create_position_for_reply(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::new(3),
            0,
        );

        // Config debt_minimum is 100_000, try to mint only 50_000
        save_increase_debt_propagation(
            deps.as_mut().storage,
            &owner.to_string(),
            Uint128::new(3),
            Some(Uint128::new(50_000)),
            None,
        );

        let assessment = mock_assessment(Decimal::from_ratio(1000u128, 1u128));
        let reply = collateral_refresh_success_reply(&assessment);

        let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();
        assert!(
            matches!(err, crate::ContractError::BelowDebtMinimum { .. }),
            "Expected BelowDebtMinimum, got: {:?}",
            err
        );
    }

    #[test]
    fn collateral_refresh_no_amount_or_ltv_errors() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        create_position_for_reply(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::new(4),
            0,
        );

        // Save propagation with neither amount nor LTV
        INCREASE_DEBT_PROPAGATION
            .save(
                deps.as_mut().storage,
                &IncreaseDebtPropagation {
                    position_owner: owner.to_string(),
                    position_id: Uint128::new(4),
                    amount: None,
                    ltv: None,
                    mint_to_addr: None,
                    peg_debt: false,
                    debt_split: None,
                },
            )
            .unwrap();

        let assessment = mock_assessment(Decimal::from_ratio(1000u128, 1u128));
        let reply = collateral_refresh_success_reply(&assessment);

        let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();
        assert!(
            matches!(err, crate::ContractError::CustomError { .. }),
            "Expected CustomError about missing amount/LTV, got: {:?}",
            err
        );
    }

    #[test]
    fn collateral_refresh_error_result_cleans_propagation() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        save_increase_debt_propagation(
            deps.as_mut().storage,
            &owner.to_string(),
            Uint128::one(),
            Some(Uint128::new(100_000)),
            None,
        );

        let reply = make_error_reply(COLLATERAL_REFRESH_REPLY_ID, "SubMsg execution failed");

        let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();
        assert!(matches!(err, crate::ContractError::CustomError { .. }));

        // Propagation state should be cleaned up even on error
        assert!(INCREASE_DEBT_PROPAGATION
            .may_load(deps.as_ref().storage)
            .unwrap()
            .is_none());
    }

    #[test]
    fn collateral_refresh_max_borrow_ltv_exceeded() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        // Create position with existing large debt
        create_position_for_reply(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::new(5),
            50_000_000, // 50M existing debt
        );

        // Try to mint another 50M, total debt_value would be huge relative to collateral
        save_increase_debt_propagation(
            deps.as_mut().storage,
            &owner.to_string(),
            Uint128::new(5),
            Some(Uint128::new(50_000_000)),
            None,
        );

        // Collateral value is only $10
        let assessment = mock_assessment(Decimal::from_ratio(10u128, 1u128));
        let reply = collateral_refresh_success_reply(&assessment);

        let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();
        assert!(
            matches!(
                err,
                crate::ContractError::MaxBorrowLTVExceeded {}
                    | crate::ContractError::Insolvent {}
            ),
            "Expected MaxBorrowLTVExceeded or Insolvent, got: {:?}",
            err
        );
    }

    // ========================================================================
    // handle_close_position_sell_reply tests
    // ========================================================================

    #[test]
    fn close_position_sell_success_creates_repay_and_withdraw_msgs() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        // Save propagation state
        CLOSE_POSITION_PROPAGATION
            .save(
                deps.as_mut().storage,
                &ClosePositionPropagation {
                    position_info: membrane::types::UserInfo {
                        position_id: Uint128::one(),
                        position_owner: owner.to_string(),
                    },
                    close_percentage: Decimal::one(),
                    max_spread: Decimal::percent(5),
                    send_to: None,
                },
            )
            .unwrap();

        // Build reply with sell_proceeds_amount
        let reply = make_success_reply(
            CLOSE_POSITION_SELL_REPLY_ID,
            vec![("sell_proceeds_amount", "1000000")],
        );

        let res = handle_reply(deps.as_mut(), env, reply).unwrap();

        // Should have submessages for repay + withdraw
        assert!(
            res.messages.len() >= 1,
            "Expected at least 1 submessage (withdraw), got {}",
            res.messages.len()
        );
        assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "close_position_sell_reply_success"));
        assert!(res.attributes.iter().any(|a| a.key == "proceeds_amount" && a.value == "1000000"));
    }

    #[test]
    fn close_position_sell_zero_proceeds_still_withdraws() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        CLOSE_POSITION_PROPAGATION
            .save(
                deps.as_mut().storage,
                &ClosePositionPropagation {
                    position_info: membrane::types::UserInfo {
                        position_id: Uint128::new(2),
                        position_owner: owner.to_string(),
                    },
                    close_percentage: Decimal::one(),
                    max_spread: Decimal::percent(5),
                    send_to: Some("recipient".to_string()),
                },
            )
            .unwrap();

        // Reply with no sell_proceeds_amount attribute -> defaults to zero
        let reply = make_success_reply(CLOSE_POSITION_SELL_REPLY_ID, vec![]);

        let res = handle_reply(deps.as_mut(), env, reply).unwrap();

        // Should still have the withdraw submessage but no repay
        assert!(
            res.messages.len() >= 1,
            "Expected at least 1 submessage (withdraw), got {}",
            res.messages.len()
        );
        assert!(res.attributes.iter().any(|a| a.key == "proceeds_amount" && a.value == "0"));
    }

    #[test]
    fn close_position_sell_error_cleans_propagation() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        CLOSE_POSITION_PROPAGATION
            .save(
                deps.as_mut().storage,
                &ClosePositionPropagation {
                    position_info: membrane::types::UserInfo {
                        position_id: Uint128::new(3),
                        position_owner: owner.to_string(),
                    },
                    close_percentage: Decimal::one(),
                    max_spread: Decimal::percent(5),
                    send_to: None,
                },
            )
            .unwrap();

        let reply = make_error_reply(CLOSE_POSITION_SELL_REPLY_ID, "sell failed");
        let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();

        assert!(matches!(err, crate::ContractError::CustomError { .. }));

        // Propagation state should be cleaned up
        assert!(CLOSE_POSITION_PROPAGATION
            .may_load(deps.as_ref().storage)
            .unwrap()
            .is_none());
    }

    // ========================================================================
    // handle_close_position_withdraw_reply tests
    // ========================================================================

    #[test]
    fn close_position_withdraw_success_cleans_propagation() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        CLOSE_POSITION_PROPAGATION
            .save(
                deps.as_mut().storage,
                &ClosePositionPropagation {
                    position_info: membrane::types::UserInfo {
                        position_id: Uint128::new(10),
                        position_owner: owner.to_string(),
                    },
                    close_percentage: Decimal::one(),
                    max_spread: Decimal::percent(5),
                    send_to: None,
                },
            )
            .unwrap();

        let reply = make_success_reply(CLOSE_POSITION_WITHDRAW_REPLY_ID, vec![]);

        let res = handle_reply(deps.as_mut(), env, reply).unwrap();

        assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "close_position_withdraw_reply_success"));
        assert!(res.attributes.iter().any(|a| a.key == "position_id" && a.value == "10"));

        // Propagation state cleaned
        assert!(CLOSE_POSITION_PROPAGATION
            .may_load(deps.as_ref().storage)
            .unwrap()
            .is_none());
    }

    #[test]
    fn close_position_withdraw_error_cleans_propagation() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        CLOSE_POSITION_PROPAGATION
            .save(
                deps.as_mut().storage,
                &ClosePositionPropagation {
                    position_info: membrane::types::UserInfo {
                        position_id: Uint128::new(11),
                        position_owner: owner.to_string(),
                    },
                    close_percentage: Decimal::one(),
                    max_spread: Decimal::percent(5),
                    send_to: None,
                },
            )
            .unwrap();

        let reply = make_error_reply(CLOSE_POSITION_WITHDRAW_REPLY_ID, "withdraw failed");
        let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();

        assert!(matches!(err, crate::ContractError::CustomError { .. }));

        // Propagation state cleaned even on error
        assert!(CLOSE_POSITION_PROPAGATION
            .may_load(deps.as_ref().storage)
            .unwrap()
            .is_none());
    }

    // ========================================================================
    // handle_deployable_venue_reply tests
    // ========================================================================

    #[test]
    fn deployable_venue_success_cleans_propagation() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        DEPLOYABLE_VENUE_PROPAGATION
            .save(
                deps.as_mut().storage,
                &DeployableVenuePropagation {
                    position_info: membrane::types::UserInfo {
                        position_id: Uint128::new(20),
                        position_owner: owner.to_string(),
                    },
                    venue_address: "venue_contract".to_string(),
                    deployed_amount: Uint128::new(5000),
                },
            )
            .unwrap();

        let reply = make_success_reply(DEPLOYABLE_VENUE_REPLY_ID, vec![]);

        let res = handle_reply(deps.as_mut(), env, reply).unwrap();

        assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "deployable_venue_reply_success"));
        assert!(res.attributes.iter().any(|a| a.key == "venue" && a.value == "venue_contract"));

        // Propagation state cleaned
        assert!(DEPLOYABLE_VENUE_PROPAGATION
            .may_load(deps.as_ref().storage)
            .unwrap()
            .is_none());
    }

    #[test]
    fn deployable_venue_error_sets_failed_liquidation() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");
        let venue_addr = deps.api.addr_make("venue_contract");

        // Create position with a deployment venue
        let position = DebtPosition {
            position_id: Uint128::new(21),
            rate_segments: vec![RateSegment {
                amount: Uint128::new(1000),
                fixed_rate: None,
            }],
            peg_rate_segments: vec![],
            deployed_to: vec![DeploymentVenue {
                address: venue_addr.clone(),
                deployed_debt_amount: Uint128::new(500),
                failed_liquidation: false,
            }],
            pending_interest: Uint128::zero(),
            total_interest_accrued: Uint128::zero(),
            vol_window_initial_debt: None,
            collateral_rate_indices: vec![],
        };
        POSITIONS
            .save(deps.as_mut().storage, owner.clone(), &vec![position])
            .unwrap();

        DEPLOYABLE_VENUE_PROPAGATION
            .save(
                deps.as_mut().storage,
                &DeployableVenuePropagation {
                    position_info: membrane::types::UserInfo {
                        position_id: Uint128::new(21),
                        position_owner: owner.to_string(),
                    },
                    venue_address: venue_addr.to_string(),
                    deployed_amount: Uint128::new(500),
                },
            )
            .unwrap();

        let reply = make_error_reply(DEPLOYABLE_VENUE_REPLY_ID, "venue execution failed");

        // Should return Ok (non-critical error handling)
        let res = handle_reply(deps.as_mut(), env, reply).unwrap();

        assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "deployable_venue_reply_error"));
        assert!(res.attributes.iter().any(|a| a.key == "failed_liquidation" && a.value == "true"));

        // Check that the venue's failed_liquidation flag is set
        let (_, pos) = crate::state::get_target_position(
            deps.as_ref().storage,
            owner,
            Uint128::new(21),
        )
        .unwrap();
        let venue = pos
            .deployed_to
            .iter()
            .find(|v| v.address == venue_addr)
            .expect("Venue should still exist");
        assert!(venue.failed_liquidation, "failed_liquidation should be set to true");

        // Propagation state cleaned
        assert!(DEPLOYABLE_VENUE_PROPAGATION
            .may_load(deps.as_ref().storage)
            .unwrap()
            .is_none());
    }

    #[test]
    fn deployable_venue_error_without_matching_venue_still_ok() {
        let mut deps = mock_dependencies();
        init_test_state(deps.as_mut().storage);
        let env = mock_env();
        let owner = deps.api.addr_make("user1");

        // Create position WITHOUT the venue that's in propagation
        create_position_with_debt(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::new(22),
            1000,
            0,
        );

        DEPLOYABLE_VENUE_PROPAGATION
            .save(
                deps.as_mut().storage,
                &DeployableVenuePropagation {
                    position_info: membrane::types::UserInfo {
                        position_id: Uint128::new(22),
                        position_owner: owner.to_string(),
                    },
                    venue_address: "nonexistent_venue".to_string(),
                    deployed_amount: Uint128::new(500),
                },
            )
            .unwrap();

        let reply = make_error_reply(DEPLOYABLE_VENUE_REPLY_ID, "venue failed");

        // Should still return Ok (non-critical)
        let res = handle_reply(deps.as_mut(), env, reply).unwrap();
        assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "deployable_venue_reply_error"));
    }

    // ========================================================================
    // handle_revenue_reply tests
    // ========================================================================

    #[test]
    fn revenue_reply_success() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let reply = make_success_reply(REVENUE_REPLY_ID, vec![]);
        let res = handle_reply(deps.as_mut(), env, reply).unwrap();

        assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "revenue_reply_success"));
    }

    #[test]
    fn revenue_reply_error_is_non_critical() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let reply = make_error_reply(REVENUE_REPLY_ID, "revenue distribution failed");

        // Should return Ok even on error (non-critical)
        let res = handle_reply(deps.as_mut(), env, reply).unwrap();
        assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "revenue_reply_error"));
        assert!(res.attributes.iter().any(|a| a.key == "error" && a.value == "revenue distribution failed"));
    }

    // ========================================================================
    // handle_reply dispatcher test
    // ========================================================================

    #[test]
    fn handle_reply_unknown_id_errors() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let reply = make_success_reply(999, vec![]);
        let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();

        match err {
            crate::ContractError::CustomError { val } => {
                assert!(
                    val.contains("Unknown reply ID"),
                    "Expected 'Unknown reply ID' in error, got: {}",
                    val
                );
            }
            other => panic!("Expected CustomError, got: {:?}", other),
        }
    }
}
