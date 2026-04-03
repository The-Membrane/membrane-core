# Liquidation Engine

> **Organ:** CDP

## Purpose

The Liquidation Engine orchestrates the multi-step liquidation of undercollateralized positions. It coordinates between the Debt, Collateral, Liq Queue, and Sell Wall contracts through a 9-step SubMsg reply chain. It implements a delay mechanism for positions near the liquidation boundary and dynamic caller fee incentives.

## Key Concepts

### Liquidation Math

The core formula lives in `get_repay_quantities`:

```
debt_value = credit_price.get_value(total_debt)
current_ltv = debt_value / collateral_value
```

**Insolvency check**: `current_ltv > avg_max_ltv`

**Repay value calculation**:
```
repay_value = ((current_ltv - borrow_ltv) / current_ltv) * debt_value
```

If `LTV >= 100%` (underwater): repay value equals the entire `collateral_value`.

**Repay amount**: `repay_value / credit_price`, capped at `total_debt`.

### Fee Structure

Two fees are assessed on every liquidation:

**Caller fee** (dynamic):
```
caller_fee_rate = current_ltv - avg_max_ltv
```

This scales linearly with how far the position has exceeded its max LTV. More undercollateralized positions offer larger caller incentives.

**Protocol fee** (static): `config.liq_fee`

**Fee cap** (insolvency protection): If `(caller_fee + protocol_fee) * repay_value > collateral_value`:
```
caller_fee = min(1%, collateral_value / repay_value)
protocol_fee = 0
```

This ensures fees never exceed available collateral.

### Liquidation Delay

Positions near the liquidation boundary receive a grace period:

```
immediate_threshold = avg_max_ltv * (1 + avg_max_threshold_to_delay)
```

**If LTV > immediate_threshold**: Liquidation executes immediately in a single call.

**If LTV <= immediate_threshold** (but still above avg_max_ltv):

1. **First call**: Starts a timer (`LIQUIDATION_TIMERS`). Caller fees are paid upfront. Default delay: `liquidation_delay = 28,800 seconds (8 hours)`.
2. **Second call** (after timer expires): Executes the full liquidation.

This gives borrowers time to add collateral or repay debt before liquidation for positions that are only slightly undercollateralized.

### 9-Step SubMsg Chain

The liquidation executes as a sequential chain of SubMsg calls with reply handlers:

| Step | Reply ID | Action | Description |
|------|----------|--------|-------------|
| 1 | `DEBT_ACCRUE_REPLY (1)` | `Debt.AccrueAndReturnDebt` | Accrue interest and get current debt amount |
| 2 | `COLLATERAL_ASSESS_REPLY (2)` | `Collateral.RefreshAndAssess` | Get collateral valuation, run insolvency check, compute fees, apply delay logic |
| 3 | `FEE_TRANSFER_REPLY (9)` | `Collateral.AuthorizedTransfer` | Transfer fee collateral to the engine, then `BankMsg::Send` caller fees to the liquidation caller |
| 4 | `LIQ_QUEUE_TRANSFER_REPLY (3)` | `Collateral.AuthorizedTransfer` | Transfer collateral to the Liq Queue contract |
| 5 | `LIQ_QUEUE_REPLY (4)` | `LiqQueue.Liquidate` (per asset) | Execute liquidation against queue bids for each collateral asset |
| 6 | `SELL_WALL_REPLY (5)` | `Collateral.AuthorizedSellCollateral` | Sell any remaining collateral not absorbed by the queue |
| 7 | `DEBT_UPDATE_REPLY (6)` | `Debt.LiquidationDebtUpdate` | Update debt records to reflect the liquidated amount |
| 8 | `CLAIM_UPDATE_REPLY (7)` | `Collateral.LiquidationClaimUpdate` | Update collateral positions, record liquidation stats (up to 500 entries in `LIQUIDATION_STATS`) |
| 9 | `BAD_DEBT_REPLY (8)` | Self-callback `BadDebtCheck` | Check for and handle any remaining bad debt (error-tolerant, does not revert the liquidation) |

Each step only proceeds if the previous step succeeds (except step 9 which is error-tolerant).

### Liquidation Statistics

