# Dynamic LTV Update State Machine (v2)

## Overview

The Collateral contract dynamically adjusts `max_LTV` and `max_borrow_LTV` for each collateral asset based on weighted average LTV values from the Disco contract. The system uses **asymmetric control**:

- **Upward:** Exponential convergence (path-independent, same result regardless of call frequency)
- **Downward:** Staged two-phase with mandatory delay period and decay-based shifts
- **Staging blocks upward:** While a downward shift is pending, all upward accrual is blocked (mandatory cooldown)

`max_borrow_LTV` is always derived: `max_borrow_LTV = max_LTV - ltv_borrow_distance`. Only `max_LTV` goes through the state machine.

---

## Config Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| `ltv_upward_kp` | 5% (0.05) | Exponential convergence rate per day |
| `ltv_downward_period` | 604,800s (1 week) | Mandatory delay before applying a downward shift |
| `ltv_downward_decay_rate` | 40% (0.40) | Fraction of the total gap applied per period |
| `ltv_borrow_distance` | 10% (0.10) | Fixed gap: `borrow_LTV = max_LTV - distance` |

---

## Per-Asset State

| Field | Storage | Description |
|-------|---------|-------------|
| `last_update_timestamp` | LTVUpdateTracker | Timestamp of last update call (both directions) |
| `staged_max_ltv` | LTVUpdateTracker | Pending downward target (`None` = no pending shift) |
| `staged_timestamp` | LTVUpdateTracker | Timer start for pending shift (`None` = no pending) |
| `manually_set_max_ltv` | MANUAL_MAX_LTVS map | Admin-set baseline LTV (drift target when Disco returns 0) |
| `disco_blocked` | DISCO_BLOCKED map | `true` when Disco returned 0 (blocks deposits via supply cap logic) |

```rust
LTVUpdateTracker {
    last_update_timestamp: u64,
    staged_max_ltv: Option<Decimal>,
    staged_timestamp: Option<u64>,
}
```

---

## States

### 1. STABLE
- **Condition:** `disco_ltv == current_ltv` AND no staged value exists
- **Behavior:** No update. LTV values remain unchanged.

### 2. ACCRUING_UPWARD
- **Condition:** `disco_ltv > current_ltv` AND **no staged value exists**
- **Behavior:** Exponential convergence toward Disco target.
- **Formula:** `new_ltv = disco - (disco - current) * e^(-kp * time_elapsed / 86400)`
- **Key property:** Path-independent — result is identical regardless of call frequency.
- **Constraint:** Cannot enter this state while a staged value exists (staging blocks upward).

### 3. STAGED_DOWNWARD
- **Condition:** `disco_ltv < current_ltv` AND no staged value exists
- **Behavior:** Records the Disco LTV as a staged value and starts the delay timer.
- **Tracker updates:**
  - `staged_max_ltv = Some(disco_ltv)`
  - `staged_timestamp = Some(current_time)`
- **LTV unchanged.**

### 4. WAITING
- **Condition:** Staged value exists AND `current_time < staged_timestamp + downward_period`
- **Behavior:** Timer is running. **LTV is frozen** regardless of Disco direction.
- **Bidirectional update rule:**
  - If `disco_ltv < current_ltv`: `staged_max_ltv = disco_ltv` (tracks Disco exactly)
  - If `disco_ltv >= current_ltv`: `staged_max_ltv = current_ltv` (zero pending shift, but timer still runs)
- **Key property:** Even if Disco recovers above current during the wait, the full delay period must elapse. This creates a mandatory cooldown after any downward signal.

### 5. APPLYING_DOWNWARD
- **Condition:** Staged value exists AND `current_time >= staged_timestamp + downward_period`
- **Behavior:** Applies decay shift, then clears staged values.
- **Formula:** `actual_shift = (current_ltv - staged_ltv) * decay_rate`
- **Result:** `new_max_ltv = current_ltv - actual_shift`
- **Side effect:** Clears `staged_max_ltv` and `staged_timestamp`. Next call evaluates fresh.
- **Note:** If staged was set to `current_ltv` (Disco recovered), the shift is zero — effectively a no-op that just clears the cooldown.

### 6. DISCO_ZERO_DRIFT
- **Condition:** Disco returns `average_max_ltv = 0` (no deposits for this asset)
- **Pre-processing:** Before entering the state machine:
  - Set `disco_ltv = manually_set_max_ltv` (admin-set baseline)
  - Set `DISCO_BLOCKED = true` (blocks new deposits via supply cap breach logic)
- **Behavior:** Enters the normal state machine with `disco_ltv = manually_set_max_ltv`. If `manual < current`, this triggers STAGED_DOWNWARD or updates an existing staged value.
- **Unblocking:** When Disco returns non-zero, `DISCO_BLOCKED = false`.

