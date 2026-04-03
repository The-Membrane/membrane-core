# Acquisition to Disco Pipeline

How new users enter the protocol via USDC deposits, earn adaptive MBRN emissions, and optionally lock into LTV Disco for risk-weighted yield.

## Overview

The Acquisition contract manages time-bound deposit windows where users deposit USDC, the protocol accrues an MBRN emission pool based on Transmuter utilization, and users claim pro-rata MBRN shares after a cliff period. The pipeline is:

```
New User (USDC)
    |
    v
Deposit into Acquisition
    |
    v
Funds forwarded to Transmuter (EnterVault, owned by contract)
    |
    v
MBRN pool accrues while utilization >= target
    |
    v
Withdrawal period ends -> acquisition_budget set
    |
    v
User chooses: Claim directly OR SendAcquisitionRewardsToDisco
    |                              |
    v                              v
Mint MBRN to user           Mint MBRN to contract
(boost disabled)             Deposit to Disco (contract owns, user manages)
                             Revenue goes to user during cliff
                             Clawback if withdrawn before cliff
```

## Acquisition Window Lifecycle

**Source:** `contracts/acquisition/src/contract.rs`

### State

| Key | Type | Purpose |
|-----|------|---------|
| `CURRENT_WINDOW_ID` | `Item<u64>` | Auto-incrementing window counter |
| `CURRENT_ACQUISITION_WINDOW` | `Item<AcquisitionWindow>` | Active window state |
| `USER_ACQUISITION_DEPOSITS` | `Map<(String, u64), AcquisitionDeposit>` | Per-user, per-window deposits |
| `USER_INTENTS` | `Map<String, Vec<MbrnIntentOption>>` | User's MBRN routing preferences |
| `ACQUISITION_CONFIG` | `Item<AcquisitionModelConfig>` | Adaptive rate control parameters |
| `ACQUISITION_STATE` | `Item<AcquisitionModelState>` | Current accrual engine state |
| `ACQUISITION_HISTORY` | `Item<Vec<AcquisitionHistoryEntry>>` | Capped at 365 entries |

### Window Structure

```rust
pub struct AcquisitionWindow {
    pub window_id: u64,
    pub start_time: u64,
    pub deposit_end: u64,        // u64::MAX until first deposit
    pub withdrawal_end: u64,     // u64::MAX until first deposit
    pub deposit_period_days: u64,
    pub total_deposit_amount: Uint128,
    pub acquisition_budget: Uint128,  // Set at withdrawal end by efficiency clamp
}
```

**Lazy timers:** `deposit_end` and `withdrawal_end` are initialized to `u64::MAX`. The first deposit triggers actual timers:
```
deposit_end = first_deposit_time + deposit_period_days * 86400
withdrawal_end = deposit_end + withdrawal_period_days * 86400
```

### Cross-Window Memory

When a new window starts:
- `bump_rate` and `last_bump_time` carry over from the previous window
- `current_acquisition_rate` carries over but is **clamped to base_rate ceiling** (only downward convergence persists)

## Deposit Flow

### `execute_deposit`

1. Validates deposit token and minimum amount
2. Calls `accrue_pool_internal` (see Accrual Engine below)
3. First deposit triggers window timers
4. Forwards USDC to Transmuter via `EnterVault`:
   ```rust
   TransmuterExecuteMsg::EnterVault {
       recipient: Some(contract_addr),  // Contract owns the deposit
       affiliate_address,
       affiliate_label,
   }
   ```
   **No VTs minted** for acquisition deposits -- the contract holds a direct deposit position.
5. Creates/updates `AcquisitionDeposit` for user with `vested_at = current_time + cliff_period_seconds`
6. Appends `AcquisitionRateAssurance` callback (runs after Transmuter processes)

## Accrual Engine

**Function:** `accrue_pool_internal` -- called on every deposit, withdraw, and `AccruePool`.

### Utilization Query

Queries `Transmuter.VaultInfo` and calculates:
```
utilization = 1 - (paired_asset_balance / total_deposit_value)
above_target = utilization >= target_utilization
```

### Pool Accrual (only when `above_target` AND pool not maxed)

**Ramp factor:** Time-gated ramp from `accrual_ramp_start_fraction` (default 25%) to 100% over `accrual_ramp_seconds` (default 24hr):

```
if time_since_first_deposit < accrual_ramp_seconds:
    ramp_progress = time_since_first / accrual_ramp_seconds
    ramp_factor = start_fraction + ramp_progress * (1 - start_fraction)
    effective_rate = current_acquisition_rate * ramp_factor
else:
    effective_rate = current_acquisition_rate
```

New MBRN accrued:
```
new_mbrn = effective_rate * elapsed_seconds
accrued_pool = min(accrued_pool + new_mbrn, max_mbrn_emission)
```

When pool reaches `max_mbrn_emission`, it is marked `pool_maxed = true`.

### Bump Rate Mechanism (pool maxed + above target)

When utilization stays above target after pool is maxed, a `bump_rate` increases per `bump_interval_seconds` (default 4.8hr = 17280s):

```
// Above target: bump increases
ticks = elapsed / bump_interval
bump_rate += bump_increment * ticks   (clamped to max_bump_rate)

// Below target: bump decreases FASTER
reduction_interval = bump_interval / reduction_speed_multiplier   (default 2x)
ticks = elapsed / reduction_interval
bump_rate -= bump_increment * ticks   (floored at zero)
```

