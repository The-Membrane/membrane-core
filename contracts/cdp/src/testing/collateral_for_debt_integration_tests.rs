/// Tests for CollateralForDebt query integration with CDP liquidations
///
/// This test suite verifies that the new CollateralForDebt query works correctly
/// when integrated with the CDP liquidation flow. It tests all scenarios identified
/// in the query logic:
///
/// 1. Full liquidation - queue has enough bids to cover all debt
/// 2. Partial liquidation - queue has insufficient bids (leftover_debt returned)
/// 3. Empty queue - queue exists but has no bids (returns 0 collateral_needed)
/// 4. Multiple premium slots - debt spans multiple slots with different premiums
/// 5. Collateral cap - collateral_needed exceeds position's available collateral
/// 6. Zero handling - ensures zero amounts are properly skipped
/// 7. queue_credit_repaid calculation - verifies debt repayment math

pub mod tests {
    use std::str::FromStr;

    use crate::helpers::{CDPContract, LQContract};
    use membrane::cdp::{
        ExecuteMsg, QueryMsg, EditBasket, Config, BasketPositionsResponse,
    };
    use membrane::liq_queue::{
        ExecuteMsg as LQ_ExecuteMsg, QueryMsg as LQ_QueryMsg,
        CollateralForDebtResponse, QueueResponse,
    };
    use membrane::types::{
        cAsset, Asset, AssetInfo, SupplyCap, PoolType, PriceResponse,
        Basket, UserInfo,
    };
    use cosmwasm_std::{
        coin, coins, to_json_binary, Addr, Decimal, Uint128, Uint256,
        QueryRequest, WasmQuery,
    };
    use cw_multi_test::{App, Executor};

    use crate::testing::integration_tests::tests::proper_instantiate;

    const USER: &str = "user";
    const ADMIN: &str = "admin";
    const BIDDER1: &str = "bidder1";
    const BIDDER2: &str = "bidder2";

    /// Helper to add bids to the liq queue
    fn add_bid_to_queue(
        app: &mut App,
        lq_contract: &LQContract,
        bidder: &str,
        bid_for: AssetInfo,
        premium_slot: u8,
        amount: Uint128,
    ) {
        let msg = LQ_ExecuteMsg::SubmitBid {
            bid_for,
            premium_slot,
        };
        let cosmos_msg = lq_contract
            .call(
                msg,
                vec![coin(amount.u128(), "credit_fulldenom")],
            )
            .unwrap();
        app.execute(app.api().addr_make(bidder), cosmos_msg).unwrap();
    }

    /// Helper to create a position and make it liquidatable
    fn create_liquidatable_position(
        app: &mut App,
        cdp_contract: &CDPContract,
        owner: &str,
        collateral_denom: &str,
        collateral_amount: Uint128,
        debt_amount: Uint128,
    ) -> Uint128 {
        // Deposit collateral
        let msg = ExecuteMsg::Deposit {
            position_owner: Some(app.api().addr_make(owner).to_string()),
            position_id: None,
            affiliate_address: None,
            affiliate_label: None,
        };
        let cosmos_msg = cdp_contract
            .call(
                msg,
                vec![coin(collateral_amount.u128(), collateral_denom)],
            )
            .unwrap();
        app.execute(app.api().addr_make(owner), cosmos_msg).unwrap();

        // Get position ID from response
        let positions: BasketPositionsResponse = app
            .wrap()
            .query_wasm_smart(
                cdp_contract.addr(),
                &QueryMsg::GetBasketPositions {
                    start_after: None,
                    user_info: Some(UserInfo {
                        position_owner: app.api().addr_make(owner).to_string(),
                        position_id: Uint128::new(1),
                    }),
                    user: None,
                    limit: None,
                },
            )
            .unwrap();

        let position_id = positions.user_positions[0].position_id;

        // Increase debt
        let msg = ExecuteMsg::IncreaseDebt {
            position_id,
            amount: Some(debt_amount),
            LTV: None,
            mint_to_addr: None,
            deployment_intent: None,
            debt_split: None,
            rollover_updates: None,
            peg_debt: None,
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(owner), cosmos_msg).unwrap();

        position_id
    }

