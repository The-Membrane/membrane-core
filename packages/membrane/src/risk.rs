#![allow(non_snake_case)]
use cosmwasm_std::Decimal;

/// Pure risk calculation functions shared across Collateral, Debt, and Liquidation contracts.
/// These are stateless - they don't read or write storage.

/// Compute the current LTV (Loan-to-Value) ratio.
/// Returns Decimal::MAX if collateral_value is zero and debt_value > 0.
/// Returns Decimal::zero() if debt_value is zero.
pub fn compute_ltv(collateral_value: Decimal, debt_value: Decimal) -> Decimal {
    if debt_value.is_zero() {
        return Decimal::zero();
    }
    if collateral_value.is_zero() {
        // Infinite LTV - position is completely insolvent
        return Decimal::MAX;
    }
    debt_value / collateral_value
}

/// Check if a position is solvent (current_ltv <= max_ltv).
/// Returns true if solvent, false if insolvent.
pub fn check_solvency(current_ltv: Decimal, max_ltv: Decimal) -> bool {
    current_ltv <= max_ltv
}

/// Check if a position can borrow more (current_ltv <= max_borrow_ltv).
/// Returns true if can borrow, false if at/over borrow limit.
pub fn can_borrow(current_ltv: Decimal, max_borrow_ltv: Decimal) -> bool {
    current_ltv <= max_borrow_ltv
}

/// Compute the weighted average LTV across multiple collateral assets.
/// `ratios` - the value ratio of each asset (value_i / total_value), must sum to ~1.0
/// `ltvs` - the LTV parameter for each asset
/// Both vectors must have the same length.
pub fn compute_weighted_avg_ltv(ratios: &[Decimal], ltvs: &[Decimal]) -> Decimal {
    if ratios.is_empty() || ratios.len() != ltvs.len() {
        return Decimal::zero();
    }
    ratios.iter()
        .zip(ltvs.iter())
        .fold(Decimal::zero(), |acc, (ratio, ltv)| {
            acc + (*ratio * *ltv)
        })
}

/// Calculate the available liquidation fee based on LTV deviation.
/// fee_range = current_ltv - max_borrow_ltv (how far over borrow limit)
/// fee = current_ltv - max_ltv (how far over liquidation threshold)
/// available_fee = (fee / current_ltv) * debt_value
pub fn compute_liquidation_fee(
    current_ltv: Decimal,
    max_ltv: Decimal,
    debt_value: Decimal,
) -> Decimal {
    if current_ltv <= max_ltv || current_ltv.is_zero() {
        return Decimal::zero();
    }
    let fee = current_ltv - max_ltv;
    (fee / current_ltv) * debt_value
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::Decimal;

    #[test]
    fn test_compute_ltv() {
        // Normal case
        let ltv = compute_ltv(
            Decimal::from_ratio(100u128, 1u128),
            Decimal::from_ratio(50u128, 1u128),
        );
        assert_eq!(ltv, Decimal::percent(50));

        // Zero debt
        let ltv = compute_ltv(Decimal::from_ratio(100u128, 1u128), Decimal::zero());
        assert_eq!(ltv, Decimal::zero());

        // Zero collateral with debt
        let ltv = compute_ltv(Decimal::zero(), Decimal::from_ratio(50u128, 1u128));
        assert_eq!(ltv, Decimal::MAX);
    }

    #[test]
    fn test_check_solvency() {
        assert!(check_solvency(Decimal::percent(50), Decimal::percent(80)));
        assert!(check_solvency(Decimal::percent(80), Decimal::percent(80)));
        assert!(!check_solvency(Decimal::percent(81), Decimal::percent(80)));
    }

    #[test]
    fn test_weighted_avg_ltv() {
        let ratios = vec![Decimal::percent(60), Decimal::percent(40)];
        let ltvs = vec![Decimal::percent(80), Decimal::percent(60)];
        let avg = compute_weighted_avg_ltv(&ratios, &ltvs);
        // 0.6 * 0.8 + 0.4 * 0.6 = 0.48 + 0.24 = 0.72
        assert_eq!(avg, Decimal::from_ratio(72u128, 100u128));
    }

    #[test]
    fn test_liquidation_fee() {
        // Position at 90% LTV, max is 80%
        let fee = compute_liquidation_fee(
            Decimal::percent(90),
            Decimal::percent(80),
            Decimal::from_ratio(100u128, 1u128),
        );
        // fee = (0.90 - 0.80) / 0.90 * 100 = 0.1/0.9 * 100 ≈ 11.11
        let expected = (Decimal::percent(10) / Decimal::percent(90)) * Decimal::from_ratio(100u128, 1u128);
        assert_eq!(fee, expected);

        // Solvent position - no fee
        let fee = compute_liquidation_fee(
            Decimal::percent(70),
            Decimal::percent(80),
            Decimal::from_ratio(100u128, 1u128),
        );
        assert_eq!(fee, Decimal::zero());
    }
}
