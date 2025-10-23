# Dynamic LTV Mechanism Implementation

## Overview

This document summarizes the implementation of a dynamic LTV (Loan-to-Value) update mechanism for the CDP contract. The mechanism automatically adjusts basket collateral asset LTVs based on average LTVs queried from the LTV Disco contract, using control theory for upward movements and staged, capped shifts for downward movements.

## Implementation Summary

### 1. State Storage (`contracts/cdp/src/state.rs`)

Added new state tracking structure:

```rust
pub struct LTVUpdateTracker {
    pub last_upward_update: u64,           // Timestamp of last upward accrual
    pub staged_max_ltv: Option<Decimal>,   // Staged downward max_LTV
    pub staged_max_borrow_ltv: Option<Decimal>, // Staged downward max_borrow_LTV
    pub staged_timestamp: Option<u64>,     // Timer start for staged downward shift
}

pub const LTV_UPDATE_TRACKERS: Map<String, LTVUpdateTracker>
```

### 2. Configuration (`packages/membrane/src/cdp.rs`)

Added three new parameters to the `Config` struct:

- `ltv_upward_kp: Decimal` - Proportional gain for upward LTV accrual (default: 5% per day)
- `ltv_downward_period: u64` - Period for downward LTV shifts in seconds (default: 604,800 = 1 week)
- `ltv_max_downward_shift: Decimal` - Max downward shift per period (default: 5%)

### 3. Core Logic (`contracts/cdp/src/ltv_updater.rs`)

New module implementing the dynamic LTV mechanism with:

#### Upward Movement (Proportional Controller)

When Disco LTV > Static LTV:
```
error = disco_ltv - static_ltv
time_factor = time_elapsed / SECONDS_PER_DAY
accrual = kp * error * time_factor
new_static_ltv = min(static_ltv + accrual, disco_ltv)
```

**Benefits:**
- Faster catch-up when far behind
- Naturally slows as it approaches target
- Cannot overshoot
- Conservative default Kp (5%) prevents wild swings during low TVL periods

#### Downward Movement (Staged with Timer)

When Disco LTV < Static LTV:

1. **First Detection:** Stage the lower Disco LTV and start timer
2. **During Wait Period:** Update staged value if Disco drops further (without resetting timer)
3. **After Period Elapses:** Apply capped shift and clear staged values

```
max_allowed_shift = current_ltv * max_shift_percent
actual_shift = min(desired_shift, max_allowed_shift)
new_ltv = current_ltv - actual_shift
```

**Benefits:**
- Provides time for users to adjust positions before liquidation
- Protects against sudden drops
- Last-minute changes don't reset the timer

### 4. ExecuteMsg Integration

Added new permissionless message:
```rust
ExecuteMsg::UpdateBasketLTVs {}
```

This can be called by anyone to trigger LTV updates.

### 5. Automatic Updates

Integrated into the accrue mechanism in `contracts/cdp/src/rates.rs`:
- LTV updates run automatically before position interest accrual
- Ensures positions always use most up-to-date LTVs

### 6. Safety Features

- **Bounds Checking:** LTVs are capped at 0-100%
- **Relationship Enforcement:** `max_borrow_LTV < max_LTV` always maintained
- **Fallback Behavior:** If Disco returns zero (no deposits), static values are used
- **Error Isolation:** Failed updates for one asset don't prevent updates for others

## Testing

Comprehensive test suite in `contracts/cdp/src/testing/ltv_dynamic_tests.rs`:

- ✅ Proportional controller accrual
- ✅ No overshoot protection
- ✅ Multi-period convergence
- ✅ Zero time elapsed handling
- ✅ Downward shift capping
- ✅ Under-cap downward shifts
- ✅ Tracker initialization
- ✅ Timer staging behavior
- ✅ Controller convergence
- ✅ Low Kp slower convergence
- ✅ LTV bounds capping

All 12 tests passing ✓

## Usage

### Direct Call

Anyone can call the update function:
```rust
ExecuteMsg::UpdateBasketLTVs {}
```

### Automatic Updates

Updates occur automatically during:
```rust
ExecuteMsg::Accrue { position_owner, position_ids }
```

### Configuration Updates

Governance can adjust parameters:
```rust
ExecuteMsg::UpdateConfig(UpdateConfig {
    ltv_upward_kp: Some(Decimal::percent(3)),        // Adjust accrual rate
    ltv_downward_period: Some(1_209_600),            // Change to 2 weeks
    ltv_max_downward_shift: Some(Decimal::percent(3)), // Reduce max shift
    ..Default::default()
})
```

## Default Configuration

- **Upward Kp:** 5% per day (conservative for low TVL periods)
- **Downward Period:** 1 week (604,800 seconds)
- **Max Downward Shift:** 5% per period

## Migration Notes

For existing deployments:
1. Default values are set in the `instantiate` function
2. Existing test cases updated to include new optional config fields
3. No breaking changes to existing functionality
4. Trackers are initialized on first use per asset

## Files Modified

1. `/Users/EBmic/membrane-core/contracts/cdp/src/state.rs` - Added LTVUpdateTracker
2. `/Users/EBmic/membrane-core/packages/membrane/src/cdp.rs` - Updated Config & UpdateConfig
3. `/Users/EBmic/membrane-core/contracts/cdp/src/ltv_updater.rs` - New module (created)
4. `/Users/EBmic/membrane-core/contracts/cdp/src/contract.rs` - Added ExecuteMsg handler
5. `/Users/EBmic/membrane-core/contracts/cdp/src/rates.rs` - Integrated into accrue
6. `/Users/EBmic/membrane-core/contracts/cdp/src/lib.rs` - Exported new module
7. `/Users/EBmic/membrane-core/contracts/cdp/src/testing/ltv_dynamic_tests.rs` - New tests
8. `/Users/EBmic/membrane-core/contracts/cdp/src/testing/mod.rs` - Added test module
9. `/Users/EBmic/membrane-core/contracts/cdp/src/testing/integration_tests.rs` - Updated config usage

## Build Status

✅ Package compiles successfully
✅ All new tests passing (12/12)
✅ No new linter errors introduced
✅ Release build successful

## Future Enhancements

Potential improvements for future consideration:
1. Per-asset configurable parameters (currently global)
2. Monitoring/alerting for downward shifts
3. Admin override for emergency situations
4. Historical LTV change tracking
5. Integration with governance proposals for parameter changes

