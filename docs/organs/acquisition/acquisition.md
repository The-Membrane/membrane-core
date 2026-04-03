# Acquisition

> **Organ:** Acquisition

## Purpose & Role

The Acquisition contract manages MBRN emission windows for user growth. Users deposit USDC during a time-bounded window, deposits flow to the Transmuter (without minting VTs), and MBRN is distributed proportionally to depositors after a cliff period. An adaptive control system adjusts emission rates based on deposit efficiency.

## State

| State Key | Type | Description |
|-----------|------|-------------|
| `CONFIG` | `Config` | Contract configuration |
| `CURRENT_WINDOW_ID` | `u64` | Current acquisition window ID |
| `USER_ACQUISITION_DEPOSITS` | `Map<(Addr, u64), AcquisitionDeposit>` | Per-user per-window deposits |
| `USER_INTENTS` | Map | User's MBRN distribution intents |
| `ACQUISITION_CONFIG` | `AcquisitionModelConfig` | Adaptive control system parameters |
| `ACQUISITION_STATE` | `AcquisitionModelState` | Live pool/rate/efficiency state |
| `ACQUISITION_HISTORY` | Capped (365) | Historical control system snapshots |

## Key Concepts

### Acquisition Window

```rust
AcquisitionWindow {
    window_id: u64,
    start_time: u64,
    deposit_end: u64,               // u64::MAX until first deposit
    withdrawal_end: u64,            // u64::MAX until first deposit
    deposit_period_days: u64,
    total_deposit_amount: Uint128,
    acquisition_budget: Uint128,    // Set by efficiency clamp after withdrawal_end
}
```

### Window Transitions

`StartAcquisitionWindow`:
- Increments `window_id`
- Sets `deposit_end = u64::MAX`, `withdrawal_end = u64::MAX` (lazy timers)
- Carries over from previous window: `bump_rate`, `last_bump_time` directly
- `current_rate = min(previous_rate, base_rate)` — only downward rate persistence across windows

### Acquisition Model Config

| Parameter | Default | Description |
|-----------|---------|-------------|
| `base_acquisition_rate` | — | Per-second MBRN emission rate (starting point) |
| `max_mbrn_emission` | — | Cap per window |
| `target_utilization` | — | Transmuter utilization threshold for accrual |
| `bump_increment` | — | Per-interval bump to CDP peg debt rate |
| `bump_interval_seconds` | — | How often bump rate changes |
| `reduction_speed_multiplier` | — | How much faster bump decreases below target |
| `efficiency_threshold` | — | Max acceptable efficiency delta for clamp |
| `max_rate_change_per_mutation` | — | Max rate change per control system mutation |
| `max_bump_rate` | — | Maximum bump rate cap |
| `mutation_cooldown_seconds` | — | Min time between worsening mutations |
| `accrual_ramp_seconds` | — | Ramp period from start to full accrual |
| `accrual_ramp_start_fraction` | — | Starting fraction during ramp |

### Acquisition Model State

```rust
AcquisitionModelState {
    current_acquisition_rate: Decimal,  // Mutated by control system
    accrued_pool: Uint128,              // Total MBRN accrued this window
    pool_maxed: bool,                   // Hit max_mbrn_emission?
    bump_rate: Decimal,                 // Current CDP bump rate
    last_bump_time: u64,                // Last bump rate change time
    window_timers_started: bool,        // First deposit received?
    total_new_deposits: Uint128,        // Sum of all deposits this window
    last_mutation_efficiency: Option<Decimal>,  // Efficiency at last mutation
    efficiency_clamped: bool,           // Was budget clamped?
}
```

### MBRN Intents

When depositing, users specify intents with ratios that must sum to approximately 1.0:

| Intent | Description |
|--------|-------------|
| `Stake` | Stake MBRN in the Staking contract |
| `DepositViaMarsMirror` | Deposit MBRN into LTV Disco (choose asset + slot) |
| `SendToAddress` | Send MBRN to a specified address |

### Rate Assurance

Self-callback `AcquisitionRateAssurance`:
1. Queries Transmuter's `AcquisitionDepositTotal`
2. Compares to window's `total_deposit_amount`
3. Allows +/- 1 tolerance for rounding
4. If mismatch: **reverts the entire transaction**

Only callable by the contract itself.

## Pool Accrual (accrue_pool_internal)

Accrual only occurs when ALL conditions are met:
- `pool_maxed == false`
- `utilization >= target_utilization`

Utilization is calculated from Transmuter's `VaultInfo`:
```
utilization = 1 - (paired_asset_balance / total_deposit_value)
```

Ramp logic (during first `accrual_ramp_seconds`):
```
time_since_first = now - first_deposit_time
if time_since_first < accrual_ramp_seconds:
    progress = time_since_first / accrual_ramp_seconds
    ramp_factor = start_fraction + progress * (1 - start_fraction)
else:
    ramp_factor = 1.0

effective_rate = current_rate * ramp_factor
new_mbrn = floor(effective_rate * elapsed_seconds)
```

