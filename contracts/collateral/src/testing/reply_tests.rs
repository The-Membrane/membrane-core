#![allow(non_snake_case)]

use cosmwasm_std::testing::mock_dependencies;
use cosmwasm_std::{Addr, Coin, Decimal, Env, Timestamp, Uint128};

use membrane::types::{Asset, AssetInfo};

use crate::reply::handle_reply;
use crate::state::{
    POSITIONS, SELL_COLLATERAL_PROPAGATION, WITHDRAW_PROPAGATION,
    WITHDRAW_REPLY_ID, SELL_COLLATERAL_REPLY_ID, DEBT_ACCRUE_VALIDATE_REPLY_ID,
};
use crate::testing::test_helpers::helpers::*;

fn mock_env_at(time_secs: u64) -> Env {
    let mut env = cosmwasm_std::testing::mock_env();
    env.block.time = Timestamp::from_seconds(time_secs);
    env
}

fn atom_info() -> AssetInfo {
    AssetInfo::NativeToken { denom: "uatom".to_string() }
}

fn osmo_info() -> AssetInfo {
    AssetInfo::NativeToken { denom: "uosmo".to_string() }
}

// ========================================================================
// Group A: handle_withdraw_reply (6 tests)
// ========================================================================

#[test]
fn withdraw_reply_success_partial() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);

    // Seed stored prices for update_basket_tally
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);
    init_stored_price(deps.as_mut().storage, "uosmo", Decimal::from_ratio(5u128, 1u128), 900);

    // Create position with 1000 uatom
    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner.clone(),
        Uint128::one(),
        vec![make_casset_with_amount("uatom", 1000, 50, 60)],
    );

    // Save withdrawal propagation: withdrawing 400 uatom
    save_withdraw_propagation(
        deps.as_mut().storage,
        owner_str.as_str(),
        Uint128::one(),
        vec![Asset { info: atom_info(), amount: Uint128::new(1000) }],
        vec![Uint128::new(400)],
        vec![Uint128::new(5000)],
    );

    let reply = make_success_reply(WITHDRAW_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Should have 1 BankMsg for sending 400 uatom
    assert_eq!(res.messages.len(), 1);

    // Position should have 600 uatom remaining
    let positions = POSITIONS.load(deps.as_ref().storage, owner).unwrap();
    assert_eq!(positions[0].collateral_assets[0].asset.amount, Uint128::new(600));

    // Propagation should be cleared
    assert!(WITHDRAW_PROPAGATION.may_load(deps.as_ref().storage).unwrap().is_none());
}

#[test]
fn withdraw_reply_success_full_removal() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner.clone(),
        Uint128::one(),
        vec![make_casset_with_amount("uatom", 1000, 50, 60)],
    );

    // Withdraw ALL 1000 uatom
    save_withdraw_propagation(
        deps.as_mut().storage,
        owner_str.as_str(),
        Uint128::one(),
        vec![Asset { info: atom_info(), amount: Uint128::new(1000) }],
        vec![Uint128::new(1000)],
        vec![Uint128::new(5000)],
    );

    let reply = make_success_reply(WITHDRAW_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    assert_eq!(res.messages.len(), 1);

    // Position should be removed entirely (empty collateral)
    let positions = POSITIONS.may_load(deps.as_ref().storage, owner).unwrap().unwrap_or_default();
    // Either empty vec or position removed
    let has_position = positions.iter().any(|p| p.position_id == Uint128::one());
    assert!(!has_position);
}

#[test]
fn withdraw_reply_error_returns_contract_error() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    let reply = make_error_reply(WITHDRAW_REPLY_ID, "position insolvent");
    let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();
    assert!(format!("{:?}", err).contains("insolvent"));
}

#[test]
fn withdraw_reply_zero_amount_no_bank_msg() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner.clone(),
        Uint128::one(),
        vec![make_casset_with_amount("uatom", 1000, 50, 60)],
    );

    // Withdraw zero
    save_withdraw_propagation(
        deps.as_mut().storage,
        owner_str.as_str(),
        Uint128::one(),
        vec![Asset { info: atom_info(), amount: Uint128::new(1000) }],
        vec![Uint128::zero()],
        vec![Uint128::new(5000)],
    );

    let reply = make_success_reply(WITHDRAW_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // No BankMsg for zero amount
    assert_eq!(res.messages.len(), 0);
}

