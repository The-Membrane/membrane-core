#![allow(non_snake_case)]

use cosmwasm_std::testing::mock_dependencies;
use cosmwasm_std::{Addr, Decimal, Uint128};

use crate::reply::handle_reply;
use crate::state::*;
use crate::testing::test_helpers::*;

// ========================================================================
// Group A: handle_debt_accrue_reply (5 tests)
// ========================================================================

#[test]
fn debt_accrue_success_stores_propagation() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // Seed state
    init_config(deps.as_mut().storage);
    let prop = default_propagation();
    save_propagation(deps.as_mut().storage, &prop);

    // Build debt accrue success reply: total_debt=1_000_000, credit_price=1.0, decimals=6
    let reply = make_debt_accrue_success_reply(1_000_000, Decimal::one(), 6);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Verify propagation was updated
    let updated_prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();
    assert_eq!(updated_prop.total_debt, Uint128::new(1_000_000));
    assert_eq!(updated_prop.credit_price.price, Decimal::one());
    assert_eq!(updated_prop.credit_price.decimals, 6u64);

    // Verify response has a SubMsg and attributes
    assert_eq!(res.messages.len(), 1);
    assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "debt_accrue_reply_success"));
    assert!(res.attributes.iter().any(|a| a.key == "total_debt" && a.value == "1000000"));
}

#[test]
fn debt_accrue_success_submsg_targets_collateral_contract_with_assess_id() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);
    let prop = default_propagation();
    save_propagation(deps.as_mut().storage, &prop);

    let reply = make_debt_accrue_success_reply(500_000, Decimal::percent(200), 6);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // The SubMsg should have COLLATERAL_ASSESS_REPLY_ID = 2
    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, COLLATERAL_ASSESS_REPLY_ID);

    // The SubMsg should target the collateral_contract
    match &res.messages[0].msg {
        cosmwasm_std::CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute {
            contract_addr, ..
        }) => {
            assert_eq!(contract_addr, "collateral_contract");
        }
        _ => panic!("Expected WasmMsg::Execute"),
    }
}

#[test]
fn debt_accrue_missing_attr_errors() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);
    let prop = default_propagation();
    save_propagation(deps.as_mut().storage, &prop);

    // Build a success reply WITHOUT the "debt_response" attribute
    let reply = make_success_reply(DEBT_ACCRUE_REPLY_ID, vec![("other_key", "other_value")]);
    let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();
    let err_string = err.to_string();
    assert!(
        err_string.contains("Attribute 'debt_response' not found"),
        "Expected 'Attribute not found' error, got: {}",
        err_string
    );
}

#[test]
fn debt_accrue_malformed_json_errors() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);
    let prop = default_propagation();
    save_propagation(deps.as_mut().storage, &prop);

    // Build a success reply with malformed JSON in debt_response
    let reply = make_success_reply(
        DEBT_ACCRUE_REPLY_ID,
        vec![("debt_response", "not-valid-json{{{")],
    );
    let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();
    let err_string = err.to_string();
    assert!(
        err_string.contains("Failed to parse"),
        "Expected parse failure error, got: {}",
        err_string
    );
}

#[test]
fn debt_accrue_error_reply_returns_error() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);
    let prop = default_propagation();
    save_propagation(deps.as_mut().storage, &prop);

    let reply = make_error_reply(DEBT_ACCRUE_REPLY_ID, "something went wrong");
    let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();
    let err_string = err.to_string();
    assert!(
        err_string.contains("Debt accrual failed"),
        "Expected 'Debt accrual failed' error, got: {}",
        err_string
    );
}

// ========================================================================
// Group B: handle_collateral_assess_reply (10 tests)
// ========================================================================

/// Helper: seed an insolvent single-asset scenario for step 2 tests.
///
/// collateral: 1_000_000 uatom, price=$10, decimals=6 -> value = 1e6/1e6 * 10 = $10
/// debt: 9_000_000 credits, credit_price=$1, decimals=6 -> value = 9e6/1e6 * 1 = $9
/// max_ltv: 80%
/// current_ltv = 9/10 = 0.9 = 90% > 80% -> insolvent
fn setup_insolvent_single_asset(deps: &mut cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>) {
    let prop = make_propagation_post_debt_accrue(9_000_000, Decimal::one(), 6);
    save_propagation(deps.as_mut().storage, &prop);
}

fn insolvent_assess_reply_single_asset() -> cosmwasm_std::Reply {
    let collateral_assets = vec![make_casset("uatom", 1_000_000, 60, 80)];
    let ratios = vec![Decimal::one()]; // single asset = 100%
    let prices = vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)];
    let avg_max_ltv = Decimal::percent(80);
    let avg_max_borrow_ltv = Decimal::percent(60);

    make_collateral_assess_success_reply(
        collateral_assets,
        ratios,
        prices,
        avg_max_ltv,
        avg_max_borrow_ltv,
        false,
        vec![],
    )
}

/// With the default test scenario (9M micro-unit debt, 1M uatom at $10),
/// fees fit within position holdings → Step 2 chains to FEE_TRANSFER_REPLY_ID
/// first, then after fee transfer completes, chains to LQ transfer.
#[test]
fn collateral_assess_insolvent_with_liq_queue_sends_lq_transfer() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config_with_liq_queue(deps.as_mut().storage);
    setup_insolvent_single_asset(&mut deps);

    let reply = insolvent_assess_reply_single_asset();
    let res = handle_reply(deps.as_mut(), env.clone(), reply).unwrap();

    // Step 2 chains to fee transfer first (fees fit within position)
    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, FEE_TRANSFER_REPLY_ID);

    // Simulate fee_transfer_reply to advance to LQ transfer
    let fee_reply = make_success_reply(FEE_TRANSFER_REPLY_ID, vec![]);
    let res2 = handle_reply(deps.as_mut(), env, fee_reply).unwrap();

    // After fee transfer, response has BankMsg::Send (caller fee) + LQ transfer SubMsg
    let lq_msg = res2.messages.iter().find(|m| m.id == LIQ_QUEUE_TRANSFER_REPLY_ID);
    assert!(lq_msg.is_some(), "Expected LIQ_QUEUE_TRANSFER_REPLY_ID after fee transfer");

    // Target should be the collateral_contract (AuthorizedTransfer to LQ)
    match &lq_msg.unwrap().msg {
        cosmwasm_std::CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute {
            contract_addr, ..
        }) => {
            assert_eq!(contract_addr, "collateral_contract");
        }
        _ => panic!("Expected WasmMsg::Execute"),
    }
}

/// When fee token amounts fit within position holdings, Step 2 chains
/// to FEE_TRANSFER_REPLY_ID (AuthorizedTransfer for fee extraction).
///
/// Uses credit_price decimals=0 so debt_value units match collateral_value.
#[test]
fn collateral_assess_insolvent_with_fees_chains_to_fee_transfer() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);

    // total_debt = 90 (no 10^6 scaling, credit decimals=0)
    // credit_price = $1, decimals=0
    // → debt_value = from_ratio(90, 1) * 1.0 = 90.0
    // collateral: 10_000_000 uatom at $10, decimals=6
    // → collateral_value = (10M/10^6) * 10 = 100
    // → current_ltv = 90/100 = 0.9 > 0.8 → insolvent
    // → excess_ltv = 0.1, repay_value = 9
    // → caller_fee = 9 * 0.01 = 0.09
    // → per-asset fee amount = get_amount(0.09) at $10/atom = 9_000 uatom
    // → 9_000 < 10_000_000 → NOT zeroed → FEE_TRANSFER_REPLY_ID
    let prop = make_propagation_post_debt_accrue(90, Decimal::one(), 0);
    save_propagation(deps.as_mut().storage, &prop);

    let collateral_assets = vec![make_casset("uatom", 10_000_000, 60, 80)];
    let ratios = vec![Decimal::one()];
    let prices = vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)];
    let reply = make_collateral_assess_success_reply(
        collateral_assets,
        ratios,
        prices,
        Decimal::percent(80),
        Decimal::percent(60),
        false,
        vec![],
    );

    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Fees fit within position → AuthorizedTransfer for fees → FEE_TRANSFER_REPLY_ID
    assert_eq!(res.messages.len(), 1);
    assert_eq!(
        res.messages[0].id, FEE_TRANSFER_REPLY_ID,
        "Expected FEE_TRANSFER_REPLY_ID when fees fit within position holdings"
    );

    // Should NOT have no_fees attribute
    assert!(
        !res.attributes.iter().any(|a| a.key == "no_fees"),
        "Should not have no_fees when fees are present"
    );

    // Verify propagation has non-empty caller_fee_assets
    let updated_prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();
    assert!(
        !updated_prop.caller_fee_assets.is_empty(),
        "caller_fee_assets should be non-empty"
    );
    assert!(
        !updated_prop.protocol_fee_assets.is_empty(),
        "protocol_fee_assets should be non-empty"
    );
}

/// Same as above but without liq queue → after fee transfer, chains to sell wall.
#[test]
fn collateral_assess_insolvent_without_liq_queue_sends_sell_wall() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // No liq_queue
    init_config(deps.as_mut().storage);
    setup_insolvent_single_asset(&mut deps);

    let reply = insolvent_assess_reply_single_asset();
    let res = handle_reply(deps.as_mut(), env.clone(), reply).unwrap();

    // Step 2 chains to fee transfer first (fees fit within position)
    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, FEE_TRANSFER_REPLY_ID);

    // Simulate fee_transfer_reply to advance to sell wall
    let fee_reply = make_success_reply(FEE_TRANSFER_REPLY_ID, vec![]);
    let res2 = handle_reply(deps.as_mut(), env, fee_reply).unwrap();

    // No LQ configured → sell wall
    let sell_wall_msg = res2.messages.iter().find(|m| m.id == SELL_WALL_REPLY_ID);
    assert!(sell_wall_msg.is_some(), "Expected SELL_WALL_REPLY_ID after fee transfer");
}