When `accrued_pool >= max_mbrn_emission`: sets `pool_maxed = true`.

## Control System Mutation

Triggered during `accrue_pool_internal`. Only runs when:
- More than 1 day has elapsed
- `accrued_pool > 0`
- Window timers have started

```
efficiency = total_new_deposits / accrued_pool

First call: sets last_mutation_efficiency = efficiency, no rate change

Improving (current >= last):
    No cooldown required
    delta = (current - last) / last
    clamped_delta = min(delta, max_rate_change_per_mutation)
    current_rate -= clamped_delta * current_rate

Worsening (current < last):
    Requires mutation_cooldown_seconds elapsed
    delta = (last - current) / last
    clamped_delta = min(delta, max_rate_change_per_mutation)
    current_rate += clamped_delta * current_rate

Rate ceiling: max_mbrn_emission / (deposit_period_days * 86400)
```

## Efficiency Clamp (apply_efficiency_clamp)

Applied once, after `withdrawal_end`:

```
realized_ratio = accrued_pool / total_new_deposits
baseline_pool = floor(base_rate * deposit_period_days * 86400)
baseline_ratio = baseline_pool / total_new_deposits
max_acceptable = baseline_ratio * (1 + efficiency_threshold)

if realized_ratio > max_acceptable:
    accrued_pool = floor(total_new_deposits * max_acceptable)

acquisition_budget = accrued_pool
```

## Bump Rate Mechanism

When `pool_maxed == true`:

**Above target utilization:**
```
ticks = elapsed / bump_interval_seconds
bump_rate += ticks * bump_increment  (capped at max_bump_rate)
```

**Below target utilization:**
```
reduction_interval = floor(bump_interval / reduction_speed_multiplier)  (min 1)
ticks = elapsed / reduction_interval
bump_rate -= ticks * bump_increment  (floored at 0)
```

Sends `SetAcquisitionBumpRate` to CDP Debt contract if bump_rate changed.

`last_bump_time` advances by `ticks * interval` (not by raw elapsed time).

Bump rate and `last_bump_time` carry over directly between windows.

## User Flows

### Flow 1: Depositing

1. Validates deposit period is active, exactly 1 fund of `deposit_token` >= `minimum_deposit`, intent ratios sum approximately 1.0
2. Calls `accrue_pool_internal`
3. **First deposit**: sets `deposit_end = now + deposit_days * 86400`, `withdrawal_end = deposit_end + withdrawal_days * 86400`
4. Sends `EnterVault` to Transmuter with `recipient = contract` (no VTs minted in Transmuter)
5. Appends `AcquisitionRateAssurance` self-callback

### Flow 2: Withdrawing

**During withdrawal period** (after `deposit_end`, before `withdrawal_end`):
- Normal withdrawal, returns USDC via Transmuter `ExitVault`

**After withdrawal + before cliff** (`withdrawal_end < now < vested_at`):
- Forced full withdrawal + clawback if `sent_to_disco == true`
- Sends `RequestUnstake` to Disco + `BurnTokens` for MBRN
- Sends `ExitVault` to Transmuter

**After withdrawal + after cliff** (`now >= vested_at`):
- ERROR: must claim first, cannot withdraw

All paths append `AcquisitionRateAssurance` self-callback.

### Flow 3: Sending MBRN to Disco (SendAcquisitionRewardsToDisco)

1. Calculates `claim_amount` pro-rata: `(budget * deposit.amount) / total_deposit_amount`
2. Finds `DepositViaMarsMirror` intent from user's intents
3. Queries `IntentBoosts` from System Discounts (but `max_boost = 0` currently, effectively disabled)
4. Mints MBRN to the **contract itself** (not to user)
5. Calls Disco `SubmitDeposit` with:
   - `owner = contract` (acquisition contract owns the deposit)
   - `manager = user`
   - `revenue_destination = user` (user earns CDT yield during cliff)
6. Sets `sent_to_disco = true` on the user's deposit

### Flow 4: Claiming (execute_transfer_deposit_ownership)

1. Applies `efficiency_clamp` if not already applied
2. Verifies cliff has passed: `current_time >= vested_at`
3. Sends `TransferDepositOwnership` to Transmuter (converts acquisition deposit to VTs owned by user)
4. **If `sent_to_disco`**: sends `UpdateDeposit` to Disco changing `owner` to user. User now has full control of their Disco deposit
5. **If NOT `sent_to_disco`**: calculates MBRN share = `(budget * deposit.amount) / total_deposit_amount`. Boost currently hardcoded to 1.0 (disabled). Mints MBRN via `neutron_proxy`