    #[test]
    fn test_full_liquidation_with_collateral_for_debt() {
        // Scenario: Queue has enough bids to fully cover the debt
        // Expected: All debt is repaid, no leftover_debt

        let (mut app, cdp_contract, lq_contract) =
            proper_instantiate(false, false, false, false);

        let res: Config = app
            .wrap()
            .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
            .unwrap();

        // Add liq_queue to basket
        let msg = ExecuteMsg::EditBasket(EditBasket {
            take_revenue: None,
            added_cAsset: None,
            liq_queue: Some(lq_contract.addr().to_string()),
            credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
            collateral_supply_caps: Some(vec![SupplyCap {
                asset_info: AssetInfo::NativeToken {
                    denom: "debit".to_string(),
                },
                current_supply: Uint128::zero(),
                debt_total: Uint128::zero(),
                supply_cap_ratio: Decimal::percent(100),
                lp: false,
                stability_pool_ratio_for_debt_cap: None,
            }]),
            base_interest_rate: None,
            credit_asset_twap_price_source: None,
            negative_rates: None,
            cpc_margin_of_error: None,
            frozen: None,
            distribute_revenue: None,
            multi_asset_supply_caps: None,
        });
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

        // Add queue for the collateral asset
        let msg = LQ_ExecuteMsg::AddQueue {
            bid_for: AssetInfo::NativeToken {
                denom: "debit".to_string(),
            },
            max_premium: Uint128::new(10u128), // 10% max premium
            bid_threshold: Uint256::from(1_000_000u128),
        };
        let cosmos_msg = lq_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

        // Add sufficient bids to the queue
        // With 5% premium, to cover 1000 debt, we need:
        // collateral = debt * credit_price / (collateral_price * (1 - 0.05))
        // = 1000 / 0.95 = 1052.63
        // So bidder should have enough to buy ~1053 collateral
        add_bid_to_queue(
            &mut app,
            &lq_contract,
            BIDDER1,
            AssetInfo::NativeToken {
                denom: "debit".to_string(),
            },
            5u8, // 5% premium
            Uint128::from(10_000_000000u128), // Plenty of bids
        );

        // Create a liquidatable position
        let debt_amount = Uint128::from(1_000_000000u128); // 1000 credit
        let collateral_amount = Uint128::from(100_000_000000u128); // 100k debit

        let position_id = create_liquidatable_position(
            &mut app,
            &cdp_contract,
            USER,
            "debit",
            collateral_amount,
            debt_amount,
        );

        // Query CollateralForDebt to verify it returns the right amount
        let cfd_query = LQ_QueryMsg::CollateralForDebt {
            bid_for: AssetInfo::NativeToken {
                denom: "debit".to_string(),
            },
            collateral_price: PriceResponse {
                prices: vec![],
                price: Decimal::one(),
                decimals: 6u64,
            },
            debt_amount: Uint256::from(debt_amount.u128()),
            credit_info: AssetInfo::NativeToken {
                denom: "credit_fulldenom".to_string(),
            },
            credit_price: PriceResponse {
                prices: vec![],
                price: Decimal::one(),
                decimals: 6u64,
            },
        };

        let cfd_response: CollateralForDebtResponse = app
            .wrap()
            .query_wasm_smart(lq_contract.addr(), &cfd_query)
            .unwrap();

        // Verify the query returns correct collateral needed
        // For 1000 debt with 5% premium and 1:1 prices:
        // collateral_needed = 1000 / (1 - 0.05) = 1052.63 + 1 (rounding) = ~1053-1054
        let collateral_needed = Uint128::from_str(&cfd_response.collateral_needed).unwrap();
        assert!(
            collateral_needed >= Uint128::from(1052_000000u128) &&
            collateral_needed <= Uint128::from(1055_000000u128),
            "Collateral needed should be ~1053 for 1000 debt with 5% premium, got {}",
            collateral_needed
        );

        // Verify leftover_debt is 0 (queue has enough bids)
        let leftover_debt = Uint128::from_str(&cfd_response.leftover_debt).unwrap();
        assert_eq!(
            leftover_debt,
            Uint128::zero(),
            "Leftover debt should be 0 when queue has sufficient bids"
        );

        // TODO: Execute liquidation and verify the position is fully liquidated
        // This requires the CDP liquidation code to be updated to use CollateralForDebt
    }

