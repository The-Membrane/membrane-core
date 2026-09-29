#![allow(non_snake_case)]

use cosmwasm_std::testing::{mock_dependencies, mock_env};
use cosmwasm_std::{
    from_json, to_json_binary, Addr, Binary, ContractResult, Decimal, SystemError, SystemResult,
    Uint128, WasmQuery,
};

use crate::liquidations::bad_debt_check;
use crate::reply::handle_reply;
use crate::state::*;
use crate::testing::test_helpers::*;

// ============================================================================
// Mock querier for cross-contract queries
//
// bad_debt_check and query_simulate_liquidation make WasmQuery::Smart calls
// to the Collateral and Debt contracts. We need a custom mock querier
// that handles these queries.
// ============================================================================

/// Create mock_dependencies with a custom querier that handles
/// GetPositionValue and GetDebtPositions queries.
fn mock_deps_with_querier(
    collateral_value: Decimal,
    avg_max_ltv: Decimal,
    remaining_debt: u128,
) -> cosmwasm_std::OwnedDeps<
    cosmwasm_std::MemoryStorage,
    cosmwasm_std::testing::MockApi,
    cosmwasm_std::testing::MockQuerier,
> {
    let mut deps = mock_dependencies();

    let collateral_value_clone = collateral_value;
    let avg_max_ltv_clone = avg_max_ltv;
    let remaining_debt_clone = remaining_debt;

    deps.querier.update_wasm(move |query| match query {
        WasmQuery::Smart { contract_addr, msg } => {
            if contract_addr == "collateral_contract" {
                // Parse as GetPositionValue query
                let response = membrane::collateral::PositionValueResponse {
                    total_value: collateral_value_clone,
                    avg_max_borrow_ltv: Decimal::percent(50),
                    avg_max_ltv: avg_max_ltv_clone,
                    avg_max_threshold_to_delay: Decimal::percent(15),
                };
                SystemResult::Ok(ContractResult::Ok(to_json_binary(&response).unwrap()))
            } else if contract_addr == "debt_contract" {
                // Parse as GetDebtPositions query
                let response = membrane::debt::DebtPositionsResponse {
                    user: "owner".to_string(),
                    positions: vec![membrane::debt::DebtPositionResponse {
                        position_id: Uint128::one(),
                        credit_amount: Uint128::new(remaining_debt_clone),
                        rate_segments: vec![],
                        peg_rate_segments: vec![],
                        deployed_to: vec![],
                        pending_interest: Uint128::zero(),
                        total_interest_accrued: Uint128::zero(),
                    }],
                };
                SystemResult::Ok(ContractResult::Ok(to_json_binary(&response).unwrap()))
            } else {
                SystemResult::Err(SystemError::InvalidRequest {
                    error: format!("Unhandled contract: {}", contract_addr),
                    request: Binary::default(),
                })
            }
        }
        _ => SystemResult::Err(SystemError::InvalidRequest {
            error: "Only Smart queries supported".to_string(),
            request: Binary::default(),
        }),
    });

    deps
}

// ============================================================================
// 1. bad_debt_check — NOW IMPLEMENTED
//
// Queries Collateral.GetPositionValue and Debt.GetDebtPositions.
// If collateral_value < $1 and remaining_debt > 0, routes to debt auction.
// ============================================================================

/// bad_debt_check: no bad debt when collateral value > $1
#[test]
fn bad_debt_check_no_bad_debt_when_collateral_sufficient() {
    let mut deps = mock_deps_with_querier(
        Decimal::from_ratio(1000u128, 1u128), // $1000 collateral
        Decimal::percent(60),
        500_000, // 500K debt
    );
    let env = mock_env();
    init_config(deps.as_mut().storage);

    let result = bad_debt_check(
        deps.as_mut(),
        env,
        Uint128::one(),
        Addr::unchecked("owner"),
    );

    assert!(result.is_ok());
    let response = result.unwrap();

    // Should have method attribute
    let method_attr = response
        .attributes
        .iter()
        .find(|a| a.key == "method")
        .expect("Should have method attribute");
    assert_eq!(method_attr.value, "bad_debt_check");

    // bad_debt_detected should be false
    let bad_debt_attr = response
        .attributes
        .iter()
        .find(|a| a.key == "bad_debt_detected")
        .expect("Should have bad_debt_detected attribute");
    assert_eq!(bad_debt_attr.value, "false");

    // No auction messages
    assert!(
        response.messages.is_empty(),
        "No messages when no bad debt"
    );
}