#[test]
fn collateral_assess_solvent_position_errors() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);

    // Solvent position: debt=$5 on $10 collateral = 50% LTV, max=80%
    let prop = make_propagation_post_debt_accrue(5_000_000, Decimal::one(), 6);
    save_propagation(deps.as_mut().storage, &prop);

    let collateral_assets = vec![make_casset("uatom", 1_000_000, 60, 80)];
    let ratios = vec![Decimal::one()];
    let prices = vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)];
    let avg_max_ltv = Decimal::percent(80);
    let avg_max_borrow_ltv = Decimal::percent(60);

    let reply = make_collateral_assess_success_reply(
        collateral_assets,
        ratios,
        prices,
        avg_max_ltv,
        avg_max_borrow_ltv,
        false,
        vec![],
    );

    let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();
    let err_string = err.to_string();
    assert!(
        err_string.contains("solvent"),
        "Expected PositionSolvent error, got: {}",
        err_string
    );
}

#[test]
fn collateral_assess_frozen_assets_errors() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);
    setup_insolvent_single_asset(&mut deps);

    // Frozen assets
    let collateral_assets = vec![make_casset("uatom", 1_000_000, 60, 80)];
    let ratios = vec![Decimal::one()];
    let prices = vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)];

    let reply = make_collateral_assess_success_reply(
        collateral_assets,
        ratios,
        prices,
        Decimal::percent(80),
        Decimal::percent(60),
        true,
        vec!["uatom".to_string()],
    );

    let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();
    let err_string = err.to_string();
    assert!(
        err_string.contains("Circuit breaker: frozen assets detected"),
        "Expected circuit breaker error, got: {}",
        err_string
    );
}

#[test]
fn collateral_assess_per_asset_repayment_empty_after_step2() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);
    setup_insolvent_single_asset(&mut deps);

    let reply = insolvent_assess_reply_single_asset();
    let _res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Check propagation was updated
    let updated_prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();

    // Fix #3: per_asset_repayment is now built in build_next_after_fees (Step 9),
    // not in handle_collateral_assess_reply (Step 2). After Step 2 it is empty.
    assert!(
        updated_prop.per_asset_repayment.is_empty(),
        "per_asset_repayment should be empty after Step 2 (built later in build_next_after_fees)"
    );
}

#[test]
fn collateral_assess_transfer_assets_capped_at_holdings() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config_with_liq_queue(deps.as_mut().storage);

    // Create a scenario with very high debt relative to collateral
    // This forces the transfer amount to be capped at the position's holdings
    // collateral: 100 uatom at $10/atom -> value = 100/1e6*10 = $0.001
    // debt: 1_000_000 credits at $1 -> value = $1
    // LTV = 1/0.001 = 1000 >> 80% -> insolvent, massive repay needed
    let prop = make_propagation_post_debt_accrue(1_000_000, Decimal::one(), 6);
    save_propagation(deps.as_mut().storage, &prop);

    let collateral_assets = vec![make_casset("uatom", 100, 60, 80)]; // tiny holdings
    let ratios = vec![Decimal::one()];
    let prices = vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)]; // $10/uatom
    let avg_max_ltv = Decimal::percent(80);
    let avg_max_borrow_ltv = Decimal::percent(60);

    let reply = make_collateral_assess_success_reply(
        collateral_assets,
        ratios,
        prices,
        avg_max_ltv,
        avg_max_borrow_ltv,
        false,
        vec![],
    );

    // Should succeed (transfer amount capped at 100 uatom holdings)
    let res = handle_reply(deps.as_mut(), env.clone(), reply).unwrap();

    // May go through fee transfer or directly to LQ transfer depending on fee cap.
    // With tiny collateral (100 uatom), fees may overflow and get zeroed, or may fit.
    let first_id = res.messages[0].id;
    if first_id == FEE_TRANSFER_REPLY_ID {
        // Chain through fee transfer
        let fee_reply = make_success_reply(FEE_TRANSFER_REPLY_ID, vec![]);
        let res2 = handle_reply(deps.as_mut(), env, fee_reply).unwrap();
        // After fee transfer, should have LQ transfer
        let has_lq = res2.messages.iter().any(|m| m.id == LIQ_QUEUE_TRANSFER_REPLY_ID);
        assert!(has_lq, "Expected LIQ_QUEUE_TRANSFER after fee transfer");
    } else {
        assert_eq!(first_id, LIQ_QUEUE_TRANSFER_REPLY_ID);
    }
}

#[test]
fn collateral_assess_stores_caller_fee_and_protocol_fee() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // Use a non-default liq_fee (5%) to verify fee calculations are stored
    init_config_custom(deps.as_mut().storage, None, Decimal::percent(5));
    setup_insolvent_single_asset(&mut deps);

    let reply = insolvent_assess_reply_single_asset();
    let _res = handle_reply(deps.as_mut(), env, reply).unwrap();

    let updated_prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();

    // With the new fee formula:
    // caller_fee_rate = current_ltv - max_ltv (dynamic, e.g. 0.9 - 0.8 = 0.1)
    // protocol_fee_rate = liq_fee (static, e.g. 0.05)
    // Both should be > 0 for an insolvent position
    assert!(
        updated_prop.caller_fee_value_paid > Decimal::zero(),
        "caller_fee should be nonzero, got: {}",
        updated_prop.caller_fee_value_paid
    );
    assert!(
        updated_prop.protocol_fee_value > Decimal::zero(),
        "protocol_fee_value should be nonzero, got: {}",
        updated_prop.protocol_fee_value
    );
    // With 5% liq_fee and 10% LTV excess (0.9 - 0.8):
    // caller_fee_rate (0.1) > protocol_fee_rate (0.05)
    assert!(
        updated_prop.caller_fee_value_paid > updated_prop.protocol_fee_value,
        "Caller fee ({}) should exceed protocol fee ({}) when LTV excess > liq_fee",
        updated_prop.caller_fee_value_paid,
        updated_prop.protocol_fee_value
    );
}

#[test]
fn collateral_assess_missing_assessment_attr_errors() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);
    setup_insolvent_single_asset(&mut deps);

    // Success reply without "assessment" attribute
    let reply = make_success_reply(COLLATERAL_ASSESS_REPLY_ID, vec![("wrong_attr", "value")]);
    let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();
    let err_string = err.to_string();
    assert!(
        err_string.contains("Attribute 'assessment' not found"),
        "Expected missing attr error, got: {}",
        err_string
    );
}

#[test]
fn collateral_assess_error_reply_returns_error() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);
    setup_insolvent_single_asset(&mut deps);

    let reply = make_error_reply(COLLATERAL_ASSESS_REPLY_ID, "assessment exploded");
    let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();
    let err_string = err.to_string();
    assert!(
        err_string.contains("Collateral assessment failed"),
        "Expected 'Collateral assessment failed' error, got: {}",
        err_string
    );
}

#[test]
fn collateral_assess_multi_asset_stores_ratios_and_prices() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);

    // 2 assets, insolvent.
    // collateral: 1_000_000 uatom at $10 (value=$10) + 2_000_000 uosmo at $5 (value=$10) = $20 total
    // debt: 18_000_000 credits at $1 (value=$18)
    // LTV = 18/20 = 0.9 = 90% > 80% -> insolvent
    let prop = make_propagation_post_debt_accrue(18_000_000, Decimal::one(), 6);
    save_propagation(deps.as_mut().storage, &prop);

    let collateral_assets = vec![
        make_casset("uatom", 1_000_000, 60, 80),
        make_casset("uosmo", 2_000_000, 60, 80),
    ];
    // ratios: each asset's value fraction.
    // uatom value=$10, uosmo value=$10, total=$20 -> 50%/50%
    let ratios = vec![Decimal::percent(50), Decimal::percent(50)];
    let prices = vec![
        make_price(Decimal::from_ratio(10u128, 1u128), 6),
        make_price(Decimal::from_ratio(5u128, 1u128), 6),
    ];
    let avg_max_ltv = Decimal::percent(80);
    let avg_max_borrow_ltv = Decimal::percent(60);

    let reply = make_collateral_assess_success_reply(
        collateral_assets,
        ratios,
        prices,
        avg_max_ltv,
        avg_max_borrow_ltv,
        false,
        vec![],
    );

    let _res = handle_reply(deps.as_mut(), env, reply).unwrap();

    let updated_prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();
    // Should have 2 ratios, prices, and collateral_assets
    assert_eq!(updated_prop.cAsset_ratios.len(), 2);
    assert_eq!(updated_prop.cAsset_prices.len(), 2);
    assert_eq!(updated_prop.collateral_assets.len(), 2);

    // Fix #3: per_asset_repayment is built in build_next_after_fees, not in Step 2.
    // After Step 2, it is empty.
    assert!(
        updated_prop.per_asset_repayment.is_empty(),
        "per_asset_repayment should be empty after Step 2 (built later in build_next_after_fees)"
    );
}

// ========================================================================
// Group C: handle_liq_queue_transfer_reply (5 tests)
// ========================================================================