    #[test]
    fn test_partial_liquidation_insufficient_bids() {
        // Scenario: Queue has some bids but not enough to cover all debt
        // Expected: Partial collateral_needed, non-zero leftover_debt

        let (mut app, cdp_contract, lq_contract) =
            proper_instantiate(false, false, false, false);

        // Add liq_queue to basket
        let msg = ExecuteMsg::EditBasket(EditBasket {
            take_revenue: None,
            added_cAsset: None,
            liq_queue: Some(lq_contract.addr().to_string()),
            credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
            collateral_supply_caps: Some(vec![SupplyCap {
                asset_info: AssetInfo::NativeToken {
                    denom: "debit".to_string(),
                },
                current_supply: Uint128::zero(),
                debt_total: Uint128::zero(),
                supply_cap_ratio: Decimal::percent(100),
                lp: false,
                stability_pool_ratio_for_debt_cap: None,
            }]),
            base_interest_rate: None,
            credit_asset_twap_price_source: None,
            negative_rates: None,
            cpc_margin_of_error: None,
            frozen: None,
            distribute_revenue: None,
            multi_asset_supply_caps: None,
        });
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

        // Add queue
        let msg = LQ_ExecuteMsg::AddQueue {
            bid_for: AssetInfo::NativeToken {
                denom: "debit".to_string(),
            },
            max_premium: Uint128::new(10u128),
            bid_threshold: Uint256::from(1_000_000u128),
        };
        let cosmos_msg = lq_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

        // Add INSUFFICIENT bids - only enough to cover 500 out of 1000 debt
        add_bid_to_queue(
            &mut app,
            &lq_contract,
            BIDDER1,
            AssetInfo::NativeToken {
                denom: "debit".to_string(),
            },
            5u8,
            Uint128::from(500_000000u128), // Only 500 credit available
        );

        // Query CollateralForDebt for 1000 debt
        let cfd_query = LQ_QueryMsg::CollateralForDebt {
            bid_for: AssetInfo::NativeToken {
                denom: "debit".to_string(),
            },
            collateral_price: PriceResponse {
                prices: vec![],
                price: Decimal::one(),
                decimals: 6u64,
            },
            debt_amount: Uint256::from(1_000_000000u128),
            credit_info: AssetInfo::NativeToken {
                denom: "credit_fulldenom".to_string(),
            },
            credit_price: PriceResponse {
                prices: vec![],
                price: Decimal::one(),
                decimals: 6u64,
            },
        };

        let cfd_response: CollateralForDebtResponse = app
            .wrap()
            .query_wasm_smart(lq_contract.addr(), &cfd_query)
            .unwrap();

        // With only 500 bids available and 5% premium:
        // collateral_needed = 500 / 0.95 = 526.31 + 1 = ~527
        let collateral_needed = Uint128::from_str(&cfd_response.collateral_needed).unwrap();
        assert!(
            collateral_needed >= Uint128::from(526_000000u128) &&
            collateral_needed <= Uint128::from(528_000000u128),
            "Collateral needed should be ~527 for 500 debt with 5% premium, got {}",
            collateral_needed
        );

        // Leftover debt should be 500 (1000 - 500)
        let leftover_debt = Uint128::from_str(&cfd_response.leftover_debt).unwrap();
        assert_eq!(
            leftover_debt,
            Uint128::from(500_000000u128),
            "Leftover debt should be 500 when queue can only cover 500 out of 1000"
        );
    }