/// bad_debt_check: bad debt detected when collateral < $1 and debt > 0
#[test]
fn bad_debt_check_detected_when_collateral_exhausted() {
    let mut deps = mock_deps_with_querier(
        Decimal::from_ratio(50u128, 100u128), // $0.50 collateral
        Decimal::percent(60),
        500_000, // 500K remaining debt
    );
    let env = mock_env();
    init_config(deps.as_mut().storage);

    let result = bad_debt_check(
        deps.as_mut(),
        env,
        Uint128::one(),
        Addr::unchecked("owner"),
    );

    assert!(result.is_ok());
    let response = result.unwrap();

    // bad_debt_detected should be true
    let bad_debt_attr = response
        .attributes
        .iter()
        .find(|a| a.key == "bad_debt_detected")
        .expect("Should have bad_debt_detected attribute");
    assert_eq!(bad_debt_attr.value, "true");

    // Should have auction SubMsg (debt_auction is configured in init_config)
    assert!(
        !response.messages.is_empty(),
        "Should have auction message for bad debt routing"
    );

    // Verify the auction message targets the debt_auction contract
    match &response.messages[0].msg {
        cosmwasm_std::CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute {
            contract_addr, ..
        }) => {
            assert_eq!(contract_addr, "debt_auction");
        }
        _ => panic!("Expected WasmMsg::Execute for auction"),
    }
}

/// bad_debt_check: no bad debt when remaining debt is zero
#[test]
fn bad_debt_check_no_bad_debt_when_debt_zero() {
    let mut deps = mock_deps_with_querier(
        Decimal::from_ratio(50u128, 100u128), // $0.50 collateral (under $1)
        Decimal::percent(60),
        0, // No remaining debt
    );
    let env = mock_env();
    init_config(deps.as_mut().storage);

    let result = bad_debt_check(
        deps.as_mut(),
        env,
        Uint128::one(),
        Addr::unchecked("owner"),
    );

    assert!(result.is_ok());
    let response = result.unwrap();

    let bad_debt_attr = response
        .attributes
        .iter()
        .find(|a| a.key == "bad_debt_detected")
        .expect("Should have bad_debt_detected attribute");
    assert_eq!(
        bad_debt_attr.value, "false",
        "No bad debt when remaining debt is zero"
    );
}

// ============================================================================
// 2. query_simulate_liquidation — NOW IMPLEMENTED
//
// Queries Collateral.GetPositionValue and Debt.GetDebtPositions.
// Runs insolvency check and returns repay estimates.
// ============================================================================

/// query_simulate_liquidation: solvent position returns is_insolvent=false
#[test]
fn simulate_liquidation_solvent_position() {
    let mut deps = mock_deps_with_querier(
        Decimal::from_ratio(10_000u128, 1u128), // $10,000 collateral
        Decimal::percent(60),                     // 60% max LTV
        5_000,                                    // $5,000 debt → LTV = 50%
    );
    init_config(deps.as_mut().storage);

    let result = crate::query::query_simulate_liquidation(
        deps.as_ref(),
        Uint128::one(),
        "owner".to_string(),
    );

    assert!(result.is_ok());
    let binary = result.unwrap();
    let response: membrane::liquidation_engine::SimulateLiquidationResponse =
        from_json(binary).unwrap();

    assert!(!response.is_insolvent, "Position should be solvent (LTV 50% < 60%)");
    assert_eq!(response.repay_amount, Uint128::zero());
    assert_eq!(response.caller_fee, Decimal::zero());
}

