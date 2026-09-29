#![allow(non_snake_case)]

use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env};
use cosmwasm_std::{Addr, Decimal, Uint128};

use crate::reply::handle_reply;
use crate::state::*;
use crate::testing::test_helpers::*;

// ============================================================================
// Propagation lifecycle tests
//
// These tests verify the LiquidationPropagation state as it flows through
// the multi-step SubMsg chain: liquidate() → debt accrue reply → collateral
// assess reply → liq queue reply(s) → sell wall reply → debt update reply
// → claim update reply.
// ============================================================================

/// 1. Calling liquidate() initializes LIQUIDATION_PROPAGATION with zeroed
///    fields and the correct position_owner / position_id.
#[test]
fn initialized_on_liquidate() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    // liquidate() loads CONFIG, so we need one saved.
    init_config(deps.as_mut().storage);

    // MockApi in cosmwasm_std 2.x requires valid bech32 addresses for
    // addr_validate(). Use addr_make() to produce one.
    let valid_owner = deps.api.addr_make("position_owner");

    let info = message_info(&Addr::unchecked("liquidator"), &[]);
    let _res = crate::liquidations::liquidate(
        deps.as_mut(),
        env,
        info,
        Uint128::one(),
        valid_owner.to_string(),
    )
    .unwrap();

    let prop = LIQUIDATION_PROPAGATION
        .load(deps.as_ref().storage)
        .unwrap();

    // Zeroed fields
    assert!(prop.per_asset_repayment.is_empty());
    assert_eq!(prop.liq_queue_repayment, Decimal::zero());
    assert_eq!(prop.protocol_fee_value, Decimal::zero());
    assert!(prop.cAsset_ratios.is_empty());
    assert!(prop.cAsset_prices.is_empty());
    assert!(prop.collateral_assets.is_empty());
    assert_eq!(prop.total_debt, Uint128::zero());
    assert_eq!(prop.credit_price.price, Decimal::zero());
    assert_eq!(prop.caller_fee_value_paid, Decimal::zero());
    assert_eq!(prop.total_repaid, Decimal::zero());

    // Identity fields
    assert_eq!(prop.position_owner, valid_owner);
    assert_eq!(prop.position_id, Uint128::one());
    assert_eq!(prop.caller, Addr::unchecked("liquidator"));
}

/// 2. After the debt accrue reply, total_debt and credit_price are updated
///    in the stored propagation.
#[test]
fn updated_after_debt_accrue() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    init_config(deps.as_mut().storage);
    let prop = default_propagation();
    save_propagation(deps.as_mut().storage, &prop);

    // Build a debt accrue success reply: 5000 CDT at $1 (6 decimals).
    let reply = make_debt_accrue_success_reply(5_000_000000, Decimal::one(), 6);

    let _res = handle_reply(deps.as_mut(), env, reply).unwrap();

    let updated = LIQUIDATION_PROPAGATION
        .load(deps.as_ref().storage)
        .unwrap();
    assert_eq!(updated.total_debt, Uint128::new(5_000_000000));
    assert_eq!(updated.credit_price.price, Decimal::one());
    assert_eq!(updated.credit_price.decimals, 6);
}

/// 3. After the collateral assess reply (with an insolvent position and a
///    liq_queue configured), collateral_assets, ratios, prices, and
///    per_asset_repayment are stored in propagation.
///
///    Scenario: 100 ATOM at $10 (value = $1000), 1000 CDT debt at $1
///    (6 decimals, total_debt = 1_000_000000), max_ltv = 60%.
///    The position is insolvent since get_repay_quantities uses raw
///    total_debt to compute debt_value, making LTV > max_ltv.
#[test]
fn updated_after_assess() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    // Need liq_queue configured so the handler takes the LQ branch.
    init_config_with_liq_queue(deps.as_mut().storage);

    // Pre-requisite: propagation must be in post-debt-accrue state.
    let prop = make_propagation_post_debt_accrue(1_000_000000, Decimal::one(), 6);
    save_propagation(deps.as_mut().storage, &prop);

    // Build a collateral assessment reply.
    let collateral_assets = vec![make_casset("uatom", 100_000000, 50, 60)];
    let ratios = vec![Decimal::one()];
    let prices = vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)];

    let reply = make_collateral_assess_success_reply(
        collateral_assets,
        ratios,
        prices,
        Decimal::percent(60),  // avg_max_ltv
        Decimal::percent(50),  // avg_max_borrow_ltv
        false,                 // any_frozen
        vec![],                // frozen_assets
    );

    let _res = handle_reply(deps.as_mut(), env, reply).unwrap();

    let updated = LIQUIDATION_PROPAGATION
        .load(deps.as_ref().storage)
        .unwrap();

    // collateral_assets stored (amount reduced by fee deduction in Step 2)
    assert_eq!(updated.collateral_assets.len(), 1);
    // Fee-deducted: original 100_000000 minus caller_fee and protocol_fee amounts
    assert!(
        updated.collateral_assets[0].asset.amount < Uint128::new(100_000000),
        "Collateral should be reduced by fee deduction, got: {}",
        updated.collateral_assets[0].asset.amount
    );
    assert!(
        updated.collateral_assets[0].asset.amount > Uint128::zero(),
        "Collateral should not be fully consumed by fees"
    );

    // Ratios and prices stored
    assert_eq!(updated.cAsset_ratios.len(), 1);
    assert_eq!(updated.cAsset_ratios[0], Decimal::one());
    assert_eq!(updated.cAsset_prices.len(), 1);
    assert_eq!(
        updated.cAsset_prices[0].price,
        Decimal::from_ratio(10u128, 1u128)
    );

    // Fix #3: per_asset_repayment is now built in build_next_after_fees (called
    // during Step 9 fee_transfer_reply or directly if no fees), not in Step 2
    // (handle_collateral_assess_reply). After Step 2, it is empty.
    assert!(
        updated.per_asset_repayment.is_empty(),
        "per_asset_repayment should be empty after Step 2 (built later in build_next_after_fees)"
    );

    // Fees stored (1% liq_fee from init_config_with_liq_queue).
    assert!(
        updated.caller_fee_value_paid > Decimal::zero(),
        "caller_fee should be positive for an insolvent position"
    );
    assert!(
        updated.protocol_fee_value > Decimal::zero(),
        "protocol_fee_value should be positive"
    );
}