/// Helper: build a post-assess propagation for LQ transfer tests.
fn setup_post_assess_two_assets(deps: &mut cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>) {
    let collateral_assets = vec![
        make_casset("uatom", 1_000_000, 60, 80),
        make_casset("uosmo", 2_000_000, 60, 80),
    ];
    let ratios = vec![Decimal::percent(50), Decimal::percent(50)];
    let prices = vec![
        make_price(Decimal::from_ratio(10u128, 1u128), 6),
        make_price(Decimal::from_ratio(5u128, 1u128), 6),
    ];
    let per_asset_repayment = vec![
        Decimal::from_ratio(500_000u128, 1u128),
        Decimal::from_ratio(500_000u128, 1u128),
    ];

    let prop = make_propagation_post_assess(
        1_000_000,             // total_debt
        Decimal::one(),        // credit_price
        6,                     // credit_decimals
        collateral_assets,
        ratios,
        prices,
        per_asset_repayment,
        Decimal::from_ratio(100u128, 1u128), // caller_fee
        Decimal::from_ratio(100u128, 1u128), // protocol_fee
        Decimal::zero(),       // total_repaid
    );
    save_propagation(deps.as_mut().storage, &prop);
}

#[test]
fn liq_queue_transfer_success_builds_per_asset_submsgs_two_assets() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config_with_liq_queue(deps.as_mut().storage);
    setup_post_assess_two_assets(&mut deps);

    let reply = make_success_reply(LIQ_QUEUE_TRANSFER_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Should build 2 SubMsgs (one per collateral asset), each with LIQ_QUEUE_REPLY_ID
    assert_eq!(res.messages.len(), 2);
    assert_eq!(res.messages[0].id, LIQ_QUEUE_REPLY_ID);
    assert_eq!(res.messages[1].id, LIQ_QUEUE_REPLY_ID);

    // Both should target the liq_queue contract
    for msg in &res.messages {
        match &msg.msg {
            cosmwasm_std::CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute {
                contract_addr, ..
            }) => {
                assert_eq!(contract_addr, "liq_queue");
            }
            _ => panic!("Expected WasmMsg::Execute"),
        }
    }
}

#[test]
fn liq_queue_transfer_success_single_asset_builds_one_submsg() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config_with_liq_queue(deps.as_mut().storage);

    let collateral_assets = vec![make_casset("uatom", 1_000_000, 60, 80)];
    let ratios = vec![Decimal::one()];
    let prices = vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)];
    let per_asset_repayment = vec![Decimal::from_ratio(1_000_000u128, 1u128)];

    let prop = make_propagation_post_assess(
        1_000_000,
        Decimal::one(),
        6,
        collateral_assets,
        ratios,
        prices,
        per_asset_repayment,
        Decimal::from_ratio(100u128, 1u128),
        Decimal::from_ratio(100u128, 1u128),
        Decimal::zero(),
    );
    save_propagation(deps.as_mut().storage, &prop);

    let reply = make_success_reply(LIQ_QUEUE_TRANSFER_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, LIQ_QUEUE_REPLY_ID);
}

#[test]
fn liq_queue_transfer_zero_collateral_amounts_skips_to_sell_wall() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config_with_liq_queue(deps.as_mut().storage);

    // Collateral assets with 0 amounts -> no LQ SubMsgs -> skip to sell wall
    let collateral_assets = vec![make_casset("uatom", 0, 60, 80)];
    let ratios = vec![Decimal::one()];
    let prices = vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)];
    let per_asset_repayment = vec![Decimal::from_ratio(1_000_000u128, 1u128)];

    let prop = make_propagation_post_assess(
        1_000_000,
        Decimal::one(),
        6,
        collateral_assets,
        ratios,
        prices,
        per_asset_repayment,
        Decimal::from_ratio(100u128, 1u128),
        Decimal::from_ratio(100u128, 1u128),
        Decimal::zero(),
    );
    save_propagation(deps.as_mut().storage, &prop);

    let reply = make_success_reply(LIQ_QUEUE_TRANSFER_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Should skip to sell wall
    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, SELL_WALL_REPLY_ID);
}

#[test]
fn liq_queue_transfer_no_liq_queue_in_config_errors() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // No liq_queue configured
    init_config(deps.as_mut().storage);
    setup_post_assess_two_assets(&mut deps);

    let reply = make_success_reply(LIQ_QUEUE_TRANSFER_REPLY_ID, vec![]);
    let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();
    let err_string = err.to_string();
    assert!(
        err_string.contains("Liq queue not configured"),
        "Expected 'Liq queue not configured' error, got: {}",
        err_string
    );
}

#[test]
fn liq_queue_transfer_error_reply_falls_back_to_sell_wall() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config_with_liq_queue(deps.as_mut().storage);
    setup_post_assess_two_assets(&mut deps);

    let reply = make_error_reply(LIQ_QUEUE_TRANSFER_REPLY_ID, "transfer failed");
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Should fall back to sell wall (Ok response, not error)
    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, SELL_WALL_REPLY_ID);
    assert!(res.attributes.iter().any(|a| a.key == "reason" && a.value == "transfer failed"));
}

// ========================================================================
// Group D: handle_liq_queue_reply (8 tests)
// ========================================================================

/// Helper: build a post-assess propagation with N per_asset_repayment entries for LQ reply tests.
fn setup_for_liq_queue_reply(
    deps: &mut cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>,
    total_debt: u128,
    per_asset_entries: Vec<Decimal>,
    total_repaid: Decimal,
) {
    let n = per_asset_entries.len();
    let collateral_assets: Vec<_> = (0..n)
        .map(|i| make_casset(&format!("udenom{}", i), 1_000_000, 60, 80))
        .collect();
    let ratios: Vec<_> = (0..n)
        .map(|_| Decimal::from_ratio(1u128, n as u128))
        .collect();
    let prices: Vec<_> = (0..n)
        .map(|_| make_price(Decimal::from_ratio(10u128, 1u128), 6))
        .collect();

    let prop = make_propagation_post_assess(
        total_debt,
        Decimal::one(),
        6,
        collateral_assets,
        ratios,
        prices,
        per_asset_entries,
        Decimal::from_ratio(100u128, 1u128),
        Decimal::from_ratio(100u128, 1u128),
        total_repaid,
    );
    save_propagation(deps.as_mut().storage, &prop);
}

#[test]
fn liq_queue_partial_repayment_more_assets_pending_no_submsg() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config_with_liq_queue(deps.as_mut().storage);

    // 2 per_asset entries -> first reply removes one, leaving 1 pending
    setup_for_liq_queue_reply(
        &mut deps,
        1_000_000,
        vec![Decimal::from_ratio(500_000u128, 1u128), Decimal::from_ratio(500_000u128, 1u128)],
        Decimal::zero(),
    );

    let reply = make_liq_queue_success_reply(200_000, 50_000, "udenom0");
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // No SubMsg because more LQ replies are expected
    assert_eq!(res.messages.len(), 0);

    // Should have remaining_assets attribute
    assert!(res.attributes.iter().any(|a| a.key == "remaining_assets" && a.value == "1"));

    // Propagation should have accumulated repayment
    let prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();
    assert_eq!(prop.liq_queue_repayment, Decimal::from_ratio(200_000u128, 1u128));
    assert_eq!(prop.total_repaid, Decimal::from_ratio(200_000u128, 1u128));
    assert_eq!(prop.per_asset_repayment.len(), 1);
}

#[test]
fn liq_queue_last_asset_fully_repaid_sends_debt_update() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config_with_liq_queue(deps.as_mut().storage);

    // 1 per_asset entry, will be empty after this reply -> last asset
    // total_debt = 1_000_000, repay_amount=1_000_000 -> fully repaid
    setup_for_liq_queue_reply(
        &mut deps,
        1_000_000,
        vec![Decimal::from_ratio(1_000_000u128, 1u128)],
        Decimal::zero(),
    );

    let reply = make_liq_queue_success_reply(1_000_000, 100_000, "udenom0");
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Fully repaid -> skip sell wall, go to debt update
    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, DEBT_UPDATE_REPLY_ID);
    assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "liq_queue_reply_fully_repaid"));
}

#[test]
fn liq_queue_last_asset_remaining_debt_sends_sell_wall() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config_with_liq_queue(deps.as_mut().storage);

    // 1 per_asset entry -> last asset after this reply
    // total_debt=1_000_000, repay_amount=500_000 -> remaining debt
    setup_for_liq_queue_reply(
        &mut deps,
        1_000_000,
        vec![Decimal::from_ratio(1_000_000u128, 1u128)],
        Decimal::zero(),
    );

    let reply = make_liq_queue_success_reply(500_000, 50_000, "udenom0");
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Remaining debt -> sell wall
    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, SELL_WALL_REPLY_ID);
    assert!(res.attributes.iter().any(|a| a.key == "remaining_debt"));
}

#[test]
fn liq_queue_missing_repay_amount_defaults_to_zero() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config_with_liq_queue(deps.as_mut().storage);

    // 1 per_asset entry
    setup_for_liq_queue_reply(
        &mut deps,
        1_000_000,
        vec![Decimal::from_ratio(1_000_000u128, 1u128)],
        Decimal::zero(),
    );

    // Reply with no repay_amount attribute -> defaults to "0"
    let reply = make_success_reply(
        LIQ_QUEUE_REPLY_ID,
        vec![("collateral_amount", "50000"), ("collateral_token", "udenom0")],
    );

    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Should still succeed. With 0 repaid and total_debt=1_000_000 -> sell wall
    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, SELL_WALL_REPLY_ID);

    // Propagation: repayment should be zero
    let prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();
    assert_eq!(prop.total_repaid, Decimal::zero());
}

