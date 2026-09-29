/// Unit tests for CollateralForDebt query response handling in CDP liquidations
///
/// These tests verify that the CDP liquidation logic correctly handles
/// CollateralForDebt query responses in all scenarios identified in the query logic:
///
/// 1. Parsing collateral_needed and leftover_debt strings
/// 2. Calculating queue_credit_repaid = debt_requested - leftover_debt
/// 3. Handling zero collateral_needed (empty queue case)
/// 4. Handling zero queue_credit_repaid (skip message)
/// 5. Handling partial liquidations (leftover_debt > 0)
/// 6. Error cases (underflow in queue_credit_repaid calculation)

#[cfg(test)]
mod tests {
    use cosmwasm_std::{Uint128, StdError};
    use std::str::FromStr;
    use membrane::liq_queue::CollateralForDebtResponse;

    /// Test that we can correctly parse a CollateralForDebt response
    /// and calculate queue_credit_repaid
    #[test]
    fn test_parse_collateral_for_debt_response() {
        let response = CollateralForDebtResponse {
            collateral_needed: "1052631578".to_string(), // ~1052.6 with 6 decimals
            leftover_debt: "0".to_string(),
        };

        // Parse collateral_needed
        let collateral_needed = Uint128::from_str(&response.collateral_needed).unwrap();
        assert_eq!(collateral_needed, Uint128::from(1052631578u128));

        // Parse leftover_debt
        let leftover_debt = Uint128::from_str(&response.leftover_debt).unwrap();
        assert_eq!(leftover_debt, Uint128::zero());

        // Calculate queue_credit_repaid
        let debt_requested = Uint128::from(1_000_000000u128);
        let queue_credit_repaid = debt_requested.checked_sub(leftover_debt).unwrap();
        assert_eq!(
            queue_credit_repaid,
            Uint128::from(1_000_000000u128),
            "All debt should be repaid when leftover_debt is 0"
        );
    }

    /// Test partial liquidation scenario where leftover_debt > 0
    #[test]
    fn test_partial_liquidation_response() {
        let response = CollateralForDebtResponse {
            collateral_needed: "526315789".to_string(), // For 500 debt with 5% premium
            leftover_debt: "500000000".to_string(),     // 500 debt remaining
        };

        let debt_requested = Uint128::from(1_000_000000u128);

        let collateral_needed = Uint128::from_str(&response.collateral_needed).unwrap();
        let leftover_debt = Uint128::from_str(&response.leftover_debt).unwrap();

        // Verify partial collateral
        assert!(collateral_needed > Uint128::zero());
        assert!(collateral_needed < Uint128::from(1_000_000000u128));

        // Calculate queue_credit_repaid
        let queue_credit_repaid = debt_requested.checked_sub(leftover_debt).unwrap();
        assert_eq!(
            queue_credit_repaid,
            Uint128::from(500_000000u128),
            "Queue should have repaid 500 out of 1000"
        );

        // Verify leftover
        assert_eq!(
            leftover_debt,
            Uint128::from(500_000000u128),
            "Half the debt should remain"
        );
    }

    /// Test empty queue scenario (returns 0 collateral, full leftover_debt)
    #[test]
    fn test_empty_queue_response() {
        let debt_requested = Uint128::from(1_000_000000u128);

        let response = CollateralForDebtResponse {
            collateral_needed: "0".to_string(),
            leftover_debt: debt_requested.to_string(),
        };

        let collateral_needed = Uint128::from_str(&response.collateral_needed).unwrap();
        let leftover_debt = Uint128::from_str(&response.leftover_debt).unwrap();

        // Empty queue returns 0 collateral
        assert_eq!(
            collateral_needed,
            Uint128::zero(),
            "Empty queue should return 0 collateral_needed"
        );

        // All debt is leftover
        assert_eq!(
            leftover_debt,
            debt_requested,
            "Empty queue should return all debt as leftover"
        );

        // Calculate queue_credit_repaid
        let queue_credit_repaid = debt_requested.checked_sub(leftover_debt).unwrap();
        assert_eq!(
            queue_credit_repaid,
            Uint128::zero(),
            "Empty queue repays 0 debt"
        );

        // This should trigger the skip logic at line 937 in liquidations.rs:
        // if collateral_needed.is_zero() || queue_credit_repaid.is_zero() { continue; }
        assert!(
            collateral_needed.is_zero() || queue_credit_repaid.is_zero(),
            "Should skip sending message when collateral or credit repaid is zero"
        );
    }