/// 4. After a liq queue reply, liq_queue_repayment and total_repaid are
///    accumulated, and the first per_asset_repayment entry is removed (FIFO).
#[test]
fn updated_after_liq_queue() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    init_config_with_liq_queue(deps.as_mut().storage);

    // Build a post-assess propagation with 2 per-asset entries.
    let collateral_assets = vec![
        make_casset("uatom", 100_000000, 50, 60),
        make_casset("uosmo", 200_000000, 50, 60),
    ];
    let ratios = vec![Decimal::percent(50), Decimal::percent(50)];
    let prices = vec![
        make_price(Decimal::from_ratio(10u128, 1u128), 6),
        make_price(Decimal::from_ratio(5u128, 1u128), 6),
    ];
    let per_asset_repayment = vec![
        Decimal::from_ratio(100u128, 1u128),
        Decimal::from_ratio(200u128, 1u128),
    ];

    let prop = make_propagation_post_assess(
        1_000_000000,
        Decimal::one(),
        6,
        collateral_assets,
        ratios,
        prices,
        per_asset_repayment,
        Decimal::from_ratio(4u128, 1u128),  // caller_fee
        Decimal::from_ratio(4u128, 1u128),  // protocol_fee
        Decimal::zero(),                     // total_repaid starts at 0
    );
    save_propagation(deps.as_mut().storage, &prop);

    // LQ reply: repay=50, collateral=5 uatom
    let reply = make_liq_queue_success_reply(50, 5_000000, "uatom");
    let _res = handle_reply(deps.as_mut(), env, reply).unwrap();

    let updated = LIQUIDATION_PROPAGATION
        .load(deps.as_ref().storage)
        .unwrap();

    // Accumulation
    assert_eq!(
        updated.liq_queue_repayment,
        Decimal::from_ratio(50u128, 1u128)
    );
    assert_eq!(
        updated.total_repaid,
        Decimal::from_ratio(50u128, 1u128)
    );

    // FIFO removal: first entry (100) removed, only second (200) remains.
    assert_eq!(updated.per_asset_repayment.len(), 1);
    assert_eq!(
        updated.per_asset_repayment[0],
        Decimal::from_ratio(200u128, 1u128)
    );
}

