#![allow(non_snake_case)]

use cosmwasm_std::testing::mock_dependencies;
use cosmwasm_std::{Decimal, Env, Timestamp, Uint128};

use membrane::types::AssetInfo;

use crate::risk_engine::update_basket_tally;
use crate::state::BASKET;
use crate::testing::test_helpers::helpers::*;

fn mock_env_at(time_secs: u64) -> Env {
    let mut env = cosmwasm_std::testing::mock_env();
    env.block.time = Timestamp::from_seconds(time_secs);
    env
}

// ========================================================================
// Group A: Basic tally updates (4 tests)
// ========================================================================

#[test]
fn add_increments_supply() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // Init basket with 2 assets, zero supply
    init_basket(deps.as_mut().storage);
    init_config(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);

    // Seed stored prices for ratio computation
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);
    init_stored_price(deps.as_mut().storage, "uosmo", Decimal::from_ratio(5u128, 1u128), 900);

    let collateral = vec![make_casset_with_amount("uatom", 500, 50, 60)];
    let full_position = vec![make_casset_with_amount("uatom", 500, 50, 60)];

    let res = update_basket_tally(
        &mut deps.as_mut(),
        env,
        collateral,
        full_position,
        true,  // add
        false, // not liquidation
    );
    assert!(res.is_ok());

    let basket = BASKET.load(deps.as_ref().storage).unwrap();
    // uatom supply cap should be 500
    assert_eq!(basket.collateral_supply_caps[0].current_supply, Uint128::new(500));
    assert_eq!(basket.collateral_types[0].asset.amount, Uint128::new(500));
}

#[test]
fn subtract_decrements_supply() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // Init basket with custom supply
    let cassets = vec![
        make_casset_with_amount("uatom", 1000, 50, 60),
        make_casset_with_amount("uosmo", 2000, 40, 50),
    ];
    let caps = vec![
        make_supply_cap_with_supply("uatom", 100, 1000),
        make_supply_cap_with_supply("uosmo", 100, 2000),
    ];
    init_basket_custom(deps.as_mut().storage, cassets, caps, vec![]);
    init_config(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);
    init_stored_price(deps.as_mut().storage, "uosmo", Decimal::from_ratio(5u128, 1u128), 900);

    let collateral = vec![make_casset_with_amount("uatom", 300, 50, 60)];
    let full_position = vec![make_casset_with_amount("uatom", 700, 50, 60)]; // remaining after withdraw

    let res = update_basket_tally(
        &mut deps.as_mut(),
        env,
        collateral,
        full_position,
        false, // subtract
        false,
    );
    assert!(res.is_ok());

    let basket = BASKET.load(deps.as_ref().storage).unwrap();
    assert_eq!(basket.collateral_supply_caps[0].current_supply, Uint128::new(700));
}

#[test]
fn subtract_more_than_supply_clamps_zero() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    let cassets = vec![make_casset_with_amount("uatom", 100, 50, 60)];
    let caps = vec![make_supply_cap_with_supply("uatom", 100, 100)];
    init_basket_custom(deps.as_mut().storage, cassets, caps, vec![]);
    init_config(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);

    // Try to subtract 500 from supply of 100
    let collateral = vec![make_casset_with_amount("uatom", 500, 50, 60)];
    let full_position = vec![];

    let res = update_basket_tally(
        &mut deps.as_mut(),
        env,
        collateral,
        full_position,
        false,
        false,
    );
    assert!(res.is_ok());

    let basket = BASKET.load(deps.as_ref().storage).unwrap();
    assert_eq!(basket.collateral_supply_caps[0].current_supply, Uint128::zero());
}

#[test]
fn from_liquidation_bypasses_cap_checks() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // Set up tiny cap ratio (1%) but large deposit
    let cassets = vec![make_casset_with_amount("uatom", 0, 50, 60)];
    let caps = vec![make_supply_cap_with_ratio("uatom", 1)]; // 1% cap
    init_basket_custom(deps.as_mut().storage, cassets, caps, vec![]);
    init_config(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);

    // from_liquidation=true bypasses cap checks
    let collateral = vec![make_casset_with_amount("uatom", 999999, 50, 60)];
    let full_position = collateral.clone();

    let res = update_basket_tally(
        &mut deps.as_mut(),
        env,
        collateral,
        full_position,
        true,
        true, // from_liquidation
    );
    assert!(res.is_ok());
}

