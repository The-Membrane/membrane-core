# Acquisition Organ

The Acquisition organ manages Membrane's **user growth engine** — time-bounded windows where users deposit USDC and earn MBRN proportional to their share of deposits. It features an adaptive control system that automatically adjusts emission rates based on deposit efficiency, and a bump rate mechanism that increases CDP peg debt interest when the MBRN pool is maxed.

## Contracts in this Organ

| Contract | Purpose |
|----------|---------|
| [Acquisition](acquisition.md) | MBRN emission windows, adaptive control, deposit tracking, Disco/Transmuter integration |

## How Acquisition Drives Growth

```
Owner starts window (increments window_id)
        |
   +----v----+    Deposit Period    +----------+
   |  Users   | ---- USDC --------> |Acquisition|
   |  deposit |                     | Contract  |
   +----------+                     +----+------+
                                         |
                                    USDC deposited
                                    to Transmuter
                                    (recipient=contract,
                                     no VTs minted)
                                         |
   +--------------------------------------------+
   |                  MBRN Pool                   |
   |  Accrues at current_rate per second          |
   |  Only when utilization >= target_utilization |
   |  Ramps from start_fraction -> 100% over     |
   |    accrual_ramp_seconds (default 24hr)       |
   |  Caps at max_mbrn_emission (pool_maxed=true) |
   |                                              |
   |  Control System:                             |
   |  efficiency = total_new_deposits / accrued   |
   |  Improving -> rate DECREASES                 |
   |  Worsening -> rate INCREASES (with cooldown) |
   |  Rate ceiling: max_emission / deposit_period |
   +-------------------+------------------------+
                        |
          After cliff (vested_at)
                        |
                 +------v------+
                 |  User claims |
                 |  MBRN share  |
                 +------+------+
                        |
           +------------+----------+
           |                       |
      Send to Disco           Mint MBRN
      (MBRN deposited,        directly
       user earns yield        to user
       during cliff via
       revenue_destination)
```

## Window Lifecycle

| Phase | Duration | What Happens |
|-------|----------|-------------|
| **Pre-deposit** | Until first deposit | Window created with `deposit_end = u64::MAX`, `withdrawal_end = u64::MAX` (lazy timers) |
| **Deposit** | `deposit_period_days` | Users deposit USDC, pool accrues MBRN. First deposit sets both timers |
| **Withdrawal** | `withdrawal_period_days` | Users can withdraw USDC normally |
| **Post-withdrawal** | Immediate | Efficiency clamp applied once (`apply_efficiency_clamp`) |
| **Post-withdrawal + before cliff** | Variable | Late withdrawal triggers forced full withdrawal + clawback if `sent_to_disco` |
| **Cliff** | `cliff_period_days` | Users can send MBRN to Disco. Users **DO earn yield** during cliff (via `revenue_destination`) |
| **Claim** | Open-ended | Users claim MBRN share + Transmuter deposit ownership |

## Vesting Yield: What Actually Happens

When a user sends their MBRN to Disco via `SendAcquisitionRewardsToDisco`:
- MBRN is deposited with the **acquisition contract as owner** and the **user as manager + revenue_destination**
- Because `revenue_destination = user`, the user **does earn CDT yield** from Disco during the cliff period
- However, the MBRN **principal is locked** (owned by the contract, not the user)
- Withdrawal before the cliff triggers **clawback**: `RequestUnstake` to Disco + `BurnTokens` for the MBRN

This is distinct from previous documentation that claimed users do NOT earn yield during cliff.

## Cross-Organ Dependencies

| Dependency | How It's Used |
|-----------|---------------|
| **Transmuter** | Deposits go to Transmuter (no VTs minted). `AccruePool` triggered on enter/exit. `TransferDepositOwnership` on claim. `AcquisitionDepositTotal` queried for rate assurance |
| **CDP (Debt)** | `SetAcquisitionBumpRate` sent when bump rate changes |
| **LTV Disco** | `SubmitDeposit` with contract as owner, user as manager + revenue_destination. `UpdateDeposit` to change owner on claim. `RequestUnstake` + `BurnTokens` for clawback |
| **Neutron Proxy** | MBRN minting for non-Disco claims |
| **System Discounts** | `IntentBoosts` queried (but `max_boost = 0` currently, effectively disabled) |

## Critical Mechanics

### Adaptive Control System

The control system prevents wasteful MBRN emissions:

- **Efficiency metric**: `total_new_deposits / accrued_pool`
- **First call**: sets baseline efficiency, no rate change
- **Improving** (current >= last): rate DECREASES by `clamped_delta * current_rate`. No cooldown required
- **Worsening** (current < last): rate INCREASES by `clamped_delta * current_rate`. Requires `mutation_cooldown_seconds` to have elapsed
- **Rate ceiling**: `max_mbrn_emission / (deposit_period_days * 86400)`
- **Rate floor**: only downward persistence across windows via `current_rate = min(previous, base_rate)` in `StartAcquisitionWindow`

### Efficiency Clamp

Applied once after `withdrawal_end` via `apply_efficiency_clamp`:

```
realized_ratio = accrued_pool / total_new_deposits
baseline_pool = floor(base_rate * deposit_period_days * 86400)
baseline_ratio = baseline_pool / total_new_deposits
max_acceptable = baseline_ratio * (1 + efficiency_threshold)

if realized_ratio > max_acceptable:
    accrued_pool = floor(total_new_deposits * max_acceptable)

acquisition_budget = accrued_pool
```

### Bump Rate Mechanism

When `pool_maxed == true` and utilization is above target:
1. `ticks = elapsed / bump_interval_seconds`
2. `bump_rate += ticks * bump_increment` (capped at `max_bump_rate`)
3. `SetAcquisitionBumpRate` sent to CDP Debt contract

When utilization is below target:
1. `reduction_interval = floor(bump_interval / reduction_speed_multiplier)` (minimum 1)
2. Each tick: `bump_rate -= bump_increment` (floored at 0)

Bump rate and `last_bump_time` carry over directly between windows.

### No Reply Handlers

All cross-contract calls (Transmuter, CDP, Disco, Neutron Proxy) are fire-and-forget `CosmosMsg`. There are no `reply` handlers in the Acquisition contract.