/// 5. Multiple liq queue replies accumulate repayment and remove
///    per_asset_repayment entries in FIFO order.
#[test]
fn multi_call_accumulation() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    init_config_with_liq_queue(deps.as_mut().storage);

    let collateral_assets = vec![
        make_casset("uatom", 100_000000, 50, 60),
        make_casset("uosmo", 200_000000, 50, 60),
        make_casset("ujuno", 300_000000, 50, 60),
    ];
    let ratios = vec![
        Decimal::from_ratio(1u128, 3u128),
        Decimal::from_ratio(1u128, 3u128),
        Decimal::from_ratio(1u128, 3u128),
    ];
    let prices = vec![
        make_price(Decimal::from_ratio(10u128, 1u128), 6),
        make_price(Decimal::from_ratio(5u128, 1u128), 6),
        make_price(Decimal::from_ratio(2u128, 1u128), 6),
    ];
    let per_asset_repayment = vec![
        Decimal::from_ratio(100u128, 1u128),
        Decimal::from_ratio(200u128, 1u128),
        Decimal::from_ratio(300u128, 1u128),
    ];

    let prop = make_propagation_post_assess(
        10_000_000000,           // large debt so LQ doesn't fully repay
        Decimal::one(),
        6,
        collateral_assets,
        ratios,
        prices,
        per_asset_repayment,
        Decimal::from_ratio(4u128, 1u128),
        Decimal::from_ratio(4u128, 1u128),
        Decimal::zero(),
    );
    save_propagation(deps.as_mut().storage, &prop);

    // --- Reply 1: repay=10 ---
    let reply1 = make_liq_queue_success_reply(10, 1_000000, "uatom");
    let _res1 = handle_reply(deps.as_mut(), env.clone(), reply1).unwrap();

    let after1 = LIQUIDATION_PROPAGATION
        .load(deps.as_ref().storage)
        .unwrap();
    assert_eq!(after1.liq_queue_repayment, Decimal::from_ratio(10u128, 1u128));
    assert_eq!(after1.total_repaid, Decimal::from_ratio(10u128, 1u128));
    assert_eq!(after1.per_asset_repayment.len(), 2);
    assert_eq!(after1.per_asset_repayment[0], Decimal::from_ratio(200u128, 1u128));
    assert_eq!(after1.per_asset_repayment[1], Decimal::from_ratio(300u128, 1u128));

    // --- Reply 2: repay=20 ---
    let reply2 = make_liq_queue_success_reply(20, 4_000000, "uosmo");
    let _res2 = handle_reply(deps.as_mut(), env.clone(), reply2).unwrap();

    let after2 = LIQUIDATION_PROPAGATION
        .load(deps.as_ref().storage)
        .unwrap();
    assert_eq!(after2.liq_queue_repayment, Decimal::from_ratio(30u128, 1u128));
    assert_eq!(after2.total_repaid, Decimal::from_ratio(30u128, 1u128));
    assert_eq!(after2.per_asset_repayment.len(), 1);
    assert_eq!(after2.per_asset_repayment[0], Decimal::from_ratio(300u128, 1u128));

    // --- Reply 3: repay=30 (last entry, triggers sell wall) ---
    let reply3 = make_liq_queue_success_reply(30, 15_000000, "ujuno");
    let _res3 = handle_reply(deps.as_mut(), env, reply3).unwrap();

    let after3 = LIQUIDATION_PROPAGATION
        .load(deps.as_ref().storage)
        .unwrap();
    assert_eq!(after3.liq_queue_repayment, Decimal::from_ratio(60u128, 1u128));
    assert_eq!(after3.total_repaid, Decimal::from_ratio(60u128, 1u128));
    assert!(after3.per_asset_repayment.is_empty());
}

/// 6. After the claim_update reply (final step), LIQUIDATION_PROPAGATION
///    is removed from storage.
#[test]
fn removed_after_claim_update() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    init_config(deps.as_mut().storage);

    // Build a post-assess propagation with empty per_asset (simulating
    // that LQ + sell wall have already been processed).
    let prop = make_propagation_post_assess(
        1_000_000000,
        Decimal::one(),
        6,
        vec![make_casset("uatom", 100_000000, 50, 60)],
        vec![Decimal::one()],
        vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)],
        vec![],                                  // per_asset_repayment empty
        Decimal::from_ratio(4u128, 1u128),       // caller_fee
        Decimal::from_ratio(4u128, 1u128),       // protocol_fee
        Decimal::from_ratio(1000u128, 1u128),    // total_repaid
    );
    save_propagation(deps.as_mut().storage, &prop);

    // Build a claim_update success reply (reply ID = 7).
    let reply = make_success_reply(CLAIM_UPDATE_REPLY_ID, vec![]);

    let _res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Propagation should be removed.
    let loaded = LIQUIDATION_PROPAGATION.may_load(deps.as_ref().storage).unwrap();
    assert!(
        loaded.is_none(),
        "LIQUIDATION_PROPAGATION should be removed after claim_update reply"
    );
}

/// 7. Intermediate reply steps (e.g., debt_accrue) do NOT remove
///    the propagation from storage.
#[test]
fn not_removed_on_intermediate_steps() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    init_config(deps.as_mut().storage);
    let prop = default_propagation();
    save_propagation(deps.as_mut().storage, &prop);

    // Debt accrue reply is an intermediate step.
    let reply = make_debt_accrue_success_reply(1_000_000000, Decimal::one(), 6);
    let _res = handle_reply(deps.as_mut(), env, reply).unwrap();

    let loaded = LIQUIDATION_PROPAGATION.may_load(deps.as_ref().storage).unwrap();
    assert!(
        loaded.is_some(),
        "LIQUIDATION_PROPAGATION should still exist after intermediate debt_accrue reply"
    );
}