#[test]
fn liq_queue_accumulation_across_two_calls() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config_with_liq_queue(deps.as_mut().storage);

    // 2 per_asset entries
    setup_for_liq_queue_reply(
        &mut deps,
        1_000_000,
        vec![Decimal::from_ratio(500_000u128, 1u128), Decimal::from_ratio(500_000u128, 1u128)],
        Decimal::zero(),
    );

    // First LQ reply: 300_000 repaid
    let reply1 = make_liq_queue_success_reply(300_000, 30_000, "udenom0");
    let _res1 = handle_reply(deps.as_mut(), env.clone(), reply1).unwrap();

    let prop_after_1 = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();
    assert_eq!(prop_after_1.total_repaid, Decimal::from_ratio(300_000u128, 1u128));
    assert_eq!(prop_after_1.per_asset_repayment.len(), 1);

    // Second LQ reply: 400_000 more repaid
    let reply2 = make_liq_queue_success_reply(400_000, 40_000, "udenom1");
    let res2 = handle_reply(deps.as_mut(), env, reply2).unwrap();

    let prop_after_2 = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();
    // Accumulated: 300_000 + 400_000 = 700_000
    assert_eq!(prop_after_2.total_repaid, Decimal::from_ratio(700_000u128, 1u128));
    assert_eq!(prop_after_2.per_asset_repayment.len(), 0);

    // Last asset, 700_000 < 1_000_000 -> remaining debt -> sell wall
    assert_eq!(res2.messages.len(), 1);
    assert_eq!(res2.messages[0].id, SELL_WALL_REPLY_ID);
}

#[test]
fn liq_queue_error_reply_falls_back_to_sell_wall() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config_with_liq_queue(deps.as_mut().storage);
    setup_for_liq_queue_reply(
        &mut deps,
        1_000_000,
        vec![Decimal::from_ratio(1_000_000u128, 1u128)],
        Decimal::zero(),
    );

    let reply = make_error_reply(LIQ_QUEUE_REPLY_ID, "liq queue exploded");
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Error -> fallback to sell wall (Ok response)
    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, SELL_WALL_REPLY_ID);
    assert!(res.attributes.iter().any(|a| a.key == "error" && a.value == "liq queue exploded"));
}

#[test]
fn liq_queue_zero_repay_still_removes_per_asset_entry() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config_with_liq_queue(deps.as_mut().storage);

    // 2 entries
    setup_for_liq_queue_reply(
        &mut deps,
        1_000_000,
        vec![Decimal::from_ratio(500_000u128, 1u128), Decimal::from_ratio(500_000u128, 1u128)],
        Decimal::zero(),
    );

    // Zero repay
    let reply = make_liq_queue_success_reply(0, 0, "udenom0");
    let _res = handle_reply(deps.as_mut(), env, reply).unwrap();

    let prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();
    // Still removes the first entry
    assert_eq!(prop.per_asset_repayment.len(), 1);
    assert_eq!(prop.total_repaid, Decimal::zero());
}

#[test]
fn liq_queue_fully_repaid_detection() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config_with_liq_queue(deps.as_mut().storage);

    // 1 entry, total_debt=500_000
    setup_for_liq_queue_reply(
        &mut deps,
        500_000,
        vec![Decimal::from_ratio(500_000u128, 1u128)],
        Decimal::zero(),
    );

    // Repay exactly total_debt
    let reply = make_liq_queue_success_reply(500_000, 50_000, "udenom0");
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // total_repaid (500_000) >= total_debt (500_000) -> debt update, skip sell wall
    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, DEBT_UPDATE_REPLY_ID);
    assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "liq_queue_reply_fully_repaid"));
}

// ========================================================================
// Group E: handle_sell_wall_reply (4 tests)
// ========================================================================

/// Helper: set up state for sell wall reply tests.
fn setup_for_sell_wall_reply(
    deps: &mut cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>,
    liq_queue_repayment: Decimal,
) {
    let collateral_assets = vec![make_casset("uatom", 1_000_000, 60, 80)];
    let ratios = vec![Decimal::one()];
    let prices = vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)];
    let per_asset_repayment = vec![];

    let mut prop = make_propagation_post_assess(
        1_000_000,
        Decimal::one(),
        6,
        collateral_assets,
        ratios,
        prices,
        per_asset_repayment,
        Decimal::from_ratio(100u128, 1u128),
        Decimal::from_ratio(100u128, 1u128),
        liq_queue_repayment,
    );
    prop.liq_queue_repayment = liq_queue_repayment;
    save_propagation(deps.as_mut().storage, &prop);
}

#[test]
fn sell_wall_success_sends_debt_update() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);
    setup_for_sell_wall_reply(&mut deps, Decimal::from_ratio(200_000u128, 1u128));

    let reply = make_success_reply(SELL_WALL_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, DEBT_UPDATE_REPLY_ID);
    assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "sell_wall_reply_success"));
}

#[test]
fn sell_wall_error_still_proceeds_to_debt_update() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);
    setup_for_sell_wall_reply(&mut deps, Decimal::from_ratio(200_000u128, 1u128));

    let reply = make_error_reply(SELL_WALL_REPLY_ID, "sell wall failed");
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Still proceeds to debt update (Ok response, not error)
    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, DEBT_UPDATE_REPLY_ID);
    assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "sell_wall_reply_partial"));
}

#[test]
fn sell_wall_success_has_liq_queue_repaid_attribute() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);
    let lq_repaid = Decimal::from_ratio(300_000u128, 1u128);
    setup_for_sell_wall_reply(&mut deps, lq_repaid);

    let reply = make_success_reply(SELL_WALL_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    assert!(res.attributes.iter().any(|a| a.key == "liq_queue_repaid" && a.value == lq_repaid.to_string()));
}

#[test]
fn sell_wall_error_has_sell_wall_error_attribute() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);
    setup_for_sell_wall_reply(&mut deps, Decimal::zero());

    let reply = make_error_reply(SELL_WALL_REPLY_ID, "swap slippage exceeded");
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    assert!(res.attributes.iter().any(|a| a.key == "sell_wall_error" && a.value == "swap slippage exceeded"));
}

// ========================================================================
// Group F: handle_debt_update_reply (5 tests)
// ========================================================================

/// Helper: set up state for debt update reply tests.
fn setup_for_debt_update_reply(
    deps: &mut cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>,
    total_repaid: Decimal,
    total_debt: u128,
    collateral_assets: Vec<membrane::types::cAsset>,
    ratios: Vec<Decimal>,
    prices: Vec<membrane::oracle::PriceResponse>,
    caller_fee: Decimal,
    protocol_fee: Decimal,
    sell_wall_amounts: Vec<membrane::types::Asset>,
) {
    let mut prop = make_propagation_post_assess(
        total_debt,
        Decimal::one(),
        6,
        collateral_assets,
        ratios,
        prices,
        vec![], // per_asset_repayment consumed by this point
        caller_fee,
        protocol_fee,
        total_repaid,
    );
    prop.sell_wall_amounts = sell_wall_amounts;
    save_propagation(deps.as_mut().storage, &prop);
}

#[test]
fn debt_update_success_sends_claim_update() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);

    let collateral_assets = vec![make_casset("uatom", 1_000_000, 60, 80)];
    let ratios = vec![Decimal::one()];
    let prices = vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)];

    setup_for_debt_update_reply(
        &mut deps,
        Decimal::from_ratio(500_000u128, 1u128), // total_repaid
        1_000_000,                                 // total_debt
        collateral_assets,
        ratios,
        prices,
        Decimal::from_ratio(50u128, 1u128),  // caller_fee
        Decimal::from_ratio(50u128, 1u128),  // protocol_fee
        vec![],  // no sell wall amounts needed for this test
    );

    let reply = make_success_reply(DEBT_UPDATE_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, CLAIM_UPDATE_REPLY_ID);
    assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "debt_update_reply_success"));

    // Target should be collateral_contract
    match &res.messages[0].msg {
        cosmwasm_std::CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute {
            contract_addr, ..
        }) => {
            assert_eq!(contract_addr, "collateral_contract");
        }
        _ => panic!("Expected WasmMsg::Execute"),
    }
}

#[test]
fn debt_update_multi_asset_sends_empty_reductions() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);

    // Two assets with different ratios
    let collateral_assets = vec![
        make_casset("uatom", 10_000_000, 60, 80),
        make_casset("uosmo", 20_000_000, 60, 80),
    ];
    let ratios = vec![Decimal::percent(40), Decimal::percent(60)];
    let prices = vec![
        make_price(Decimal::from_ratio(10u128, 1u128), 6),
        make_price(Decimal::from_ratio(5u128, 1u128), 6),
    ];

    // sell_wall_amounts are stored by build_sell_wall_submsg but NOT used for
    // LiquidationClaimUpdate anymore (Fix #1: collateral contract's internal
    // reply handler already reduces claims for sold collateral).
    let sell_wall_amounts = vec![
        make_asset("uatom", 4_000_000),
        make_asset("uosmo", 12_000_000),
    ];

    setup_for_debt_update_reply(
        &mut deps,
        Decimal::from_ratio(1_000_000u128, 1u128), // total_repaid
        1_000_000,
        collateral_assets,
        ratios,
        prices,
        Decimal::from_ratio(100u128, 1u128), // caller_fee
        Decimal::from_ratio(100u128, 1u128), // protocol_fee
        sell_wall_amounts,
    );

    let reply = make_success_reply(DEBT_UPDATE_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Should produce a SubMsg to LiquidationClaimUpdate
    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, CLAIM_UPDATE_REPLY_ID);

    // Fix #1: LiquidationClaimUpdate now receives EMPTY collateral_reductions
    // because all claim reductions are already handled by the collateral contract:
    //   - Fees: AuthorizedTransfer (Step 9)
    //   - LQ: AuthorizedTransfer (Step 3)
    //   - Sell wall: collateral contract's handle_sell_collateral_reply
    match &res.messages[0].msg {
        cosmwasm_std::CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute {
            msg, ..
        }) => {
            let exec_msg: membrane::collateral::ExecuteMsg =
                cosmwasm_std::from_json(msg).unwrap();
            match exec_msg {
                membrane::collateral::ExecuteMsg::LiquidationClaimUpdate {
                    collateral_reductions, ..
                } => {
                    assert!(
                        collateral_reductions.is_empty(),
                        "collateral_reductions should be empty (claims handled by collateral contract)"
                    );
                }
                _ => panic!("Expected LiquidationClaimUpdate"),
            }
        }
        _ => panic!("Expected WasmMsg::Execute"),
    }
}