#[test]
fn withdraw_reply_multiple_assets() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);
    init_stored_price(deps.as_mut().storage, "uosmo", Decimal::from_ratio(5u128, 1u128), 900);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner.clone(),
        Uint128::one(),
        vec![
            make_casset_with_amount("uatom", 1000, 50, 60),
            make_casset_with_amount("uosmo", 2000, 40, 50),
        ],
    );

    save_withdraw_propagation(
        deps.as_mut().storage,
        owner_str.as_str(),
        Uint128::one(),
        vec![
            Asset { info: atom_info(), amount: Uint128::new(1000) },
            Asset { info: osmo_info(), amount: Uint128::new(2000) },
        ],
        vec![Uint128::new(300), Uint128::new(500)],
        vec![Uint128::new(5000), Uint128::new(8000)],
    );

    let reply = make_success_reply(WITHDRAW_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // 2 BankMsgs (one per asset)
    assert_eq!(res.messages.len(), 2);

    // Position should have 700 uatom and 1500 uosmo
    let positions = POSITIONS.load(deps.as_ref().storage, owner).unwrap();
    assert_eq!(positions[0].collateral_assets[0].asset.amount, Uint128::new(700));
    assert_eq!(positions[0].collateral_assets[1].asset.amount, Uint128::new(1500));
}

#[test]
fn withdraw_reply_empty_position_removed() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);

    let owner = Addr::unchecked(owner_str.clone());
    // Create 2 positions
    init_position(
        deps.as_mut().storage,
        owner.clone(),
        Uint128::one(),
        vec![make_casset_with_amount("uatom", 500, 50, 60)],
    );
    init_position(
        deps.as_mut().storage,
        owner.clone(),
        Uint128::new(2),
        vec![make_casset_with_amount("uatom", 1000, 50, 60)],
    );

    // Withdraw all from position 1
    save_withdraw_propagation(
        deps.as_mut().storage,
        owner_str.as_str(),
        Uint128::one(),
        vec![Asset { info: atom_info(), amount: Uint128::new(500) }],
        vec![Uint128::new(500)],
        vec![Uint128::new(5000)],
    );

    let reply = make_success_reply(WITHDRAW_REPLY_ID, vec![]);
    let _ = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Position 1 should be removed, position 2 should remain
    let positions = POSITIONS.load(deps.as_ref().storage, owner).unwrap();
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].position_id, Uint128::new(2));
}

// ========================================================================
// Group B: handle_debt_accrue_validate_reply (6 tests)
// ========================================================================

#[test]
fn accrue_validate_reply_success_partial() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner.clone(),
        Uint128::one(),
        vec![make_casset_with_amount("uatom", 1000, 50, 60)],
    );

    save_withdraw_propagation(
        deps.as_mut().storage,
        owner_str.as_str(),
        Uint128::one(),
        vec![Asset { info: atom_info(), amount: Uint128::new(1000) }],
        vec![Uint128::new(400)],
        vec![Uint128::new(5000)],
    );

    let reply = make_success_reply(DEBT_ACCRUE_VALIDATE_REPLY_ID, vec![
        ("total_debt", "50000"),
    ]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    assert_eq!(res.messages.len(), 1);

    let positions = POSITIONS.load(deps.as_ref().storage, owner).unwrap();
    assert_eq!(positions[0].collateral_assets[0].asset.amount, Uint128::new(600));

    assert!(WITHDRAW_PROPAGATION.may_load(deps.as_ref().storage).unwrap().is_none());
}