---

## State Diagram

```
                        disco == current
                        (no staged)
                     +-------------------+
                     v                   |
                +--------+              |
   +----------->| STABLE |<--------+   |
   |            +--------+         |   |
   |               |    |          |   |
   |    disco >    |    | disco <  |   |
   |    current    |    | current  |   |
   |  (no staged!) |    |          |   |
   |               v    v          |   |
   |   +----------+    +--------+  |   |
   |   | ACCRUING |    | STAGED |  |   |
   |   | UPWARD   |    | (start)|  |   |
   |   | (exp.)   |    +--------+  |   |
   |   +----------+       |        |   |
   |        |              v        |   |
   |        |     +-----------+     |   |
   |        |     |  WAITING  |     |   |
   |        |     |  (frozen) |     |   |
   |        |     |  (bidi    |     |   |
   |        |     |   update) |     |   |
   |        |     +-----------+     |   |
   |        |          |            |   |
   |        |          | timer      |   |
   |   BLOCKED!        | elapsed    |   |
   |   (staged         v            |   |
   |    exists)   +-----------+     |   |
   |              | APPLYING  |-----+   |
   +<-------------| (decay)   |         |
    (next call)   +-----------+         |
                                        |
   DISCO_ZERO_DRIFT:                    |
   Disco returns 0 ->                  |
     disco = manual_ltv                 |
     DISCO_BLOCKED = true               |
     Enters normal flow ----------------+
   (Non-zero Disco clears DISCO_BLOCKED)
```

**Critical rule:** The arrow from ACCRUING_UPWARD to WAITING is labeled "BLOCKED!" — you cannot enter ACCRUING_UPWARD while a staged value exists. The staged shift must complete (APPLYING clears it) before upward accrual can resume.

---

## Transition Table

| From | Condition | To | Action |
|------|-----------|-----|--------|
| ANY (staged exists) | timer elapsed | APPLYING | Apply decay shift; clear staged |
| ANY (staged exists) | timer NOT elapsed, `disco < current` | WAITING | `staged = disco` |
| ANY (staged exists) | timer NOT elapsed, `disco >= current` | WAITING | `staged = current` (zero shift, cooldown) |
| NO staged | `disco > current` | ACCRUING_UPWARD | Exponential convergence |
| NO staged | `disco < current` | STAGED_DOWNWARD | Stage value; start timer |
| NO staged | `disco == current` | STABLE | No change |
| PRE-MACHINE | Disco returns 0 | DISCO_ZERO_DRIFT | `disco = manual_ltv`; block deposits |
| PRE-MACHINE | Disco returns non-zero | (normal flow) | Unblock deposits |

---

## Per-Call Flow

```
UpdateBasketLTVs (permissionless)
  |
  +-- For each collateral asset in basket:
  |     |
  |     +-- Query Disco: GetAverageLTVs { assets: [denom] }
  |     |
  |     +-- PRE-MACHINE: Disco zero handling
  |     |     If disco == 0:
  |     |       disco_ltv = manually_set_max_ltv
  |     |       DISCO_BLOCKED = true
  |     |     Else:
  |     |       disco_ltv = disco response
  |     |       DISCO_BLOCKED = false
  |     |
  |     +-- Load or init LTVUpdateTracker
  |     |     (On first init: save current max_LTV to MANUAL_MAX_LTVS)
  |     |
  |     +-- STATE MACHINE: process_ltv_update()
  |     |     Priority 1: If staged exists
  |     |       If timer elapsed -> APPLYING (decay shift, clear staged)
  |     |       Else -> WAITING (update staged bidirectionally, LTV frozen)
  |     |     Priority 2: No staged
  |     |       disco > current -> ACCRUING_UPWARD (exponential)
  |     |       disco < current -> STAGED_DOWNWARD (start timer)
  |     |       disco == current -> STABLE (no change)
  |     |
  |     +-- If updated:
  |     |     Set max_LTV = new value
  |     |     Derive max_borrow_LTV = max_LTV - ltv_borrow_distance
  |     |     Cap both to [0%, 100%], ensure borrow < max
  |     |     Record historical snapshot (daily granularity)
  |     |
  |     +-- Update last_update_timestamp
  |     +-- Save tracker
  |     |
  |     +-- On error: log warning, continue to next asset
  |
  +-- Save updated basket
  +-- Return response with attributes
```

---

## Borrow LTV Derivation

The `max_borrow_LTV` is never tracked independently. After every `max_LTV` update:

```
max_borrow_LTV = max(0, max_LTV - ltv_borrow_distance)
```