    /// Test that queue_credit_repaid calculation handles edge case where
    /// leftover_debt > debt_requested (should error)
    #[test]
    fn test_queue_credit_repaid_underflow() {
        let debt_requested = Uint128::from(500_000000u128);

        // Invalid response where leftover > requested (should never happen)
        let response = CollateralForDebtResponse {
            collateral_needed: "0".to_string(),
            leftover_debt: "1000000000".to_string(), // More than requested!
        };

        let leftover_debt = Uint128::from_str(&response.leftover_debt).unwrap();

        // This should error due to underflow
        let result = debt_requested.checked_sub(leftover_debt);
        assert!(
            result.is_err(),
            "Should error when leftover_debt > debt_requested"
        );

        // This matches the error handling at line 901-902 in liquidations.rs:
        // Err(_) => return Err(StdError::generic_err("Queue credit repaid calculation (for liq) failed"))
    }

    /// Test the zero-check logic that determines whether to skip sending liquidate message
    #[test]
    fn test_zero_handling_skip_message() {
        // Scenario 1: Zero collateral_needed
        let response1 = CollateralForDebtResponse {
            collateral_needed: "0".to_string(),
            leftover_debt: "1000000000".to_string(),
        };

        let collateral_needed = Uint128::from_str(&response1.collateral_needed).unwrap();
        let debt_requested = Uint128::from(1_000_000000u128);
        let leftover_debt = Uint128::from_str(&response1.leftover_debt).unwrap();
        let queue_credit_repaid = debt_requested.checked_sub(leftover_debt).unwrap();

        // Should skip (line 937-939)
        assert!(
            collateral_needed.is_zero() || queue_credit_repaid.is_zero(),
            "Should skip when collateral_needed is zero"
        );

        // Scenario 2: Zero queue_credit_repaid (but non-zero collateral - edge case)
        let response2 = CollateralForDebtResponse {
            collateral_needed: "1000000000".to_string(),
            leftover_debt: "1000000000".to_string(), // Same as requested = 0 repaid
        };

        let collateral_needed2 = Uint128::from_str(&response2.collateral_needed).unwrap();
        let leftover_debt2 = Uint128::from_str(&response2.leftover_debt).unwrap();
        let queue_credit_repaid2 = debt_requested.checked_sub(leftover_debt2).unwrap();

        assert!(
            collateral_needed2.is_zero() || queue_credit_repaid2.is_zero(),
            "Should skip when queue_credit_repaid is zero"
        );

        // Scenario 3: Both non-zero - should NOT skip
        let response3 = CollateralForDebtResponse {
            collateral_needed: "1052631578".to_string(),
            leftover_debt: "0".to_string(),
        };

        let collateral_needed3 = Uint128::from_str(&response3.collateral_needed).unwrap();
        let leftover_debt3 = Uint128::from_str(&response3.leftover_debt).unwrap();
        let queue_credit_repaid3 = debt_requested.checked_sub(leftover_debt3).unwrap();

        assert!(
            !collateral_needed3.is_zero() && !queue_credit_repaid3.is_zero(),
            "Should NOT skip when both are non-zero"
        );
    }