/// query_simulate_liquidation: insolvent position returns repay estimates
#[test]
fn simulate_liquidation_insolvent_position() {
    let mut deps = mock_deps_with_querier(
        Decimal::from_ratio(10_000u128, 1u128), // $10,000 collateral
        Decimal::percent(60),                     // 60% max LTV
        8_000,                                    // $8,000 debt → LTV = 80%
    );
    init_config(deps.as_mut().storage);

    let result = crate::query::query_simulate_liquidation(
        deps.as_ref(),
        Uint128::one(),
        "owner".to_string(),
    );

    assert!(result.is_ok());
    let binary = result.unwrap();
    let response: membrane::liquidation_engine::SimulateLiquidationResponse =
        from_json(binary).unwrap();

    assert!(response.is_insolvent, "Position should be insolvent (LTV 80% > 60%)");
    assert!(response.repay_amount > Uint128::zero(), "Repay amount should be non-zero");
    assert!(response.caller_fee > Decimal::zero(), "Caller fee should be non-zero");
    assert!(response.protocol_fee > Decimal::zero(), "Protocol fee should be non-zero");
    assert!(response.current_ltv > Decimal::percent(60), "LTV should exceed max");
}

// ============================================================================
// 3. Liquidation stats query
// ============================================================================

/// query_liquidation_stats: empty when no events recorded
#[test]
fn query_liquidation_stats_empty() {
    let mut deps = mock_dependencies();
    init_config(deps.as_mut().storage);

    let result = crate::query::query_liquidation_stats(
        deps.as_ref(),
        None,
        None,
    );

    assert!(result.is_ok());
    let binary = result.unwrap();
    let response: membrane::liquidation_engine::LiquidationStatsResponse =
        from_json(binary).unwrap();

    assert_eq!(response.total_count, 0);
    assert!(response.events.is_empty());
}

/// query_liquidation_stats: returns stored events
#[test]
fn query_liquidation_stats_returns_events() {
    let mut deps = mock_dependencies();
    init_config(deps.as_mut().storage);

    // Store some events
    let events = vec![
        LiquidationEvent {
            position_id: Uint128::one(),
            position_owner: Addr::unchecked("owner1"),
            caller: Addr::unchecked("liquidator1"),
            total_debt: Uint128::new(1_000_000),
            total_repaid: Decimal::from_ratio(800_000u128, 1u128),
            bad_debt: Uint128::new(200_000),
            caller_fee_value: Decimal::from_ratio(10_000u128, 1u128),
            protocol_fee_value: Decimal::zero(),
            timestamp: 1000,
            outcome: LiquidationOutcome::Immediate,
            current_ltv: Decimal::percent(90),
            timer_started_at: None,
            position_assets: vec![],
        },
        LiquidationEvent {
            position_id: Uint128::new(2),
            position_owner: Addr::unchecked("owner2"),
            caller: Addr::unchecked("liquidator2"),
            total_debt: Uint128::new(500_000),
            total_repaid: Decimal::from_ratio(500_000u128, 1u128),
            bad_debt: Uint128::zero(),
            caller_fee_value: Decimal::from_ratio(5_000u128, 1u128),
            protocol_fee_value: Decimal::zero(),
            timestamp: 2000,
            outcome: LiquidationOutcome::DelayExpired,
            current_ltv: Decimal::percent(85),
            timer_started_at: Some(1500),
            position_assets: vec![],
        },
    ];
    LIQUIDATION_STATS
        .save(deps.as_mut().storage, &events)
        .unwrap();

    let result = crate::query::query_liquidation_stats(
        deps.as_ref(),
        None,
        None,
    );

    assert!(result.is_ok());
    let binary = result.unwrap();
    let response: membrane::liquidation_engine::LiquidationStatsResponse =
        from_json(binary).unwrap();

    assert_eq!(response.total_count, 2);
    assert_eq!(response.events.len(), 2);
    assert_eq!(response.events[0].position_id, Uint128::one());
    assert_eq!(response.events[0].bad_debt, Uint128::new(200_000));
    assert_eq!(response.events[1].position_id, Uint128::new(2));
    assert_eq!(response.events[1].bad_debt, Uint128::zero());
}

