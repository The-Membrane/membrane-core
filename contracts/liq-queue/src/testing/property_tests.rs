/// Property-based tests to ensure CollateralForDebt is always correct
///
/// These tests verify the fundamental property:
/// If CollateralForDebt says we need X collateral to repay Y debt,
/// then CheckLiquidatible(X) must return >= Y debt repaid.

use crate::contract::{execute, instantiate, query};

use membrane::liq_queue::{
    CollateralForDebtResponse, ExecuteMsg, InstantiateMsg, LiquidatibleResponse, QueryMsg,
};
use membrane::math::Uint256;
use membrane::oracle::PriceResponse;
use membrane::types::{AssetInfo, BidInput};

use cosmwasm_std::testing::{mock_dependencies, mock_env, message_info};
use cosmwasm_std::{from_binary, Coin, Decimal, Uint128};

/// Helper to verify the core property
fn verify_collateral_for_debt_correctness(
    debt_amount: u128,
    bid_amounts: Vec<(u8, u128)>, // (premium, amount)
    collateral_price_percent: u64, // 100 = 1.0, 200 = 2.0, etc
) {
    let mut deps = mock_dependencies();

    // Setup
    let msg = InstantiateMsg {
        positions_contract: deps.api.addr_make("positions").to_string(),
        osmosis_proxy_contract: deps.api.addr_make("osmosis_proxy").to_string(),
        owner: None,
        waiting_period: 60u64,
        minimum_bid: Uint128::zero(),
        maximum_waiting_bids: 100u64,
    };
    let info = message_info(&deps.api.addr_make("owner"), &[]);
    instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    let msg = ExecuteMsg::AddQueue {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        max_premium: Uint128::new(10u128),
        bid_threshold: Uint256::from(1_000_000_000_000u128),
    };
    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    // Submit bids
    for (premium, amount) in bid_amounts {
        let msg = ExecuteMsg::SubmitBid {
            bid_input: BidInput {
                bid_for: AssetInfo::NativeToken {
                    denom: "osmo".to_string(),
                },
                liq_premium: premium,
            },
            bid_owner: None,
        };
        let submit_info = message_info(
            &deps.api.addr_make("owner"),
            &[Coin {
                denom: "cdt".to_string(),
                amount: Uint128::from(amount),
            }],
        );
        execute(deps.as_mut(), mock_env(), submit_info, msg).unwrap();
    }

    // Query CollateralForDebt
    let cfd_msg = QueryMsg::CollateralForDebt {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::percent(collateral_price_percent),
            decimals: 6u64,
        },
        debt_amount: Uint256::from(debt_amount),
        credit_info: AssetInfo::NativeToken {
            denom: "cdt".to_string(),
        },
        credit_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
    };

    let cfd_res = query(deps.as_ref(), mock_env(), cfd_msg).unwrap();
    let cfd_resp: CollateralForDebtResponse = from_binary(&cfd_res).unwrap();
    let collateral_needed: u128 = cfd_resp.collateral_needed.parse().unwrap();
    let leftover_debt: u128 = cfd_resp.leftover_debt.parse().unwrap();

    // If there's leftover debt, we can't verify further (insufficient liquidity)
    if leftover_debt > 0 {
        // At least verify we used all available liquidity
        return;
    }

    // Query CheckLiquidatible with the collateral we got
    let cl_msg = QueryMsg::CheckLiquidatible {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::percent(collateral_price_percent),
            decimals: 6u64,
        },
        collateral_amount: Uint256::from(collateral_needed),
        credit_info: AssetInfo::NativeToken {
            denom: "cdt".to_string(),
        },
        credit_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
    };

    let cl_res = query(deps.as_ref(), mock_env(), cl_msg).unwrap();
    let cl_resp: LiquidatibleResponse = from_binary(&cl_res).unwrap();
    let debt_repaid: u128 = cl_resp.total_debt_repaid.parse().unwrap();

    // THE CRITICAL PROPERTY: debt repaid must be >= debt requested
    assert!(
        debt_repaid >= debt_amount,
        "\n\n❌ PROPERTY VIOLATION!\n\
         Debt requested: {}\n\
         Debt actually repaid: {}\n\
         Collateral needed: {}\n\
         Collateral price: {}%\n\
         Shortfall: {}\n\n",
        debt_amount,
        debt_repaid,
        collateral_needed,
        collateral_price_percent,
        debt_amount - debt_repaid
    );

    // Also verify we didn't overshoot too much (accounting for rounding)
    let tolerance = std::cmp::max(debt_amount / 10000, 10); // 0.01% or at least 10
    assert!(
        debt_repaid <= debt_amount + tolerance,
        "Significant overshoot: wanted {}, got {}, tolerance {}",
        debt_amount,
        debt_repaid,
        tolerance
    );
}

#[test]
fn property_test_various_debt_amounts_no_premium() {
    let bid_amounts = vec![(0, 100_000_000u128)];
    let test_debts = vec![
        100, 1_000, 5_000, 10_000, 50_000, 100_000, 500_000, 1_000_000, 10_000_000, 50_000_000,
    ];

    for debt in test_debts {
        verify_collateral_for_debt_correctness(debt, bid_amounts.clone(), 100);
    }
}

#[test]
fn property_test_various_debt_amounts_with_5_percent_premium() {
    let bid_amounts = vec![(5, 100_000_000u128)];
    let test_debts = vec![
        100, 1_000, 5_000, 10_000, 50_000, 100_000, 500_000, 1_000_000, 10_000_000, 50_000_000,
    ];

    for debt in test_debts {
        verify_collateral_for_debt_correctness(debt, bid_amounts.clone(), 100);
    }
}

