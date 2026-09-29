use crate::contract::{execute, instantiate, query};

use membrane::liq_queue::{
    CollateralForDebtResponse, ExecuteMsg, InstantiateMsg, LiquidatibleResponse, QueryMsg,
};
use membrane::math::Uint256;
use membrane::oracle::PriceResponse;
use membrane::types::{AssetInfo, BidInput};

use cosmwasm_std::testing::{mock_dependencies, mock_env, message_info};
use cosmwasm_std::{from_binary, Coin, Decimal, Uint128};

/// Helper function to setup a basic queue with bids
fn setup_queue_with_bids(
    deps: &mut cosmwasm_std::OwnedDeps<
        cosmwasm_std::MemoryStorage,
        cosmwasm_std::testing::MockApi,
        cosmwasm_std::testing::MockQuerier,
    >,
    bids: Vec<(u8, Uint128)>, // (premium, amount) pairs
) {
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
        bid_threshold: Uint256::from(1_000_000_000u128),
    };
    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    // Submit bids
    for (premium, amount) in bids {
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
                amount,
            }],
        );
        execute(deps.as_mut(), mock_env(), submit_info, msg).unwrap();
    }
}

#[test]
fn test_collateral_for_debt_basic_no_premium() {
    let mut deps = mock_dependencies();
    setup_queue_with_bids(&mut deps, vec![(0, Uint128::from(1000u128))]);

    // Query: Need to repay 500 debt
    let msg = QueryMsg::CollateralForDebt {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
        debt_amount: Uint256::from(500u128),
        credit_info: AssetInfo::NativeToken {
            denom: "cdt".to_string(),
        },
        credit_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
    };

    let res = query(deps.as_ref(), mock_env(), msg).unwrap();
    let resp: CollateralForDebtResponse = from_binary(&res).unwrap();

    // With 0% premium and 1:1 prices, collateral needed should be at least debt (may be slightly more due to rounding)
    let collateral: u128 = resp.collateral_needed.parse().unwrap();
    assert!(collateral >= 500 && collateral <= 501, "Collateral should be 500-501, got {}", collateral);
    assert_eq!(resp.leftover_debt, String::from("0"));
}

#[test]
fn test_collateral_for_debt_with_10_percent_premium() {
    let mut deps = mock_dependencies();
    setup_queue_with_bids(&mut deps, vec![(10, Uint128::from(1000u128))]);

    // Query: Need to repay 900 debt with 10% premium
    // collateral_needed = 900 / (1 - 0.10) = 900 / 0.9 = 1000
    let msg = QueryMsg::CollateralForDebt {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
        debt_amount: Uint256::from(900u128),
        credit_info: AssetInfo::NativeToken {
            denom: "cdt".to_string(),
        },
        credit_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
    };

    let res = query(deps.as_ref(), mock_env(), msg).unwrap();
    let resp: CollateralForDebtResponse = from_binary(&res).unwrap();

    // With 10% premium, need ~1000 collateral to repay 900 debt (may be slightly more due to rounding)
    let collateral: u128 = resp.collateral_needed.parse().unwrap();
    assert!(collateral >= 1000 && collateral <= 1001, "Collateral should be 1000-1001, got {}", collateral);
    assert_eq!(resp.leftover_debt, String::from("0"));
}

#[test]
fn test_collateral_for_debt_insufficient_liquidity() {
    let mut deps = mock_dependencies();
    setup_queue_with_bids(&mut deps, vec![(0, Uint128::from(500u128))]);

    // Query: Need to repay 1000 debt but only 500 available
    let msg = QueryMsg::CollateralForDebt {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
        debt_amount: Uint256::from(1000u128),
        credit_info: AssetInfo::NativeToken {
            denom: "cdt".to_string(),
        },
        credit_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
    };

    let res = query(deps.as_ref(), mock_env(), msg).unwrap();
    let resp: CollateralForDebtResponse = from_binary(&res).unwrap();

    // Should use all available liquidity (may be slightly more due to rounding)
    let collateral: u128 = resp.collateral_needed.parse().unwrap();
    assert!(collateral >= 500 && collateral <= 501, "Collateral should be 500-501, got {}", collateral);
    // Should have 500 leftover debt
    assert_eq!(resp.leftover_debt, String::from("500"));
}