/// 8. position_owner and position_id are preserved through reply steps
///    (they are identity fields that never change).
#[test]
fn position_owner_id_preserved() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    init_config(deps.as_mut().storage);

    let mut prop = default_propagation();
    prop.position_owner = Addr::unchecked("custom_owner");
    prop.position_id = Uint128::new(42);
    save_propagation(deps.as_mut().storage, &prop);

    // Run through debt accrue reply.
    let reply = make_debt_accrue_success_reply(5_000_000000, Decimal::one(), 6);
    let _res = handle_reply(deps.as_mut(), env, reply).unwrap();

    let updated = LIQUIDATION_PROPAGATION
        .load(deps.as_ref().storage)
        .unwrap();
    assert_eq!(updated.position_owner, Addr::unchecked("custom_owner"));
    assert_eq!(updated.position_id, Uint128::new(42));
}

/// 9. Calling a reply handler when no propagation is saved should error
///    (the handler tries to load from storage).
#[test]
fn missing_propagation_errors() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    init_config(deps.as_mut().storage);
    // Deliberately do NOT save propagation.

    let reply = make_debt_accrue_success_reply(1_000_000000, Decimal::one(), 6);
    let result = handle_reply(deps.as_mut(), env, reply);

    assert!(
        result.is_err(),
        "Should error when LIQUIDATION_PROPAGATION is not in storage"
    );
}

/// 10. FIFO per_asset_repayment removal across sequential LQ replies:
///     each reply removes exactly the first entry regardless of value.
#[test]
fn fifo_per_asset_removal() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    init_config_with_liq_queue(deps.as_mut().storage);

    let collateral_assets = vec![
        make_casset("uatom", 100_000000, 50, 60),
        make_casset("uosmo", 200_000000, 50, 60),
        make_casset("ujuno", 300_000000, 50, 60),
    ];
    let ratios = vec![
        Decimal::from_ratio(1u128, 3u128),
        Decimal::from_ratio(1u128, 3u128),
        Decimal::from_ratio(1u128, 3u128),
    ];
    let prices = vec![
        make_price(Decimal::from_ratio(10u128, 1u128), 6),
        make_price(Decimal::from_ratio(5u128, 1u128), 6),
        make_price(Decimal::from_ratio(2u128, 1u128), 6),
    ];

    // Distinctive fractional values so we can verify FIFO order.
    let per_asset_repayment = vec![
        Decimal::from_ratio(15u128, 10u128), // 1.5
        Decimal::from_ratio(25u128, 10u128), // 2.5
        Decimal::from_ratio(35u128, 10u128), // 3.5
    ];

    let prop = make_propagation_post_assess(
        10_000_000000,           // large debt so LQ doesn't fully repay
        Decimal::one(),
        6,
        collateral_assets,
        ratios,
        prices,
        per_asset_repayment,
        Decimal::from_ratio(4u128, 1u128),
        Decimal::from_ratio(4u128, 1u128),
        Decimal::zero(),
    );
    save_propagation(deps.as_mut().storage, &prop);

    // --- After 1st LQ reply: [1.5] removed → [2.5, 3.5] ---
    let reply1 = make_liq_queue_success_reply(1, 1_000000, "uatom");
    let _res1 = handle_reply(deps.as_mut(), env.clone(), reply1).unwrap();

    let after1 = LIQUIDATION_PROPAGATION
        .load(deps.as_ref().storage)
        .unwrap();
    assert_eq!(after1.per_asset_repayment.len(), 2);
    assert_eq!(after1.per_asset_repayment[0], Decimal::from_ratio(25u128, 10u128));
    assert_eq!(after1.per_asset_repayment[1], Decimal::from_ratio(35u128, 10u128));

    // --- After 2nd LQ reply: [2.5] removed → [3.5] ---
    let reply2 = make_liq_queue_success_reply(1, 1_000000, "uosmo");
    let _res2 = handle_reply(deps.as_mut(), env.clone(), reply2).unwrap();

    let after2 = LIQUIDATION_PROPAGATION
        .load(deps.as_ref().storage)
        .unwrap();
    assert_eq!(after2.per_asset_repayment.len(), 1);
    assert_eq!(after2.per_asset_repayment[0], Decimal::from_ratio(35u128, 10u128));

    // --- After 3rd LQ reply: [3.5] removed → [] ---
    let reply3 = make_liq_queue_success_reply(1, 1_000000, "ujuno");
    let _res3 = handle_reply(deps.as_mut(), env, reply3).unwrap();

    let after3 = LIQUIDATION_PROPAGATION
        .load(deps.as_ref().storage)
        .unwrap();
    assert!(
        after3.per_asset_repayment.is_empty(),
        "All per_asset_repayment entries should be removed after 3 LQ replies"
    );
}