**Example with default 10% distance:**
- `max_LTV = 80%` -> `max_borrow_LTV = 70%`
- `max_LTV = 60%` -> `max_borrow_LTV = 50%`
- `max_LTV = 8%` -> `max_borrow_LTV = 0%` (floored)

After derivation, a safety check ensures `max_borrow_LTV < max_LTV`. If they're equal, borrow is capped to 95% of max.

---

## Upward Accrual: Exponential Convergence

**Formula:**
```
new_ltv = disco - (disco - current) * e^(-kp * time_elapsed / 86400)
```

**Path independence:** The result is identical regardless of how frequently the function is called:
- One call over 24 hours = same as 24 calls over 1 hour each
- This is because `current_ltv` encodes all prior history

**Implementation:** Uses a 3-term Taylor series approximation `e^x ≈ 1 + x + x²/2 + x³/6` with the exponent capped at 1.0 for <2% error. For `e^(-x)`, compute `1 / e^x`.

**Convergence behavior:** With `kp = 5%`:
- After 1 day: covers `1 - e^(-0.05)` ≈ 4.88% of the gap
- After 1 week: covers `1 - e^(-0.35)` ≈ 29.5% of the gap
- After 2 weeks: covers `1 - e^(-0.70)` ≈ 50.3% of the gap
- Half-life ≈ 13.9 days

**Example:** Current 50%, Disco 80%, kp = 5%
- Day 1: `80 - 30 * e^(-0.05)` = 80 - 28.54 = 51.46%
- Day 7: `80 - 30 * e^(-0.35)` = 80 - 21.15 = 58.85%
- Day 14: `80 - 30 * e^(-0.70)` = 80 - 14.88 = 65.12%

---

## Downward Shift: Decay Rate

**Formula:**
```
total_shift = current_ltv - staged_ltv
actual_shift = total_shift * decay_rate
new_ltv = current_ltv - actual_shift
```

Where `decay_rate` defaults to 40%.

**Convergence behavior:** Each period takes a fraction of the remaining gap:
- Period 1: shift = gap * 0.4 (covers 40% of gap)
- Period 2: shift = remaining_gap * 0.4 (covers 24% of original gap)
- Period 3: shift = remaining_gap * 0.4 (covers 14.4% of original gap)
- ...converges toward staged value with exponentially decreasing steps

**Example:** Current 80%, Disco drops to 40%, decay_rate = 40%
- Period 1: gap = 40%, shift = 16%, new LTV = 64%
- Period 2: gap = 24%, shift = 9.6%, new LTV = 54.4%
- Period 3: gap = 14.4%, shift = 5.76%, new LTV = 48.64%
- ...converges toward 40%

---

## Mandatory Cooldown

After any downward signal (Disco drops below current), the full `downward_period` must elapse before upward accrual can resume, even if Disco recovers:

1. Disco drops below current → STAGED_DOWNWARD (timer starts)
2. Disco recovers above current during wait → `staged = current` (zero shift), but timer keeps running
3. Timer elapses → APPLYING applies zero shift, clears staged
4. **Next call:** No staged value → can now enter ACCRUING_UPWARD

This prevents rapid oscillation and ensures stability after volatile signals.

---

## Zero Disco: Drift + Deposit Blocking

When Disco returns `average_max_ltv = 0` (no deposits for an asset):

1. **Drift target:** `disco_ltv = manually_set_max_ltv` (admin-set baseline). The state machine proceeds normally with this substituted value, causing a gradual drift back to the initial parameters.
2. **Deposit blocking:** `DISCO_BLOCKED = true` for this asset. This integrates into the existing supply cap check logic — a blocked asset behaves identically to a supply cap breach:
   - Blocks new deposits of the asset
   - Blocks withdrawals of OTHER assets that would increase the blocked asset's ratio
   - Allows direct withdrawal of the blocked asset itself
3. **Unblocking:** When Disco returns non-zero, `DISCO_BLOCKED = false`.

The `manually_set_max_ltv` is initialized from the asset's `max_LTV` when the tracker is first created, and updated when admin calls `EditcAsset` with a new `max_LTV` value.

---

## Historical Snapshots

Every LTV update records a snapshot (one per day max, 365 entries max):

```rust
LTVSnapshot {
    timestamp: u64,
    max_ltv: Decimal,
    max_borrow_ltv: Decimal,
}
```

Queryable via `GetHistoricalLTVs { asset_denom, start_time, end_time, limit }`.

---

## Graceful Degradation

- If querying Disco fails for one asset, the error is logged as a warning attribute and other assets continue updating.
- Each asset's tracker is independent; one asset's failure doesn't affect others.
- Zero Disco response triggers drift (not skip) + deposit blocking.