// ========================================================================
// Group B: Single-asset supply cap enforcement (5 tests)
// ========================================================================

#[test]
fn deposit_over_cap_errors() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // uatom has 50% supply cap, uosmo has 50%
    let cassets = vec![
        make_casset_with_amount("uatom", 0, 50, 60),
        make_casset_with_amount("uosmo", 0, 40, 50),
    ];
    let caps = vec![
        make_supply_cap_with_ratio("uatom", 50),
        make_supply_cap_with_ratio("uosmo", 50),
    ];
    init_basket_custom(deps.as_mut().storage, cassets, caps, vec![]);
    init_config(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);

    // uatom price=10, uosmo price=5
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);
    init_stored_price(deps.as_mut().storage, "uosmo", Decimal::from_ratio(5u128, 1u128), 900);

    // Deposit 1000 uatom (value=10000) when uosmo supply=0
    // Ratio = 10000 / 10000 = 100% > 50% cap
    let collateral = vec![make_casset_with_amount("uatom", 1000, 50, 60)];
    let full_position = collateral.clone();

    let res = update_basket_tally(
        &mut deps.as_mut(),
        env,
        collateral,
        full_position,
        true,
        false,
    );
    assert!(res.is_err());
    assert!(format!("{:?}", res.unwrap_err()).contains("Supply cap ratio"));
}

#[test]
fn zero_cap_ratio_skipped() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // uatom has 0% cap (expunged), uosmo has 100%
    let cassets = vec![
        make_casset_with_amount("uatom", 0, 50, 60),
        make_casset_with_amount("uosmo", 500, 40, 50),
    ];
    let caps = vec![
        make_supply_cap_with_ratio("uatom", 0), // expunged
        make_supply_cap_with_supply("uosmo", 100, 500),
    ];
    init_basket_custom(deps.as_mut().storage, cassets, caps, vec![]);
    init_config(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);
    init_stored_price(deps.as_mut().storage, "uosmo", Decimal::from_ratio(5u128, 1u128), 900);

    // Deposit uatom - zero cap should be skipped
    let collateral = vec![make_casset_with_amount("uatom", 100, 50, 60)];
    let full_position = collateral.clone();

    let res = update_basket_tally(
        &mut deps.as_mut(),
        env,
        collateral,
        full_position,
        true,
        false,
    );
    assert!(res.is_ok());
}

#[test]
fn withdrawing_own_asset_never_blocks() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // Both at 50% cap, both have equal supply → both at 50% ratio
    let cassets = vec![
        make_casset_with_amount("uatom", 1000, 50, 60),
        make_casset_with_amount("uosmo", 1000, 40, 50),
    ];
    let caps = vec![
        make_supply_cap_with_supply("uatom", 50, 1000),
        make_supply_cap_with_supply("uosmo", 50, 1000),
    ];
    init_basket_custom(deps.as_mut().storage, cassets, caps, vec![]);
    init_config(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);
    init_stored_price(deps.as_mut().storage, "uosmo", Decimal::from_ratio(10u128, 1u128), 900);

    // Withdraw 500 uatom - position only has uatom (only asset)
    let collateral = vec![make_casset_with_amount("uatom", 500, 50, 60)];
    // full_positions_assets = remaining after withdraw - just the asset being withdrawn
    let full_position = vec![make_casset_with_amount("uatom", 500, 50, 60)];

    let res = update_basket_tally(
        &mut deps.as_mut(),
        env,
        collateral,
        full_position,
        false,
        false,
    );
    // This should succeed - withdrawing own asset is exempt when it's the only asset
    assert!(res.is_ok());
}