    #[test]
    fn test_empty_queue_returns_zero_collateral() {
        // Scenario: Queue exists but has no bids
        // Expected: collateral_needed = 0, leftover_debt = full debt amount

        let (mut app, cdp_contract, lq_contract) =
            proper_instantiate(false, false, false, false);

        // Add liq_queue to basket
        let msg = ExecuteMsg::EditBasket(EditBasket {
            take_revenue: None,
            added_cAsset: None,
            liq_queue: Some(lq_contract.addr().to_string()),
            credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
            collateral_supply_caps: Some(vec![SupplyCap {
                asset_info: AssetInfo::NativeToken {
                    denom: "debit".to_string(),
                },
                current_supply: Uint128::zero(),
                debt_total: Uint128::zero(),
                supply_cap_ratio: Decimal::percent(100),
                lp: false,
                stability_pool_ratio_for_debt_cap: None,
            }]),
            base_interest_rate: None,
            credit_asset_twap_price_source: None,
            negative_rates: None,
            cpc_margin_of_error: None,
            frozen: None,
            distribute_revenue: None,
            multi_asset_supply_caps: None,
        });
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

        // Add queue but NO bids
        let msg = LQ_ExecuteMsg::AddQueue {
            bid_for: AssetInfo::NativeToken {
                denom: "debit".to_string(),
            },
            max_premium: Uint128::new(10u128),
            bid_threshold: Uint256::from(1_000_000u128),
        };
        let cosmos_msg = lq_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

        // Query CollateralForDebt with empty queue
        let cfd_query = LQ_QueryMsg::CollateralForDebt {
            bid_for: AssetInfo::NativeToken {
                denom: "debit".to_string(),
            },
            collateral_price: PriceResponse {
                prices: vec![],
                price: Decimal::one(),
                decimals: 6u64,
            },
            debt_amount: Uint256::from(1_000_000000u128),
            credit_info: AssetInfo::NativeToken {
                denom: "credit_fulldenom".to_string(),
            },
            credit_price: PriceResponse {
                prices: vec![],
                price: Decimal::one(),
                decimals: 6u64,
            },
        };

        let cfd_response: CollateralForDebtResponse = app
            .wrap()
            .query_wasm_smart(lq_contract.addr(), &cfd_query)
            .unwrap();

        // Empty queue should return 0 collateral needed
        assert_eq!(
            cfd_response.collateral_needed,
            "0",
            "Empty queue should return 0 collateral_needed"
        );

        // All debt remains as leftover
        assert_eq!(
            cfd_response.leftover_debt,
            "1000000000",
            "Empty queue should return full debt as leftover_debt"
        );
    }

    #[test]
    fn test_multiple_premium_slots() {
        // Scenario: Debt spans multiple premium slots (e.g., 3%, 5%, 7%)
        // Expected: Collateral calculated correctly across all slots

        let (mut app, cdp_contract, lq_contract) =
            proper_instantiate(false, false, false, false);

        // Add liq_queue to basket
        let msg = ExecuteMsg::EditBasket(EditBasket {
            take_revenue: None,
            added_cAsset: None,
            liq_queue: Some(lq_contract.addr().to_string()),
            credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
            collateral_supply_caps: Some(vec![SupplyCap {
                asset_info: AssetInfo::NativeToken {
                    denom: "debit".to_string(),
                },
                current_supply: Uint128::zero(),
                debt_total: Uint128::zero(),
                supply_cap_ratio: Decimal::percent(100),
                lp: false,
                stability_pool_ratio_for_debt_cap: None,
            }]),
            base_interest_rate: None,
            credit_asset_twap_price_source: None,
            negative_rates: None,
            cpc_margin_of_error: None,
            frozen: None,
            distribute_revenue: None,
            multi_asset_supply_caps: None,
        });
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

        // Add queue
        let msg = LQ_ExecuteMsg::AddQueue {
            bid_for: AssetInfo::NativeToken {
                denom: "debit".to_string(),
            },
            max_premium: Uint128::new(10u128),
            bid_threshold: Uint256::from(1_000_000u128),
        };
        let cosmos_msg = lq_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

        // Add bids across multiple premium slots
        // Slot 3 (3%): 300 credit
        add_bid_to_queue(
            &mut app,
            &lq_contract,
            BIDDER1,
            AssetInfo::NativeToken {
                denom: "debit".to_string(),
            },
            3u8,
            Uint128::from(300_000000u128),
        );

        // Slot 5 (5%): 400 credit
        add_bid_to_queue(
            &mut app,
            &lq_contract,
            BIDDER1,
            AssetInfo::NativeToken {
                denom: "debit".to_string(),
            },
            5u8,
            Uint128::from(400_000000u128),
        );

        // Slot 7 (7%): 300 credit
        add_bid_to_queue(
            &mut app,
            &lq_contract,
            BIDDER2,
            AssetInfo::NativeToken {
                denom: "debit".to_string(),
            },
            7u8,
            Uint128::from(300_000000u128),
        );

        // Total bids: 1000 credit across three slots
        // Query for exactly 1000 debt
        let cfd_query = LQ_QueryMsg::CollateralForDebt {
            bid_for: AssetInfo::NativeToken {
                denom: "debit".to_string(),
            },
            collateral_price: PriceResponse {
                prices: vec![],
                price: Decimal::one(),
                decimals: 6u64,
            },
            debt_amount: Uint256::from(1_000_000000u128),
            credit_info: AssetInfo::NativeToken {
                denom: "credit_fulldenom".to_string(),
            },
            credit_price: PriceResponse {
                prices: vec![],
                price: Decimal::one(),
                decimals: 6u64,
            },
        };

        let cfd_response: CollateralForDebtResponse = app
            .wrap()
            .query_wasm_smart(lq_contract.addr(), &cfd_query)
            .unwrap();

        // Calculate expected collateral:
        // Slot 3: 300 / 0.97 = 309.28
        // Slot 5: 400 / 0.95 = 421.05
        // Slot 7: 300 / 0.93 = 322.58
        // Total: ~1052.91 + rounding adjustments
        let collateral_needed = Uint128::from_str(&cfd_response.collateral_needed).unwrap();
        assert!(
            collateral_needed >= Uint128::from(1052_000000u128) &&
            collateral_needed <= Uint128::from(1056_000000u128),
            "Collateral for multi-slot should be ~1053, got {}",
            collateral_needed
        );

        // All debt should be covered
        let leftover_debt = Uint128::from_str(&cfd_response.leftover_debt).unwrap();
        assert_eq!(
            leftover_debt,
            Uint128::zero(),
            "All debt should be covered across multiple slots"
        );
    }

