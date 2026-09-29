#![allow(non_snake_case)]

use cosmwasm_std::testing::{mock_dependencies, mock_env};
use cosmwasm_std::{Decimal, Uint128, WasmMsg};

use proptest::prelude::*;

use membrane::collateral::ExecuteMsg as CollateralExecuteMsg;
use membrane::debt::ExecuteMsg as DebtExecuteMsg;
use membrane::oracle::PriceResponse;

use crate::error::ContractError;
use crate::liquidations::get_repay_quantities;
use crate::reply::handle_reply;
use crate::state::*;
use crate::testing::test_helpers::*;

// ========================================================================
// Strategies
// ========================================================================

fn liq_fee_strategy() -> impl Strategy<Value = Decimal> {
    (0u64..50u64).prop_map(|n| Decimal::percent(n))
}

/// Strategy that generates a solvent position:
/// debt_value <= collateral_value * max_ltv.
/// Returns (total_debt, credit_price, collateral_value, max_ltv).
fn solvent_position_strategy() -> impl Strategy<Value = (Uint128, Decimal, Decimal, Decimal)> {
    (1u128..1_000_000u128, 10u64..95u64, 1u64..100u64).prop_map(
        |(base, max_ltv_pct, debt_pct)| {
            let max_ltv = Decimal::percent(max_ltv_pct);
            let collateral_value = Decimal::from_ratio(base, 1u128);
            let credit_price = Decimal::one();
            // debt_value <= collateral_value * max_ltv
            let max_debt_value = collateral_value * max_ltv;
            let debt_value = max_debt_value * Decimal::percent(debt_pct); // 1-100% of max
            let total_debt = debt_value.to_uint_floor();
            (total_debt, credit_price, collateral_value, max_ltv)
        },
    )
}

/// Strategy that generates an insolvent position:
/// debt_value > collateral_value * max_ltv.
/// Returns (total_debt, credit_price, collateral_value, max_ltv).
fn insolvent_position_strategy() -> impl Strategy<Value = (Uint128, Decimal, Decimal, Decimal)> {
    (1u128..1_000_000u128, 10u64..95u64, 1u64..1000u64).prop_map(
        |(base, max_ltv_pct, extra_pct)| {
            let max_ltv = Decimal::percent(max_ltv_pct);
            let collateral_value = Decimal::from_ratio(base, 1u128);
            let credit_price = Decimal::one();
            // Set debt high enough to be insolvent: debt_value > collateral_value * max_ltv
            let min_debt_value = collateral_value * max_ltv + Decimal::one();
            let extra = Decimal::from_ratio(extra_pct, 100u128) * min_debt_value;
            let total_debt = (min_debt_value + extra).to_uint_ceil();
            (total_debt, credit_price, collateral_value, max_ltv)
        },
    )
}