#[test]
fn withdraw_asset_a_when_b_over_cap_blocks() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    // uatom at 60% of value, uosmo at 40% - cap is 50% for both
    // After withdrawing uosmo, uatom ratio goes up past 50% cap
    let cassets = vec![
        make_casset_with_amount("uatom", 600, 50, 60),
        make_casset_with_amount("uosmo", 400, 40, 50),
    ];
    let caps = vec![
        make_supply_cap_with_supply("uatom", 50, 600),
        make_supply_cap_with_supply("uosmo", 50, 400),
    ];
    init_basket_custom(deps.as_mut().storage, cassets, caps, vec![]);
    init_config(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    // Equal prices so value = amount
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::one(), 900);
    init_stored_price(deps.as_mut().storage, "uosmo", Decimal::one(), 900);

    // Withdraw 200 uosmo - position has both assets
    let collateral = vec![make_casset_with_amount("uosmo", 200, 40, 50)];
    // Remaining position has both (multiple assets)
    let full_position = vec![
        make_casset_with_amount("uatom", 600, 50, 60),
        make_casset_with_amount("uosmo", 200, 40, 50),
    ];

    let res = update_basket_tally(
        &mut deps.as_mut(),
        env,
        collateral,
        full_position,
        false,
        false,
    );
    // After withdraw: uatom=600, uosmo=200, total=800
    // uatom ratio = 600/800 = 75% > 50% cap → should error
    assert!(res.is_err());
}

#[test]
fn at_cap_boundary_errors() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    let cassets = vec![
        make_casset_with_amount("uatom", 0, 50, 60),
        make_casset_with_amount("uosmo", 1, 40, 50),
    ];
    let caps = vec![
        make_supply_cap_with_ratio("uatom", 50),
        make_supply_cap_with_supply("uosmo", 100, 1),
    ];
    init_basket_custom(deps.as_mut().storage, cassets, caps, vec![]);
    init_config(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::one(), 900);
    init_stored_price(deps.as_mut().storage, "uosmo", Decimal::one(), 900);

    // Deposit 100 uatom → ratio = 100/101 ≈ 99% > 50%
    let collateral = vec![make_casset_with_amount("uatom", 100, 50, 60)];
    let full_position = collateral.clone();

    let res = update_basket_tally(
        &mut deps.as_mut(),
        env,
        collateral,
        full_position,
        true,
        false,
    );
    assert!(res.is_err());
}

// ========================================================================
// Group C: Multi-asset supply cap enforcement (4 tests)
// ========================================================================

#[test]
fn multi_asset_under_cap_passes() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    use membrane::types::MultiAssetSupplyCap;

    let cassets = vec![
        make_casset_with_amount("uatom", 0, 50, 60),
        make_casset_with_amount("uosmo", 0, 40, 50),
        make_casset_with_amount("ustars", 1000, 30, 40),
    ];
    let caps = vec![
        make_supply_cap("uatom"),
        make_supply_cap("uosmo"),
        make_supply_cap_with_supply("ustars", 100, 1000),
    ];
    let multi_caps = vec![MultiAssetSupplyCap {
        assets: vec![
            AssetInfo::NativeToken { denom: "uatom".to_string() },
            AssetInfo::NativeToken { denom: "uosmo".to_string() },
        ],
        supply_cap_ratio: Decimal::percent(80), // combined cap 80%
    }];
    init_basket_custom(deps.as_mut().storage, cassets, caps, multi_caps);
    init_config(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::one(), 900);
    init_stored_price(deps.as_mut().storage, "uosmo", Decimal::one(), 900);
    init_stored_price(deps.as_mut().storage, "ustars", Decimal::one(), 900);

    // Deposit 100 uatom: total = 100+0+1000=1100, combined ratio = 100/1100 ≈ 9% < 80%
    let collateral = vec![make_casset_with_amount("uatom", 100, 50, 60)];
    let full_position = collateral.clone();

    let res = update_basket_tally(
        &mut deps.as_mut(),
        env,
        collateral,
        full_position,
        true,
        false,
    );
    assert!(res.is_ok());
}

