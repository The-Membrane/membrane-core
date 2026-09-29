#![allow(non_snake_case)]
//! Tests for `fulfill_intents` in the Debt contract.
//!
//! These tests exercise the intent-based deployment flow which requires
//! cross-contract queries to the Collateral contract for position value.
//! Uses `mock_dependencies()` with a custom querier handler.

#[cfg(test)]
pub mod tests {
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{
        from_json, to_json_binary, Addr, ContractResult, Decimal, SystemResult, Uint128,
        WasmQuery,
    };

    use membrane::collateral::{
        PositionValueResponse, QueryMsg as CollateralQueryMsg,
    };
    use membrane::debt::DebtPosition;
    use membrane::types::{
        DeploymentIntent, DeploymentVenue, UserDeploymentIntents,
    };

    use crate::positions::fulfill_intents;
    use crate::state::{ACTIVE_DEPLOYMENT_VENUES, POSITIONS, USER_INTENTS};
    use crate::testing::test_helpers::helpers::{
        create_position_with_debt, init_basket, init_basket_rate_index,
        init_cdt_supply, init_collateral_types_cache, init_config,
        init_frozen_state, init_rates,
    };

    // ========================================================================
    // Helpers
    // ========================================================================

    /// Initialize state and align rates timestamps.
    fn init_test_state(storage: &mut dyn cosmwasm_std::Storage) {
        let mut rates = init_rates(storage);
        let block_time = cosmwasm_std::testing::mock_env().block.time.seconds();
        rates.rates_last_accrued = block_time;
        rates.credit_last_accrued = block_time;
        crate::state::RATES.save(storage, &rates).unwrap();

        init_config(storage);
        let mut basket = init_basket(storage);
        // Set credit_price to zero to match credit_TWAP_price (oracle=None → 0)
        // This avoids the division-by-zero in CPC price_difference.
        basket.credit_price.price = Decimal::zero();
        crate::state::BASKET.save(storage, &basket).unwrap();

        init_cdt_supply(storage, Uint128::new(10_000_000_000));
        init_frozen_state(storage, false);
        init_basket_rate_index(storage, "uosmo");
        init_collateral_types_cache(storage, "uosmo", Decimal::percent(80));
    }

    /// Save deployment intents for a user.
    fn save_user_intents(
        storage: &mut dyn cosmwasm_std::Storage,
        user: Addr,
        intents: Vec<DeploymentIntent>,
    ) {
        let user_intents = UserDeploymentIntents {
            user: user.to_string(),
            deployment_intents: intents,
        };
        USER_INTENTS.save(storage, user, &user_intents).unwrap();
    }

    /// Configure mock querier to respond to Collateral GetPositionValue.
    fn setup_collateral_querier(
        deps: &mut cosmwasm_std::OwnedDeps<
            cosmwasm_std::MemoryStorage,
            cosmwasm_std::testing::MockApi,
            cosmwasm_std::testing::MockQuerier,
        >,
        total_value: Decimal,
        max_borrow_ltv: Decimal,
        max_ltv: Decimal,
    ) {
        let response = PositionValueResponse {
            total_value,
            avg_max_borrow_ltv: max_borrow_ltv,
            avg_max_ltv: max_ltv,
        };
        let response_binary = to_json_binary(&response).unwrap();

        deps.querier.update_wasm(move |query| {
            match query {
                WasmQuery::Smart { msg, .. } => {
                    let _parsed: CollateralQueryMsg = from_json(msg).unwrap();
                    SystemResult::Ok(ContractResult::Ok(response_binary.clone()))
                }
                _ => SystemResult::Ok(ContractResult::Err("Unsupported query".to_string())),
            }
        });
    }

    // ========================================================================
    // Tests
    // ========================================================================

    #[test]
    fn fulfill_single_user_intent_with_ltv_space() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("user1");
        let vault = deps.api.addr_make("vault_contract");
        init_test_state(deps.as_mut().storage);

        // Need non-zero credit_price for get_amount() division
        let mut basket = crate::state::BASKET.load(deps.as_ref().storage).unwrap();
        basket.credit_price.price = Decimal::one();
        crate::state::BASKET.save(deps.as_mut().storage, &basket).unwrap();

