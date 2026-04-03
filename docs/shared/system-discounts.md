# System Discounts

> **Used by:** CDP (Debt interest rates), Acquisition (intent boosts), Transmuter (retention boosts)

## Purpose

The System Discounts contract calculates per-user discount rates based on MBRN staking in LTV Disco, transmuter deposits, and optional static overrides. Discounts reduce CDP interest rates and provide intent boosts for protocol participation.

## Key Concepts

### Discount Priority

Discounts are resolved in strict priority order. The first match wins:

1. **Timed discount period**: If a global timed discount is active, it applies to everyone. Returns immediately.
2. **Static discounts**: Per-address override. If the user has a static discount set, it is returned directly.
3. **Calculated discount**: Combined from MBRN Disco deposits and transmuter participation.

### Calculated MBRN Discount

`calculate_mbrn_discount` queries LTV Disco for user deposits via `GetAllUserDeposits`.

For each deposit, the effective duration is:

```
effective_days = days_since_deposit + lock_duration_days
```

Then `calculate_time_curve_discount` applies a two-phase curve:

```
Phase 1 (first month):
  first_month_progress = min(effective_days / 30, 1.0)
  first_month_contribution = first_month_progress * first_month_discount

Phase 2 (remaining):
  remaining_progress = min((effective_days - 30) / (curve_duration - 30), 1.0)
  remaining_contribution = remaining_progress * remaining_discount

total = min(first_month_contribution + remaining_contribution, max_discount)
```

Where:

- `first_month_discount = 45%` of `stable_backing_max_discount` (i.e., 60% of 75% = 45%)
- `remaining_discount = 30%` of `stable_backing_max_discount` (i.e., 40% of 75% = 30%)
- After 30 days, both phases contribute.
- At `curve_duration` (90 days), maximum discount is reached.

### Transmuter Discount

Transmuter participation adds to the MBRN discount. The transmuter multiplier scales the transmuter's contribution:

```
transmuter_discount = transmuter_amount * transmuter_multiplier
```

The combined total (MBRN + transmuter) is capped at `max_discount`.

### User Boost

**Currently DISABLED** -- the `UserBoost` query always returns zero.

### Intent Boosts

Per intent, the boost is calculated from lock duration:

```
boost = lock_days / ceiling * max_boost
```

Where:

- `ceiling` = lock duration ceiling from config.
- `max_boost` = 9%.
- `SendToAddress` intents always return 0 boost.

## Default Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `stable_backing_max_discount` | 75% | Maximum discount from stable backing |
| `first_month_discount` | 45% | Discount achievable in first 30 days (60% of max) |
| `remaining_discount` | 30% | Additional discount from days 30-90 (40% of max) |
| `curve_duration` | 90 days | Days to reach maximum calculated discount |
| `first_month` | 30 days | Duration of accelerated first phase |
| `debt_multiplier` | 18x | Multiplier for debt-based discount calculation |
| `transmuter_multiplier` | 2x | Multiplier for transmuter contribution |
| `max_discount` | 100% | Hard cap on total discount |
| `max_boost` | 9% | Maximum intent boost |
| `mbrn_at_max_discount` | 100,000 MBRN | MBRN amount that achieves maximum discount |

## User Flows

### Check Your Discount

1. Query `UserDiscount` with your address.
2. Contract checks timed discount (global), then static override, then calculates from deposits.
3. Returns your effective discount rate as a percentage.

### How Discount Grows Over Time

| Day | First Month Phase | Remaining Phase | Total |
|-----|------------------|----------------|-------|
| 0 | 0% | 0% | 0% |
| 15 | 22.5% | 0% | 22.5% |
| 30 | 45% | 0% | 45% |
| 60 | 45% | 15% | 60% |
| 90+ | 45% | 30% | 75% |

(Assuming max `stable_backing_max_discount` of 75% and no transmuter contribution.)

### Intent Boost Calculation

1. Query `IntentBoosts` with intent details.
2. For each intent with a lock duration: `boost = lock_days / ceiling * 9%`.
3. `SendToAddress` intents get zero boost.

## Cross-Contract Interactions

| Direction | Contract | Call | Purpose |
|-----------|----------|------|---------|
| Query | LTV Disco | `GetAllUserDeposits` | Get deposit amounts and durations for MBRN discount |
| Queried by | CDP (Debt) | `UserDiscount` | Reduce interest rate |
| Queried by | Acquisition | `IntentBoosts` | Calculate intent boost percentages |
| Queried by | Transmuter | `UserDiscount` | Apply retention boost |

## Important Invariants

1. Priority is strict: timed > static > calculated. A timed discount overrides everything.
2. The two-phase curve front-loads 60% of the max discount into the first 30 days.
3. `UserBoost` is disabled and returns zero -- do not rely on it.
4. `SendToAddress` intents always receive zero boost regardless of lock duration.
5. The combined MBRN + transmuter discount is capped at `max_discount` (100%).
6. `first_month_discount + remaining_discount` equals `stable_backing_max_discount` (45% + 30% = 75%).