#[test]
fn multi_asset_over_cap_errors() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    use membrane::types::MultiAssetSupplyCap;

    let cassets = vec![
        make_casset_with_amount("uatom", 0, 50, 60),
        make_casset_with_amount("uosmo", 0, 40, 50),
        make_casset_with_amount("ustars", 100, 30, 40),
    ];
    let caps = vec![
        make_supply_cap("uatom"),
        make_supply_cap("uosmo"),
        make_supply_cap_with_supply("ustars", 100, 100),
    ];
    let multi_caps = vec![MultiAssetSupplyCap {
        assets: vec![
            AssetInfo::NativeToken { denom: "uatom".to_string() },
            AssetInfo::NativeToken { denom: "uosmo".to_string() },
        ],
        supply_cap_ratio: Decimal::percent(50), // combined cap 50%
    }];
    init_basket_custom(deps.as_mut().storage, cassets, caps, multi_caps);
    init_config(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::one(), 900);
    init_stored_price(deps.as_mut().storage, "uosmo", Decimal::one(), 900);
    init_stored_price(deps.as_mut().storage, "ustars", Decimal::one(), 900);

    // Deposit 900 uatom + 100 uosmo: total = 900+100+100=1100
    // Combined ratio = (900+100)/1100 ≈ 90.9% > 50%
    let collateral = vec![
        make_casset_with_amount("uatom", 900, 50, 60),
        make_casset_with_amount("uosmo", 100, 40, 50),
    ];
    let full_position = collateral.clone();

    let res = update_basket_tally(
        &mut deps.as_mut(),
        env,
        collateral,
        full_position,
        true,
        false,
    );
    assert!(res.is_err());
    assert!(format!("{:?}", res.unwrap_err()).contains("Multi-Asset supply cap"));
}

#[test]
fn multi_asset_withdraw_exemption() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    use membrane::types::MultiAssetSupplyCap;

    let cassets = vec![
        make_casset_with_amount("uatom", 500, 50, 60),
        make_casset_with_amount("uosmo", 500, 40, 50),
    ];
    let caps = vec![
        make_supply_cap_with_supply("uatom", 100, 500),
        make_supply_cap_with_supply("uosmo", 100, 500),
    ];
    let multi_caps = vec![MultiAssetSupplyCap {
        assets: vec![
            AssetInfo::NativeToken { denom: "uatom".to_string() },
            AssetInfo::NativeToken { denom: "uosmo".to_string() },
        ],
        supply_cap_ratio: Decimal::percent(50),
    }];
    init_basket_custom(deps.as_mut().storage, cassets, caps, multi_caps);
    init_config(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::one(), 900);
    init_stored_price(deps.as_mut().storage, "uosmo", Decimal::one(), 900);

    // Withdraw uatom (only asset in position after full withdrawal)
    let collateral = vec![make_casset_with_amount("uatom", 500, 50, 60)];
    // full_positions_assets: the position only has uatom (just this one asset, len=1)
    let full_position = vec![make_casset_with_amount("uatom", 0, 50, 60)];

    let res = update_basket_tally(
        &mut deps.as_mut(),
        env,
        collateral,
        full_position,
        false,
        false,
    );
    // Should succeed due to withdrawal exemption
    assert!(res.is_ok());
}

#[test]
fn empty_multi_caps_no_check() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_basket(deps.as_mut().storage); // default has empty multi_asset_supply_caps
    init_config(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);
    init_stored_price(deps.as_mut().storage, "uosmo", Decimal::from_ratio(5u128, 1u128), 900);

    let collateral = vec![make_casset_with_amount("uatom", 100, 50, 60)];
    let full_position = collateral.clone();

    let res = update_basket_tally(
        &mut deps.as_mut(),
        env,
        collateral,
        full_position,
        true,
        false,
    );
    assert!(res.is_ok());
}

// ========================================================================
// Group D: Volatility transform (3 tests)
// ========================================================================