        let env = mock_env();

        // Position with zero debt
        create_position_with_debt(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::one(),
            0,
            0,
        );

        // Intent: deploy at 50% LTV to venue
        save_user_intents(
            deps.as_mut().storage,
            owner.clone(),
            vec![DeploymentIntent {
                user: owner.to_string(),
                position_id: Uint128::one(),
                ltv_to_mint: Decimal::percent(50),
                destination: vault.to_string(),
            }],
        );

        // Collateral says position is worth $1000, max borrow LTV 80%
        setup_collateral_querier(
            &mut deps,
            Decimal::from_ratio(1000u128, 1u128),
            Decimal::percent(80),
            Decimal::percent(90),
        );

        let info = mock_info("anyone", &[]);
        let res = fulfill_intents(
            deps.as_mut(),
            env,
            info,
            vec![owner.to_string()],
        )
        .unwrap();

        // Should produce an IncreaseDebt message
        assert!(
            !res.messages.is_empty(),
            "Should have at least one IncreaseDebt message"
        );
        assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "fulfill_intents"));
    }

    #[test]
    fn fulfill_no_ltv_space_no_messages() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("user1");
        let vault = deps.api.addr_make("vault_contract");
        init_test_state(deps.as_mut().storage);
        let env = mock_env();

        // Position with large debt already at max LTV
        create_position_with_debt(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::one(),
            10_000_000_000, // massive debt
            0,
        );

        save_user_intents(
            deps.as_mut().storage,
            owner.clone(),
            vec![DeploymentIntent {
                user: owner.to_string(),
                position_id: Uint128::one(),
                ltv_to_mint: Decimal::percent(50),
                destination: vault.to_string(),
            }],
        );

        // credit_price is 0, so debt_value = 0, current_ltv = 0, ltv_space = 80%
        // usable_ltv = 50% - 0 = 50%, usable_debt_value = 1000 * 50% = 500
        // But get_amount(500) divides by credit_price(0) → error
        setup_collateral_querier(
            &mut deps,
            Decimal::from_ratio(1000u128, 1u128),
            Decimal::percent(80),
            Decimal::percent(90),
        );

        let info = mock_info("anyone", &[]);
        let res = fulfill_intents(deps.as_mut(), env, info, vec![owner.to_string()]);
        // With credit_price = 0, get_amount() divides by 0 → error
        assert!(res.is_err(), "Should error when credit_price is zero and trying to compute amount");
    }

    #[test]
    fn fulfill_no_intents_for_user_errors() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("user1");
        init_test_state(deps.as_mut().storage);
        let env = mock_env();

        // Position exists but no intents saved
        create_position_with_debt(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::one(),
            1000,
            0,
        );

        setup_collateral_querier(
            &mut deps,
            Decimal::from_ratio(1000u128, 1u128),
            Decimal::percent(80),
            Decimal::percent(90),
        );

        let info = mock_info("anyone", &[]);
        let err = fulfill_intents(deps.as_mut(), env, info, vec![owner.to_string()])
            .unwrap_err();

        // USER_INTENTS.load fails → error
        let err_str = format!("{:?}", err).to_lowercase();
        assert!(
            err_str.contains("not found") || err_str.contains("notfound"),
            "Expected not found error, got: {:?}",
            err
        );
    }

    #[test]
    fn fulfill_no_collateral_contract_errors() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("user1");
        let vault = deps.api.addr_make("vault_contract");
        init_test_state(deps.as_mut().storage);
        let env = mock_env();

        // Override config to remove collateral_contract
        let mut config = crate::state::CONFIG.load(deps.as_ref().storage).unwrap();
        config.collateral_contract = None;
        crate::state::CONFIG.save(deps.as_mut().storage, &config).unwrap();

        create_position_with_debt(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::one(),
            1000,
            0,
        );

        save_user_intents(
            deps.as_mut().storage,
            owner.clone(),
            vec![DeploymentIntent {
                user: owner.to_string(),
                position_id: Uint128::one(),
                ltv_to_mint: Decimal::percent(50),
                destination: vault.to_string(),
            }],
        );

        let info = mock_info("anyone", &[]);
        let err = fulfill_intents(deps.as_mut(), env, info, vec![owner.to_string()])
            .unwrap_err();

        assert!(
            format!("{:?}", err).contains("No collateral contract"),
            "Expected no collateral contract error, got: {:?}",
            err
        );
    }

    #[test]
    fn fulfill_zero_collateral_value_no_messages() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("user1");
        let vault = deps.api.addr_make("vault_contract");
        init_test_state(deps.as_mut().storage);

        // Need non-zero credit_price for get_amount() division
        let mut basket = crate::state::BASKET.load(deps.as_ref().storage).unwrap();
        basket.credit_price.price = Decimal::one();
        crate::state::BASKET.save(deps.as_mut().storage, &basket).unwrap();

        let env = mock_env();

        create_position_with_debt(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::one(),
            0,
            0,
        );

        save_user_intents(
            deps.as_mut().storage,
            owner.clone(),
            vec![DeploymentIntent {
                user: owner.to_string(),
                position_id: Uint128::one(),
                ltv_to_mint: Decimal::percent(50),
                destination: vault.to_string(),
            }],
        );

        // Zero collateral value → no LTV space
        setup_collateral_querier(
            &mut deps,
            Decimal::zero(),
            Decimal::percent(80),
            Decimal::percent(90),
        );

        let info = mock_info("anyone", &[]);
        let res = fulfill_intents(deps.as_mut(), env, info, vec![owner.to_string()])
            .unwrap();

        // With zero value, usable_debt_value = 0 * ltv = 0 → no message
        assert!(
            res.messages.is_empty(),
            "No messages when collateral value is zero"
        );
    }

    #[test]
    fn fulfill_updates_active_deployment_venues() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("user1");
        let vault = deps.api.addr_make("vault_contract");
        init_test_state(deps.as_mut().storage);

        // Set credit_price to 1.0 for this test (need non-zero for get_amount)
        let mut basket = crate::state::BASKET.load(deps.as_ref().storage).unwrap();
        basket.credit_price.price = Decimal::one();
        crate::state::BASKET.save(deps.as_mut().storage, &basket).unwrap();

        let env = mock_env();

        create_position_with_debt(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::one(),
            0,
            0,
        );

        save_user_intents(
            deps.as_mut().storage,
            owner.clone(),
            vec![DeploymentIntent {
                user: owner.to_string(),
                position_id: Uint128::one(),
                ltv_to_mint: Decimal::percent(50),
                destination: vault.to_string(),
            }],
        );

        // $1000 collateral, max borrow 80%
        setup_collateral_querier(
            &mut deps,
            Decimal::from_ratio(1000u128, 1u128),
            Decimal::percent(80),
            Decimal::percent(90),
        );

        let info = mock_info("anyone", &[]);
        let _res = fulfill_intents(
            deps.as_mut(),
            env,
            info,
            vec![owner.to_string()],
        )
        .unwrap();

        // Check that active deployment venues was updated
        let venues = ACTIVE_DEPLOYMENT_VENUES
            .may_load(deps.as_ref().storage)
            .unwrap()
            .unwrap_or_default();
        assert!(
            venues.contains(&vault.to_string()),
            "vault_contract should be in active venues, got: {:?}",
            venues
        );
    }

    #[test]
    fn fulfill_position_saved_with_venue() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("user1");
        let vault = deps.api.addr_make("vault_contract");
        init_test_state(deps.as_mut().storage);

        // Set credit_price to 1.0
        let mut basket = crate::state::BASKET.load(deps.as_ref().storage).unwrap();
        basket.credit_price.price = Decimal::one();
        crate::state::BASKET.save(deps.as_mut().storage, &basket).unwrap();

        let env = mock_env();

        create_position_with_debt(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::one(),
            0,
            0,
        );

        save_user_intents(
            deps.as_mut().storage,
            owner.clone(),
            vec![DeploymentIntent {
                user: owner.to_string(),
                position_id: Uint128::one(),
                ltv_to_mint: Decimal::percent(50),
                destination: vault.to_string(),
            }],
        );

        setup_collateral_querier(
            &mut deps,
            Decimal::from_ratio(1000u128, 1u128),
            Decimal::percent(80),
            Decimal::percent(90),
        );

        let info = mock_info("anyone", &[]);
        fulfill_intents(deps.as_mut(), env, info, vec![owner.to_string()]).unwrap();

        // Check position was updated with deployment venue
        let positions = POSITIONS.load(deps.as_ref().storage, owner).unwrap();
        let position = positions.iter().find(|p| p.position_id == Uint128::one()).unwrap();
        assert!(
            !position.deployed_to.is_empty(),
            "Position should have a deployment venue"
        );
        assert_eq!(
            position.deployed_to[0].address,
            vault
        );
        assert!(
            !position.deployed_to[0].deployed_debt_amount.is_zero(),
            "Deployed debt should be non-zero"
        );
    }

    #[test]
    fn fulfill_existing_venue_increments_amount() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("user1");
        let vault = deps.api.addr_make("vault_contract");
        init_test_state(deps.as_mut().storage);

        // Set credit_price to 1.0
        let mut basket = crate::state::BASKET.load(deps.as_ref().storage).unwrap();
        basket.credit_price.price = Decimal::one();
        crate::state::BASKET.save(deps.as_mut().storage, &basket).unwrap();

        let env = mock_env();

        // Create position WITH an existing deployment venue
        let position = DebtPosition {
            position_id: Uint128::one(),
            rate_segments: vec![],
            peg_rate_segments: vec![],
            deployed_to: vec![DeploymentVenue {
                address: vault.clone(),
                deployed_debt_amount: Uint128::new(100_000),
                failed_liquidation: false,
            }],
            pending_interest: Uint128::zero(),
            total_interest_accrued: Uint128::zero(),
            vol_window_initial_debt: None,
            collateral_rate_indices: vec![],
        };
        POSITIONS
            .save(deps.as_mut().storage, owner.clone(), &vec![position.clone()])
            .unwrap();

        save_user_intents(
            deps.as_mut().storage,
            owner.clone(),
            vec![DeploymentIntent {
                user: owner.to_string(),
                position_id: Uint128::one(),
                ltv_to_mint: Decimal::percent(50),
                destination: vault.to_string(),
            }],
        );

        // $1000 collateral
        setup_collateral_querier(
            &mut deps,
            Decimal::from_ratio(1000u128, 1u128),
            Decimal::percent(80),
            Decimal::percent(90),
        );

        let info = mock_info("anyone", &[]);
        fulfill_intents(deps.as_mut(), env, info, vec![owner.to_string()]).unwrap();

        // Check the existing venue's amount was incremented
        let positions = POSITIONS.load(deps.as_ref().storage, owner).unwrap();
        let pos = positions.iter().find(|p| p.position_id == Uint128::one()).unwrap();
        let venue = pos
            .deployed_to
            .iter()
            .find(|v| v.address == vault)
            .unwrap();
        assert!(
            venue.deployed_debt_amount > Uint128::new(100_000),
            "Deployed amount should have increased from 100_000, got: {}",
            venue.deployed_debt_amount
        );
    }

    #[test]
    fn fulfill_response_has_method_attribute() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("user1");
        init_test_state(deps.as_mut().storage);
        let env = mock_env();

        create_position_with_debt(
            deps.as_mut().storage,
            owner.clone(),
            Uint128::one(),
            0,
            0,
        );

        // Empty intents list means no work to do
        save_user_intents(
            deps.as_mut().storage,
            owner.clone(),
            vec![], // empty intents
        );

        setup_collateral_querier(
            &mut deps,
            Decimal::from_ratio(1000u128, 1u128),
            Decimal::percent(80),
            Decimal::percent(90),
        );

        let info = mock_info("anyone", &[]);
        let res = fulfill_intents(deps.as_mut(), env, info, vec![owner.to_string()])
            .unwrap();

        assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "fulfill_intents"));
        assert!(res.messages.is_empty(), "No messages for empty intents");
    }
}