#[test]
fn debt_update_reductions_always_empty() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);

    // Tiny holdings, large repay
    let collateral_assets = vec![make_casset("uatom", 100, 60, 80)]; // only 100 uatom
    let ratios = vec![Decimal::one()];
    let prices = vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)]; // $10/uatom

    let sell_wall_amounts = vec![
        make_asset("uatom", 100),
    ];

    setup_for_debt_update_reply(
        &mut deps,
        Decimal::from_ratio(10_000_000u128, 1u128), // huge total_repaid
        10_000_000,
        collateral_assets,
        ratios,
        prices,
        Decimal::from_ratio(1_000u128, 1u128),
        Decimal::from_ratio(1_000u128, 1u128),
        sell_wall_amounts,
    );

    let reply = make_success_reply(DEBT_UPDATE_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Fix #1: collateral_reductions are always empty — the collateral contract's
    // internal reply handler already reduces claims for AuthorizedTransfer and
    // AuthorizedSellCollateral operations.
    match &res.messages[0].msg {
        cosmwasm_std::CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute {
            msg, ..
        }) => {
            let exec_msg: membrane::collateral::ExecuteMsg =
                cosmwasm_std::from_json(msg).unwrap();
            match exec_msg {
                membrane::collateral::ExecuteMsg::LiquidationClaimUpdate {
                    collateral_reductions, ..
                } => {
                    assert!(
                        collateral_reductions.is_empty(),
                        "collateral_reductions should be empty (claims handled by collateral contract)"
                    );
                }
                _ => panic!("Expected LiquidationClaimUpdate"),
            }
        }
        _ => panic!("Expected WasmMsg::Execute"),
    }
}

#[test]
fn debt_update_sell_wall_amounts_not_used_for_reductions() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);

    let collateral_assets = vec![make_casset("uatom", 10_000_000, 60, 80)];
    let ratios = vec![Decimal::one()];
    let prices = vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)]; // $10/uatom

    // sell_wall_amounts are stored by build_sell_wall_submsg but NOT passed to
    // LiquidationClaimUpdate (Fix #1: double claim reduction prevention).
    let sell_wall_amounts = vec![
        make_asset("uatom", 100_000),
    ];

    setup_for_debt_update_reply(
        &mut deps,
        Decimal::from_ratio(1_000_000u128, 1u128),  // total_repaid from sell wall
        1_000_000,        // total_debt
        collateral_assets,
        ratios,
        prices,
        Decimal::zero(), // zero fees
        Decimal::zero(),
        sell_wall_amounts,
    );

    let reply = make_success_reply(DEBT_UPDATE_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Should still produce a claim update SubMsg
    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, CLAIM_UPDATE_REPLY_ID);

    // Fix #1: sell_wall_amounts are NOT used for LiquidationClaimUpdate reductions.
    // The collateral contract's handle_sell_collateral_reply already reduces claims.
    match &res.messages[0].msg {
        cosmwasm_std::CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute {
            msg, ..
        }) => {
            let exec_msg: membrane::collateral::ExecuteMsg =
                cosmwasm_std::from_json(msg).unwrap();
            match exec_msg {
                membrane::collateral::ExecuteMsg::LiquidationClaimUpdate {
                    collateral_reductions, ..
                } => {
                    assert!(
                        collateral_reductions.is_empty(),
                        "collateral_reductions should be empty even with sell_wall_amounts stored"
                    );
                }
                _ => panic!("Expected LiquidationClaimUpdate"),
            }
        }
        _ => panic!("Expected WasmMsg::Execute"),
    }
}

#[test]
fn debt_update_zero_sell_wall_produces_empty_reductions() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);

    // total_repaid = 0, liq_queue_repayment = 0 → sell wall repaid = 0
    // Only sell wall amounts are reduced via LiquidationClaimUpdate.
    // Fees and LQ are already deducted via AuthorizedTransfer.
    let collateral_assets = vec![make_casset("uatom", 10_000_000, 60, 80)];
    let ratios = vec![Decimal::one()];
    let prices = vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)];

    setup_for_debt_update_reply(
        &mut deps,
        Decimal::zero(),  // zero total_repaid
        1_000_000,        // total_debt
        collateral_assets,
        ratios,
        prices,
        Decimal::zero(),
        Decimal::zero(),
        vec![],  // empty sell_wall_amounts
    );

    let reply = make_success_reply(DEBT_UPDATE_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Should produce claim update with empty reductions (sell wall repaid = 0)
    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, CLAIM_UPDATE_REPLY_ID);

    match &res.messages[0].msg {
        cosmwasm_std::CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute {
            msg, ..
        }) => {
            let exec_msg: membrane::collateral::ExecuteMsg =
                cosmwasm_std::from_json(msg).unwrap();
            match exec_msg {
                membrane::collateral::ExecuteMsg::LiquidationClaimUpdate {
                    collateral_reductions, ..
                } => {
                    // Zero sell wall → no reductions needed
                    assert!(collateral_reductions.is_empty(),
                        "Zero sell wall repaid should produce empty reductions");
                }
                _ => panic!("Expected LiquidationClaimUpdate"),
            }
        }
        _ => panic!("Expected WasmMsg::Execute"),
    }
}

#[test]
fn debt_update_error_reply_returns_error() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);

    let collateral_assets = vec![make_casset("uatom", 1_000_000, 60, 80)];
    let ratios = vec![Decimal::one()];
    let prices = vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)];

    setup_for_debt_update_reply(
        &mut deps,
        Decimal::from_ratio(500_000u128, 1u128),
        1_000_000,
        collateral_assets,
        ratios,
        prices,
        Decimal::zero(),
        Decimal::zero(),
        vec![],
    );

    let reply = make_error_reply(DEBT_UPDATE_REPLY_ID, "debt contract crashed");
    let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();
    let err_string = err.to_string();
    assert!(
        err_string.contains("Debt update failed"),
        "Expected 'Debt update failed' error, got: {}",
        err_string
    );
}

// ========================================================================
// Group G: handle_claim_update_reply (5 tests)
// ========================================================================

/// Helper: set up state for claim update reply tests.
fn setup_for_claim_update_reply(
    deps: &mut cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>,
    total_debt: u128,
    total_repaid: Decimal,
    caller_fee: Decimal,
) {
    let collateral_assets = vec![make_casset("uatom", 1_000_000, 60, 80)];
    let ratios = vec![Decimal::one()];
    let prices = vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)];

    let prop = make_propagation_post_assess(
        total_debt,
        Decimal::one(),
        6,
        collateral_assets,
        ratios,
        prices,
        vec![],
        caller_fee,
        Decimal::zero(),
        total_repaid,
    );
    save_propagation(deps.as_mut().storage, &prop);
}

#[test]
fn claim_update_bad_debt_detected_sends_record_bad_debt() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);

    // total_repaid = 500_000, total_debt = 1_000_000 -> bad debt = 500_000
    setup_for_claim_update_reply(
        &mut deps,
        1_000_000,
        Decimal::from_ratio(500_000u128, 1u128),
        Decimal::zero(),
    );

    let reply = make_success_reply(CLAIM_UPDATE_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Should have a RecordBadDebt message
    let has_bad_debt_msg = res.messages.iter().any(|sub| {
        if let cosmwasm_std::CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute { contract_addr, msg, .. }) = &sub.msg {
            if contract_addr == "debt_contract" {
                if let Ok(exec_msg) = cosmwasm_std::from_json::<membrane::debt::ExecuteMsg>(msg) {
                    matches!(exec_msg, membrane::debt::ExecuteMsg::RecordBadDebt { .. })
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        }
    });
    assert!(has_bad_debt_msg, "Expected RecordBadDebt message for bad debt scenario");
}

#[test]
fn claim_update_no_bad_debt_no_record_bad_debt_msg() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);

    // total_repaid = 1_000_000 >= total_debt = 1_000_000 -> no bad debt
    setup_for_claim_update_reply(
        &mut deps,
        1_000_000,
        Decimal::from_ratio(1_000_000u128, 1u128),
        Decimal::zero(),
    );

    let reply = make_success_reply(CLAIM_UPDATE_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Should NOT have a RecordBadDebt message in the regular messages
    let has_bad_debt_msg = res.messages.iter().any(|sub| {
        if let cosmwasm_std::CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute { contract_addr, msg, .. }) = &sub.msg {
            if contract_addr == "debt_contract" {
                if let Ok(exec_msg) = cosmwasm_std::from_json::<membrane::debt::ExecuteMsg>(msg) {
                    matches!(exec_msg, membrane::debt::ExecuteMsg::RecordBadDebt { .. })
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        }
    });
    assert!(!has_bad_debt_msg, "Should not have RecordBadDebt when fully repaid");
}

#[test]
fn claim_update_propagation_cleaned_up() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);
    setup_for_claim_update_reply(
        &mut deps,
        1_000_000,
        Decimal::from_ratio(1_000_000u128, 1u128),
        Decimal::zero(),
    );

    // Confirm propagation exists before
    assert!(LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).is_ok());

    let reply = make_success_reply(CLAIM_UPDATE_REPLY_ID, vec![]);
    let _res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Propagation should be removed
    assert!(
        LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).is_err(),
        "LIQUIDATION_PROPAGATION should be removed after claim update"
    );
}