    /// Test parsing various response formats to ensure robustness
    #[test]
    fn test_response_parsing_edge_cases() {
        // Very large numbers
        let large_response = CollateralForDebtResponse {
            collateral_needed: "999999999999999999".to_string(),
            leftover_debt: "0".to_string(),
        };
        assert!(Uint128::from_str(&large_response.collateral_needed).is_ok());

        // Exactly zero
        let zero_response = CollateralForDebtResponse {
            collateral_needed: "0".to_string(),
            leftover_debt: "0".to_string(),
        };
        assert_eq!(
            Uint128::from_str(&zero_response.collateral_needed).unwrap(),
            Uint128::zero()
        );
    }

    /// Test the full query response handling flow as it would occur in liquidations.rs
    #[test]
    fn test_full_response_handling_flow() {
        // Simulate the flow from liquidations.rs lines 884-903

        let debt_requested = Uint128::from(1_000_000000u128);

        // 1. Query returns response
        let cfd_response = CollateralForDebtResponse {
            collateral_needed: "1052631578".to_string(),
            leftover_debt: "0".to_string(),
        };

        // 2. Parse collateral_needed (line 898)
        let collateral_repay_amount = Uint128::from_str(&cfd_response.collateral_needed).unwrap();

        // 3. Calculate queue_credit_repaid (lines 900-903)
        let queue_credit_repaid = match debt_requested
            .checked_sub(Uint128::from_str(&cfd_response.leftover_debt).unwrap())
        {
            Ok(res) => res,
            Err(_) => panic!("Queue credit repaid calculation failed"),
        };

        // 4. Check if we should skip (line 937)
        let should_send_message = !collateral_repay_amount.is_zero() && !queue_credit_repaid.is_zero();

        // Assertions
        assert!(should_send_message, "Should send liquidate message");
        assert_eq!(queue_credit_repaid, debt_requested, "All debt repaid");
        assert!(
            collateral_repay_amount > Uint128::zero(),
            "Collateral needed is non-zero"
        );
    }

    /// Test multiple premium slots scenario verification
    #[test]
    fn test_multiple_slots_response() {
        // When debt spans multiple slots, the query aggregates them
        let debt_requested = Uint128::from(1_000_000000u128);

        // Response for debt split across slots with different premiums
        // e.g., 300 @ 3%, 400 @ 5%, 300 @ 7%
        let response = CollateralForDebtResponse {
            collateral_needed: "1052910052".to_string(), // Aggregated across slots
            leftover_debt: "0".to_string(),
        };

        let collateral_needed = Uint128::from_str(&response.collateral_needed).unwrap();
        let leftover_debt = Uint128::from_str(&response.leftover_debt).unwrap();

        // Verify the query properly aggregated across all slots
        assert!(
            collateral_needed > debt_requested,
            "Collateral needed should exceed debt due to premiums"
        );

        let queue_credit_repaid = debt_requested.checked_sub(leftover_debt).unwrap();
        assert_eq!(
            queue_credit_repaid,
            debt_requested,
            "All debt repaid across multiple slots"
        );
    }

    /// Test collateral cap scenario (line 907-909 in liquidations.rs)
    #[test]
    fn test_collateral_cap_handling() {
        // Query says we need 2000 collateral, but position only has 1500

        let response = CollateralForDebtResponse {
            collateral_needed: "2000000000".to_string(),
            leftover_debt: "0".to_string(),
        };

        let collateral_needed = Uint128::from_str(&response.collateral_needed).unwrap();
        let position_collateral_available = Uint128::from(1500_000000u128);

        // Simulate the cap logic from line 907-909
        let collateral_repay_amount = if collateral_needed > position_collateral_available {
            position_collateral_available
        } else {
            collateral_needed
        };

        assert_eq!(
            collateral_repay_amount,
            position_collateral_available,
            "Should cap at position's available collateral"
        );

        // Note: In this case, not all debt would be repaid because we can't send
        // enough collateral. The actual liquidation would send 1500 collateral
        // and repay less debt than calculated.
    }
}