/// query_liquidation_stats: pagination with start_after and limit
#[test]
fn query_liquidation_stats_pagination() {
    let mut deps = mock_dependencies();
    init_config(deps.as_mut().storage);

    // Store 5 events
    let events: Vec<LiquidationEvent> = (0..5)
        .map(|i| LiquidationEvent {
            position_id: Uint128::new(i as u128),
            position_owner: Addr::unchecked(format!("owner{}", i)),
            caller: Addr::unchecked("liquidator"),
            total_debt: Uint128::new(100_000 * (i as u128 + 1)),
            total_repaid: Decimal::zero(),
            bad_debt: Uint128::zero(),
            caller_fee_value: Decimal::zero(),
            protocol_fee_value: Decimal::zero(),
            timestamp: 1000 + (i as u64 * 100),
            outcome: LiquidationOutcome::Immediate,
            current_ltv: Decimal::percent(90),
            timer_started_at: None,
            position_assets: vec![],
        })
        .collect();
    LIQUIDATION_STATS
        .save(deps.as_mut().storage, &events)
        .unwrap();

    // Get first 2
    let result = crate::query::query_liquidation_stats(deps.as_ref(), Some(0), Some(2));
    let response: membrane::liquidation_engine::LiquidationStatsResponse =
        from_json(result.unwrap()).unwrap();
    assert_eq!(response.events.len(), 2);
    assert_eq!(response.total_count, 5);

    // Get next 2 (start_after=2)
    let result = crate::query::query_liquidation_stats(deps.as_ref(), Some(2), Some(2));
    let response: membrane::liquidation_engine::LiquidationStatsResponse =
        from_json(result.unwrap()).unwrap();
    assert_eq!(response.events.len(), 2);
    assert_eq!(response.events[0].position_id, Uint128::new(2));

    // Get last 1 (start_after=4)
    let result = crate::query::query_liquidation_stats(deps.as_ref(), Some(4), Some(10));
    let response: membrane::liquidation_engine::LiquidationStatsResponse =
        from_json(result.unwrap()).unwrap();
    assert_eq!(response.events.len(), 1);
    assert_eq!(response.events[0].position_id, Uint128::new(4));
}

// ============================================================================
// 3b. GetLastLiquidationTimestampForAsset query
// ============================================================================

/// Returns the latest timestamp for events involving the queried asset
#[test]
fn query_last_liq_timestamp_for_asset_returns_latest() {
    let mut deps = mock_dependencies();
    init_config(deps.as_mut().storage);

    let events = vec![
        LiquidationEvent {
            position_id: Uint128::one(),
            position_owner: Addr::unchecked("owner1"),
            caller: Addr::unchecked("liquidator"),
            total_debt: Uint128::new(1_000_000),
            total_repaid: Decimal::zero(),
            bad_debt: Uint128::zero(),
            caller_fee_value: Decimal::zero(),
            protocol_fee_value: Decimal::zero(),
            timestamp: 1000,
            outcome: LiquidationOutcome::Immediate,
            current_ltv: Decimal::percent(90),
            timer_started_at: None,
            position_assets: vec![PositionAssetSnapshot {
                denom: "atom".to_string(),
                amount: Uint128::new(100),
                value: Decimal::from_ratio(100u128, 1u128),
                max_ltv: Decimal::percent(80),
            }],
        },
        LiquidationEvent {
            position_id: Uint128::new(2),
            position_owner: Addr::unchecked("owner2"),
            caller: Addr::unchecked("liquidator"),
            total_debt: Uint128::new(500_000),
            total_repaid: Decimal::zero(),
            bad_debt: Uint128::zero(),
            caller_fee_value: Decimal::zero(),
            protocol_fee_value: Decimal::zero(),
            timestamp: 3000,
            outcome: LiquidationOutcome::DelayExpired,
            current_ltv: Decimal::percent(85),
            timer_started_at: Some(2500),
            position_assets: vec![PositionAssetSnapshot {
                denom: "atom".to_string(),
                amount: Uint128::new(200),
                value: Decimal::from_ratio(200u128, 1u128),
                max_ltv: Decimal::percent(80),
            }],
        },
    ];
    LIQUIDATION_STATS.save(deps.as_mut().storage, &events).unwrap();

    let result = crate::query::query_last_liquidation_timestamp_for_asset(
        deps.as_ref(),
        "atom".to_string(),
    ).unwrap();
    let response: membrane::liquidation_engine::LastLiquidationTimestampForAssetResponse =
        from_json(result).unwrap();

    // Should return max(timestamp=3000, timer_started_at=2500) = 3000
    assert_eq!(response.timestamp, Some(3000));
}