#[test]
fn claim_update_bad_debt_check_callback_submsg_present() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);
    setup_for_claim_update_reply(
        &mut deps,
        1_000_000,
        Decimal::from_ratio(1_000_000u128, 1u128),
        Decimal::zero(),
    );

    let reply = make_success_reply(CLAIM_UPDATE_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Should have a BadDebtCheck callback SubMsg (reply_on_error with BAD_DEBT_REPLY_ID)
    let has_callback = res.messages.iter().any(|sub| {
        // The callback targets the contract itself (env.contract.address)
        // and uses BAD_DEBT_REPLY_ID
        sub.id == BAD_DEBT_REPLY_ID
    });
    assert!(has_callback, "Expected BadDebtCheck callback SubMsg");

    // Verify it targets the contract itself with a BadDebtCheck callback
    let callback_msg = res.messages.iter().find(|sub| sub.id == BAD_DEBT_REPLY_ID).unwrap();
    match &callback_msg.msg {
        cosmwasm_std::CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute {
            msg, ..
        }) => {
            let exec_msg: membrane::liquidation_engine::ExecuteMsg =
                cosmwasm_std::from_json(msg).unwrap();
            match exec_msg {
                membrane::liquidation_engine::ExecuteMsg::Callback(
                    membrane::liquidation_engine::CallbackMsg::BadDebtCheck { position_id, .. }
                ) => {
                    assert_eq!(position_id, Uint128::one());
                }
                _ => panic!("Expected BadDebtCheck callback"),
            }
        }
        _ => panic!("Expected WasmMsg::Execute"),
    }
}

#[test]
fn claim_update_error_reply_returns_error() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);
    setup_for_claim_update_reply(
        &mut deps,
        1_000_000,
        Decimal::from_ratio(1_000_000u128, 1u128),
        Decimal::zero(),
    );

    let reply = make_error_reply(CLAIM_UPDATE_REPLY_ID, "claim update exploded");
    let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();
    let err_string = err.to_string();
    assert!(
        err_string.contains("Claim update failed"),
        "Expected 'Claim update failed' error, got: {}",
        err_string
    );
}

// ========================================================================
// Group H: FIXED GAPS — Tests verifying previously-identified bugs are fixed
// ========================================================================

/// FIXED: Caller fees are now sent in Step 9 (handle_fee_transfer_reply),
/// not in Step 7 (handle_claim_update_reply).
///
/// handle_claim_update_reply records the caller_fee_value in attributes
/// but does NOT send BankMsg::Send (that happens earlier in the chain).
#[test]
fn claim_update_records_caller_fee_but_no_bank_send() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);

    // Set up with a non-zero caller fee
    let caller_fee = Decimal::from_ratio(50_000u128, 1u128); // $50,000 in fee value
    setup_for_claim_update_reply(
        &mut deps,
        1_000_000,
        Decimal::from_ratio(1_000_000u128, 1u128),
        caller_fee,
    );

    let reply = make_success_reply(CLAIM_UPDATE_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Check: caller_fee_value is recorded in attributes
    let fee_attr = res.attributes.iter().find(|a| a.key == "caller_fee_value");
    assert!(fee_attr.is_some(), "caller_fee_value attribute should exist");
    assert_ne!(
        fee_attr.unwrap().value, "0",
        "Caller fee value should be non-zero"
    );

    // Caller fees are now sent in Step 9 (fee_transfer_reply), NOT in Step 7.
    // Verify NO BankMsg::Send in claim update reply.
    let bank_send = res.messages.iter().find(|sub| {
        matches!(&sub.msg, cosmwasm_std::CosmosMsg::Bank(cosmwasm_std::BankMsg::Send { .. }))
    });
    assert!(
        bank_send.is_none(),
        "BankMsg::Send should NOT exist in claim_update_reply (moved to fee_transfer_reply)"
    );
}

/// FIXED: handle_fee_transfer_reply sends caller fees via BankMsg::Send.
///
/// Step 9 receives the fee collateral via AuthorizedTransfer, then sends
/// caller_fee_assets to the liquidator via BankMsg::Send.
#[test]
fn fee_transfer_reply_sends_caller_fee_via_bank_msg() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);

    // Build a propagation with non-empty caller_fee_assets
    let mut prop = make_propagation_post_assess(
        1_000_000,
        Decimal::one(),
        6,
        vec![make_casset("uatom", 1_000_000, 60, 80)],
        vec![Decimal::one()],
        vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)],
        vec![],
        Decimal::from_ratio(100u128, 1u128), // caller_fee
        Decimal::from_ratio(100u128, 1u128), // protocol_fee
        Decimal::from_ratio(500_000u128, 1u128), // total_repaid
    );
    prop.caller_fee_assets = vec![cosmwasm_std::Coin {
        denom: "uatom".to_string(),
        amount: Uint128::new(5_000),
    }];
    save_propagation(deps.as_mut().storage, &prop);

    let reply = make_success_reply(FEE_TRANSFER_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // BankMsg::Send should be present for the caller fee
    let bank_send = res.messages.iter().find(|sub| {
        matches!(&sub.msg, cosmwasm_std::CosmosMsg::Bank(cosmwasm_std::BankMsg::Send { .. }))
    });
    assert!(
        bank_send.is_some(),
        "BankMsg::Send should exist in fee_transfer_reply for caller fee distribution"
    );

    // Verify it's sent to the liquidator address
    if let cosmwasm_std::CosmosMsg::Bank(cosmwasm_std::BankMsg::Send { to_address, amount }) = &bank_send.unwrap().msg {
        assert_eq!(to_address, "liquidator", "Caller fee should be sent to the liquidator");
        assert_eq!(amount.len(), 1);
        assert_eq!(amount[0].denom, "uatom");
        assert_eq!(amount[0].amount, Uint128::new(5_000));
    } else {
        panic!("Expected BankMsg::Send");
    }

    // Should also chain to next step (sell wall since no LQ configured)
    let wasm_msgs: Vec<_> = res.messages.iter().filter(|sub| {
        matches!(&sub.msg, cosmwasm_std::CosmosMsg::Wasm(_))
    }).collect();
    assert!(!wasm_msgs.is_empty(), "Should chain to LQ transfer or sell wall");
}

/// FIXED: BadDebtCheck callback SubMsg now uses BAD_DEBT_REPLY_ID (8).
///
/// Previously: Used DEBT_ACCRUE_REPLY_ID (1), causing reply routing collision.
/// Now: Uses dedicated BAD_DEBT_REPLY_ID (8) with a no-op handler.
#[test]
fn claim_update_bad_debt_callback_uses_correct_reply_id() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);
    setup_for_claim_update_reply(
        &mut deps,
        1_000_000,
        Decimal::from_ratio(1_000_000u128, 1u128),
        Decimal::zero(),
    );

    let reply = make_success_reply(CLAIM_UPDATE_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Find the BadDebtCheck callback SubMsg
    let bad_debt_sub = res.messages.iter().find(|sub| {
        if let cosmwasm_std::CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute { msg, .. }) = &sub.msg {
            if let Ok(exec_msg) = cosmwasm_std::from_json::<membrane::liquidation_engine::ExecuteMsg>(msg) {
                matches!(exec_msg, membrane::liquidation_engine::ExecuteMsg::Callback(
                    membrane::liquidation_engine::CallbackMsg::BadDebtCheck { .. }
                ))
            } else {
                false
            }
        } else {
            false
        }
    });

    assert!(bad_debt_sub.is_some(), "BadDebtCheck callback SubMsg should exist");
    let bad_debt_sub = bad_debt_sub.unwrap();

    // FIXED: Uses BAD_DEBT_REPLY_ID (8), not DEBT_ACCRUE_REPLY_ID (1)
    assert_ne!(
        bad_debt_sub.id, DEBT_ACCRUE_REPLY_ID,
        "BadDebtCheck should NOT use DEBT_ACCRUE_REPLY_ID"
    );
    assert_eq!(
        bad_debt_sub.id, BAD_DEBT_REPLY_ID,
        "BadDebtCheck should use BAD_DEBT_REPLY_ID ({})",
        BAD_DEBT_REPLY_ID
    );
}

/// FIXED: Liquidation stats are now recorded after claim update.
///
/// Previously: No LIQUIDATION_STATS storage existed in the liquidation engine.
/// Now: A LiquidationEvent is stored in LIQUIDATION_STATS after each liquidation.
#[test]
fn claim_update_liquidation_stats_recorded() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);
    setup_for_claim_update_reply(
        &mut deps,
        1_000_000,
        Decimal::from_ratio(800_000u128, 1u128), // partial repayment → bad debt
        Decimal::zero(),
    );

    let reply = make_success_reply(CLAIM_UPDATE_REPLY_ID, vec![]);
    let _res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // FIXED: LIQUIDATION_STATS should now contain one event
    let stats = LIQUIDATION_STATS.load(deps.as_ref().storage).unwrap();
    assert_eq!(stats.len(), 1, "Should have exactly one liquidation event");

    let event = &stats[0];
    assert_eq!(event.position_id, Uint128::one());
    assert_eq!(event.position_owner, Addr::unchecked("position_owner"));
    assert_eq!(event.caller, Addr::unchecked("liquidator"));
    assert_eq!(event.total_debt, Uint128::new(1_000_000));
    assert_eq!(event.total_repaid, Decimal::from_ratio(800_000u128, 1u128));
    assert_eq!(event.bad_debt, Uint128::new(200_000)); // 1M - 800K
    assert_eq!(event.timestamp, 1000);
}