#[test]
fn volatility_applied_when_10_entries() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    let cassets = vec![
        make_casset_with_amount("uatom", 0, 50, 60),
        make_casset_with_amount("uosmo", 1000, 40, 50),
    ];
    let caps = vec![
        make_supply_cap_with_ratio("uatom", 80), // 80% cap
        make_supply_cap_with_supply("uosmo", 100, 1000),
    ];
    init_basket_custom(deps.as_mut().storage, cassets, caps, vec![]);
    init_config(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::one(), 900);
    init_stored_price(deps.as_mut().storage, "uosmo", Decimal::one(), 900);

    // 10 volatility entries with index = 0.5 → effective cap = 80% * 0.5 = 40%
    let vol_list: Vec<Decimal> = vec![Decimal::percent(1); 10];
    init_volatility(
        deps.as_mut().storage,
        "uatom",
        Decimal::from_ratio(5u128, 10u128), // 0.5
        vol_list.clone(),
        vol_list,
    );

    // Deposit 500 uatom → ratio = 500/1500 ≈ 33.3% < 40% transformed cap → ok
    let collateral = vec![make_casset_with_amount("uatom", 500, 50, 60)];
    let full_position = collateral.clone();

    let res = update_basket_tally(
        &mut deps.as_mut(),
        env,
        collateral,
        full_position,
        true,
        false,
    );
    assert!(res.is_ok());
}

#[test]
fn volatility_skipped_when_less_than_10() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    let cassets = vec![
        make_casset_with_amount("uatom", 0, 50, 60),
        make_casset_with_amount("uosmo", 1000, 40, 50),
    ];
    let caps = vec![
        make_supply_cap_with_ratio("uatom", 80),
        make_supply_cap_with_supply("uosmo", 100, 1000),
    ];
    init_basket_custom(deps.as_mut().storage, cassets, caps, vec![]);
    init_config(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::one(), 900);
    init_stored_price(deps.as_mut().storage, "uosmo", Decimal::one(), 900);

    // Only 5 entries → volatility transform NOT applied, cap stays 80%
    let vol_list: Vec<Decimal> = vec![Decimal::percent(1); 5];
    init_volatility(
        deps.as_mut().storage,
        "uatom",
        Decimal::from_ratio(1u128, 10u128), // 0.1 (very low, would reduce cap if applied)
        vol_list.clone(),
        vol_list,
    );

    // Deposit 700 uatom → ratio = 700/1700 ≈ 41.2% < 80% untransformed cap → ok
    let collateral = vec![make_casset_with_amount("uatom", 700, 50, 60)];
    let full_position = collateral.clone();

    let res = update_basket_tally(
        &mut deps.as_mut(),
        env,
        collateral,
        full_position,
        true,
        false,
    );
    assert!(res.is_ok());
}

#[test]
fn transformed_ratio_capped_at_one() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    let cassets = vec![
        make_casset_with_amount("uatom", 0, 50, 60),
        make_casset_with_amount("uosmo", 100, 40, 50),
    ];
    let caps = vec![
        make_supply_cap_with_ratio("uatom", 90), // 90% cap
        make_supply_cap_with_supply("uosmo", 100, 100),
    ];
    init_basket_custom(deps.as_mut().storage, cassets, caps, vec![]);
    init_config(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::one(), 900);
    init_stored_price(deps.as_mut().storage, "uosmo", Decimal::one(), 900);

    // High volatility index = 2.0 → 90% * 2.0 = 180%, capped at 100%
    let vol_list: Vec<Decimal> = vec![Decimal::percent(1); 10];
    init_volatility(
        deps.as_mut().storage,
        "uatom",
        Decimal::from_ratio(2u128, 1u128), // 2.0
        vol_list.clone(),
        vol_list,
    );

    // Deposit 50 uatom → ratio = 50/150 ≈ 33.3% < 100% capped transformed → ok
    let collateral = vec![make_casset_with_amount("uatom", 50, 50, 60)];
    let full_position = collateral.clone();

    let res = update_basket_tally(
        &mut deps.as_mut(),
        env,
        collateral,
        full_position,
        true,
        false,
    );
    assert!(res.is_ok());
}
