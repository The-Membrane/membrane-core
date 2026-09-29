# CDP Contract Test Setup Guide for LLMs

This guide helps LLMs identify and fix common test setup issues in the CDP contract test suite. It's based on real issues encountered during development.

## Table of Contents
1. [Accrual Time Initialization](#accrual-time-initialization)
2. [Oracle Setup Requirements](#oracle-setup-requirements)
3. [Minimum Debt Requirements](#minimum-debt-requirements)
4. [Basket Totals Assertions](#basket-totals-assertions)
5. [Address Creation](#address-creation)
6. [Funding and Token Amounts](#funding-and-token-amounts)
7. [Common Error Patterns](#common-error-patterns)

---

## Accrual Time Initialization

### Problem
Tests that advance time and then accrue interest fail because `rates_last_accrued` in the basket isn't initialized, causing `time_elapsed` to be incorrect or zero.

### Symptoms
- Accrual tests fail with `amount_after == amount_before` (no accrual happened)
- Fixed rate expiration tests don't detect expiration
- Time-based calculations return zero or incorrect values

### Solution
**Always accrue once before advancing time** to initialize `basket.rates_last_accrued`.

### Pattern to Fix

**❌ WRONG:**
```rust
// Advance time first
advance_time(&mut app, SECONDS_PER_YEAR / 2);

// Accrue - this will fail because rates_last_accrued wasn't initialized
let accrue_msg = ExecuteMsg::Accrue {
    position_owner: Some(user_addr.to_string()),
    position_ids: vec![Uint128::new(1)],
};
app.execute(user_addr.clone(), cdp_contract.call(accrue_msg, vec![]).unwrap())
    .unwrap();
```

**✅ CORRECT:**
```rust
// Accrue once to initialize rates_last_accrued (needed for time_elapsed calculation)
let accrue_msg_init = ExecuteMsg::Accrue {
    position_owner: Some(user_addr.to_string()),
    position_ids: vec![Uint128::new(1)],
};
app.execute(user_addr.clone(), cdp_contract.call(accrue_msg_init, vec![]).unwrap())
    .unwrap();

// Now advance time
advance_time(&mut app, SECONDS_PER_YEAR / 2);

// Accrue again (now with time elapsed)
let accrue_msg = ExecuteMsg::Accrue {
    position_owner: Some(user_addr.to_string()),
    position_ids: vec![Uint128::new(1)],
};
app.execute(user_addr.clone(), cdp_contract.call(accrue_msg, vec![]).unwrap())
    .unwrap();
```

### When to Apply
- Any test that calls `advance_time()` followed by `Accrue`
- Tests checking fixed rate expiration
- Tests verifying accrual calculations
- Tests with time-based logic

### Files to Check
- `contracts/cdp/src/testing/multiple_debt_rate_tests.rs`
- `contracts/cdp/src/testing/integration_tests.rs`
- Any test file with accrual logic

---

## Oracle Setup Requirements

### Problem
Some operations require the oracle to be set (`basket.oracle_set == true`), but rollover-only updates should work without it.

### Symptoms
- Error: `"No repayment price set for this basket"`
- Rollover update tests fail even when no debt is being increased

### Solution
**For rollover-only updates** (when `amount` is `None` and only `rollover_updates` is provided), the contract should allow the operation without requiring oracle setup.

### Pattern to Fix

**❌ WRONG:**
```rust
// This will fail if oracle isn't set, even though we're not increasing debt
let update_msg = ExecuteMsg::IncreaseDebt {
    position_id: Uint128::new(1),
    amount: None,
    LTV: None,
    mint_to_addr: None,
    deployment_intent: None,
    debt_split: None,
    rollover_updates: Some(vec![(1u8, true)]),
};
```

**✅ CORRECT:**
The contract code should handle this (already fixed in `positions.rs`), but tests should ensure:
1. Oracle is set if you're increasing debt
2. Oracle doesn't need to be set for rollover-only updates

**Test Setup:**
```rust
// If you need oracle for debt increase, set it up:
let admin_addr = app.api().addr_make(ADMIN);
setup_basket_for_accrual(&mut app, &cdp_contract, &admin_addr);
// This sets base_interest_rate, which may also set oracle_set
```

### When to Apply
- Tests that only update rollover flags without increasing debt
- Tests that fail with "No repayment price set" when not increasing debt

---

## Minimum Debt Requirements

### Problem
The contract enforces a minimum debt requirement (default: 2000 USD = 2B tokens with 6 decimals). Repayments that would leave debt below this threshold will fail.

### Symptoms
- Error: `"Position's debt ({debt}) is below minimum: {minimum}"`
- Repayment tests fail when trying to repay too much

### Solution
**Calculate repayment amounts that leave the position above minimum debt**, or close the position entirely.

### Pattern to Fix

**❌ WRONG:**
```rust
// This will fail if repaying this amount leaves debt below minimum
let repay_amount = Uint128::new(25_000_000); // Might be too much
app.send_tokens(
    Addr::unchecked("bigger_bank"),
    user_addr.clone(),
    &[coin(repay_amount.u128(), "credit_fulldenom")],
)
.unwrap();

let repay_msg = ExecuteMsg::Repay {
    position_id: Uint128::new(1),
    position_owner: Some(user_addr.to_string()),
    send_excess_to: None,
    debt_split: Some(repay_split),
};
app.execute(user_addr.clone(), cdp_contract.call(repay_msg, vec![coin(repay_amount.u128(), "credit_fulldenom")]).unwrap())
    .unwrap();
```

**✅ CORRECT:**
```rust
// Get current position to calculate safe repayment amount
let position_before = get_position(&app, &cdp_contract, Uint128::new(1), &user_addr);
let total_debt = position_before.rate_segments.iter().map(|s| s.amount).sum::<Uint128>();

// Minimum debt is 2B tokens (2000 USD with 6 decimals)
const MIN_DEBT: Uint128 = Uint128::new(2_000_000_000);

// Calculate safe repayment: leave at least minimum debt
let max_repay = if total_debt > MIN_DEBT {
    total_debt.checked_sub(MIN_DEBT).unwrap_or(Uint128::zero())
} else {
    // Can't repay anything without going below minimum
    Uint128::zero()
};

// Or, if testing segment removal, repay most of a segment but leave a small amount
let one_month_segment = position_before.rate_segments.iter()
    .find(|s| s.fixed_rate.as_ref().map(|fr| fr.end.duration_months == 1).unwrap_or(false))
    .unwrap();
let one_month_amount = one_month_segment.amount;
// Leave 100M to ensure we stay above minimum (if other segments exist)
let repay_amount = one_month_amount.checked_sub(Uint128::new(100_000_000))
    .unwrap_or(one_month_amount);

// Fund and repay
app.send_tokens(
    Addr::unchecked("bigger_bank"),
    user_addr.clone(),
    &[coin(repay_amount.u128(), "credit_fulldenom")],
)
.unwrap();
```

### When to Apply
- Any test that repays debt
- Tests that remove segments by repaying them
- Tests that verify partial repayments

### Constants to Know
- Default minimum debt: `2_000_000_000` tokens (2B)
- This equals 2000 USD when credit price is $1 and decimals are 6

---

## Basket Totals Assertions

### Problem
Basket totals assertions may use outdated expected values that don't match current test debt amounts.

### Symptoms
- Assertion failures: `assert_eq!(basket.credit_asset.three_month_amount, Uint128::new(10_000_000))` fails
- Expected values are much smaller than actual values

### Solution
**Use the actual debt amount from the test**, not hardcoded small values.

### Pattern to Fix

**❌ WRONG:**
```rust
let increase_msg = ExecuteMsg::IncreaseDebt {
    position_id: Uint128::new(1),
    amount: Some(Uint128::new(2_000_000_000)), // 2B tokens
    // ...
};

// Later assertion uses wrong expected value
let basket = get_basket(&app, &cdp_contract);
assert_eq!(basket.credit_asset.three_month_amount, Uint128::new(10_000_000)); // ❌ Wrong!
```

**✅ CORRECT:**
```rust
let debt_amount = Uint128::new(2_000_000_000); // 2B tokens
let increase_msg = ExecuteMsg::IncreaseDebt {
    position_id: Uint128::new(1),
    amount: Some(debt_amount),
    debt_split: Some(DebtSplit {
        variable: Some(Decimal::zero()),
        three_month: Some(Decimal::percent(100)), // All in 3-month
        // ...
    }),
    // ...
};

// Use the actual debt amount in assertions
let basket = get_basket(&app, &cdp_contract);
assert_eq!(basket.credit_asset.three_month_amount, debt_amount); // ✅ Correct!
```

### When to Apply
- Any test that asserts basket totals
- Tests that verify debt split across rate categories
- Tests checking `credit_asset.variable_amount`, `one_month_amount`, `three_month_amount`, `six_month_amount`

---

## Address Creation

### Problem
Using `Addr::unchecked()` creates invalid bech32 addresses that fail validation in the contract.

### Symptoms
- Error: `"Error decoding bech32"`
- Instantiation fails
- Address validation errors

### Solution
**Always use `app.api().addr_make()` to create valid addresses** in tests.

### Pattern to Fix

**❌ WRONG:**
```rust
let admin_addr = Addr::unchecked(ADMIN);
let user_addr = Addr::unchecked(USER);
```

**✅ CORRECT:**
```rust
let admin_addr = app.api().addr_make(ADMIN);
let user_addr = app.api().addr_make(USER);
```

### Exception
For funding from "bigger_bank", you can use `Addr::unchecked("bigger_bank")` because it's only used for `send_tokens`, not contract calls:

```rust
// This is OK for send_tokens
app.send_tokens(
    Addr::unchecked("bigger_bank"),
    user_addr.clone(),
    &coins(100_000_000_000, "debit"),
)
.unwrap();
```

### When to Apply
- All address creation in tests
- Helper functions that return addresses
- Any place where addresses are passed to contract calls

---

## Funding and Token Amounts

### Problem
Tests may not fund users with enough tokens, or use amounts that are too small for minimum debt requirements.

### Symptoms
- Error: `"Overflow: Cannot Sub with given operands"` during deposit
- Error: `"Makes position insolvent"` when trying to borrow
- Insufficient funds errors

### Solution
**Fund users with sufficient tokens and use appropriate debt amounts.**

### Pattern to Fix

**❌ WRONG:**
```rust
// Not enough tokens
app.send_tokens(
    Addr::unchecked("bigger_bank"),
    user_addr.clone(),
    &coins(1_000_000, "debit"), // Too small
)
.unwrap();

// Deposit too little
app.execute(
    user_addr.clone(),
    cdp_contract.call(deposit_msg, coins(1_000_000, "debit")).unwrap(),
)
.unwrap();

// Try to borrow too little (below minimum)
let increase_msg = ExecuteMsg::IncreaseDebt {
    position_id: Uint128::new(1),
    amount: Some(Uint128::new(1_000_000)), // Below minimum of 2B
    // ...
};
```

**✅ CORRECT:**
```rust
// Fund with sufficient tokens (100B is safe for most tests)
app.send_tokens(
    Addr::unchecked("bigger_bank"),
    user_addr.clone(),
    &coins(100_000_000_000, "debit"), // 100B tokens
)
.unwrap();

// Deposit sufficient collateral
app.execute(
    user_addr.clone(),
    cdp_contract.call(deposit_msg, coins(100_000_000_000, "debit")).unwrap(),
)
.unwrap();

// Borrow at least minimum debt (2B tokens = 2000 USD)
let increase_msg = ExecuteMsg::IncreaseDebt {
    position_id: Uint128::new(1),
    amount: Some(Uint128::new(2_000_000_000)), // 2B = minimum debt
    // ...
};
```

### Recommended Amounts
- **Collateral deposit**: `100_000_000_000` (100B tokens) - provides enough collateral for borrowing
- **Debt increase**: `2_000_000_000` (2B tokens) - meets minimum debt requirement
- **Credit tokens for repayment**: Calculate based on actual debt, but ensure sufficient funds

### When to Apply
- All tests that deposit collateral
- All tests that increase debt
- All tests that repay debt
- Any test with funding operations

---

## Common Error Patterns

### Pattern 1: "Makes position insolvent"
**Cause**: Trying to borrow more than collateral value allows, or LTV disco returns incorrect max_borrow_LTV.

**Most Common Cause**: **Insufficient collateral deposited** - tests fund users with 100B but only deposit 1B, causing total_asset_value to be $1,000 instead of $100,000.

**Check**:
1. **FIRST**: Verify deposit amount matches funding amount (most common issue!)
   - If you fund with `coins(100_000_000_000, "debit")`, deposit should also be `coins(100_000_000_000, "debit")`
   - Check debug output: `total_asset_value (USD)` should be $100,000 for 100B tokens at $1
2. Is collateral value sufficient? (100B tokens at $1 = $100,000 should allow $2,000 debt)
3. Is LTV disco mock configured correctly?
4. Is oracle returning correct prices?

**Debug Output to Check**:
```
[INSOLVENCY_DEBUG] Asset 0 value calc: amount=100000000000, price=1, decimals=6, value=100000  ✅ Correct
[INSOLVENCY_DEBUG] Asset 0 value calc: amount=1000000000, price=1, decimals=6, value=1000     ❌ Wrong (only 1B deposited)
```

**Note**: This is often a test setup issue with deposit amounts, oracle/LTV disco mocks, not a contract bug.

### Pattern 2: "Position's debt is below minimum"
**Cause**: Repayment would leave debt below 2B tokens.

**Fix**: See [Minimum Debt Requirements](#minimum-debt-requirements) section.

### Pattern 3: "No repayment price set for this basket"
**Cause**: Trying to increase debt when oracle isn't set, OR contract bug when only updating rollover.

**Fix**: 
- For debt increases: Set up oracle first
- For rollover-only updates: Should work without oracle (check contract code)

### Pattern 4: Accrual not happening
**Cause**: `rates_last_accrued` not initialized before time advancement.

**Fix**: See [Accrual Time Initialization](#accrual-time-initialization) section.

### Pattern 5: Basket totals not updating
**Cause**: 
1. Accrual not happening (see Pattern 4)
2. Basket totals update logic issue in contract
3. Assertions using wrong expected values

**Fix**: 
- Check accrual is working
- Verify contract logic for basket totals updates
- Use correct expected values in assertions

---

## Helper Functions Reference

### Standard Test Setup Pattern

```rust
#[test]
fn test_example() {
    // 1. Instantiate contracts
    let (mut app, cdp_contract, _lq_contract) = proper_instantiate(false, false, false, false);
    
    // 2. Get addresses (use addr_make, not unchecked)
    let admin_addr = app.api().addr_make(ADMIN);
    let user_addr = app.api().addr_make(USER);
    
    // 3. Setup basket for accrual (sets interest rates)
    setup_basket_for_accrual(&mut app, &cdp_contract, &admin_addr);
    
    // 4. Fund user with sufficient tokens
    app.send_tokens(
        Addr::unchecked("bigger_bank"),
        user_addr.clone(),
        &coins(100_000_000_000, "debit"),
    )
    .unwrap();
    
    // 5. Deposit collateral
    let deposit_msg = ExecuteMsg::Deposit {
        position_owner: Some(user_addr.to_string()),
        position_id: None,
        affiliate_address: None,
    };
    app.execute(
        user_addr.clone(),
        cdp_contract.call(deposit_msg, coins(100_000_000_000, "debit")).unwrap(),
    )
    .unwrap();
    
    // 6. Increase debt (at least 2B for minimum)
    let increase_msg = ExecuteMsg::IncreaseDebt {
        position_id: Uint128::new(1),
        amount: Some(Uint128::new(2_000_000_000)),
        // ...
    };
    app.execute(user_addr.clone(), cdp_contract.call(increase_msg, vec![]).unwrap())
        .unwrap();
    
    // 7. If testing accrual/time-based logic:
    //    - Accrue once to initialize rates_last_accrued
    //    - Advance time
    //    - Accrue again
    let accrue_msg_init = ExecuteMsg::Accrue {
        position_owner: Some(user_addr.to_string()),
        position_ids: vec![Uint128::new(1)],
    };
    app.execute(user_addr.clone(), cdp_contract.call(accrue_msg_init, vec![]).unwrap())
        .unwrap();
    
    advance_time(&mut app, SECONDS_PER_YEAR / 2);
    
    let accrue_msg = ExecuteMsg::Accrue {
        position_owner: Some(user_addr.to_string()),
        position_ids: vec![Uint128::new(1)],
    };
    app.execute(user_addr.clone(), cdp_contract.call(accrue_msg, vec![]).unwrap())
        .unwrap();
    
    // 8. Assertions
    let position = get_position(&app, &cdp_contract, Uint128::new(1), &user_addr);
    // ... verify results
}
```

---

## Quick Checklist

When fixing a failing test, check:

- [ ] Are addresses created with `app.api().addr_make()`?
- [ ] Is user funded with sufficient tokens (100B+)?
- [ ] **Is deposit amount matching funding amount?** (Common issue: fund 100B but deposit 1B)
- [ ] Is debt amount at least 2B (minimum requirement)?
- [ ] If testing accrual, is there an initial accrual call before advancing time?
- [ ] Are basket total assertions using correct expected values?
- [ ] If repaying, does it leave debt above minimum (2B)?
- [ ] Is `setup_basket_for_accrual` called if testing interest accrual?
- [ ] For rollover-only updates, is oracle setup required? (Shouldn't be)

---

## File Locations

- Test files: `contracts/cdp/src/testing/`
- Main test file: `contracts/cdp/src/testing/multiple_debt_rate_tests.rs`
- Integration tests: `contracts/cdp/src/testing/integration_tests.rs`
- Contract code: `contracts/cdp/src/`
- Types: `packages/membrane/src/types.rs`
- CDP messages: `packages/membrane/src/cdp.rs`

---

## Constants Reference

```rust
const SECONDS_PER_MONTH: u64 = 2_592_000; // 30 days
const SECONDS_PER_YEAR: u64 = 31_536_000;

// Minimum debt: 2000 USD = 2B tokens (with 6 decimals and $1 price)
const MIN_DEBT: Uint128 = Uint128::new(2_000_000_000);

// Recommended amounts
const RECOMMENDED_COLLATERAL: u128 = 100_000_000_000; // 100B tokens
const RECOMMENDED_DEBT: u128 = 2_000_000_000; // 2B tokens (minimum)
```

---

## Debugging Tips

1. **Add println! statements** to see actual values:
   ```rust
   let basket = get_basket(&app, &cdp_contract);
   println!("Basket totals: variable={}, one_month={}, three_month={}, six_month={}", 
       basket.credit_asset.variable_amount,
       basket.credit_asset.one_month_amount,
       basket.credit_asset.three_month_amount,
       basket.credit_asset.six_month_amount);
   ```

2. **Check position state**:
   ```rust
   let position = get_position(&app, &cdp_contract, Uint128::new(1), &user_addr);
   println!("Position segments: {:?}", position.rate_segments);
   ```

3. **Verify time advancement**:
   ```rust
   println!("Time before: {}", app.block_info().time.seconds());
   advance_time(&mut app, SECONDS_PER_YEAR);
   println!("Time after: {}", app.block_info().time.seconds());
   ```

4. **Check basket state**:
   ```rust
   let basket = get_basket(&app, &cdp_contract);
   println!("rates_last_accrued: {}", basket.rates_last_accrued);
   println!("oracle_set: {}", basket.oracle_set);
   ```

---

## Debugging Insolvency Check Failures

### Problem
Tests fail with `"Makes position insolvent: (true, Decimal(2), Uint128(1950))"` when trying to borrow debt, even when collateral value should be sufficient.

### Root Cause Analysis
The insolvency check compares:
- `current_LTV = debt_value / total_asset_value`
- Against `avg_borrow_LTV` (max borrow LTV) when `max_borrow=true`
- Against `avg_max_LTV` (liquidation LTV) when `max_borrow=false`

If `current_LTV > avg_borrow_LTV`, the position is considered insolvent.

### Debugging Steps

1. **Enable Debug Logging**
   The contract now includes comprehensive logging with `[INSOLVENCY_DEBUG]` prefix. Run tests with `--nocapture` to see all values:

   ```bash
   cargo test --package cdp --lib multiple_debt_rate -- --nocapture 2>&1 | grep "INSOLVENCY_DEBUG"
   ```

2. **Check Key Values**
   The logging will show:
   - Collateral asset amounts (raw tokens)
   - Asset prices from oracle
   - Asset values (price × amount)
   - Total asset value (sum of all asset values)
   - Debt amount (raw tokens)
   - Debt value (USD)
   - LTV disco responses (max_ltv, max_borrow_ltv)
   - Final avg_borrow_LTV and avg_max_LTV
   - Current LTV calculation
   - Final insolvency result

3. **Common Issues to Check**

   **Issue 1: Incorrect Asset Prices**
   - Check oracle mock returns correct prices
   - Verify price decimals match asset decimals
   - Example: If asset has 6 decimals and price is $1, value should be `amount / 10^6 * price`

   **Issue 2: LTV Disco Returns Wrong Values**
   - Check LTV disco mock returns reasonable LTVs (typically 50-80% for max_borrow_LTV)
   - If LTV disco returns zero, falls back to `cAsset.max_borrow_LTV` and `cAsset.max_LTV`
   - Verify fallback values are set correctly in basket creation

   **Issue 3: Total Asset Value Too Low**
   - Check if `total_asset_value` is calculated correctly
   - Verify all collateral assets are included
   - Check if price × amount calculation is correct

   **Issue 4: Debt Value Calculation**
   - Verify `credit_price.get_value(credit_amount)` is correct
   - Check credit price decimals match credit token decimals
   - Example: If credit has 6 decimals and price is $0.98, value should be `amount / 10^6 * 0.98`

### Example Debug Output Analysis

```
[INSOLVENCY_DEBUG] === ENTERING INSOLVENCY_CHECK ===
[INSOLVENCY_DEBUG] credit_amount: 2000000000
[INSOLVENCY_DEBUG] credit_price: price=0.98, decimals=6
[INSOLVENCY_DEBUG] debt_value (USD): 1960
[INSOLVENCY_DEBUG] === ENTERING get_avg_LTV ===
[INSOLVENCY_DEBUG] Asset 0 value calc: amount=100000000000, price=1, decimals=6, value=100000
[INSOLVENCY_DEBUG] total_asset_value (USD): 100000
[INSOLVENCY_DEBUG] ltv_tuples: max_ltv=0.75, max_borrow_ltv=0.50
[INSOLVENCY_DEBUG] Final avg_borrow_LTV: 0.50
[INSOLVENCY_DEBUG] current_LTV: 0.0196
[INSOLVENCY_DEBUG] Checking max_borrow: current_LTV (0.0196) > avg_borrow_LTV (0.50) = false
```

**Analysis**: 
- Debt value: $1,960
- Total asset value: $100,000
- Current LTV: 1.96% (very low, should pass)
- Max borrow LTV: 50%
- Result: Should NOT be insolvent (1.96% < 50%)

If this shows insolvent=true, check:
1. Is `avg_borrow_LTV` actually 0.50 or something much lower?
2. Is `total_asset_value` actually 100000 or much lower?
3. Is `debt_value` much higher than expected?

### Fixing Insolvency Issues

**If LTV Disco Returns Wrong Values:**
```rust
// In test setup, ensure LTV disco mock returns reasonable values
// Check integration_tests.rs for LTV disco mock implementation
// Typical values: max_borrow_LTV = 50-70%, max_LTV = 70-85%
```

**If Asset Prices Are Wrong:**
```rust
// Check oracle mock in integration_tests.rs
// Ensure prices match expected values (usually $1 for test assets)
// Verify decimals are correct (usually 6 for test assets)
```

**If Total Asset Value Is Too Low:**
```rust
// Check get_asset_values function
// Verify: value = price * (amount / 10^decimals)
// Example: 100B tokens at $1 with 6 decimals = 100,000 USD
```

**If Debt Value Is Too High:**
```rust
// Check credit_price.get_value() calculation
// Verify: value = price * (amount / 10^decimals)
// Example: 2B tokens at $0.98 with 6 decimals = 1,960 USD
```

### Test Setup for Insolvency Checks

Ensure your test setup includes:

```rust
// 1. Sufficient collateral (100B tokens = $100,000 at $1/ea)
app.send_tokens(
    Addr::unchecked("bigger_bank"),
    user_addr.clone(),
    &coins(100_000_000_000, "debit"), // 100B
)
.unwrap();

// 2. Deposit collateral
app.execute(
    user_addr.clone(),
    cdp_contract.call(deposit_msg, coins(100_000_000_000, "debit")).unwrap(),
)
.unwrap();

// 3. Borrow reasonable amount (2B = $2,000, well below $50,000 max at 50% LTV)
let increase_msg = ExecuteMsg::IncreaseDebt {
    position_id: Uint128::new(1),
    amount: Some(Uint128::new(2_000_000_000)), // 2B tokens
    // ...
};
```

### Expected Values for 100B Collateral, 2B Debt

With proper setup:
- **Collateral**: 100B tokens at $1 = $100,000
- **Debt**: 2B tokens at $0.98 = $1,960
- **Current LTV**: $1,960 / $100,000 = 1.96%
- **Max Borrow LTV**: Typically 50-70%
- **Result**: Should pass (1.96% < 50%)

If you see:
- `total_asset_value` much lower than $100,000 → Check oracle prices **OR check deposit amount in test**
- `avg_borrow_LTV` much lower than 0.50 → Check LTV disco mock
- `debt_value` much higher than $1,960 → Check credit price calculation

### Common Root Cause: Insufficient Collateral Deposited

**Problem Found**: Many tests were funding users with 100B tokens but only depositing 1B tokens, causing `total_asset_value` to be $1,000 instead of $100,000.

**Symptoms**:
- `total_asset_value (USD): 1000` instead of `100000`
- `current_LTV: 2` (200%) instead of `0.02` (2%)
- Position marked as insolvent even with reasonable debt

**Fix**:
```rust
// ❌ WRONG: Only deposits 1B tokens = $1,000 collateral
app.execute(
    user_addr.clone(),
    cdp_contract.call(deposit_msg, coins(1_000_000_000, "debit")).unwrap(),
)
.unwrap();

// ✅ CORRECT: Deposits 100B tokens = $100,000 collateral
app.execute(
    user_addr.clone(),
    cdp_contract.call(deposit_msg, coins(100_000_000_000, "debit")).unwrap(),
)
.unwrap();
```

**Check**: Ensure deposit amount matches funding amount. If you fund with 100B, deposit 100B (or at least enough to support the debt you want to borrow).

---

This guide should help LLMs quickly identify and fix common test setup issues in the CDP contract test suite.

---

## Systematic Test Debugging Process for CDP Tests

This section documents the systematic approach used to debug and fix 10 failing tests in `multiple_debt_rate_tests.rs`, reducing them to 0 failures. Use this process when facing multiple test failures.

### Step 1: Run All Tests and Capture Output

```bash
cargo test --package cdp --lib multiple_debt_rate -- --nocapture 2>&1 | tee test_output.txt
```

Count failures and record the total: e.g., "10 of 19 tests failing."

### Step 2: Categorize Failures by Root Cause

Group failing tests by their error messages/patterns rather than fixing one-by-one:

| Category | Error Pattern | Tests Affected | Root Cause |
|----------|--------------|----------------|------------|
| Minimum debt violations | `"Position's debt (X) is below minimum: 2000"` | 5 repayment tests | Initial debt of 2B ($2,000) is exactly the minimum; any repayment drops below |
| Insufficient funding | `"Overflow: Cannot Sub with given operands"` on deposit | 1 stress test | User not funded with enough tokens for multiple deposits |
| Wrong address type | `"Overflow: Cannot Sub with given operands"` on send_tokens | 1 stress test | `addr_make("bigger_bank")` creates different address than `Addr::unchecked("bigger_bank")` which is pre-funded |
| Missing init accrual | Assertion failures (no accrual happened) | 2 accrual tests | No initial `Accrue` call before `advance_time` |
| Contract logic bugs | Basket totals not updating correctly | Multiple tests | Unsigned integer arithmetic or timing bugs in `rates.rs` |

### Step 3: Fix Test Setup Issues First (Low-Risk)

Address setup issues before touching contract code:

#### Fix: Increase Debt for Repayment Tests
```rust
// ❌ WRONG: 2B = $2,000 (exactly minimum, can't repay anything)
amount: Some(Uint128::new(2_000_000_000)),

// ✅ CORRECT: 4B = $4,000 (leaves room for repayment)
amount: Some(Uint128::new(4_000_000_000)),
```

#### Fix: Fund User for Multi-Position Tests
```rust
// ❌ WRONG: Default funding, not enough for 5 × 100B deposits
// (no explicit funding)

// ✅ CORRECT: Fund 500B before loop
app.send_tokens(
    Addr::unchecked("bigger_bank"),
    user_addr.clone(),
    &coins(500_000_000_000, "debit"),
).unwrap();
```

#### Fix: Use Correct Address Type for Pre-Funded Accounts
```rust
// ❌ WRONG: addr_make creates a different bech32 address
let bigger_bank_addr = app.api().addr_make("bigger_bank");
app.send_tokens(bigger_bank_addr, user_addr.clone(), &coins);

// ✅ CORRECT: mock_app() pre-funds Addr::unchecked("bigger_bank")
app.send_tokens(Addr::unchecked("bigger_bank"), user_addr.clone(), &coins);
```

#### Fix: Add Init Accrual Before Time Advancement
```rust
// ❌ WRONG: advance_time without initializing rates_last_accrued
advance_time(&mut app, SECONDS_PER_MONTH + 86400);
// Accrue — but rates_last_accrued is 0, so time_elapsed is wrong

// ✅ CORRECT: accrue once first to set rates_last_accrued
let accrue_msg_init = ExecuteMsg::Accrue {
    position_owner: Some(user_addr.to_string()),
    position_ids: vec![Uint128::new(1)],
};
app.execute(user_addr.clone(), cdp_contract.call(accrue_msg_init, vec![]).unwrap()).unwrap();
advance_time(&mut app, SECONDS_PER_MONTH + 86400);
// Now accrue with correct time_elapsed
```

### Step 4: Re-Run Tests After Setup Fixes

```bash
cargo test --package cdp --lib multiple_debt_rate -- --nocapture 2>&1
```

Track progress: "Went from 10 failures → 6 failures after setup fixes."

### Step 5: Investigate Remaining Failures (Potential Contract Logic Issues)

Add debug println! to remaining failing tests:

```rust
println!("basket before: variable={}, one_month={}, three_month={}, six_month={}",
    basket_before.credit_asset.variable_amount,
    basket_before.credit_asset.one_month_amount,
    basket_before.credit_asset.three_month_amount,
    basket_before.credit_asset.six_month_amount);
// ... run operation ...
println!("basket after: variable={}, one_month={}, three_month={}, six_month={}",
    basket_after.credit_asset.variable_amount,
    basket_after.credit_asset.one_month_amount,
    basket_after.credit_asset.three_month_amount,
    basket_after.credit_asset.six_month_amount);
```

### Step 6: Identify and Fix Contract Logic Bugs

Two bugs were found and fixed in `contracts/cdp/src/rates.rs`:

#### Bug 1: Unsigned Integer Basket Delta Tracking
**Problem**: `basket_deltas` used `Uint128` (unsigned) to track changes, but converting fixed→variable requires *subtracting* from one category and *adding* to another. `Uint128::checked_sub` from 0 returns 0 via `unwrap_or`, silently losing the subtraction.

**Fix**: Use separate `basket_adds` and `basket_subs` tuples:
```rust
// ❌ WRONG: single Uint128 delta can't go negative
let mut basket_deltas = (Uint128::zero(), Uint128::zero(), Uint128::zero(), Uint128::zero());
1 => basket_deltas.1 = basket_deltas.1.checked_sub(amount).unwrap_or(Uint128::zero()), // Always 0!

// ✅ CORRECT: separate add/sub tracking
let mut basket_adds = (Uint128::zero(), Uint128::zero(), Uint128::zero(), Uint128::zero());
let mut basket_subs = (Uint128::zero(), Uint128::zero(), Uint128::zero(), Uint128::zero());
1 => basket_subs.1 += amount,
// Apply: basket.one_month = basket.one_month + adds.1 - subs.1
```

#### Bug 2: rates_last_accrued Updated Before Fixed Rate Accrual Reads It
**Problem**: `get_credit_rate_of_change()` internally calls `update_rate_indices()` which sets `basket.rates_last_accrued = current_time`. Fixed rate accrual then reads `last_accrued_time` which already equals `current_time`, making `time_elapsed = 0`.

**Fix**: Save `last_accrued_time` *before* calling `get_credit_rate_of_change()`:
```rust
// ❌ WRONG: rates_last_accrued already updated by get_credit_rate_of_change
let (rate_of_change, ratios) = get_credit_rate_of_change(...)?;
let last_accrued_time = basket.rates_last_accrued; // == current_time!

// ✅ CORRECT: save before the call modifies it
let last_accrued_time = basket.rates_last_accrued;
let current_time = env.block.time.seconds();
let (rate_of_change, ratios) = get_credit_rate_of_change(...)?;
```

### Step 7: Verify No Regressions

After all fixes, run the full test suite:
```bash
cargo test --package cdp --lib multiple_debt_rate -- --nocapture 2>&1
# Result: 19/19 passing

# Also verify pre-existing integration tests aren't affected:
cargo test --package cdp --lib integration_tests -- 2>&1 | tail -1
# Note: pre-existing failures (77) should remain unchanged
```

### Key Takeaways

1. **Categorize before fixing** — grouping failures by root cause prevents redundant work
2. **Fix test setup first** — 40% of failures (4/10) were pure test setup issues
3. **Look for unsigned arithmetic traps** — `Uint128` can't represent negative deltas; use separate add/sub tracking
4. **Check mutation order** — functions that modify shared state (like `basket.rates_last_accrued`) must be called in the right order relative to code that reads that state
5. **Use 4B debt for repayment tests** — provides $2,000 headroom above the $2,000 minimum
6. **Always use `app.api().addr_make("bigger_bank")` or `api.addr_make("bigger_bank")` (in AppBuilder)** for addresses — `Addr::unchecked("bigger_bank")` fails bech32 validation

---

## Bech32 Address Migration (Critical)

### Problem
The CDP contract validates all addresses as bech32 format. Old tests used `Addr::unchecked("name")` with human-readable strings which now fail validation.

### Comprehensive Fix Required

1. **`mock_app()` / `AppBuilder::build()`**: Use `api.addr_make("name")` (the second closure param):
```rust
// ❌ OLD
AppBuilder::new().build(|router, _, storage| {
    bank.init_balance(storage, &Addr::unchecked("bigger_bank"), vec![...]).unwrap();

// ✅ NEW
AppBuilder::new().build(|router, api, storage| {
    bank.init_balance(storage, &api.addr_make("bigger_bank"), vec![...]).unwrap();
```

2. **Test execution calls**: Use `app.api().addr_make("name")`:
```rust
// ❌ OLD
app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();

// ✅ NEW
app.execute(app.api().addr_make("bigger_bank"), cosmos_msg).unwrap();
```

3. **String params in messages** (position_owner, user, send_to, affiliate_address):
```rust
// ❌ OLD
position_owner: Some("bigger_bank".to_string()),
affiliate_address: "affiliate1".to_string(),

// ✅ NEW
position_owner: Some(app.api().addr_make("bigger_bank").to_string()),
affiliate_address: app.api().addr_make("affiliate1").to_string(),
```

4. **Balance queries**:
```rust
// ❌ OLD
app.wrap().query_all_balances("contract5").unwrap()

// ✅ NEW - query config first to get actual contract address
let cfg: Config = app.wrap().query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {}).unwrap();
app.wrap().query_all_balances(&cfg.staking_contract.unwrap().to_string()).unwrap()
```

5. **Hardcoded contract addresses** ("contract8", "contract9") no longer valid:
```rust
// ❌ OLD
discounts_contract: Some(String::from("contract8")),

// ✅ NEW - query config for actual bech32 contract address
let config: Config = app.wrap().query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {}).unwrap();
discounts_contract: Some(config.discounts_contract.unwrap().to_string()),
```

6. **DO NOT change** addresses in:
   - Mock contract handler closures (the `mock_*_contract()` functions)
   - Struct constructors used in mock query responses (e.g., `owner: Addr::unchecked("")`)
   - Contract labels in `instantiate_contract(... "test" ...)` calls

### Struct Field Changes (Old Tests to New)

When migrating old test code, these struct fields changed:

| Old Field | New Replacement |
|-----------|----------------|
| `hike_rates: Some(false)` on cAsset | `force_redemptions: None` |
| `hike_rates` on EditcAsset | Remove entirely |
| `rate_hike_rate` on UpdateConfig | Remove; add `max_comparative_rate_value: None` |
| (missing) on IncreaseDebt | Add `debt_split: None, rollover_updates: None` |
| (missing) on Deposit | Add `affiliate_address: None` |
| (missing) on Repay | Add `debt_split: None` |