// ========================================================================
// Group I: Unknown reply ID (1 test)
// ========================================================================

#[test]
fn unknown_reply_id_returns_error() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);

    let reply = make_success_reply(999, vec![]);
    let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();
    let err_string = err.to_string();
    assert!(
        err_string.contains("Unknown reply ID"),
        "Expected 'Unknown reply ID' error, got: {}",
        err_string
    );
}

// ========================================================================
// Group J: Coverage gap tests — fee cap, error paths, edge cases
// ========================================================================

/// Fee cap / bad debt safety net: when position is deeply underwater,
/// total estimated fees exceed collateral value. The fee cap should:
/// - Cap caller_fee_rate to min(1%, collateral_value/repay_value)
/// - Zero out protocol_fee_rate
///
/// CDP parity: lines 237-255 in cdp/liquidations.rs
#[test]
fn collateral_assess_fee_cap_deeply_underwater() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // 1% liq_fee, no LQ → sell wall after fees
    init_config(deps.as_mut().storage);

    // Step 1 complete: 90M CDT at $1/CDT, decimals=6
    // debt_value = 90_000_000 * 1 / 10^6 = $90
    let prop = make_propagation_post_debt_accrue(90_000_000, Decimal::one(), 6);
    save_propagation(deps.as_mut().storage, &prop);

    // Assessment: 1M uatom at $10, decimals=6
    // collateral_value = 1_000_000 * 10 / 10^6 = $10
    // LTV = $90 / $10 = 900% >> max_ltv=50%
    //
    // From get_repay_quantities:
    //   current_ltv = 9.0, repay_value = collateral_value = $10 (LTV >= 100%)
    //   caller_fee_rate = 9.0 - 0.5 = 8.5 (850%)
    //   protocol_fee_rate = 0.01 (1%)
    //   Estimated total fee = $10 * (8.5 + 0.01) = $85.1 > $10 → CAP TRIGGERS
    //   After cap: caller_fee_rate = min(1%, $10/$10) = 1%, protocol = 0%
    let reply = make_collateral_assess_success_reply(
        vec![make_casset("uatom", 1_000_000, 40, 50)],
        vec![Decimal::one()],
        vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)],
        Decimal::percent(50),  // avg_max_ltv
        Decimal::percent(40),  // avg_max_borrow_ltv
        false,
        vec![],
    );

    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    let prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();

    // Protocol fee should be zeroed (bad debt safety net)
    assert!(
        prop.protocol_fee_value.is_zero(),
        "Protocol fee should be zeroed when fee cap triggers, got: {}",
        prop.protocol_fee_value
    );
    assert!(
        prop.protocol_fee_assets.is_empty(),
        "Protocol fee assets should be empty when fee cap triggers"
    );

    // Caller fee capped to 1%:
    // collateral_repay_amount = get_amount($10 * 1.0) = 10 * 10^6 / 10 = 1,000,000 uatom
    // caller_fee = 1,000,000 * 1% = 10,000 uatom
    assert_eq!(prop.caller_fee_assets.len(), 1, "Should have one fee coin");
    assert_eq!(prop.caller_fee_assets[0].denom, "uatom");
    assert_eq!(prop.caller_fee_assets[0].amount, Uint128::new(10_000));

    // Fee-deducted collateral should be 1M - 10K = 990,000
    assert_eq!(prop.collateral_assets[0].asset.amount, Uint128::new(990_000));

    // Chains to FEE_TRANSFER_REPLY_ID (fees exist)
    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, FEE_TRANSFER_REPLY_ID);
}

/// Fee transfer error: graceful degradation.
/// When AuthorizedTransfer for fees fails, liquidation continues
/// without distributing fees — chains to sell wall (no LQ configured).
#[test]
fn fee_transfer_error_gracefully_continues_liquidation() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // No LQ → build_next_after_fees → sell wall
    init_config(deps.as_mut().storage);

    let mut prop = make_propagation_post_assess(
        5_000_000,
        Decimal::one(), 6,
        vec![make_casset("uatom", 900_000, 60, 80)],
        vec![Decimal::one()],
        vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)],
        vec![],
        Decimal::from_ratio(1u128, 10u128),
        Decimal::from_ratio(1u128, 100u128),
        Decimal::zero(),
    );
    prop.caller_fee_assets = vec![cosmwasm_std::Coin {
        denom: "uatom".to_string(),
        amount: Uint128::new(5_000),
    }];
    save_propagation(deps.as_mut().storage, &prop);

    // Error reply for fee transfer step
    let reply = make_error_reply(FEE_TRANSFER_REPLY_ID, "insufficient funds");
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // No BankMsg::Send (fee transfer failed, so fees stay with position)
    assert!(
        !res.messages
            .iter()
            .any(|m| matches!(m.msg, cosmwasm_std::CosmosMsg::Bank(..))),
        "Should not send fees when transfer failed"
    );

    // Should still chain to sell wall (graceful degradation)
    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, SELL_WALL_REPLY_ID);

    // Error recorded in attributes
    assert!(
        res.attributes
            .iter()
            .any(|a| a.key == "method" && a.value == "fee_transfer_reply_failed"),
        "Should have fee_transfer_reply_failed attribute"
    );
    assert!(
        res.attributes
            .iter()
            .any(|a| a.key == "error" && a.value == "insufficient funds"),
        "Should record the error message"
    );
}

/// Zero fees: when a position is barely insolvent and fee amounts round to 0,
/// the engine should skip the fee transfer step entirely and chain directly
/// to build_next_after_fees (sell wall since no LQ configured).
#[test]
fn collateral_assess_zero_fees_skips_fee_transfer() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // 1% liq_fee, no LQ
    init_config(deps.as_mut().storage);

    // Step 1 complete: 9 CDT at $1, decimals=0
    // debt_value = 9 * 1 / 10^0 = $9
    let prop = make_propagation_post_debt_accrue(9, Decimal::one(), 0);
    save_propagation(deps.as_mut().storage, &prop);

    // Assessment: 10 uatom at $1, decimals=0
    // collateral_value = 10 * 1 / 10^0 = $10
    // LTV = $9 / $10 = 90%, max_ltv=80% → insolvent
    //
    // From get_repay_quantities:
    //   repay_value = 9 * (0.9 - 0.6) / 0.9 = 3.0
    //   caller_fee_rate = 0.9 - 0.8 = 0.1 (10%)
    //   protocol_fee_rate = 0.01 (1%)
    //
    // Per-asset fee computation:
    //   collateral_repay_amount = get_amount(3.0 * 1.0) = 3 * 10^0 / 1 = 3 uatom
    //   caller_fee = 3 * 0.10 = 0.3 → to_uint_floor() = 0 (rounds to zero)
    //   protocol_fee = 3 * 0.01 = 0.03 → to_uint_floor() = 0 (rounds to zero)
    //   total_fee_assets is empty → skips fee transfer
    let reply = make_collateral_assess_success_reply(
        vec![make_casset("uatom", 10, 60, 80)],
        vec![Decimal::one()],
        vec![make_price(Decimal::one(), 0)],
        Decimal::percent(80),
        Decimal::percent(60),
        false,
        vec![],
    );

    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    let prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();

    // Both fee asset lists should be empty (rounded to 0)
    assert!(
        prop.caller_fee_assets.is_empty(),
        "Caller fee should be empty when amounts round to 0"
    );
    assert!(
        prop.protocol_fee_assets.is_empty(),
        "Protocol fee should be empty when amounts round to 0"
    );

    // Should skip FEE_TRANSFER_REPLY_ID and chain directly to sell wall
    assert_eq!(res.messages.len(), 1);
    assert_ne!(
        res.messages[0].id, FEE_TRANSFER_REPLY_ID,
        "Should NOT chain to fee transfer when fees are zero"
    );
    assert_eq!(
        res.messages[0].id, SELL_WALL_REPLY_ID,
        "Should chain directly to sell wall (no LQ, no fees)"
    );
}

/// Fee transfer success with LQ configured + no credit_denom:
/// build_next_after_fees uses proportional fallback (not CollateralForDebt query)
/// to calculate LQ transfer amounts. Verifies BankMsg::Send for caller fees
/// and LQ transfer SubMsg.
#[test]
fn fee_transfer_success_chains_to_lq_proportional_fallback() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // LQ configured, credit_denom = None → proportional fallback
    init_config_with_liq_queue(deps.as_mut().storage);

    // Post-assess: 5M CDT debt, 900K uatom post-fee at $10
    // debt_value = 5M * $1 / 10^6 = $5
    // total_repay_value = $5 → asset_repay_value = $5 * 1.0 = $5
    // asset_repay_amount = $5 * 10^6 / $10 = 500,000 uatom
    // min(500,000, 900,000) = 500,000 → transfer 500K uatom to LQ
    let mut prop = make_propagation_post_assess(
        5_000_000,
        Decimal::one(), 6,
        vec![make_casset("uatom", 900_000, 60, 80)],
        vec![Decimal::one()],
        vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)],
        vec![],
        Decimal::from_ratio(1u128, 10u128),
        Decimal::from_ratio(1u128, 100u128),
        Decimal::zero(),
    );
    prop.caller_fee_assets = vec![cosmwasm_std::Coin {
        denom: "uatom".to_string(),
        amount: Uint128::new(5_000),
    }];
    save_propagation(deps.as_mut().storage, &prop);

    let reply = make_success_reply(FEE_TRANSFER_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Should have 2 messages:
    // [0] BankMsg::Send caller fees (from add_messages)
    // [1] AuthorizedTransfer to LQ (from add_submessage)
    assert_eq!(res.messages.len(), 2, "Expected BankMsg::Send + LQ transfer");

    // First: BankMsg::Send for caller fees
    match &res.messages[0].msg {
        cosmwasm_std::CosmosMsg::Bank(cosmwasm_std::BankMsg::Send { to_address, amount }) => {
            assert_eq!(to_address, "liquidator");
            assert_eq!(amount.len(), 1);
            assert_eq!(amount[0].denom, "uatom");
            assert_eq!(amount[0].amount, Uint128::new(5_000));
        }
        _ => panic!("Expected BankMsg::Send for caller fees"),
    }

    // Second: LQ transfer SubMsg (AuthorizedTransfer to liq_queue)
    assert_eq!(
        res.messages[1].id, LIQ_QUEUE_TRANSFER_REPLY_ID,
        "Should chain to LQ transfer (not sell wall)"
    );

    // Verify per_asset_repayment was populated by build_next_after_fees
    let updated_prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();
    assert_eq!(
        updated_prop.per_asset_repayment.len(),
        1,
        "Should have 1 per_asset_repayment entry for 1 collateral asset"
    );
}