#[test]
fn test_collateral_for_debt_multiple_slots() {
    let mut deps = mock_dependencies();
    // 0% premium: 1000 available
    // 10% premium: 1000 available
    setup_queue_with_bids(
        &mut deps,
        vec![
            (0, Uint128::from(1000u128)),
            (10, Uint128::from(1000u128)),
        ],
    );

    // Query: Need to repay 1800 debt
    // First slot (0%): uses 1000 debt, needs 1000 collateral
    // Second slot (10%): uses 800 debt, needs 800/0.9 = 888.888... ≈ 889 collateral
    // Total: 1889 collateral
    let msg = QueryMsg::CollateralForDebt {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
        debt_amount: Uint256::from(1800u128),
        credit_info: AssetInfo::NativeToken {
            denom: "cdt".to_string(),
        },
        credit_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
    };

    let res = query(deps.as_ref(), mock_env(), msg).unwrap();
    let resp: CollateralForDebtResponse = from_binary(&res).unwrap();

    // Should use both slots
    let collateral_needed: u128 = resp.collateral_needed.parse().unwrap();
    // Allow for rounding: should be around 1888-1890 (with +1 buffer per slot used)
    assert!(collateral_needed >= 1888 && collateral_needed <= 1890, "Expected 1888-1890, got {}", collateral_needed);
    assert_eq!(resp.leftover_debt, String::from("0"));
}

