# Collateral

> **Organ:** CDP

## Purpose

The Collateral contract manages all deposited assets backing CDT debt. It handles deposits, withdrawals, supply cap enforcement, per-asset circuit breakers, and dynamic LTV parameter updates driven by Disco insurance data.

## Key Concepts

### Positions

Each user can hold up to **9 positions** (`MAX_POSITIONS_PER_USER=9`). A position contains a list of `cAsset` entries representing deposited collateral types and amounts. Positions are identified by `position_id` (auto-incrementing counter stored in `BASKET.current_position_id`).

### LTV Parameters

Each collateral asset has two LTV values:

- **max_LTV**: The liquidation threshold. Positions exceeding this are eligible for liquidation.
- **max_borrow_LTV**: The maximum LTV at which new debt can be taken. Always equals `max_LTV - ltv_borrow_distance` (default 10%).

If `max_borrow_LTV` would exceed `max_LTV`, it is capped at 95% of `max_LTV`.

Weighted average LTVs for a position are computed as:

```
avg_max_ltv = sum(value_ratio_i * max_LTV_i)
```

where `value_ratio_i` is the USD value of asset `i` divided by total position value.

### LTV Update Mechanism (UpdateBasketLTVs)

This is a **permissionless** function. Anyone can call it. For each collateral asset, it queries Disco for the `average_max_ltv`.

**Upward movement** (Disco LTV > current LTV):

Exponential convergence toward the Disco target:

```
new_ltv = disco_ltv - (disco_ltv - current_ltv) * e^(-kp * dt / 86400)
```

- `kp` = `ltv_upward_kp` (default 5% per day, i.e., 0.05)
- `dt` = seconds elapsed since last update
- Exponent is capped at 1.0
- Taylor series approximation is used for `e^x`
- Result is **path-independent**: same outcome regardless of how often it is called

**Downward movement** (Disco LTV < current LTV):

Uses a staged delay mechanism:

1. First call: stages the target value with a timer (`ltv_downward_period`, default 604,800 seconds = 7 days)
2. While staged: LTV is frozen at current value, but the staged target tracks Disco bidirectionally
3. After timer expires: applies partial decay: `actual_shift = (current - staged) * decay_rate` (default 40%)

**Disco fallback**: If Disco returns 0 for an asset, `MANUAL_MAX_LTVS` are used as the target and `DISCO_BLOCKED` is set to true.

### Circuit Breakers

Per-asset automatic freeze mechanism stored in `ASSET_CIRCUIT_BREAKERS`.

**Reference price**: Average of the last 5 historical oracle prices for the asset.

**Deviation check**:
```
deviation = |current_price - reference_price| / reference_price
```

**Threshold**: 10% (hardcoded).

- If `deviation > 10%`: asset is frozen. Blocks withdrawals, debt increases, and liquidations for that asset.
- If `deviation <= 10%`: asset automatically unfreezes.

The check runs during every withdrawal.

### Supply Caps (Risk Engine)

Two types of caps enforced by `risk_engine::update_basket_tally`:

**Individual caps**: For each asset, `value_ratio_i > supply_cap_ratio_i` triggers an error.

**Multi-asset caps**: Groups of related assets share a combined cap. `sum(ratios for grouped assets) > multi_cap` triggers an error.

**Volatility transform**: When `volatility_list` has at least 10 entries, the effective cap is adjusted:

```
effective_cap = cap_ratio * volatility_index
```

where `volatility_index` is capped at 1.0 (caps can only decrease, never increase beyond the base ratio).

Liquidations **skip** supply cap checks. Caps are position-aware.

### Collateral Rate Assurance

A self-callback mechanism (`CollateralRateAssurance`) that verifies collateral token exchange rates haven't deviated:

```
rate = balance * 1_000_000 / supply
```

Tolerance: 1 unit. Errors if rates diverge beyond this tolerance.

## User Flows

### Deposit

1. User sends native tokens with `Deposit { position_id }` message
2. Contract checks global `FROZEN` flag
3. Maps `info.funds` to `cAsset` entries via `basket.collateral_types`
4. If `position_id = None`: creates new position (increments `current_position_id`)
5. If existing position: adds amounts to matching `cAsset` entries or pushes new entries
6. Calls `risk_engine::update_basket_tally(add=true)` to enforce supply caps
7. Sends fire-and-forget SubMsg to `Debt.NotifyDeposit`
8. Attempts best-effort oracle price refresh

