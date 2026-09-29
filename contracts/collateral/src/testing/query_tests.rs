#![allow(non_snake_case)]

use cosmwasm_std::testing::mock_dependencies;
use cosmwasm_std::{from_json, Addr, Decimal, Uint128};

use membrane::collateral::PositionValueResponse;

use crate::query::query_position_value;
use crate::testing::test_helpers::helpers::*;

// ========================================================================
// Group C: query_position_value (2 tests)
// ========================================================================

#[test]
fn position_value_no_stored_prices() {
    let mut deps = mock_dependencies();
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner,
        Uint128::one(),
        vec![make_casset_with_amount("uatom", 1000, 50, 60)],
    );

    let res = query_position_value(deps.as_ref(), owner_str.to_string(), Uint128::one()).unwrap();
    let value: PositionValueResponse = from_json(res).unwrap();
    assert_eq!(value.total_value, Decimal::zero());
    assert_eq!(value.avg_max_borrow_ltv, Decimal::zero());
    assert_eq!(value.avg_max_ltv, Decimal::zero());
}

#[test]
fn position_value_multi_asset() {
    let mut deps = mock_dependencies();
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);

    // Seed prices
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);
    init_stored_price(deps.as_mut().storage, "uosmo", Decimal::from_ratio(5u128, 1u128), 900);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner,
        Uint128::one(),
        vec![
            make_casset_with_amount("uatom", 100, 50, 60),
            make_casset_with_amount("uosmo", 200, 40, 50),
        ],
    );

    let res = query_position_value(deps.as_ref(), owner_str.to_string(), Uint128::one()).unwrap();
    let value: PositionValueResponse = from_json(res).unwrap();

    // uatom value = 100 * 10 = 1000, uosmo value = 200 * 5 = 1000
    // total = 2000
    assert_eq!(value.total_value, Decimal::from_ratio(2000u128, 1u128));

    // Weighted avg LTVs: both 50% weight
    // avg_max_ltv = 0.5 * 60% + 0.5 * 50% = 55%
    // avg_max_borrow_ltv = 0.5 * 50% + 0.5 * 40% = 45%
    assert_eq!(value.avg_max_ltv, Decimal::percent(55));
    assert_eq!(value.avg_max_borrow_ltv, Decimal::percent(45));

    // Both assets use default 15% threshold -> avg = 15%
    assert_eq!(value.avg_max_threshold_to_delay, Decimal::percent(15));
}

// ========================================================================
// Group D: avg_max_threshold_to_delay weighted average
// ========================================================================

/// Single collateral: avg_max_threshold_to_delay equals that asset's threshold.
#[test]
fn position_value_single_collateral_threshold() {
    let mut deps = mock_dependencies();
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket_custom(
        deps.as_mut().storage,
        vec![make_casset_with_threshold("uatom", 0, 50, 60, 20)],
        vec![make_supply_cap("uatom")],
        vec![],
    );

    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner,
        Uint128::one(),
        vec![make_casset_with_threshold("uatom", 100, 50, 60, 20)],
    );

    let res = query_position_value(deps.as_ref(), owner_str.to_string(), Uint128::one()).unwrap();
    let value: PositionValueResponse = from_json(res).unwrap();

    assert_eq!(value.avg_max_threshold_to_delay, Decimal::percent(20));
}

/// Multi-collateral with different thresholds: weighted average is correct.
#[test]
fn position_value_multi_collateral_weighted_threshold() {
    let mut deps = mock_dependencies();
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket_custom(
        deps.as_mut().storage,
        vec![
            make_casset_with_threshold("uatom", 0, 50, 60, 10),
            make_casset_with_threshold("uosmo", 0, 40, 50, 30),
        ],
        vec![make_supply_cap("uatom"), make_supply_cap("uosmo")],
        vec![],
    );

    // uatom: 100 * 10 = $1000, uosmo: 200 * 5 = $1000 -> equal weights
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);
    init_stored_price(deps.as_mut().storage, "uosmo", Decimal::from_ratio(5u128, 1u128), 900);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner,
        Uint128::one(),
        vec![
            make_casset_with_threshold("uatom", 100, 50, 60, 10),
            make_casset_with_threshold("uosmo", 200, 40, 50, 30),
        ],
    );

    let res = query_position_value(deps.as_ref(), owner_str.to_string(), Uint128::one()).unwrap();
    let value: PositionValueResponse = from_json(res).unwrap();

    // 50% * 10% + 50% * 30% = 20%
    assert_eq!(value.avg_max_threshold_to_delay, Decimal::percent(20));
}

/// Multi-collateral with unequal weights: weighted average reflects ratio.
#[test]
fn position_value_unequal_weights_threshold() {
    let mut deps = mock_dependencies();
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket_custom(
        deps.as_mut().storage,
        vec![
            make_casset_with_threshold("uatom", 0, 50, 60, 10),
            make_casset_with_threshold("uosmo", 0, 40, 50, 30),
        ],
        vec![make_supply_cap("uatom"), make_supply_cap("uosmo")],
        vec![],
    );

    // uatom: 300 * 10 = $3000 (75%), uosmo: 200 * 5 = $1000 (25%)
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);
    init_stored_price(deps.as_mut().storage, "uosmo", Decimal::from_ratio(5u128, 1u128), 900);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner,
        Uint128::one(),
        vec![
            make_casset_with_threshold("uatom", 300, 50, 60, 10),
            make_casset_with_threshold("uosmo", 200, 40, 50, 30),
        ],
    );

    let res = query_position_value(deps.as_ref(), owner_str.to_string(), Uint128::one()).unwrap();
    let value: PositionValueResponse = from_json(res).unwrap();

    // 75% * 10% + 25% * 30% = 7.5% + 7.5% = 15%
    assert_eq!(value.avg_max_threshold_to_delay, Decimal::percent(15));
}