#[test]
fn accrue_validate_reply_success_full_removal() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner.clone(),
        Uint128::one(),
        vec![make_casset_with_amount("uatom", 1000, 50, 60)],
    );

    save_withdraw_propagation(
        deps.as_mut().storage,
        owner_str.as_str(),
        Uint128::one(),
        vec![Asset { info: atom_info(), amount: Uint128::new(1000) }],
        vec![Uint128::new(1000)],
        vec![Uint128::new(5000)],
    );

    let reply = make_success_reply(DEBT_ACCRUE_VALIDATE_REPLY_ID, vec![]);
    let _ = handle_reply(deps.as_mut(), env, reply).unwrap();

    let positions = POSITIONS.may_load(deps.as_ref().storage, owner).unwrap().unwrap_or_default();
    let has_position = positions.iter().any(|p| p.position_id == Uint128::one());
    assert!(!has_position);
}

#[test]
fn accrue_validate_reply_error() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    let reply = make_error_reply(DEBT_ACCRUE_VALIDATE_REPLY_ID, "debt validation failed");
    let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();
    assert!(format!("{:?}", err).contains("validation failed"));
}

#[test]
fn accrue_validate_parses_total_debt() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner.clone(),
        Uint128::one(),
        vec![make_casset_with_amount("uatom", 1000, 50, 60)],
    );

    save_withdraw_propagation(
        deps.as_mut().storage,
        owner_str.as_str(),
        Uint128::one(),
        vec![Asset { info: atom_info(), amount: Uint128::new(1000) }],
        vec![Uint128::new(100)],
        vec![Uint128::new(5000)],
    );

    // Include total_debt attribute
    let reply = make_success_reply(DEBT_ACCRUE_VALIDATE_REPLY_ID, vec![
        ("total_debt", "999999"),
    ]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Should succeed - total_debt is parsed but only used internally
    assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "debt_accrue_validate_reply_success"));
}

#[test]
fn accrue_validate_missing_total_debt_defaults_gracefully() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner.clone(),
        Uint128::one(),
        vec![make_casset_with_amount("uatom", 1000, 50, 60)],
    );

    save_withdraw_propagation(
        deps.as_mut().storage,
        owner_str.as_str(),
        Uint128::one(),
        vec![Asset { info: atom_info(), amount: Uint128::new(1000) }],
        vec![Uint128::new(100)],
        vec![Uint128::new(5000)],
    );

    // No total_debt attribute
    let reply = make_success_reply(DEBT_ACCRUE_VALIDATE_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Should still succeed with default zero
    assert!(res.attributes.len() > 0);
}

#[test]
fn accrue_validate_position_removed_when_empty() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner.clone(),
        Uint128::one(),
        vec![make_casset_with_amount("uatom", 500, 50, 60)],
    );

    save_withdraw_propagation(
        deps.as_mut().storage,
        owner_str.as_str(),
        Uint128::one(),
        vec![Asset { info: atom_info(), amount: Uint128::new(500) }],
        vec![Uint128::new(500)],
        vec![Uint128::new(5000)],
    );

    let reply = make_success_reply(DEBT_ACCRUE_VALIDATE_REPLY_ID, vec![]);
    let _ = handle_reply(deps.as_mut(), env, reply).unwrap();

    let positions = POSITIONS.may_load(deps.as_ref().storage, owner).unwrap().unwrap_or_default();
    assert!(positions.is_empty() || !positions.iter().any(|p| p.position_id == Uint128::one()));
}

// ========================================================================
// Group C: handle_sell_collateral_reply (4 tests)
// ========================================================================

#[test]
fn sell_reply_success_partial() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner.clone(),
        Uint128::one(),
        vec![make_casset_with_amount("uatom", 1000, 50, 60)],
    );

    save_sell_collateral_propagation(
        deps.as_mut().storage,
        owner_str.as_str(),
        Uint128::one(),
        vec![Coin { denom: "uatom".to_string(), amount: Uint128::new(400) }],
    );

    let reply = make_success_reply(SELL_COLLATERAL_REPLY_ID, vec![]);
    let res = handle_reply(deps.as_mut(), env, reply).unwrap();

    // No BankMsg for sell (swaps are SubMsgs), just attributes
    assert!(res.attributes.iter().any(|a| a.key == "method"));

    // Position should have 600 remaining
    let positions = POSITIONS.load(deps.as_ref().storage, owner).unwrap();
    assert_eq!(positions[0].collateral_assets[0].asset.amount, Uint128::new(600));

    assert!(SELL_COLLATERAL_PROPAGATION.may_load(deps.as_ref().storage).unwrap().is_none());
}