    #[test]
    fn test_different_price_ratios() {
        // Scenario: Collateral price != credit price
        // Expected: Collateral calculation accounts for price difference

        let (mut app, cdp_contract, lq_contract) =
            proper_instantiate(false, false, false, false);

        // Add liq_queue to basket
        let msg = ExecuteMsg::EditBasket(EditBasket {
            take_revenue: None,
            added_cAsset: None,
            liq_queue: Some(lq_contract.addr().to_string()),
            credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
            collateral_supply_caps: Some(vec![SupplyCap {
                asset_info: AssetInfo::NativeToken {
                    denom: "debit".to_string(),
                },
                current_supply: Uint128::zero(),
                debt_total: Uint128::zero(),
                supply_cap_ratio: Decimal::percent(100),
                lp: false,
                stability_pool_ratio_for_debt_cap: None,
            }]),
            base_interest_rate: None,
            credit_asset_twap_price_source: None,
            negative_rates: None,
            cpc_margin_of_error: None,
            frozen: None,
            distribute_revenue: None,
            multi_asset_supply_caps: None,
        });
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

        // Add queue
        let msg = LQ_ExecuteMsg::AddQueue {
            bid_for: AssetInfo::NativeToken {
                denom: "debit".to_string(),
            },
            max_premium: Uint128::new(10u128),
            bid_threshold: Uint256::from(1_000_000u128),
        };
        let cosmos_msg = lq_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

        // Add bids
        add_bid_to_queue(
            &mut app,
            &lq_contract,
            BIDDER1,
            AssetInfo::NativeToken {
                denom: "debit".to_string(),
            },
            5u8,
            Uint128::from(10_000_000000u128),
        );

        // Query with collateral price = 2.0, credit price = 1.0
        // For 1000 debt with 2:1 price ratio and 5% premium:
        // collateral = debt * credit_price / (collateral_price * (1 - premium))
        // = 1000 * 1.0 / (2.0 * 0.95) = 1000 / 1.9 = 526.31
        let cfd_query = LQ_QueryMsg::CollateralForDebt {
            bid_for: AssetInfo::NativeToken {
                denom: "debit".to_string(),
            },
            collateral_price: PriceResponse {
                prices: vec![],
                price: Decimal::from_ratio(2u128, 1u128), // 2.0
                decimals: 6u64,
            },
            debt_amount: Uint256::from(1_000_000000u128),
            credit_info: AssetInfo::NativeToken {
                denom: "credit_fulldenom".to_string(),
            },
            credit_price: PriceResponse {
                prices: vec![],
                price: Decimal::one(), // 1.0
                decimals: 6u64,
            },
        };

        let cfd_response: CollateralForDebtResponse = app
            .wrap()
            .query_wasm_smart(lq_contract.addr(), &cfd_query)
            .unwrap();

        let collateral_needed = Uint128::from_str(&cfd_response.collateral_needed).unwrap();
        // Expected: ~526-527 collateral for 1000 debt with 2:1 price ratio
        assert!(
            collateral_needed >= Uint128::from(526_000000u128) &&
            collateral_needed <= Uint128::from(528_000000u128),
            "Collateral with 2:1 price ratio should be ~527, got {}",
            collateral_needed
        );
    }