### Withdraw

1. User calls `Withdraw { position_id, assets }`
2. Contract checks `FROZEN` flag
3. Validates requested amounts against position balances
4. Runs circuit breaker check for each asset being withdrawn (queries oracle, computes deviation from 5-price average)
5. Computes remaining position value and weighted average LTVs
6. Saves `WithdrawPropagation` to state
7. Sends SubMsg to `Debt.AccrueAndValidate` (reply on success, `WITHDRAW_REPLY_ID=4`)
8. On success reply:
   - Sends `BankMsg::Send` to transfer assets to user
   - Removes withdrawn amounts from position
   - Calls `risk_engine::update_basket_tally(add=false)` to update supply caps

## Technical Details

### State

| Key | Type | Description |
|-----|------|-------------|
| `CONFIG` | `Config` | LTV parameters (`ltv_upward_kp`, `ltv_downward_period`, `ltv_downward_decay_rate`, `ltv_borrow_distance`), contract addresses |
| `BASKET` | `BasketCollateral` | `collateral_types`, `supply_caps`, `multi_asset_supply_caps`, `current_position_id` |
| `POSITIONS` | `Map<Addr, Vec<CollateralPosition>>` | All user positions, up to 9 per user |
| `FROZEN` | `bool` | Global freeze flag |
| `VOLATILITY` | per-asset | `volatility_index` + raw price list |
| `STORED_PRICES` | per-asset | Cached oracle prices |
| `ASSET_CIRCUIT_BREAKERS` | per-asset | Freeze status with 10% deviation threshold |
| `LTV_UPDATE_TRACKERS` | per-asset | Staged downward LTV values + timestamps |
| `MANUAL_MAX_LTVS` | per-asset | Fallback LTVs when Disco returns 0 |
| `DISCO_BLOCKED` | `bool` | True when Disco returns 0 for any asset |

### Constants

| Name | Value |
|------|-------|
| `MAX_POSITIONS_PER_USER` | 9 |
| `MAX_ORACLE_ENTRIES` | 365 |
| `SECONDS_PER_DAY` | 86,400 |
| `MAX_LTV_HISTORY_ENTRIES` | 365 |
| Circuit breaker deviation threshold | 10% |

### Config Defaults

| Field | Default | Description |
|-------|---------|-------------|
| `ltv_upward_kp` | 5% per day (0.05) | Speed of upward LTV convergence |
| `ltv_downward_period` | 604,800s (7 days) | Delay before downward LTV changes apply |
| `ltv_downward_decay_rate` | 40% | Fraction of the downward shift actually applied |
| `ltv_borrow_distance` | 10% | Gap between max_LTV and max_borrow_LTV |

## Cross-Contract Interactions

| Direction | Target | Message | Purpose |
|-----------|--------|---------|---------|
| Outbound | Debt | `NotifyDeposit` | Fire-and-forget notification on deposit |
| Outbound | Debt | `AccrueAndValidate` | Solvency check before withdrawal completes |
| Outbound | Disco | Query `average_max_ltv` | Fetch insurance-driven LTV targets |
| Outbound | Oracle | Query prices | Price lookups for circuit breakers and valuations |
| Inbound | Debt | `RefreshAndAssess` | Debt contract requests collateral assessment for borrows |
| Inbound | Liq Engine | `AuthorizedTransfer` | Transfer collateral during liquidation |
| Inbound | Liq Engine | `LiquidationClaimUpdate` | Update positions after liquidation |
| Inbound | Liq Engine | `AuthorizedSellCollateral` | Sell remaining collateral to sell wall |

## Important Invariants

1. **Position limit**: No user can have more than 9 positions.
2. **Supply caps**: Total value ratio of any asset (or asset group) must not exceed its cap ratio (adjusted by volatility index when available). Liquidations are exempt.
3. **Circuit breaker**: Any asset with >10% oracle price deviation from its 5-price historical average is automatically frozen. Frozen assets block withdrawals, new debt, and liquidations.
4. **Solvency on withdraw**: Every withdrawal must pass `Debt.AccrueAndValidate` before assets leave the contract.
5. **LTV bounds**: `max_borrow_LTV` is always `max_LTV - ltv_borrow_distance`, capped at 95% of `max_LTV`.
6. **Collateral rate**: LP token exchange rates must remain within 1 unit tolerance of their stored reference.
