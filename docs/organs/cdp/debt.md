# Debt

> **Organ:** CDP

## Purpose

The Debt contract manages all CDT debt: issuance, repayment, interest accrual, rate segment accounting, and the adaptive interest rate model. It maintains separate tracking for regular debt and peg-incentivized debt, each with their own rate indices.

## Key Concepts

### Rate Segments

Each position's debt is divided into **rate segments** (`RateSegment`), which can be either variable or fixed:

```
RateSegment {
    amount: Uint128,
    fixed_rate: Option<FixedRate>,
}

FixedRate {
    rate: Decimal,
    end: FixedRateEnd {
        end_time: u64,
        rollover: bool,
        duration_months: u64,
    },
}
```

- **Variable segments**: Interest accrues based on the global rate index ratio (`new_amount = amount * current_index / previous_index`)
- **Fixed segments**: Interest accrues at the locked rate: `accrued = amount * fixed_rate * (elapsed / SECONDS_PER_YEAR)`
- **Expired fixed with rollover=true**: Recalculates a new fixed rate and resets the timer
- **Expired fixed with rollover=false**: Converts to a variable segment

Positions track both `rate_segments` (regular debt) and `peg_rate_segments` (peg-incentivized debt), plus `collateral_rate_indices` per deposited asset.

### Fixed Rate Creation

When a user borrows with a fixed rate option, the debt is split according to `DebtSplit` fractions across available durations (1 month, 3 month, 6 month). The fixed rate is calculated as:

```
fixed_rate = base_rate * cap.multiplier
```

A shared cap prevents excessive fixed-rate exposure:

```
if (combined_regular_fixed + combined_peg_fixed) > cap_percentage * total_debt:
    error
```

### Repayment Priority

Repayment burns segments in this order:

1. Variable (regular)
2. 1-month fixed (regular)
3. 3-month fixed (regular)
4. 6-month fixed (regular)
5. Variable (peg)
6. 1-month fixed (peg)
7. 3-month fixed (peg)
8. 6-month fixed (peg)

Liquidation uses **proportional** distribution across pools, with variable segments consumed first within each pool.

### Interest Rate Model

The adaptive rate model in `rates.rs` operates in three stages:

**Stage 1 - Destination Rate Calculation**:

For each collateral asset, query Disco for total deposit tokens:

```
deposit_ratio_i = asset_deposits_i / total_deposits
insurance_ratio_i = deposit_ratio_i / tvl_ratio_i
```

The asset with the highest `insurance_ratio` gets:
```
destination_rate = base_rate * (1 / max_LTV)
```

All other assets:
```
destination_rate = base_rate * (highest_insurance_ratio / asset_insurance_ratio)
```

Capped at `max_adaptive_rate`. Fallback (no Disco data): `base_rate * (1 / max_LTV)`.

**Stage 2 - Adaptive Smoothing**:

```
err = (destination - current) / current
dt = elapsed / SECONDS_PER_YEAR
exponent = speed * |err| * dt    (capped at 1.0)
new_rate = current * exp(speed * err * dt)
```

Taylor series approximation for `exp()`. Result is clamped to `[min_rate, max_rate]`. The algorithm does not overshoot the destination.

**Stage 3 - Rate Index Accumulation**:

Two separate indices are maintained per collateral asset in `BASKET_RATE_INDICES`:

```
// Regular debt
rate_index += accumulate_interest_dec(rate_index, base_rate, elapsed)

// Peg debt
peg_rate_index += accumulate_interest_dec(peg_rate_index, base_rate + acquisition_bump * (1/max_LTV), elapsed)
```

Where:
```
accumulate_interest_dec(decimal, rate, time) = decimal * rate * (time / SECONDS_PER_YEAR)
```

### Interest Discount

Before applying interest, the contract queries `UserDiscount` from the discount module:

```
discounted_interest = undiscounted_interest * (1 - discount)
```

### Acquisition Bump Rate

Set exclusively by the acquisition contract via `SetAcquisitionBumpRate`. This rate is stored in `RATES.acquisition_bump_rate` and only affects the **peg debt rate index** accumulation, not regular debt.

## User Flows

### Borrow (IncreaseDebt)

1. User calls `IncreaseDebt { position_id, amount, LTV, mint_to_addr }`
2. Contract checks `FROZEN` flag
3. Accrues interest on the user's position locally
4. Sends SubMsg to `Collateral.RefreshAndAssess` (reply on `COLLATERAL_REFRESH_REPLY_ID=1`)
5. On reply, the contract:
   - Parses the collateral assessment response
   - Checks for frozen assets (circuit breaker)
   - Calculates `total_collateral_value` from the assessment
   - Determines mint amount:
     - If explicit `amount`: uses it directly
     - If `LTV` target: `mint_amount = (collateral_value * ltv - existing_debt) / credit_price`
   - Solvency check: `LTV <= avg_max_borrow_ltv AND LTV <= avg_max_ltv`
   - Debt minimum check: total debt must meet `debt_minimum`
   - Creates rate segments based on `DebtSplit` configuration
   - Mints CDT via `chain_proxy.MintTokens`