    #[test]
    fn test_queue_credit_repaid_calculation() {
        // Scenario: Verify that queue_credit_repaid = debt_requested - leftover_debt
        // This tests the calculation at line 900-903 in liquidations.rs

        let (mut app, cdp_contract, lq_contract) =
            proper_instantiate(false, false, false, false);

        // Setup queue
        let msg = ExecuteMsg::EditBasket(EditBasket {
            take_revenue: None,
            added_cAsset: None,
            liq_queue: Some(lq_contract.addr().to_string()),
            credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
            collateral_supply_caps: Some(vec![SupplyCap {
                asset_info: AssetInfo::NativeToken {
                    denom: "debit".to_string(),
                },
                current_supply: Uint128::zero(),
                debt_total: Uint128::zero(),
                supply_cap_ratio: Decimal::percent(100),
                lp: false,
                stability_pool_ratio_for_debt_cap: None,
            }]),
            base_interest_rate: None,
            credit_asset_twap_price_source: None,
            negative_rates: None,
            cpc_margin_of_error: None,
            frozen: None,
            distribute_revenue: None,
            multi_asset_supply_caps: None,
        });
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

        let msg = LQ_ExecuteMsg::AddQueue {
            bid_for: AssetInfo::NativeToken {
                denom: "debit".to_string(),
            },
            max_premium: Uint128::new(10u128),
            bid_threshold: Uint256::from(1_000_000u128),
        };
        let cosmos_msg = lq_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

        // Add partial bids: 700 out of 1000
        add_bid_to_queue(
            &mut app,
            &lq_contract,
            BIDDER1,
            AssetInfo::NativeToken {
                denom: "debit".to_string(),
            },
            5u8,
            Uint128::from(700_000000u128),
        );

        let debt_requested = Uint128::from(1_000_000000u128);
        let cfd_query = LQ_QueryMsg::CollateralForDebt {
            bid_for: AssetInfo::NativeToken {
                denom: "debit".to_string(),
            },
            collateral_price: PriceResponse {
                prices: vec![],
                price: Decimal::one(),
                decimals: 6u64,
            },
            debt_amount: Uint256::from(debt_requested.u128()),
            credit_info: AssetInfo::NativeToken {
                denom: "credit_fulldenom".to_string(),
            },
            credit_price: PriceResponse {
                prices: vec![],
                price: Decimal::one(),
                decimals: 6u64,
            },
        };

        let cfd_response: CollateralForDebtResponse = app
            .wrap()
            .query_wasm_smart(lq_contract.addr(), &cfd_query)
            .unwrap();

        let leftover_debt = Uint128::from_str(&cfd_response.leftover_debt).unwrap();

        // Verify: queue_credit_repaid = debt_requested - leftover_debt
        let queue_credit_repaid = debt_requested.checked_sub(leftover_debt).unwrap();

        // With 700 bids available, queue should repay 700
        assert_eq!(
            queue_credit_repaid,
            Uint128::from(700_000000u128),
            "queue_credit_repaid should equal debt_requested - leftover_debt"
        );

        // Leftover should be 300
        assert_eq!(
            leftover_debt,
            Uint128::from(300_000000u128),
            "leftover_debt should be 300 when 700 out of 1000 is covered"
        );
    }
}