#[test]
fn sell_reply_fully_sold_asset_removed() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);
    init_stored_price(deps.as_mut().storage, "uosmo", Decimal::from_ratio(5u128, 1u128), 900);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner.clone(),
        Uint128::one(),
        vec![
            make_casset_with_amount("uatom", 1000, 50, 60),
            make_casset_with_amount("uosmo", 2000, 40, 50),
        ],
    );

    // Sell all uatom
    save_sell_collateral_propagation(
        deps.as_mut().storage,
        owner_str.as_str(),
        Uint128::one(),
        vec![Coin { denom: "uatom".to_string(), amount: Uint128::new(1000) }],
    );

    let reply = make_success_reply(SELL_COLLATERAL_REPLY_ID, vec![]);
    let _ = handle_reply(deps.as_mut(), env, reply).unwrap();

    // Position should only have uosmo
    let positions = POSITIONS.load(deps.as_ref().storage, owner).unwrap();
    assert_eq!(positions[0].collateral_assets.len(), 1);
    assert_eq!(positions[0].collateral_assets[0].asset.info.to_string(), "uosmo");
}

#[test]
fn sell_reply_position_removed_when_all_sold() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner.clone(),
        Uint128::one(),
        vec![make_casset_with_amount("uatom", 1000, 50, 60)],
    );

    save_sell_collateral_propagation(
        deps.as_mut().storage,
        owner_str.as_str(),
        Uint128::one(),
        vec![Coin { denom: "uatom".to_string(), amount: Uint128::new(1000) }],
    );

    let reply = make_success_reply(SELL_COLLATERAL_REPLY_ID, vec![]);
    let _ = handle_reply(deps.as_mut(), env, reply).unwrap();

    let positions = POSITIONS.may_load(deps.as_ref().storage, owner).unwrap().unwrap_or_default();
    let has_position = positions.iter().any(|p| p.position_id == Uint128::one());
    assert!(!has_position);
}

#[test]
fn sell_reply_error() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    let reply = make_error_reply(SELL_COLLATERAL_REPLY_ID, "swap failed");
    let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();
    assert!(format!("{:?}", err).contains("swap failed"));
}

// ========================================================================
// Group D: Dispatcher (2 tests)
// ========================================================================

#[test]
fn unknown_reply_id_errors() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    let reply = make_success_reply(999, vec![]);
    let err = handle_reply(deps.as_mut(), env, reply).unwrap_err();
    assert!(format!("{:?}", err).contains("Unknown reply ID"));
}

#[test]
fn each_known_id_dispatches() {
    // Just verify the dispatcher doesn't error on valid IDs
    // (will error for other reasons like missing state, but the dispatch itself works)
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // WITHDRAW_REPLY_ID will fail due to missing propagation state - but it dispatches
    let reply = make_success_reply(WITHDRAW_REPLY_ID, vec![]);
    let err = handle_reply(deps.as_mut(), env.clone(), reply);
    // Should fail with state error, NOT "Unknown reply ID"
    assert!(!format!("{:?}", err).contains("Unknown reply ID"));

    // SELL_COLLATERAL_REPLY_ID same
    let reply = make_success_reply(SELL_COLLATERAL_REPLY_ID, vec![]);
    let err = handle_reply(deps.as_mut(), env.clone(), reply);
    assert!(!format!("{:?}", err).contains("Unknown reply ID"));

    // DEBT_ACCRUE_VALIDATE_REPLY_ID same
    let reply = make_success_reply(DEBT_ACCRUE_VALIDATE_REPLY_ID, vec![]);
    let err = handle_reply(deps.as_mut(), env, reply);
    assert!(!format!("{:?}", err).contains("Unknown reply ID"));
}