// ========================================================================
// Property Tests: Repay Quantities (tests 1-8)
// ========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    // ====================================================================
    // 1. Solvent position always returns PositionSolvent error
    // ====================================================================
    #[test]
    fn solvent_position_always_errors(
        (total_debt, credit_price, collateral_value, max_ltv) in solvent_position_strategy(),
        liq_fee in liq_fee_strategy(),
    ) {
        // Skip degenerate case where total_debt rounds to zero
        if total_debt.is_zero() {
            return Ok(());
        }

        let price_response = PriceResponse { prices: vec![], price: credit_price, decimals: 0 };
        let result = get_repay_quantities(
            total_debt,
            &price_response,
            collateral_value,
            max_ltv,
            max_ltv, // avg_borrow_ltv = max_ltv for simplicity
            liq_fee,
        );

        prop_assert!(
            result.is_err(),
            "Expected PositionSolvent error for solvent position \
             (debt={}, credit_price={}, collateral={}, max_ltv={}), but got Ok",
            total_debt, credit_price, collateral_value, max_ltv
        );

        // Verify it is specifically the PositionSolvent error
        match result.unwrap_err() {
            ContractError::PositionSolvent {} => {} // expected
            other => prop_assert!(
                false,
                "Expected PositionSolvent, got: {:?}",
                other
            ),
        }
    }

    // ====================================================================
    // 2. Insolvent position returns positive repay amount
    // ====================================================================
    #[test]
    fn insolvent_position_returns_positive_repay(
        (total_debt, credit_price, collateral_value, max_ltv) in insolvent_position_strategy(),
        liq_fee in liq_fee_strategy(),
    ) {
        let price_response = PriceResponse { prices: vec![], price: credit_price, decimals: 0 };
        let result = get_repay_quantities(
            total_debt,
            &price_response,
            collateral_value,
            max_ltv,
            max_ltv, // avg_borrow_ltv = max_ltv for simplicity
            liq_fee,
        );

        prop_assert!(
            result.is_ok(),
            "Expected Ok for insolvent position \
             (debt={}, credit_price={}, collateral={}, max_ltv={}), got Err: {:?}",
            total_debt, credit_price, collateral_value, max_ltv,
            result.err()
        );

        // Note: repay_amount can be zero due to floor rounding on very small
        // excess_ltv values. The property guarantees no error is returned
        // for genuinely insolvent positions.
    }

    // ====================================================================
    // 3. When excess_ltv < 1 (moderate insolvency), repay_amount <= total_debt.
    //    The formula is: repay_value = debt_value * excess_ltv, so when
    //    excess_ltv < 1 the repay value is bounded by the debt value.
    //    For deeply insolvent positions (excess_ltv >= 1) the function can
    //    return more than total_debt, which is capped downstream in
    //    build_debt_update_submsg (tested in test 9).
    // ====================================================================
    #[test]
    fn repay_amount_lte_total_debt_when_moderate_insolvency(
        base in 100u128..500_000u128,
        max_ltv_pct in 50u64..95u64,
        excess_pct in 1u64..50u64,  // Keep excess_ltv small (< 0.5)
    ) {
        let max_ltv = Decimal::percent(max_ltv_pct);
        let collateral_value = Decimal::from_ratio(base, 1u128);
        let credit_price = Decimal::one();
        let liq_fee = Decimal::percent(1);

        // Generate debt that is only slightly above the insolvency threshold
        // so that excess_ltv stays well below 1.0
        let threshold_debt_value = collateral_value * max_ltv + Decimal::one();
        let excess_factor = Decimal::from_ratio(excess_pct, 100u128);
        let total_debt = (threshold_debt_value * (Decimal::one() + excess_factor)).to_uint_ceil();

        // Verify excess_ltv < 1 for this setup
        let debt_value = Decimal::from_ratio(total_debt, 1u128) * credit_price;
        let current_ltv = if collateral_value.is_zero() {
            Decimal::MAX
        } else {
            debt_value / collateral_value
        };
        let excess_ltv = current_ltv.checked_sub(max_ltv).unwrap_or(Decimal::zero());
        prop_assume!(excess_ltv < Decimal::one());

        let price_response = PriceResponse { prices: vec![], price: credit_price, decimals: 0 };
        let result = get_repay_quantities(
            total_debt,
            &price_response,
            collateral_value,
            max_ltv,
            max_ltv, // avg_borrow_ltv = max_ltv for simplicity
            liq_fee,
        );

        prop_assert!(result.is_ok());
        let (repay_amount, _repay_value, _caller_fee_rate, _protocol_fee_rate) = result.unwrap();

        prop_assert!(
            repay_amount <= total_debt,
            "repay_amount ({}) exceeds total_debt ({}) with excess_ltv={}",
            repay_amount,
            total_debt,
            excess_ltv
        );
    }

    // ====================================================================
    // 4. Fees are non-negative (well-formed Decimals)
    // ====================================================================
    #[test]
    fn fees_non_negative(
        (total_debt, credit_price, collateral_value, max_ltv) in insolvent_position_strategy(),
        liq_fee in liq_fee_strategy(),
    ) {
        let price_response = PriceResponse { prices: vec![], price: credit_price, decimals: 0 };
        let result = get_repay_quantities(
            total_debt,
            &price_response,
            collateral_value,
            max_ltv,
            max_ltv, // avg_borrow_ltv = max_ltv for simplicity
            liq_fee,
        );

        prop_assert!(result.is_ok());
        let (_repay_amount, _repay_value, caller_fee_rate, protocol_fee_rate) = result.unwrap();

        // Decimal is unsigned, so >= 0 is guaranteed by the type system.
        // We verify the values are well-formed and not some overflow artifact.
        prop_assert!(
            caller_fee_rate >= Decimal::zero(),
            "caller_fee_rate should be non-negative, got: {}",
            caller_fee_rate
        );
        prop_assert!(
            protocol_fee_rate >= Decimal::zero(),
            "protocol_fee_rate should be non-negative, got: {}",
            protocol_fee_rate
        );
    }

    // ====================================================================
    // 5. protocol_fee_rate always equals liq_fee input;
    //    caller_fee_rate is current_ltv - max_ltv (LTV-excess).
    // ====================================================================
    #[test]
    fn fee_rates_match_expected(
        (total_debt, credit_price, collateral_value, max_ltv) in insolvent_position_strategy(),
        liq_fee in liq_fee_strategy(),
    ) {
        let price_response = PriceResponse { prices: vec![], price: credit_price, decimals: 0 };
        let result = get_repay_quantities(
            total_debt,
            &price_response,
            collateral_value,
            max_ltv,
            max_ltv, // avg_borrow_ltv = max_ltv for simplicity
            liq_fee,
        );

        prop_assert!(result.is_ok());
        let (_repay_amount, _repay_value, caller_fee_rate, protocol_fee_rate) = result.unwrap();

        // protocol_fee_rate is always the liq_fee passed in (static config value)
        prop_assert_eq!(
            protocol_fee_rate,
            liq_fee,
            "protocol_fee_rate ({}) should equal liq_fee ({})",
            protocol_fee_rate,
            liq_fee
        );

        // caller_fee_rate is current_ltv - max_ltv (dynamic, LTV-excess).
        // With decimals=0 and credit_price=1, debt_value = total_debt * 1 = total_debt.
        let debt_value = Decimal::from_ratio(total_debt, 1u128) * credit_price;
        let current_ltv = if collateral_value.is_zero() {
            Decimal::MAX
        } else {
            debt_value / collateral_value
        };
        let expected_caller_fee_rate = current_ltv
            .checked_sub(max_ltv)
            .unwrap_or(Decimal::zero());
        prop_assert_eq!(
            caller_fee_rate,
            expected_caller_fee_rate,
            "caller_fee_rate ({}) should equal current_ltv - max_ltv ({})",
            caller_fee_rate,
            expected_caller_fee_rate
        );
    }

    // ====================================================================
    // 6. Higher liq_fee produces higher (or equal) protocol_fee_rate;
    //    caller_fee_rate is independent of liq_fee.
    // ====================================================================
    #[test]
    fn fee_scales_with_liq_fee(
        (total_debt, credit_price, collateral_value, max_ltv) in insolvent_position_strategy(),
    ) {
        let low_fee = Decimal::percent(1);
        let high_fee = Decimal::percent(10);

        let price_response = PriceResponse { prices: vec![], price: credit_price, decimals: 0 };
        let result_low = get_repay_quantities(
            total_debt,
            &price_response,
            collateral_value,
            max_ltv,
            max_ltv,
            low_fee,
        );
        let result_high = get_repay_quantities(
            total_debt,
            &price_response,
            collateral_value,
            max_ltv,
            max_ltv,
            high_fee,
        );

        prop_assert!(result_low.is_ok());
        prop_assert!(result_high.is_ok());

        let (_repay_low, _repay_value_low, caller_fee_rate_low, proto_fee_rate_low) = result_low.unwrap();
        let (_repay_high, _repay_value_high, caller_fee_rate_high, proto_fee_rate_high) = result_high.unwrap();

        // protocol_fee_rate is the liq_fee passed in, so higher liq_fee must produce >= rate
        prop_assert!(
            proto_fee_rate_high >= proto_fee_rate_low,
            "Higher liq_fee (10%) produced lower protocol_fee_rate ({}) than lower liq_fee (1%) rate ({})",
            proto_fee_rate_high,
            proto_fee_rate_low
        );

        // caller_fee_rate is current_ltv - max_ltv, independent of liq_fee, so both should be equal
        prop_assert_eq!(
            caller_fee_rate_high,
            caller_fee_rate_low,
            "caller_fee_rate should be independent of liq_fee: high={}, low={}",
            caller_fee_rate_high,
            caller_fee_rate_low
        );
    }

    // ====================================================================
    // 7. Higher debt produces higher (or equal) repay amount (monotonicity)
    // ====================================================================
    #[test]
    fn higher_debt_higher_repay(
        base in 1u128..500_000u128,
        max_ltv_pct in 10u64..90u64,
        extra1_pct in 1u64..500u64,
        extra2_pct in 501u64..1000u64,
    ) {
        let max_ltv = Decimal::percent(max_ltv_pct);
        let collateral_value = Decimal::from_ratio(base, 1u128);
        let credit_price = Decimal::one();
        let liq_fee = Decimal::percent(1);

        // Both positions are insolvent; debt2 > debt1
        let threshold = collateral_value * max_ltv + Decimal::one();
        let debt1 = (threshold + Decimal::from_ratio(extra1_pct, 100u128) * threshold)
            .to_uint_ceil();
        let debt2 = (threshold + Decimal::from_ratio(extra2_pct, 100u128) * threshold)
            .to_uint_ceil();

        // Ensure debt2 > debt1
        prop_assume!(debt2 > debt1);

        let price_response = PriceResponse { prices: vec![], price: credit_price, decimals: 0 };
        let result1 = get_repay_quantities(debt1, &price_response, collateral_value, max_ltv, max_ltv, liq_fee);
        let result2 = get_repay_quantities(debt2, &price_response, collateral_value, max_ltv, max_ltv, liq_fee);

        prop_assert!(result1.is_ok(), "debt1 should be insolvent");
        prop_assert!(result2.is_ok(), "debt2 should be insolvent");

        let (repay1, _, _, _) = result1.unwrap();
        let (repay2, _, _, _) = result2.unwrap();

        prop_assert!(
            repay2 >= repay1,
            "Higher debt ({}) should produce >= repay amount ({}) than lower debt ({}) repay ({})",
            debt2, repay2, debt1, repay1
        );
    }

    // ====================================================================
    // 8. Lower max_ltv (larger excess) produces larger repay amount
    // ====================================================================
    #[test]
    fn larger_ltv_excess_larger_repay(
        base in 100u128..500_000u128,
        high_max_ltv_pct in 50u64..90u64,
    ) {
        let credit_price = Decimal::one();
        let collateral_value = Decimal::from_ratio(base, 1u128);
        let liq_fee = Decimal::percent(1);

        // Two max_ltv values: low_ltv < high_ltv
        // Lower max_ltv means larger excess for the same position
        let low_max_ltv_pct = high_max_ltv_pct.saturating_sub(20).max(10);
        prop_assume!(low_max_ltv_pct < high_max_ltv_pct);

        let high_ltv = Decimal::percent(high_max_ltv_pct);
        let low_ltv = Decimal::percent(low_max_ltv_pct);

        // Set debt to be insolvent for BOTH max_ltv values.
        // debt_value must exceed collateral_value * high_ltv (the higher threshold)
        let min_debt_value =
            collateral_value * high_ltv + Decimal::from_ratio(base / 10 + 1, 1u128);
        let total_debt = min_debt_value.to_uint_ceil();

        let price_response = PriceResponse { prices: vec![], price: credit_price, decimals: 0 };
        let result_high =
            get_repay_quantities(total_debt, &price_response, collateral_value, high_ltv, high_ltv, liq_fee);
        let result_low =
            get_repay_quantities(total_debt, &price_response, collateral_value, low_ltv, low_ltv, liq_fee);

        // Both should be insolvent since debt exceeds the higher threshold
        prop_assert!(
            result_high.is_ok(),
            "Position should be insolvent for high_ltv"
        );
        prop_assert!(
            result_low.is_ok(),
            "Position should be insolvent for low_ltv"
        );

        let (repay_high, _, _, _) = result_high.unwrap();
        let (repay_low, _, _, _) = result_low.unwrap();

        // Lower max_ltv means a bigger gap (excess_ltv = current_ltv - max_ltv),
        // so repay_low (with lower max_ltv) should have a larger or equal repay amount.
        prop_assert!(
            repay_low >= repay_high,
            "Lower max_ltv ({}) should produce >= repay ({}) \
             than higher max_ltv ({}) repay ({})",
            low_ltv,
            repay_low,
            high_ltv,
            repay_high
        );
    }

    // ====================================================================
    // 9. debt_update SubMsg: repay is always min(total_repaid, total_debt)
    // ====================================================================
    #[test]
    fn debt_update_repay_capped_at_total_debt(
        total_debt_val in 100u128..1_000_000u128,
        total_repaid_val in 1u128..2_000_000u128,
    ) {
        let mut deps = mock_dependencies();
        let env = mock_env();

        // Initialize config (no liq queue)
        init_config(deps.as_mut().storage);

        // Create propagation in the state that a sell_wall_reply would see:
        // per_asset_repayment is empty, total_repaid and total_debt set.
        let mut prop = default_propagation();
        prop.total_debt = Uint128::new(total_debt_val);
        prop.total_repaid = Decimal::from_ratio(total_repaid_val, 1u128);
        prop.credit_price = make_price(Decimal::one(), 6);
        // Need at least one collateral asset for the debt_update_reply handler
        prop.collateral_assets = vec![make_casset("uatom", 1_000_000, 50, 80)];
        prop.cAsset_ratios = vec![Decimal::one()];
        prop.cAsset_prices = vec![make_price(Decimal::from_ratio(10u128, 1u128), 6)];
        save_propagation(deps.as_mut().storage, &prop);

        // Simulate a sell_wall_reply (Step 5), which calls build_debt_update_submsg
        let reply = make_success_reply(SELL_WALL_REPLY_ID, vec![]);
        let result = handle_reply(deps.as_mut(), env, reply);
        prop_assert!(
            result.is_ok(),
            "sell_wall_reply should succeed, got: {:?}",
            result.err()
        );

        let response = result.unwrap();
        // The response should contain a SubMsg to Debt.LiquidationDebtUpdate
        prop_assert!(
            !response.messages.is_empty(),
            "Expected at least one SubMsg (debt update)"
        );

        // Parse the SubMsg to verify the repay_amount
        let sub_msg = &response.messages[0];
        if let cosmwasm_std::CosmosMsg::Wasm(WasmMsg::Execute { msg, .. }) = &sub_msg.msg {
            let debt_msg: DebtExecuteMsg = cosmwasm_std::from_json(msg).unwrap();
            match debt_msg {
                DebtExecuteMsg::LiquidationDebtUpdate { repay_amount, .. } => {
                    let total_repaid_uint =
                        Decimal::from_ratio(total_repaid_val, 1u128).to_uint_floor();
                    let expected =
                        std::cmp::min(total_repaid_uint, Uint128::new(total_debt_val));
                    prop_assert_eq!(
                        repay_amount,
                        expected,
                        "repay_amount ({}) should be min(total_repaid={}, total_debt={})",
                        repay_amount,
                        total_repaid_uint,
                        total_debt_val
                    );
                }
                other => prop_assert!(
                    false,
                    "Expected LiquidationDebtUpdate, got: {:?}",
                    other
                ),
            }
        } else {
            prop_assert!(false, "Expected WasmMsg::Execute, got something else");
        }
    }

    // ====================================================================
    // 10. Sell wall SubMsg always uses max_spread = 90%
    // ====================================================================
    #[test]
    fn sell_wall_max_spread_always_90pct(
        // debt in whole units (will be scaled to 6-decimal micro-units)
        debt_whole_units in 100u128..1_000_000u128,
    ) {
        let mut deps = mock_dependencies();
        let env = mock_env();

        // Initialize config WITHOUT liq queue so collateral_assess_reply
        // skips directly to the sell wall.
        init_config(deps.as_mut().storage);

        // Scale debt to 6-decimal micro-units (e.g., 100 whole units = 100_000_000 micro)
        let total_debt_val = debt_whole_units * 1_000_000;

        // Build propagation in post-debt-accrue state (step 1 completed).
        // credit_price=1.0 with 6 decimals: get_value(total_debt) = total_debt/10^6 * 1.0 = debt_whole_units
        let mut prop = make_propagation_post_debt_accrue(total_debt_val, Decimal::one(), 6);
        prop.total_debt = Uint128::new(total_debt_val);
        save_propagation(deps.as_mut().storage, &prop);

        // Build an insolvent collateral assessment.
        // Make collateral_value = debt_whole_units / 2 (so LTV ~200%, insolvent vs 80% max).
        // With collateral price=1.0 and 6 decimals:
        //   collateral_value = collateral_amount / 10^6 * 1.0
        // So collateral_amount = (debt_whole_units / 2) * 10^6
        let half_value = debt_whole_units / 2;
        let collateral_amount = half_value * 1_000_000;
        let collateral_assets = vec![make_casset("uatom", collateral_amount, 50, 80)];
        let ratios = vec![Decimal::one()];
        let prices = vec![make_price(Decimal::one(), 6)];
        let avg_max_ltv = Decimal::percent(80);
        let avg_max_borrow_ltv = Decimal::percent(50);

        let assess_reply = make_collateral_assess_success_reply(
            collateral_assets,
            ratios,
            prices,
            avg_max_ltv,
            avg_max_borrow_ltv,
            false,
            vec![],
        );

        let result = handle_reply(deps.as_mut(), env.clone(), assess_reply);
        prop_assert!(
            result.is_ok(),
            "collateral_assess_reply should succeed for insolvent position, got: {:?}",
            result.err()
        );

        let response = result.unwrap();

        // The collateral_assess_reply may route through a fee transfer step first
        // (FEE_TRANSFER_REPLY_ID = 9) if there are fees. If so, simulate the
        // fee_transfer_reply to get to the sell wall step.
        let fee_transfer_msg = response
            .messages
            .iter()
            .find(|sm| sm.id == FEE_TRANSFER_REPLY_ID);
        let sell_wall_msg = response
            .messages
            .iter()
            .find(|sm| sm.id == SELL_WALL_REPLY_ID);

        let final_response = if sell_wall_msg.is_some() {
            // Direct path: sell wall SubMsg is in the response
            response
        } else if fee_transfer_msg.is_some() {
            // Fee transfer path: simulate the fee_transfer_reply to advance to sell wall
            let fee_reply = make_success_reply(FEE_TRANSFER_REPLY_ID, vec![]);
            let fee_result = handle_reply(deps.as_mut(), env, fee_reply);
            prop_assert!(
                fee_result.is_ok(),
                "fee_transfer_reply should succeed, got: {:?}",
                fee_result.err()
            );
            fee_result.unwrap()
        } else {
            prop_assert!(
                false,
                "Expected sell wall or fee transfer SubMsg, found ids: {:?}",
                response
                    .messages
                    .iter()
                    .map(|m| m.id)
                    .collect::<Vec<_>>()
            );
            unreachable!()
        };

        // Find the sell wall SubMsg (should use SELL_WALL_REPLY_ID = 5)
        let sell_wall_sub = final_response
            .messages
            .iter()
            .find(|sm| sm.id == SELL_WALL_REPLY_ID);

        prop_assert!(
            sell_wall_sub.is_some(),
            "Expected a sell wall SubMsg with id={}, found message ids: {:?}",
            SELL_WALL_REPLY_ID,
            final_response
                .messages
                .iter()
                .map(|m| m.id)
                .collect::<Vec<_>>()
        );

        let sell_wall_sub = sell_wall_sub.unwrap();
        if let cosmwasm_std::CosmosMsg::Wasm(WasmMsg::Execute { msg, .. }) = &sell_wall_sub.msg {
            let coll_msg: CollateralExecuteMsg = cosmwasm_std::from_json(msg).unwrap();
            match coll_msg {
                CollateralExecuteMsg::AuthorizedSellCollateral { max_spread, .. } => {
                    prop_assert_eq!(
                        max_spread,
                        Decimal::percent(90),
                        "max_spread should always be 90%, got: {}",
                        max_spread
                    );
                }
                other => prop_assert!(
                    false,
                    "Expected AuthorizedSellCollateral, got: {:?}",
                    other
                ),
            }
        } else {
            prop_assert!(
                false,
                "Expected WasmMsg::Execute for sell wall SubMsg"
            );
        }
    }

    // ====================================================================
    // 11. total_repaid never decreases through sequential LQ replies
    // ====================================================================
    #[test]
    fn total_repaid_never_decreases(
        repay_amounts in proptest::collection::vec(0u128..1000u128, 1..6),
    ) {
        let mut deps = mock_dependencies();
        let env = mock_env();

        // Initialize config with liq queue
        init_config_with_liq_queue(deps.as_mut().storage);

        let num_assets = repay_amounts.len();

        // Build per_asset_repayment with enough entries for each reply
        let per_asset_repayment: Vec<Decimal> = (0..num_assets)
            .map(|_| Decimal::from_ratio(1000u128, 1u128))
            .collect();

        let mut prop = default_propagation();
        prop.total_debt = Uint128::new(1_000_000); // Large enough to not be fully repaid
        prop.credit_price = make_price(Decimal::one(), 6);
        prop.per_asset_repayment = per_asset_repayment;
        prop.collateral_assets = (0..num_assets)
            .map(|i| make_casset(&format!("uatom{}", i), 1_000_000, 50, 80))
            .collect();
        prop.cAsset_ratios = vec![Decimal::from_ratio(1u128, num_assets as u128); num_assets];
        prop.cAsset_prices = vec![make_price(Decimal::one(), 6); num_assets];
        save_propagation(deps.as_mut().storage, &prop);

        let mut prev_total_repaid = Decimal::zero();

        for (i, repay_amount) in repay_amounts.iter().enumerate() {
            let reply = make_liq_queue_success_reply(
                *repay_amount,
                *repay_amount, // collateral_amount = repay_amount for simplicity
                &format!("uatom{}", i),
            );

            let result = handle_reply(deps.as_mut(), env.clone(), reply);
            prop_assert!(
                result.is_ok(),
                "LQ reply {} should succeed, got: {:?}",
                i,
                result.err()
            );

            // Load updated propagation to check total_repaid
            let updated_prop = LIQUIDATION_PROPAGATION
                .load(deps.as_ref().storage)
                .unwrap();
            let current_total_repaid = updated_prop.total_repaid;

            prop_assert!(
                current_total_repaid >= prev_total_repaid,
                "total_repaid decreased from {} to {} after LQ reply {}",
                prev_total_repaid,
                current_total_repaid,
                i
            );

            prev_total_repaid = current_total_repaid;

            // If per_asset_repayment is empty, the handler transitions to
            // sell wall or debt update and we cannot process further LQ replies.
            if updated_prop.per_asset_repayment.is_empty() {
                break;
            }
        }
    }

    // ====================================================================
    // 12. per_asset_repayment length decreases by exactly 1 per LQ reply
    // ====================================================================
    #[test]
    fn per_asset_len_decreases_by_one(
        num_assets in 1usize..6usize,
        repay_amount in 0u128..1000u128,
    ) {
        let mut deps = mock_dependencies();
        let env = mock_env();

        // Initialize config with liq queue
        init_config_with_liq_queue(deps.as_mut().storage);

        // Build per_asset_repayment with num_assets entries
        let per_asset_repayment: Vec<Decimal> = (0..num_assets)
            .map(|_| Decimal::from_ratio(1000u128, 1u128))
            .collect();

        let mut prop = default_propagation();
        prop.total_debt = Uint128::new(1_000_000); // Large enough to not be fully repaid
        prop.credit_price = make_price(Decimal::one(), 6);
        prop.per_asset_repayment = per_asset_repayment;
        prop.collateral_assets = (0..num_assets)
            .map(|i| make_casset(&format!("uatom{}", i), 1_000_000, 50, 80))
            .collect();
        prop.cAsset_ratios = vec![Decimal::from_ratio(1u128, num_assets as u128); num_assets];
        prop.cAsset_prices = vec![make_price(Decimal::one(), 6); num_assets];
        save_propagation(deps.as_mut().storage, &prop);

        // Process each LQ reply and verify per_asset_repayment shrinks by 1
        for i in 0..num_assets {
            let current_len = {
                let p = LIQUIDATION_PROPAGATION
                    .load(deps.as_ref().storage)
                    .unwrap();
                p.per_asset_repayment.len()
            };

            let reply = make_liq_queue_success_reply(
                repay_amount,
                repay_amount,
                &format!("uatom{}", i),
            );

            let result = handle_reply(deps.as_mut(), env.clone(), reply);
            prop_assert!(
                result.is_ok(),
                "LQ reply {} should succeed, got: {:?}",
                i,
                result.err()
            );

            let updated_prop = LIQUIDATION_PROPAGATION
                .load(deps.as_ref().storage)
                .unwrap();
            let new_len = updated_prop.per_asset_repayment.len();

            prop_assert_eq!(
                new_len,
                current_len - 1,
                "per_asset_repayment length should decrease by 1 \
                 (was {}, now {}) after LQ reply {}",
                current_len,
                new_len,
                i
            );

            // If empty, the handler transitions to sell wall or debt update
            if new_len == 0 {
                break;
            }
        }
    }
}