/// Returns None when no events match the queried asset
#[test]
fn query_last_liq_timestamp_for_asset_no_match() {
    let mut deps = mock_dependencies();
    init_config(deps.as_mut().storage);

    let events = vec![LiquidationEvent {
        position_id: Uint128::one(),
        position_owner: Addr::unchecked("owner1"),
        caller: Addr::unchecked("liquidator"),
        total_debt: Uint128::new(1_000_000),
        total_repaid: Decimal::zero(),
        bad_debt: Uint128::zero(),
        caller_fee_value: Decimal::zero(),
        protocol_fee_value: Decimal::zero(),
        timestamp: 1000,
        outcome: LiquidationOutcome::Immediate,
        current_ltv: Decimal::percent(90),
        timer_started_at: None,
        position_assets: vec![PositionAssetSnapshot {
            denom: "atom".to_string(),
            amount: Uint128::new(100),
            value: Decimal::from_ratio(100u128, 1u128),
            max_ltv: Decimal::percent(80),
        }],
    }];
    LIQUIDATION_STATS.save(deps.as_mut().storage, &events).unwrap();

    let result = crate::query::query_last_liquidation_timestamp_for_asset(
        deps.as_ref(),
        "osmo".to_string(), // Different asset
    ).unwrap();
    let response: membrane::liquidation_engine::LastLiquidationTimestampForAssetResponse =
        from_json(result).unwrap();

    assert_eq!(response.timestamp, None);
}

/// Returns None when no liquidation events exist at all
#[test]
fn query_last_liq_timestamp_for_asset_empty_stats() {
    let mut deps = mock_dependencies();
    init_config(deps.as_mut().storage);

    // No events stored
    let result = crate::query::query_last_liquidation_timestamp_for_asset(
        deps.as_ref(),
        "atom".to_string(),
    ).unwrap();
    let response: membrane::liquidation_engine::LastLiquidationTimestampForAssetResponse =
        from_json(result).unwrap();

    assert_eq!(response.timestamp, None);
}