### Repay

1. User sends CDT with `Repay { position_id }` message
2. Contract accrues interest on the position
3. Burns segments in priority order (variable first, then fixed by duration, regular before peg)
4. Accumulates `pending_revenue` in the basket from interest paid
5. Returns any excess CDT to the user

## Technical Details

### State

| Key | Type | Description |
|-----|------|-------------|
| `CONFIG` | `Config` | `irm_config`, `debt_minimum`, `rate_slope_multiplier`, `affiliate_fee_max`, contract addresses |
| `BASKET` | `BasketDebt` | `credit_asset` with `CreditAssetBreakdown` (8 fields: variable/1mo/3mo/6mo for regular+peg), `credit_price`, `pending_revenue`, `pending_bad_debt` |
| `POSITIONS` | `Map<Addr, Vec<DebtPosition>>` | Per-user positions with `rate_segments`, `peg_rate_segments`, `deployed_to`, `pending_interest`, `vol_window_initial_debt`, `collateral_rate_indices` |
| `RATES` | struct | `base_interest_rate`, `current_adaptive_rate` (Vec), `peg_current_adaptive_rate` (Vec), `acquisition_bump_rate` |
| `BASKET_RATE_INDICES` | `Map` per asset | `rate_index` + `peg_rate_index` per collateral type |
| `CDT_SUPPLY` | `Uint128` | Total CDT supply tracking |
| `AFFILIATES` | Map | Affiliate fee tracking |
| `USER_INTENTS` | Map | User intent storage |
| `COLLATERAL_TYPES_CACHE` | cached | Collateral type data cache |

### Constants

| Name | Value |
|------|-------|
| `SECONDS_PER_YEAR` | 31,536,000 |
| `MINIMUM_LIQUIDITY` | 2,000,000,000,000 |
| `MAX_RATE_HISTORY_ENTRIES` | 365 |
| `seconds_per_month` | 2,592,000 (30 days) |

### CreditAssetBreakdown Fields

The basket tracks 8 debt pools:

| Field | Description |
|-------|-------------|
| `variable` | Regular variable-rate debt |
| `one_month` | Regular 1-month fixed debt |
| `three_month` | Regular 3-month fixed debt |
| `six_month` | Regular 6-month fixed debt |
| `peg_variable` | Peg variable-rate debt |
| `peg_one_month` | Peg 1-month fixed debt |
| `peg_three_month` | Peg 3-month fixed debt |
| `peg_six_month` | Peg 6-month fixed debt |

## Cross-Contract Interactions

| Direction | Target | Message | Purpose |
|-----------|--------|---------|---------|
| Outbound | Collateral | `RefreshAndAssess` | Get collateral valuation + LTV data for borrow solvency check |
| Outbound | Chain Proxy | `MintTokens` | Mint CDT when debt is created |
| Outbound | Disco | Query deposits | Get deposit data for destination rate calculation |
| Outbound | Discount module | Query `UserDiscount` | Get interest rate discount for user |
| Inbound | Collateral | `AccrueAndValidate` | Validate solvency before withdrawals |
| Inbound | Collateral | `NotifyDeposit` | Fire-and-forget deposit notification |
| Inbound | Liq Engine | `AccrueAndReturnDebt` | Return current debt for liquidation calculation |
| Inbound | Liq Engine | `LiquidationDebtUpdate` | Update debt after liquidation |
| Inbound | Acquisition | `SetAcquisitionBumpRate` | Set the peg debt rate bump |

## Important Invariants

1. **Debt minimum**: Every position must maintain at least `debt_minimum` in total debt (or be fully repaid to zero).
2. **Solvency**: No borrow can result in `LTV > avg_max_borrow_ltv` or `LTV > avg_max_ltv`.
3. **Rate index monotonicity**: Rate indices only increase over time; they accumulate interest proportionally to elapsed time.
4. **Fixed rate cap**: Total fixed-rate debt (regular + peg combined) cannot exceed `cap_percentage` of total debt.
5. **Peg rate separation**: Peg debt always accrues at `base_rate + acquisition_bump * (1/max_LTV)`, separate from regular debt's `base_rate`.
6. **Repayment ordering**: Variable debt is always repaid before fixed debt; regular debt before peg debt.
7. **Minimum liquidity**: The protocol retains `MINIMUM_LIQUIDITY = 2,000,000,000,000` as a floor.