Bump rate changes are sent to CDP via `SetAcquisitionBumpRate`.

Bump rate also decays when pool is NOT maxed but utilization drops below target.

### Efficiency Control System

Active after day 1 of deposit period, when `accrued_pool > 0`:

```
current_efficiency = total_new_deposits / accrued_pool
```

| Direction | Action | Cooldown |
|-----------|--------|----------|
| Improving (efficiency >= last) | Decrease rate by delta | None (always apply) |
| Worsening (efficiency < last) | Increase rate by delta | `mutation_cooldown_seconds` (default 8hr) |

Rate change:
```
delta = |current_efficiency - last_efficiency| / last_efficiency
clamped_delta = min(delta, max_rate_change_per_mutation)
rate_change = clamped_delta * current_acquisition_rate
```

Rate ceiling: `max_mbrn_emission / (deposit_period_days * 86400)`

### Efficiency Clamp (at withdrawal end)

Applied once when `current_time >= withdrawal_end` (idempotent via `efficiency_clamped` flag):

```
realized_ratio = accrued_pool / total_new_deposits
baseline_pool = base_acquisition_rate * deposit_period_seconds
baseline_ratio = baseline_pool / total_new_deposits
max_acceptable = baseline_ratio * (1 + efficiency_threshold)

if realized_ratio > max_acceptable:
    accrued_pool = total_new_deposits * max_acceptable
```

Sets `window.acquisition_budget = accrued_pool`.

## Withdrawal Flow

### During withdrawal period (`deposit_end <= now <= withdrawal_end`)

Normal withdrawal: exit Transmuter via `ExitVault`, reduce deposit and window totals.

### After withdrawal period, before cliff

Force full withdrawal + **clawback** if rewards were sent to Disco:

1. `LtvDisco.RequestUnstake` -- unstake the MBRN deposit
2. `NeutronProxy.BurnTokens` -- burn the clawed-back MBRN
3. Reset all disco-related fields on the deposit

### After cliff

Cannot withdraw through Acquisition. Must claim ownership first, then withdraw through Transmuter directly.

## Claim Flow (Direct)

### `execute_claim` / `execute_transfer_deposit_ownership`

Available after cliff (`current_time >= deposit.vested_at`):

1. Applies efficiency clamp if not already done
2. Transfers Transmuter deposit ownership to user:
   ```rust
   TransmuterExecuteMsg::TransferDepositOwnership {
       user: contract_addr,
       amount: deposit.amount,
       new_owner: user,
   }
   ```
3. If `sent_to_disco == true`: transfers Disco deposit ownership to user via `LtvDisco.UpdateDeposit { deposit_owner: user }`
4. If `sent_to_disco != true`: mints MBRN directly to user:
   ```
   claim_amount = acquisition_budget * (deposit.amount / total_deposit_amount)
   boost_multiplier = 1.0   // UserBoost DISABLED, hardcoded
   mbrn_minted = claim_amount * boost_multiplier
   ```
5. Removes user deposit entry

## Send to Disco Flow

### `execute_send_acquisition_rewards_to_disco`

Available after `withdrawal_end`. Requires a `DepositViaMarsMirror` intent (asset + slot).

1. Applies efficiency clamp
2. Calculates pro-rata MBRN claim:
   ```
   claim_amount = acquisition_budget * (deposit.amount / total_deposit_amount)
   ```
3. Queries `SystemDiscounts.IntentBoosts` for the disco intent:
   ```
   boost = min(lock_duration_days / lock_ceiling, 1.0) * max_boost
   ```
   Note: `UserBoost` is disabled (hardcoded zero), so `total_boost = intent_boost` only.
4. Calculates boosted amount:
   ```
   base_amount = claim_amount * intent_ratio
   boosted_amount = base_amount * (1 + total_boost)
   ```
5. Mints MBRN to **contract** (not user):
   ```rust
   NeutronProxyExecuteMsg::MintTokens {
       denom: mbrn_denom,
       amount: boosted_amount,
       mint_to_address: contract_addr,
   }
   ```
6. Submits to Disco with ownership structure:
   ```rust
   LtvDiscoExecuteMsg::SubmitDeposit {
       deposit_owner: Some(contract_addr),  // Contract owns
       manager: Some(user),                  // User manages
       revenue_destination: Some(user),      // User earns revenue
   }
   ```
   **User DOES earn revenue during cliff** because `revenue_destination = user`.
7. Stores disco deposit ID, asset, slot, and claimed amount for potential clawback

## Intent System

Users declare MBRN routing preferences via `MbrnIntentOption`:

```rust
pub enum MbrnIntentType {
    Stake {},                              // Stake in governance
    DepositViaMarsMirror { asset, slot },   // Route to Disco via Mars Mirror
    SendToAddress { address },             // Direct send
}
```

Each intent has a `ratio` (must sum to 1.0 across all intents) and an optional `lock` with `locked_until` timestamp for boost calculation.

## History Tracking

Every `accrue_pool_internal` call appends to `ACQUISITION_HISTORY` (capped at 365 entries):

```rust
pub struct AcquisitionHistoryEntry {
    pub window_id: u64,
    pub timestamp: u64,
    pub current_acquisition_rate: Decimal,
    pub bump_rate: Decimal,
    pub accrued_pool: Uint128,
    pub pool_maxed: bool,
    pub utilization: Decimal,
    pub efficiency: Option<Decimal>,
    pub total_new_deposits: Uint128,
}
```
