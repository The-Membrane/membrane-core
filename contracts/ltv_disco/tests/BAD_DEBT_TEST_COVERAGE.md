# Bad Debt Flow Test Coverage

## Summary
Comprehensive test suite for the bad debt flow between CDP and LTV Disco contracts. All tests passing: **21 tests total** (12 unit tests + 9 integration tests).

## Test Files

### 1. `bad_debt_tests.rs` (12 tests)
Unit tests for individual bad debt scenarios:

#### Revenue Fulfillment Tests
- ✅ `test_bad_debt_fully_covered_by_dispersals` - Bad debt fully covered by active dispersal
- ✅ `test_bad_debt_uses_active_then_pending_dispersal` - Uses active then pending dispersal

#### Deposit Slashing Tests
- ✅ `test_bad_debt_requires_deposit_slashing` - Slashes deposits when no dispersals available
- ✅ `test_bad_debt_slashes_highest_ltv_first` - Slashes from highest LTV slots first
- ✅ `test_bad_debt_early_exit_optimization` - Early exit optimization works correctly

#### Mixed Scenarios
- ✅ `test_bad_debt_mixed_revenue_and_slashing` - Combination of revenue and slashing
- ✅ `test_slot_bad_debt_tracking_with_multiple_groups` - Bad debt tracking across multiple groups

#### Security & Edge Cases
- ✅ `test_bad_debt_only_cdp_can_call` - Only CDP can call AddBadDebt

#### Stress Tests
- ✅ `stress_test_massive_bad_debt` - 50M CDT bad debt (50% of deposits)
- ✅ `stress_test_sequential_bad_debt_events` - 20 sequential bad debt events
- ✅ `stress_test_extreme_price_ratios` - Extreme price ratios (100:1)
- ✅ `stress_test_depleting_all_deposits` - Bad debt exceeding all deposits

### 2. `bad_debt_integration_tests.rs` (9 tests)
End-to-end integration tests for the full CDP ↔ Disco flow:

#### End-to-End Flow Tests
- ✅ `test_end_to_end_bad_debt_fulfilled_by_revenue` - Full flow: CDP → Disco → CDP (revenue only)
- ✅ `test_end_to_end_bad_debt_with_slashing_and_swap` - Full flow with deposit slashing and swap
- ✅ `test_end_to_end_mixed_revenue_and_slashing` - Full flow with mixed revenue + slashing

#### Fuzzing Tests
- ✅ `fuzz_test_random_bad_debt_amounts` - Tests 9 different bad debt amounts (1 to 1B)
- ✅ `fuzz_test_random_price_ratios` - Tests 8 different price ratios (0.01 to 1000)

#### Edge Cases
- ✅ `test_bad_debt_exactly_equals_deposits` - Bad debt exactly equals available deposits
- ✅ `test_zero_bad_debt` - Zero bad debt handling
- ✅ `test_swap_failure_handling` - Swap failure scenario handling

#### Concurrency Tests
- ✅ `test_concurrent_bad_debt_events` - 50 sequential bad debt events with state consistency checks

## Test Coverage Areas

### ✅ Bad Debt Waterfall
- Revenue dispersals (active + pending)
- Deposit slashing (highest LTV first)
- Mixed fulfillment scenarios

### ✅ State Consistency
- Bad debt tracking in CDT terms
- Deposit group updates
- Slot-level bad debt accumulation
- Total deposit calculations

### ✅ Message Flow
- `AddBadDebt` from CDP
- `FulfillBadDebt` to CDP (with CDT)
- Swap message creation and reply handling
- Balance tracking before/after swaps

### ✅ Price Conversion
- Oracle price queries
- CDT to collateral conversion
- Collateral to CDT conversion
- Extreme price ratios

### ✅ Security
- Authorization checks (only CDP can call)
- Unauthorized access prevention

### ✅ Edge Cases
- Zero bad debt
- Bad debt exceeding deposits
- Exact deposit matching
- Swap failures
- Concurrent events

### ✅ Performance
- Early exit optimization
- Large-scale bad debt (50M CDT)
- Sequential event handling (50 events)
- Multiple deposit groups

## Key Test Scenarios Verified

1. **Revenue-First Fulfillment**: Bad debt is fulfilled from dispersals before slashing deposits
2. **Highest LTV First**: Deposits are slashed from highest LTV slots first (risk-based)
3. **State Tracking**: Bad debt is tracked in CDT terms at slot level
4. **Message Flow**: Proper `FulfillBadDebt` messages sent to CDP with correct amounts
5. **Swap Integration**: Collateral slashing triggers swap, reply handler sends CDT to CDP
6. **Price Handling**: Works correctly across wide range of price ratios
7. **Concurrency**: Multiple sequential bad debt events maintain state consistency
8. **Edge Cases**: Handles zero, exact matches, and exceeding deposits gracefully

## Running Tests

```bash
# Run all bad debt tests
cargo test --package ltv_disco --test bad_debt_tests --test bad_debt_integration_tests

# Run specific test file
cargo test --package ltv_disco --test bad_debt_tests
cargo test --package ltv_disco --test bad_debt_integration_tests

# Run with output
cargo test --package ltv_disco --test bad_debt_tests -- --nocapture
```

## Test Results
✅ **All 21 tests passing**
- 12 unit tests in `bad_debt_tests.rs`
- 9 integration tests in `bad_debt_integration_tests.rs`






