#[test]
fn test_collateral_for_debt_vs_check_liquidatible_no_premium() {
    let mut deps = mock_dependencies();
    setup_queue_with_bids(&mut deps, vec![(0, Uint128::from(10000u128))]);

    let debt_amount = 5000u128;

    // Query CollateralForDebt
    let cfd_msg = QueryMsg::CollateralForDebt {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
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

    // Query CheckLiquidatible with the collateral amount we got
    let cl_msg = QueryMsg::CheckLiquidatible {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
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

    // CRITICAL ASSERTION: The debt repaid should be >= the debt we wanted to repay
    assert!(
        debt_repaid >= debt_amount,
        "CollateralForDebt returned insufficient collateral! \
         Wanted to repay: {}, Actually repaid: {}, Collateral used: {}",
        debt_amount,
        debt_repaid,
        collateral_needed
    );

    // Should be nearly equal (allowing for rounding)
    assert!(
        debt_repaid <= debt_amount + 1,
        "Excessive collateral: wanted {}, got {}",
        debt_amount,
        debt_repaid
    );
}

#[test]
fn test_collateral_for_debt_vs_check_liquidatible_with_premium() {
    let mut deps = mock_dependencies();
    setup_queue_with_bids(&mut deps, vec![(5, Uint128::from(10000u128))]);

    let debt_amount = 5000u128;

    // Query CollateralForDebt
    let cfd_msg = QueryMsg::CollateralForDebt {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
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

    // Query CheckLiquidatible with the collateral amount we got
    let cl_msg = QueryMsg::CheckLiquidatible {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
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

    // CRITICAL ASSERTION: The debt repaid should be >= the debt we wanted to repay
    assert!(
        debt_repaid >= debt_amount,
        "CollateralForDebt returned insufficient collateral with 5% premium! \
         Wanted to repay: {}, Actually repaid: {}, Collateral used: {}",
        debt_amount,
        debt_repaid,
        collateral_needed
    );

    // Should be nearly equal (allowing for rounding)
    assert!(
        debt_repaid <= debt_amount + 2,
        "Excessive collateral: wanted {}, got {}",
        debt_amount,
        debt_repaid
    );
}

#[test]
fn test_collateral_for_debt_vs_check_liquidatible_multiple_slots() {
    let mut deps = mock_dependencies();
    setup_queue_with_bids(
        &mut deps,
        vec![
            (0, Uint128::from(3000u128)),
            (5, Uint128::from(3000u128)),
            (10, Uint128::from(3000u128)),
        ],
    );

    let debt_amount = 7500u128;

    // Query CollateralForDebt
    let cfd_msg = QueryMsg::CollateralForDebt {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
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

    // Query CheckLiquidatible with the collateral amount we got
    let cl_msg = QueryMsg::CheckLiquidatible {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
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

    // CRITICAL ASSERTION: The debt repaid should be >= the debt we wanted to repay
    assert!(
        debt_repaid >= debt_amount,
        "CollateralForDebt returned insufficient collateral across multiple slots! \
         Wanted to repay: {}, Actually repaid: {}, Collateral used: {}",
        debt_amount,
        debt_repaid,
        collateral_needed
    );

    // Should be nearly equal (allowing for rounding)
    assert!(
        debt_repaid <= debt_amount + 3,
        "Excessive collateral: wanted {}, got {}",
        debt_amount,
        debt_repaid
    );
}

#[test]
fn test_collateral_for_debt_with_different_prices() {
    let mut deps = mock_dependencies();
    setup_queue_with_bids(&mut deps, vec![(5, Uint128::from(10000u128))]);

    // Collateral is worth 2x credit
    // Need to repay 1000 credit
    // With 5% premium: collateral_needed = 1000 * 1.0 / (2.0 * 0.95) = 1000 / 1.9 ≈ 526
    let debt_amount = 1000u128;

    let cfd_msg = QueryMsg::CollateralForDebt {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::percent(200), // 2.0
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

    // Verify with CheckLiquidatible
    let cl_msg = QueryMsg::CheckLiquidatible {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::percent(200),
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

    // CRITICAL ASSERTION
    assert!(
        debt_repaid >= debt_amount,
        "CollateralForDebt returned insufficient collateral with different prices! \
         Wanted to repay: {}, Actually repaid: {}, Collateral used: {}",
        debt_amount,
        debt_repaid,
        collateral_needed
    );
}

#[test]
fn test_collateral_for_debt_large_numbers() {
    let mut deps = mock_dependencies();
    setup_queue_with_bids(
        &mut deps,
        vec![
            (0, Uint128::from(1_000_000_000u128)),
            (10, Uint128::from(1_000_000_000u128)),
        ],
    );

    let debt_amount = 1_500_000_000u128;

    let cfd_msg = QueryMsg::CollateralForDebt {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::percent(200),
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

    // Verify with CheckLiquidatible
    let cl_msg = QueryMsg::CheckLiquidatible {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::percent(200),
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

    // CRITICAL ASSERTION for large numbers
    assert!(
        debt_repaid >= debt_amount,
        "CollateralForDebt returned insufficient collateral with large numbers! \
         Wanted to repay: {}, Actually repaid: {}, Collateral used: {}",
        debt_amount,
        debt_repaid,
        collateral_needed
    );

    // Allow slightly more tolerance for rounding with large numbers
    let tolerance = debt_amount / 100000; // 0.001% tolerance
    assert!(
        debt_repaid <= debt_amount + tolerance,
        "Excessive collateral: wanted {}, got {}, tolerance: {}",
        debt_amount,
        debt_repaid,
        tolerance
    );
}

#[test]
fn test_collateral_for_debt_edge_case_empty_queue() {
    let mut deps = mock_dependencies();
    // Setup queue but don't add any bids
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
        bid_threshold: Uint256::from(1_000_000_000u128),
    };
    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    // Try to repay debt with empty queue
    let cfd_msg = QueryMsg::CollateralForDebt {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
        debt_amount: Uint256::from(1000u128),
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

    // Should need 0 collateral and have all debt leftover
    assert_eq!(cfd_resp.collateral_needed, String::from("0"));
    assert_eq!(cfd_resp.leftover_debt, String::from("1000"));
}