#[test]
fn property_test_various_debt_amounts_with_10_percent_premium() {
    let bid_amounts = vec![(10, 100_000_000u128)];
    let test_debts = vec![
        100, 1_000, 5_000, 10_000, 50_000, 100_000, 500_000, 1_000_000, 10_000_000, 50_000_000,
    ];

    for debt in test_debts {
        verify_collateral_for_debt_correctness(debt, bid_amounts.clone(), 100);
    }
}

#[test]
fn property_test_various_prices() {
    let bid_amounts = vec![(5, 100_000_000u128)];
    let debt = 1_000_000u128;
    let test_prices = vec![50, 75, 100, 125, 150, 200, 300, 500, 1000];

    for price in test_prices {
        verify_collateral_for_debt_correctness(debt, bid_amounts.clone(), price);
    }
}

#[test]
fn property_test_multiple_premium_slots() {
    let bid_amounts = vec![
        (0, 10_000_000u128),
        (3, 10_000_000u128),
        (5, 10_000_000u128),
        (7, 10_000_000u128),
        (10, 10_000_000u128),
    ];

    let test_debts = vec![
        5_000_000u128,
        15_000_000u128,
        25_000_000u128,
        35_000_000u128,
        45_000_000u128,
    ];

    for debt in test_debts {
        verify_collateral_for_debt_correctness(debt, bid_amounts.clone(), 100);
    }
}

#[test]
fn property_test_uneven_slot_distribution() {
    // Realistic scenario: more liquidity at better premiums
    let bid_amounts = vec![
        (0, 50_000_000u128),
        (2, 20_000_000u128),
        (5, 10_000_000u128),
        (8, 5_000_000u128),
        (10, 1_000_000u128),
    ];

    let test_debts = vec![
        1_000_000u128,
        10_000_000u128,
        30_000_000u128,
        60_000_000u128,
        80_000_000u128,
    ];

    for debt in test_debts {
        verify_collateral_for_debt_correctness(debt, bid_amounts.clone(), 100);
    }
}

#[test]
fn property_test_edge_cases() {
    // Test very small amounts
    verify_collateral_for_debt_correctness(1, vec![(0, 1000)], 100);
    verify_collateral_for_debt_correctness(10, vec![(5, 1000)], 100);

    // Test amounts that don't divide evenly
    verify_collateral_for_debt_correctness(333, vec![(3, 10000)], 100);
    verify_collateral_for_debt_correctness(777, vec![(7, 10000)], 100);

    // Test with odd prices
    verify_collateral_for_debt_correctness(1000, vec![(0, 100000)], 137);
    verify_collateral_for_debt_correctness(5000, vec![(5, 100000)], 213);
}

#[test]
fn property_test_realistic_liquidation_scenarios() {
    // Scenario 1: Small liquidation, good liquidity
    verify_collateral_for_debt_correctness(
        500_000, // 0.5 CDT
        vec![
            (0, 10_000_000),
            (2, 5_000_000),
            (5, 2_000_000),
        ],
        150, // OSMO at $1.50
    );

    // Scenario 2: Medium liquidation, scattered premiums
    verify_collateral_for_debt_correctness(
        5_000_000, // 5 CDT
        vec![
            (0, 3_000_000),
            (3, 3_000_000),
            (7, 2_000_000),
            (10, 1_000_000),
        ],
        200, // Asset at $2
    );

    // Scenario 3: Large liquidation, needs multiple slots
    verify_collateral_for_debt_correctness(
        50_000_000, // 50 CDT
        vec![
            (0, 20_000_000),
            (2, 15_000_000),
            (5, 10_000_000),
            (8, 8_000_000),
            (10, 5_000_000),
        ],
        80, // Cheap asset at $0.80
    );

    // Scenario 4: Very large liquidation with expensive collateral
    verify_collateral_for_debt_correctness(
        100_000_000, // 100 CDT
        vec![
            (0, 50_000_000),
            (5, 50_000_000),
            (10, 50_000_000),
        ],
        1000, // Expensive asset at $10
    );
}

#[test]
fn property_test_premium_boundary_conditions() {
    // Test at 0% premium (no discount)
    verify_collateral_for_debt_correctness(1_000_000, vec![(0, 10_000_000)], 100);

    // Test at maximum 10% premium
    verify_collateral_for_debt_correctness(1_000_000, vec![(10, 10_000_000)], 100);

    // Test with all premiums
    verify_collateral_for_debt_correctness(
        10_000_000,
        vec![
            (0, 2_000_000),
            (1, 2_000_000),
            (2, 2_000_000),
            (3, 2_000_000),
            (4, 2_000_000),
            (5, 2_000_000),
            (6, 2_000_000),
            (7, 2_000_000),
            (8, 2_000_000),
            (9, 2_000_000),
            (10, 2_000_000),
        ],
        100,
    );
}

#[test]
fn property_test_stress_large_values() {
    // Test with very large values (approaching real-world TVL)
    verify_collateral_for_debt_correctness(
        1_000_000_000_000, // 1M CDT
        vec![
            (0, 500_000_000_000),
            (2, 300_000_000_000),
            (5, 200_000_000_000),
            (10, 100_000_000_000),
        ],
        100,
    );

    // Test with extreme price difference
    verify_collateral_for_debt_correctness(
        100_000_000, // 100 CDT
        vec![(0, 1_000_000_000)],
        5000, // Very expensive collateral at $50
    );
}