/// Excludes SavedByDelay events — these represent positions that recovered
#[test]
fn query_last_liq_timestamp_excludes_saved_by_delay() {
    let mut deps = mock_dependencies();
    init_config(deps.as_mut().storage);

    let events = vec![
        LiquidationEvent {
            position_id: Uint128::one(),
            position_owner: Addr::unchecked("owner1"),
            caller: Addr::unchecked("liquidator"),
            total_debt: Uint128::new(1_000_000),
            total_repaid: Decimal::zero(),
            bad_debt: Uint128::zero(),
            caller_fee_value: Decimal::zero(),
            protocol_fee_value: Decimal::zero(),
            timestamp: 1000,
            outcome: LiquidationOutcome::Immediate,
            current_ltv: Decimal::percent(90),
            timer_started_at: None,
            position_assets: vec![PositionAssetSnapshot {
                denom: "atom".to_string(),
                amount: Uint128::new(100),
                value: Decimal::from_ratio(100u128, 1u128),
                max_ltv: Decimal::percent(80),
            }],
        },
        // More recent event, but SavedByDelay — should be skipped
        LiquidationEvent {
            position_id: Uint128::new(2),
            position_owner: Addr::unchecked("owner2"),
            caller: Addr::unchecked("liquidator"),
            total_debt: Uint128::zero(),
            total_repaid: Decimal::zero(),
            bad_debt: Uint128::zero(),
            caller_fee_value: Decimal::zero(),
            protocol_fee_value: Decimal::zero(),
            timestamp: 5000,
            outcome: LiquidationOutcome::SavedByDelay,
            current_ltv: Decimal::percent(75),
            timer_started_at: Some(4000),
            position_assets: vec![PositionAssetSnapshot {
                denom: "atom".to_string(),
                amount: Uint128::new(200),
                value: Decimal::from_ratio(200u128, 1u128),
                max_ltv: Decimal::percent(80),
            }],
        },
    ];
    LIQUIDATION_STATS.save(deps.as_mut().storage, &events).unwrap();

    let result = crate::query::query_last_liquidation_timestamp_for_asset(
        deps.as_ref(),
        "atom".to_string(),
    ).unwrap();
    let response: membrane::liquidation_engine::LastLiquidationTimestampForAssetResponse =
        from_json(result).unwrap();

    // Should return 1000 from the Immediate event, skipping the SavedByDelay at 5000
    assert_eq!(response.timestamp, Some(1000));
}

/// Uses timer_started_at when it's greater than timestamp
#[test]
fn query_last_liq_timestamp_uses_timer_started_at_when_greater() {
    let mut deps = mock_dependencies();
    init_config(deps.as_mut().storage);

    let events = vec![LiquidationEvent {
        position_id: Uint128::one(),
        position_owner: Addr::unchecked("owner1"),
        caller: Addr::unchecked("liquidator"),
        total_debt: Uint128::new(1_000_000),
        total_repaid: Decimal::zero(),
        bad_debt: Uint128::zero(),
        caller_fee_value: Decimal::zero(),
        protocol_fee_value: Decimal::zero(),
        timestamp: 1000,
        outcome: LiquidationOutcome::TimerStarted,
        current_ltv: Decimal::percent(82),
        timer_started_at: Some(1000), // Same as timestamp for TimerStarted
        position_assets: vec![PositionAssetSnapshot {
            denom: "atom".to_string(),
            amount: Uint128::new(100),
            value: Decimal::from_ratio(100u128, 1u128),
            max_ltv: Decimal::percent(80),
        }],
    }];
    LIQUIDATION_STATS.save(deps.as_mut().storage, &events).unwrap();

    let result = crate::query::query_last_liquidation_timestamp_for_asset(
        deps.as_ref(),
        "atom".to_string(),
    ).unwrap();
    let response: membrane::liquidation_engine::LastLiquidationTimestampForAssetResponse =
        from_json(result).unwrap();

    // max(1000, 1000) = 1000
    assert_eq!(response.timestamp, Some(1000));
}

