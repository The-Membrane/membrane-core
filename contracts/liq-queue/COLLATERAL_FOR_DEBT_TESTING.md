# CollateralForDebt Query Testing Documentation

## Overview

The `CollateralForDebt` query is a new liquidation queue query that calculates the exact amount of collateral needed to repay a specified debt amount, properly accounting for liquidation premiums. This solves the inefficiency in the original `CheckLiquidatible` query which would always leave premium leftovers.

## Core Property Being Tested

**The Fundamental Invariant:**
```
If CollateralForDebt(debt_amount) returns collateral_needed,
then CheckLiquidatible(collateral_needed) MUST return debt_repaid >= debt_amount
```

This property ensures that the new query never under-calculates the required collateral, which would result in incomplete debt repayment.

## Test Coverage

### 1. Basic Functionality Tests (`collateral_for_debt_tests.rs`)

These tests verify correct behavior in standard scenarios:

- **`test_collateral_for_debt_basic_no_premium`**: Baseline test with 0% premium, 1:1 prices
- **`test_collateral_for_debt_with_10_percent_premium`**: Verifies premium calculation (10% premium should require ~11% more collateral)
- **`test_collateral_for_debt_insufficient_liquidity`**: Handles case where liquidation queue can't fully cover the debt
- **`test_collateral_for_debt_multiple_slots`**: Tests iteration across multiple premium slots
- **`test_collateral_for_debt_edge_case_empty_queue`**: Gracefully handles empty liquidation queue

### 2. Comparison Tests (`collateral_for_debt_tests.rs`)

These tests verify the core invariant by comparing with `CheckLiquidatible`:

- **`test_collateral_for_debt_vs_check_liquidatible_no_premium`**: Validates consistency with 0% premium
- **`test_collateral_for_debt_vs_check_liquidatible_with_premium`**: Validates with 5% premium
- **`test_collateral_for_debt_vs_check_liquidatible_multiple_slots`**: Validates across 3 different premium slots
- **`test_collateral_for_debt_with_different_prices`**: Validates with non-1:1 price ratios
- **`test_collateral_for_debt_large_numbers`**: Validates with realistic large amounts (1B+ units)

Each comparison test:
1. Calculates `collateral_needed` using `CollateralForDebt(debt_amount)`
2. Queries `CheckLiquidatible(collateral_needed)` to get `debt_repaid`
3. **Asserts that `debt_repaid >= debt_amount`** (the critical property)
4. Also verifies minimal overshoot (accounting for rounding)

### 3. Property-Based Tests (`property_tests.rs`)

These tests exhaustively verify the invariant across many scenarios:

#### Debt Amount Variations
- **`property_test_various_debt_amounts_no_premium`**: 10 different debt amounts (100 to 50M) with 0% premium
- **`property_test_various_debt_amounts_with_5_percent_premium`**: Same amounts with 5% premium
- **`property_test_various_debt_amounts_with_10_percent_premium`**: Same amounts with 10% premium

#### Price Ratio Variations
- **`property_test_various_prices`**: Tests 9 different price ratios (0.5x to 10x)

#### Multiple Premium Slots
- **`property_test_multiple_premium_slots`**: Tests with 5 different premium levels (0%, 3%, 5%, 7%, 10%)
- **`property_test_uneven_slot_distribution`**: Realistic scenario with more liquidity at better premiums
- **`property_test_premium_boundary_conditions`**: Tests all 11 premium slots (0-10%)

#### Realistic Scenarios
- **`property_test_realistic_liquidation_scenarios`**: 4 real-world liquidation scenarios:
  - Small liquidation (0.5 CDT) with good liquidity
  - Medium liquidation (5 CDT) with scattered premiums
  - Large liquidation (50 CDT) needing multiple slots
  - Very large liquidation (100 CDT) with expensive collateral

#### Edge Cases
- **`property_test_edge_cases`**: Tests tiny amounts (1 unit), odd numbers (333, 777), odd prices (137%, 213%)
- **`property_test_stress_large_values`**: Tests with very large values (1M CDT, $50 collateral price)

## Test Statistics

- **Total test functions**: 20+
- **Total test executions**: 100+ (many tests loop over multiple scenarios)
- **Scenarios covered**:
  - Debt amounts: 1 to 1,000,000,000,000 (1 unit to 1M CDT)
  - Premiums: 0% to 10% in 1% increments
  - Price ratios: 0.5x to 50x
  - Slot configurations: 1 to 11 slots

## How to Run Tests

```bash
# Run all liquidation queue tests
cd /Users/EBmic/membrane-core/contracts/liq-queue
cargo test

# Run only CollateralForDebt tests
cargo test collateral_for_debt

# Run only property tests
cargo test property_test

# Run with output for detailed assertions
cargo test -- --nocapture
```

## Critical Assertions

Every comparison/property test includes this critical assertion:

```rust
assert!(
    debt_repaid >= debt_amount,
    "CollateralForDebt returned insufficient collateral! \
     Wanted to repay: {}, Actually repaid: {}, Collateral used: {}",
    debt_amount,
    debt_repaid,
    collateral_needed
);
```

If this assertion ever fails, it means:
- The math in `CollateralForDebt` is incorrect
- Premium calculation is wrong
- There's a rounding error that's too large
- **The query would cause incomplete liquidations in production**

## Math Verification

The formula used in `CollateralForDebt`:

```rust
// For each premium slot:
debt_repayable_by_slot = min(remaining_debt, slot_total_bids)
debt_value = debt_repayable_by_slot * credit_price
collateral_needed = debt_value / (collateral_price * (1 - premium))
```

This is the inverse of the formula used in `CheckLiquidatible`:

```rust
// CheckLiquidatible uses:
collateral_value = collateral_amount * collateral_price * (1 - premium)
debt_repaid = collateral_value / credit_price
```

The tests verify that these formulas are consistent inverses of each other.

## Tolerance for Rounding

Due to Decimal256 arithmetic, small rounding differences are expected. The tests allow for:
- **Standard tolerance**: ±1-3 units depending on amounts
- **Large number tolerance**: 0.001% (1/100000) for amounts > 1M
- **Minimum tolerance**: At least ±10 units for very large amounts

This tolerance is conservative and well within acceptable bounds for liquidation operations.

## Continuous Testing

These tests should be run:
1. Before any changes to `CollateralForDebt` or `CheckLiquidatible`
2. Before any changes to premium calculation logic
3. Before any changes to the liquidation queue contract
4. As part of CI/CD pipeline
5. Before deploying contract updates

## Future Enhancements

Potential additional tests:
- Fuzz testing with random inputs
- Tests with non-power-of-10 decimals
- Tests with CW20 tokens (once supported)
- Gas consumption benchmarks
- Integration tests with actual liquidation flow from CDP contract