## Execute Messages

| Message | Auth | Description |
|---------|------|-------------|
| `StartAcquisitionWindow` | Owner | Create new window, increment window_id, carry over bump state |
| `Deposit` | Anyone | Deposit USDC during deposit period. First deposit starts timers |
| `Withdraw` | Depositor | Withdraw during withdrawal period. Late withdrawal triggers clawback |
| `AccruePool` | Permissionless | Accrue MBRN pool, apply control system, update bump rate |
| `Claim` / `ClaimForUser` | Depositor / Anyone | Claim MBRN + Transmuter deposit after cliff |
| `SendAcquisitionRewardsToDisco` | Depositor | Route MBRN to Disco before cliff |
| `AcquisitionRateAssurance` | Self | Verify Transmuter deposit accounting integrity |
| `UpdateConfig` | Owner | Update configuration |

## Query Messages

| Query | Returns | Description |
|-------|---------|-------------|
| `Config` | `Config` | Contract configuration |
| `AcquisitionConfig` | `AcquisitionModelConfig` | Control system parameters |
| `CurrentAcquisitionWindow` | `AcquisitionWindow` | Current window state |
| `ActiveAcquisitionWindow` | `AcquisitionWindow` | Active (non-expired) window |
| `UserAcquisitionDeposit` | `AcquisitionDeposit` | User's deposit in a window |
| `AcquisitionModelState` | `AcquisitionModelState` | Live pool/rate/efficiency state |
| `AcquisitionHistory` | `Vec<AcquisitionHistoryEntry>` | Historical snapshots (max 365) |

## Cross-Contract Interactions

### Calls (Outgoing)

| Contract | Message | Purpose |
|----------|---------|---------|
| **Transmuter** | `EnterVault` | Deposit USDC (recipient = contract, no VTs minted) |
| **Transmuter** | `ExitVault` | Return deposits to users |
| **Transmuter** | `TransferDepositOwnership` | Convert acquisition deposit to user-owned VTs |
| **Transmuter** | `AcquisitionDepositTotal` query | Rate assurance verification |
| **CDP (Debt)** | `SetAcquisitionBumpRate` | Adjust peg debt rates |
| **LTV Disco** | `SubmitDeposit` | Deposit MBRN with contract as owner, user as manager + revenue_destination |
| **LTV Disco** | `UpdateDeposit` | Transfer ownership to user on claim |
| **LTV Disco** | `RequestUnstake` | Clawback path |
| **Neutron Proxy** | Mint MBRN | For non-Disco claims |
| **System Discounts** | `IntentBoosts` query | Boost calculation (currently disabled, max_boost = 0) |

All outgoing calls are fire-and-forget `CosmosMsg`. No reply handlers.

### Called By (Incoming)

| Contract | Message | Purpose |
|----------|---------|---------|
| **Transmuter** | `AccruePool` notification | Trigger pool accrual on utilization changes |
| **Anyone** | `AccruePool` | Permissionless pool accrual |
| **Users** | `Deposit`, `Withdraw`, `Claim` | Direct user interaction |

## Important Invariants & Edge Cases

- **Lazy timers**: Window timers are `u64::MAX` until first deposit. This prevents empty windows from expiring
- **Ramp period**: Accrual starts at `accrual_ramp_start_fraction` and linearly ramps to 100% over `accrual_ramp_seconds`. Prevents front-running window start
- **Efficiency clamp is one-shot**: `apply_efficiency_clamp` runs once after `withdrawal_end` and sets `efficiency_clamped = true`. Subsequent calls skip it
- **Mutation cooldown asymmetry**: Worsening mutations (rate increase) require cooldown. Improving mutations (rate decrease) are immediate. The system is quick to reduce spending but slow to increase it
- **Vesting yield via revenue_destination**: Users who send MBRN to Disco DO earn CDT yield during cliff because `revenue_destination = user`. The MBRN principal remains locked (owned by contract)
- **Clawback on late withdrawal**: Withdrawing after `withdrawal_end` but before cliff triggers forced full withdrawal + Disco unstake + MBRN burn
- **Cannot withdraw after cliff**: Must claim instead. Withdrawal after `vested_at` errors
- **Rate assurance uses tolerance**: Transmuter's `AcquisitionDepositTotal` must match within +/- 1 of the window's `total_deposit_amount`
- **History cap**: `ACQUISITION_HISTORY` capped at 365 entries
- **Boost disabled**: `IntentBoosts` is queried but the boost multiplier is effectively 1.0 (hardcoded, `max_boost = 0`)
- **Downward rate persistence**: `StartAcquisitionWindow` sets `current_rate = min(previous, base_rate)`. Rates can only carry downward across windows, never upward
- **Bump rate persists fully**: Unlike `current_rate`, `bump_rate` and `last_bump_time` carry over directly without modification between windows