// ========================================================================
// Group I: build_sell_wall_submsg venue_recall_amount tests
// ========================================================================

/// Test that build_sell_wall_submsg subtracts venue_recall_amount from leftover_credit.
///
/// Setup:
///   - total_debt = 1_000_000 (credit units)
///   - venue_recall_amount = 200_000
///   - liq_queue_repayment = 0
///   - credit_price = $1, decimals = 6
///   - collateral: 10_000_000 uatom at $10/atom, decimals = 6
///   - single asset, ratio = 1.0
///
/// Expected:
///   leftover_credit = total_debt - liq_queue_repaid - venue_recall_amount
///                   = 1_000_000 - 0 - 200_000 = 800_000
///   leftover_value = get_value(800_000) = (800_000 / 10^6) * $1 = $0.8
///   sell_value = leftover_value * ratio(1.0) = $0.8
///   sell_amount = get_amount($0.8) at $10/atom = 0.8/10 * 10^6 = 80_000 uatom
///   sell_amount capped at 10_000_000 (holdings) → 80_000
///
///   Without venue_recall_amount (baseline):
///   leftover_credit = 1_000_000 - 0 = 1_000_000
///   leftover_value = 1_000_000 / 10^6 * $1 = $1.0
///   sell_amount = 1.0/10 * 10^6 = 100_000 uatom
///
///   So venue recall reduces sell amount from 100_000 to 80_000.
#[test]
fn sell_wall_subtracts_venue_recall_amount() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // Config without liq_queue so build_next_after_fees falls through to sell wall
    init_config(deps.as_mut().storage);

    // Build propagation with venue_recall_amount = 200_000
    let collateral_assets = vec![make_casset("uatom", 10_000_000, 60, 80)];
    let ratios = vec![Decimal::one()];
    let prices = vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)];

    let mut prop = make_propagation_post_assess(
        1_000_000,        // total_debt (credit units)
        Decimal::one(),   // credit_price = $1
        6,                // credit_decimals
        collateral_assets,
        ratios,
        prices,
        vec![],           // per_asset_repayment (empty, consumed by this point)
        Decimal::zero(),  // caller_fee
        Decimal::zero(),  // protocol_fee
        Decimal::zero(),  // total_repaid
    );
    prop.venue_recall_amount = Uint128::new(200_000);
    save_propagation(deps.as_mut().storage, &prop);

    // Trigger build_sell_wall_submsg via handle_liq_queue_transfer_reply error path.
    // When the LQ transfer errors, it falls back to sell wall directly.
    // However, init_config has liq_queue=None so this would error.
    // Instead, use init_config_with_liq_queue and trigger the error path.
    // Actually, we need to re-init with liq_queue for this path to work.
    init_config_with_liq_queue(deps.as_mut().storage);

    let reply = make_error_reply(LIQ_QUEUE_TRANSFER_REPLY_ID, "transfer failed");
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Should fall back to sell wall
    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, SELL_WALL_REPLY_ID);

    // Verify propagation sell_wall_amounts reflect the reduced leftover
    let updated_prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();

    // sell_wall_amounts should have 1 entry (uatom)
    assert_eq!(
        updated_prop.sell_wall_amounts.len(),
        1,
        "Expected 1 sell_wall_amount entry for single collateral asset"
    );

    // Expected sell amount:
    // leftover_credit = 1_000_000 - 0 - 200_000 = 800_000
    // leftover_value = (800_000 / 10^6) * $1 = $0.8
    // sell_value = $0.8 * ratio(1.0) = $0.8
    // sell_amount = ($0.8 / $10) * 10^6 = 80_000 uatom
    assert_eq!(
        updated_prop.sell_wall_amounts[0].amount,
        Uint128::new(80_000),
        "Sell amount should be 80_000 (reduced by venue_recall_amount=200_000)"
    );
}

/// Verify that with venue_recall_amount=0 the full debt is used for sell wall.
/// This is the baseline comparison for the test above.
#[test]
fn sell_wall_no_venue_recall_uses_full_debt() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config_with_liq_queue(deps.as_mut().storage);

    // Same setup as above but venue_recall_amount = 0
    let collateral_assets = vec![make_casset("uatom", 10_000_000, 60, 80)];
    let ratios = vec![Decimal::one()];
    let prices = vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)];

    let prop = make_propagation_post_assess(
        1_000_000,
        Decimal::one(),
        6,
        collateral_assets,
        ratios,
        prices,
        vec![],
        Decimal::zero(),
        Decimal::zero(),
        Decimal::zero(),
    );
    // venue_recall_amount defaults to 0
    save_propagation(deps.as_mut().storage, &prop);

    let reply = make_error_reply(LIQ_QUEUE_TRANSFER_REPLY_ID, "transfer failed");
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, SELL_WALL_REPLY_ID);

    let updated_prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();

    assert_eq!(updated_prop.sell_wall_amounts.len(), 1);

    // Without venue recall:
    // leftover_credit = 1_000_000 - 0 - 0 = 1_000_000
    // leftover_value = (1_000_000 / 10^6) * $1 = $1.0
    // sell_amount = ($1.0 / $10) * 10^6 = 100_000 uatom
    assert_eq!(
        updated_prop.sell_wall_amounts[0].amount,
        Uint128::new(100_000),
        "Sell amount should be 100_000 without venue recall (full debt)"
    );
}

/// Test that venue_recall_amount equal to total_debt results in zero sell wall amounts.
#[test]
fn sell_wall_venue_recall_covers_full_debt() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config_with_liq_queue(deps.as_mut().storage);

    let collateral_assets = vec![make_casset("uatom", 10_000_000, 60, 80)];
    let ratios = vec![Decimal::one()];
    let prices = vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)];

    let mut prop = make_propagation_post_assess(
        1_000_000,
        Decimal::one(),
        6,
        collateral_assets,
        ratios,
        prices,
        vec![],
        Decimal::zero(),
        Decimal::zero(),
        Decimal::zero(),
    );
    // Venue recall covers 100% of debt
    prop.venue_recall_amount = Uint128::new(1_000_000);
    save_propagation(deps.as_mut().storage, &prop);

    let reply = make_error_reply(LIQ_QUEUE_TRANSFER_REPLY_ID, "transfer failed");
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, SELL_WALL_REPLY_ID);

    let updated_prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();

    // leftover_credit = 1_000_000 - 0 - 1_000_000 = 0
    // No sell amounts needed
    assert!(
        updated_prop.sell_wall_amounts.is_empty(),
        "Sell wall amounts should be empty when venue recall covers full debt, got: {:?}",
        updated_prop.sell_wall_amounts
    );
}

/// Test venue_recall_amount with partial liq_queue_repayment.
/// Both should be subtracted from total_debt.
#[test]
fn sell_wall_venue_recall_plus_liq_queue_both_subtracted() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config_with_liq_queue(deps.as_mut().storage);

    let collateral_assets = vec![make_casset("uatom", 10_000_000, 60, 80)];
    let ratios = vec![Decimal::one()];
    let prices = vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)];

    let mut prop = make_propagation_post_assess(
        1_000_000,
        Decimal::one(),
        6,
        collateral_assets,
        ratios,
        prices,
        vec![],
        Decimal::zero(),
        Decimal::zero(),
        Decimal::zero(),
    );
    prop.venue_recall_amount = Uint128::new(200_000);
    prop.liq_queue_repayment = Decimal::from_ratio(300_000u128, 1u128);
    save_propagation(deps.as_mut().storage, &prop);

    let reply = make_error_reply(LIQ_QUEUE_TRANSFER_REPLY_ID, "transfer failed");
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.messages[0].id, SELL_WALL_REPLY_ID);

    let updated_prop = LIQUIDATION_PROPAGATION.load(deps.as_ref().storage).unwrap();

    assert_eq!(updated_prop.sell_wall_amounts.len(), 1);

    // leftover_credit = 1_000_000 - 300_000 (lq) - 200_000 (venue) = 500_000
    // leftover_value = (500_000 / 10^6) * $1 = $0.5
    // sell_amount = ($0.5 / $10) * 10^6 = 50_000 uatom
    assert_eq!(
        updated_prop.sell_wall_amounts[0].amount,
        Uint128::new(50_000),
        "Sell amount should be 50_000 (debt minus lq_repay minus venue_recall)"
    );
}