/// Multi-asset position: returns timestamp when queried asset is one of multiple
#[test]
fn query_last_liq_timestamp_multi_asset_position() {
    let mut deps = mock_dependencies();
    init_config(deps.as_mut().storage);

    let events = vec![LiquidationEvent {
        position_id: Uint128::one(),
        position_owner: Addr::unchecked("owner1"),
        caller: Addr::unchecked("liquidator"),
        total_debt: Uint128::new(1_000_000),
        total_repaid: Decimal::zero(),
        bad_debt: Uint128::zero(),
        caller_fee_value: Decimal::zero(),
        protocol_fee_value: Decimal::zero(),
        timestamp: 2000,
        outcome: LiquidationOutcome::Immediate,
        current_ltv: Decimal::percent(90),
        timer_started_at: None,
        position_assets: vec![
            PositionAssetSnapshot {
                denom: "atom".to_string(),
                amount: Uint128::new(50),
                value: Decimal::from_ratio(50u128, 1u128),
                max_ltv: Decimal::percent(80),
            },
            PositionAssetSnapshot {
                denom: "osmo".to_string(),
                amount: Uint128::new(100),
                value: Decimal::from_ratio(100u128, 1u128),
                max_ltv: Decimal::percent(70),
            },
        ],
    }];
    LIQUIDATION_STATS.save(deps.as_mut().storage, &events).unwrap();

    // Both assets should match
    let result_atom = crate::query::query_last_liquidation_timestamp_for_asset(
        deps.as_ref(), "atom".to_string(),
    ).unwrap();
    let resp_atom: membrane::liquidation_engine::LastLiquidationTimestampForAssetResponse =
        from_json(result_atom).unwrap();
    assert_eq!(resp_atom.timestamp, Some(2000));

    let result_osmo = crate::query::query_last_liquidation_timestamp_for_asset(
        deps.as_ref(), "osmo".to_string(),
    ).unwrap();
    let resp_osmo: membrane::liquidation_engine::LastLiquidationTimestampForAssetResponse =
        from_json(result_osmo).unwrap();
    assert_eq!(resp_osmo.timestamp, Some(2000));

    // Unrelated asset should not match
    let result_btc = crate::query::query_last_liquidation_timestamp_for_asset(
        deps.as_ref(), "btc".to_string(),
    ).unwrap();
    let resp_btc: membrane::liquidation_engine::LastLiquidationTimestampForAssetResponse =
        from_json(result_btc).unwrap();
    assert_eq!(resp_btc.timestamp, None);
}

// ============================================================================
// 4. Reply dispatch edge cases (VALID — these test real logic)
// ============================================================================

/// handle_reply with an unknown reply ID (999) returns an error
/// containing "Unknown reply ID".
#[test]
fn unknown_reply_id_errors() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let reply = make_success_reply(999, vec![]);
    let result = handle_reply(deps.as_mut(), env, reply);

    assert!(result.is_err(), "Unknown reply ID should produce an error");
    let err_str = result.unwrap_err().to_string();
    assert!(
        err_str.contains("Unknown reply ID"),
        "Error should mention 'Unknown reply ID', got: {}",
        err_str
    );
}

/// Multiple unknown reply IDs (0, 9, 100) all produce "Unknown reply ID"
/// errors, confirming the dispatcher rejects anything outside [1..8].
#[test]
fn multiple_unknown_ids_error() {
    let unknown_ids: Vec<u64> = vec![0, 10, 100];

    for id in unknown_ids {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let reply = make_success_reply(id, vec![]);
        let result = handle_reply(deps.as_mut(), env, reply);

        assert!(
            result.is_err(),
            "Reply ID {} should produce an error",
            id
        );
        let err_str = result.unwrap_err().to_string();
        assert!(
            err_str.contains("Unknown reply ID"),
            "Reply ID {} should produce 'Unknown reply ID' error, got: {}",
            id,
            err_str
        );
    }
}

/// BAD_DEBT_REPLY_ID (8) is now a valid handler — returns Ok with no-op.
#[test]
fn bad_debt_reply_id_is_handled() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let reply = make_success_reply(BAD_DEBT_REPLY_ID, vec![]);
    let result = handle_reply(deps.as_mut(), env, reply);

    assert!(result.is_ok(), "BAD_DEBT_REPLY_ID should be handled");
    let response = result.unwrap();
    let method_attr = response
        .attributes
        .iter()
        .find(|a| a.key == "method")
        .expect("Should have method attribute");
    assert_eq!(method_attr.value, "bad_debt_reply_noop");
}