The contract maintains `LIQUIDATION_STATS` as a `Vec` of up to 500 entries, recording historical liquidation data for monitoring and analysis.

## User Flows

### Triggering a Liquidation

1. Anyone calls `Liquidate { position_id, position_owner }`
2. Step 1: Engine sends `Debt.AccrueAndReturnDebt` to get fresh debt data
3. Step 2: Engine sends `Collateral.RefreshAndAssess` to get collateral valuation
   - Computes `current_ltv` and checks insolvency
   - Computes repay amount and fees
   - Checks delay: if below immediate threshold, starts timer (first call) or verifies timer expired (second call)
4. Step 3: Transfers fee collateral out of the position, sends caller fees
5. Steps 4-5: Sends collateral to Liq Queue for absorption
6. Step 6: Sells remaining collateral via sell wall
7. Steps 7-8: Updates debt and collateral records
8. Step 9: Bad debt check (best-effort)

### Delayed Liquidation Flow

1. First call: Timer starts, caller receives upfront fees, no collateral is liquidated yet
2. Borrower has 8 hours (default) to remedy their position
3. Second call (after 8 hours): Full liquidation executes if position is still undercollateralized

## Technical Details

### State

| Key | Type | Description |
|-----|------|-------------|
| `CONFIG` | `Config` | `liquidation_delay` (default 28,800s), `liq_fee` (protocol fee rate), contract addresses, threshold parameters |
| `LIQUIDATION_PROPAGATION` | temporary | In-flight liquidation data passed between reply steps |
| `LIQUIDATION_TIMERS` | `Map` per position | Timestamps for delayed liquidations |
| `LIQUIDATION_STATS` | `Vec` (max 500) | Historical liquidation records |

### Reply IDs

| ID | Constant | Step |
|----|----------|------|
| 1 | `DEBT_ACCRUE_REPLY` | Debt accrual |
| 2 | `COLLATERAL_ASSESS_REPLY` | Collateral assessment |
| 3 | `LIQ_QUEUE_TRANSFER_REPLY` | Collateral transfer to queue |
| 4 | `LIQ_QUEUE_REPLY` | Queue liquidation execution |
| 5 | `SELL_WALL_REPLY` | Sell wall execution |
| 6 | `DEBT_UPDATE_REPLY` | Debt record update |
| 7 | `CLAIM_UPDATE_REPLY` | Claim record update |
| 8 | `BAD_DEBT_REPLY` | Bad debt check |
| 9 | `FEE_TRANSFER_REPLY` | Fee transfer |

## Cross-Contract Interactions

| Direction | Target | Message | Purpose |
|-----------|--------|---------|---------|
| Outbound | Debt | `AccrueAndReturnDebt` | Get accrued debt for liquidation math |
| Outbound | Debt | `LiquidationDebtUpdate` | Write-off liquidated debt |
| Outbound | Collateral | `RefreshAndAssess` | Get collateral valuation and LTV data |
| Outbound | Collateral | `AuthorizedTransfer` | Move collateral for fees and queue absorption |
| Outbound | Collateral | `AuthorizedSellCollateral` | Sell remaining collateral to sell wall |
| Outbound | Collateral | `LiquidationClaimUpdate` | Update position after liquidation |
| Outbound | Liq Queue | `Liquidate` | Execute per-asset liquidation against queue bids |
| Self | Self | `BadDebtCheck` | Error-tolerant bad debt handling |

## Important Invariants

1. **Fee cap**: Fees can never exceed available collateral. In insolvency, caller fee is capped at 1% and protocol fee is zeroed.
2. **Caller incentive scaling**: Caller fee grows linearly with the severity of undercollateralization (`current_ltv - avg_max_ltv`), incentivizing timely liquidation.
3. **Delay protection**: Positions only slightly above `avg_max_ltv` receive an 8-hour grace period; severely undercollateralized positions are liquidated immediately.
4. **Atomicity**: Steps 1-8 are atomic (failure in any step reverts the entire chain). Step 9 (bad debt) is error-tolerant and will not revert a successful liquidation.
5. **Statistics cap**: At most 500 liquidation records are retained in `LIQUIDATION_STATS`.
6. **Repay cap**: The repay amount is always capped at `total_debt`; liquidation never overpays.
