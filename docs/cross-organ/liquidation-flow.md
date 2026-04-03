# Liquidation Flow

The complete liquidation lifecycle from insolvency detection through bad debt resolution, implemented as a 9-step SubMsg reply chain in the Liquidation Engine contract.

## Overview

Membrane's liquidation is a **multi-stage filtration system** that processes insolvent positions through progressively more aggressive mechanisms:

```
Insolvency detected
       |
       v
  [Timer delay check] -- under threshold --> start timer, pay caller fee, return
       |
       | (over threshold OR timer expired)
       v
  [Liq Queue] -- Liquity-style product/sum snapshots
       |
       | (remaining collateral)
       v
  [Sell Wall] -- market sells via DEX
       |
       v
  [Bad Debt Check] -- if collateral < $1 and debt > 0 --> MBRN auction
```

## SubMsg Reply Chain

**Source:** `contracts/liquidation-engine/src/reply.rs`, `contracts/liquidation-engine/src/state.rs`

The liquidation executes as a chain of SubMsgs where each reply handler triggers the next step. The chain is: **1 -> 2 -> 9 -> 3 -> 4 -> 5 -> 6 -> 7 -> 8**.

### Reply IDs

| ID | Constant | Step | Contract Call |
|----|----------|------|---------------|
| 1 | `DEBT_ACCRUE_REPLY_ID` | Accrue debt | `Debt.AccrueAndReturnDebt` |
| 2 | `COLLATERAL_ASSESS_REPLY_ID` | Assess collateral | `Collateral.RefreshAndAssess` |
| 9 | `FEE_TRANSFER_REPLY_ID` | Extract fees | `Collateral.AuthorizedTransfer` (fees to engine) + `BankMsg::Send` (caller fees) |
| 3 | `LIQ_QUEUE_TRANSFER_REPLY_ID` | Transfer to LQ | `Collateral.AuthorizedTransfer` (collateral to LQ) |
| 4 | `LIQ_QUEUE_REPLY_ID` | Execute LQ | `LQ.Liquidate` per asset |
| 5 | `SELL_WALL_REPLY_ID` | Sell remaining | `Collateral.AuthorizedSellCollateral` |
| 6 | `DEBT_UPDATE_REPLY_ID` | Update debt | `Debt.LiquidationDebtUpdate` |
| 7 | `CLAIM_UPDATE_REPLY_ID` | Update claims | `Collateral.LiquidationClaimUpdate` + stats |
| 8 | `BAD_DEBT_REPLY_ID` | Bad debt check | Self-callback `BadDebtCheck` (reply_on_error, no-op handler) |

### Propagation State

All intermediate data is carried through the chain via `LIQUIDATION_PROPAGATION`:

```rust
pub struct LiquidationPropagation {
    pub per_asset_repayment: Vec<...>,
    pub liq_queue_repayment: Decimal,
    pub protocol_fee_value: Decimal,
    pub caller_fee_assets: Vec<Coin>,
    pub protocol_fee_assets: Vec<Coin>,
    pub sell_wall_amounts: Vec<...>,
    pub cAsset_ratios: Vec<Decimal>,
    pub cAsset_prices: Vec<PriceResponse>,
    pub collateral_assets: Vec<...>,
    pub total_debt: Uint128,
    pub credit_price: PriceResponse,
    pub caller_fee_value_paid: Decimal,
    pub total_repaid: Decimal,
    pub venue_recall_amount: Uint128,
    pub position_owner: Addr,
    pub position_id: Uint128,
    pub caller: Addr,
    pub timer_start_only: bool,
}
```

## Step-by-Step Detail

### Step 1: Debt Accrual (Reply ID = 1)

**Entry point:** `liquidate()` in `liquidations.rs`

Sends `SubMsg::reply_on_success` to `Debt.AccrueAndReturnDebt` with position owner and ID.

**Reply handler:** `handle_debt_accrue_reply`
- Parses `AccrueDebtResponse` from `"debt_response"` attribute (JSON binary in event attribute)
- Stores `total_debt`, `credit_price`, `venue_recall_amount` in propagation
- Chains to `Collateral.RefreshAndAssess`

### Step 2: Collateral Assessment (Reply ID = 2)

**Reply handler:** `handle_collateral_assess_reply`

Parses `CollateralAssessmentResponse` from `"assessment"` attribute. Performs:

1. **Circuit breaker:** Errors if any collateral asset is frozen (`assessment.any_frozen`)
2. **Collateral value:** Sums `price.get_value(amount)` across all position assets
3. **Repay quantities:** Calls `get_repay_quantities()`
4. **Liquidation delay logic** (see below)
5. **Fee cap** (see below)
6. **Per-asset fee computation** (see below)
7. Chains to `FEE_TRANSFER_REPLY_ID` if fees exist, otherwise skips to LQ

#### Repay Quantity Formula

```
current_ltv = compute_ltv(collateral_value, debt_value)

if current_ltv >= 1.0:
    repay_value = collateral_value   // use all collateral
else:
    excess = current_ltv - avg_borrow_ltv
    fraction = excess / current_ltv
    repay_value = debt_value * fraction

repay_amount = min(credit_price.get_amount(repay_value), total_debt)
```

#### Liquidation Delay

```
immediate_threshold = avg_max_ltv * (1 + avg_max_threshold_to_delay)
```

Three paths:

| Condition | Action |
|-----------|--------|
| `current_ltv > immediate_threshold` | Clear any timer, proceed immediately |
| Timer exists and expired | Clear timer, proceed with full liquidation |
| Timer exists but active | Reject with `LiquidationTimerActive` error |
| No timer, under threshold | Save timer, zero protocol fee, set `timer_start_only = true` |

Timer state: `LIQUIDATION_TIMERS: Map<(&str, &str), LiquidationTimer>` keyed by `(position_owner, position_id)`.

When `timer_start_only = true`, the chain proceeds through fee extraction (caller fees only, protocol fee zeroed) and returns early -- no LQ, no sell wall, no debt update.

The delay window is empirically tuned per asset. See `docs/liquidation_delay_window.md` for the analysis showing a 4% max window within 1 hour covers 99% of recovery scenarios for BTC, ATOM, and OSMO.

#### Fee Computation

```
caller_fee_rate  = current_ltv - avg_max_ltv     // dynamic
protocol_fee_rate = config.liq_fee                // static
```

**Fee cap:** If `(caller + protocol) * repay_value > collateral_value`:
```
caller_fee_rate = min(1%, collateral_value / repay_value)
protocol_fee_rate = 0
```

**Per-asset fees** (mirrors CDP `per_asset_fulfillments` lines 745-858):

For each collateral asset `i`:
```
collateral_repay_amount = get_amount(repay_value * ratio[i])
caller_fee_amount  = collateral_repay_amount * caller_fee_rate
protocol_fee_amount = collateral_repay_amount * protocol_fee_rate
```

Each fee is capped at position amount. Caller fee deducted first, protocol fee from remainder. If fee >= position amount, it is zeroed (not clamped to position amount).

### Step 9: Fee Transfer (Reply ID = 9)

**Reply handler:** `handle_fee_transfer_reply`

`Collateral.AuthorizedTransfer` moves combined fee assets from position to liquidation engine contract. Then:

1. Sends caller fees via `BankMsg::Send` to `prop.caller`
2. Protocol fees: currently stay in liquidation engine (TODO: route to revenue distributor via `AddNonCdtRevenue`)
3. If `timer_start_only`: returns early after paying caller
4. Otherwise: chains to LQ transfer via `build_next_after_fees`

If the transfer fails, liquidation continues without fees (graceful degradation).

### Step 3: LQ Transfer (Reply ID = 3)

**Reply handler:** `handle_liq_queue_transfer_reply`

Transfers collateral from position to Liq Queue via `Collateral.AuthorizedTransfer`, then builds per-asset `LQ.Liquidate` SubMsgs.

### Step 4: Liq Queue Execution (Reply ID = 4)

**Reply handler:** `handle_liq_queue_reply`

The Liq Queue uses **Liquity-style product/sum snapshots** for O(1) per-user liquidation claims. Each asset gets its own `Liquidate` call. The reply captures how much CDT was repaid by the queue.

Remaining collateral (not absorbed by LQ) proceeds to sell wall.

### Step 5: Sell Wall (Reply ID = 5)

**Reply handler:** `handle_sell_wall_reply`

Calls `Collateral.AuthorizedSellCollateral` to market-sell remaining collateral via DEX. The sell proceeds (CDT) count toward total repayment.

### Step 6: Debt Update (Reply ID = 6)

**Reply handler:** `handle_debt_update_reply`

Calls `Debt.LiquidationDebtUpdate` to reduce the position's outstanding debt by the total amount repaid across LQ and sell wall.

### Step 7: Claim Update (Reply ID = 7)

**Reply handler:** `handle_claim_update_reply`

Calls `Collateral.LiquidationClaimUpdate` to adjust the position's collateral claims. Records liquidation stats in `LIQUIDATION_STATS` (capped at `MAX_LIQUIDATION_STATS = 500` entries):

```rust
pub struct LiquidationEvent {
    pub position_id: Uint128,
    pub position_owner: Addr,
    pub caller: Addr,
    pub total_debt: Uint128,
    pub total_repaid: Decimal,
    pub bad_debt: Uint128,
    pub caller_fee_value: Decimal,
    pub protocol_fee_value: Decimal,
    pub timestamp: u64,
}
```

Chains to `BadDebtCheck` as self-callback.

### Step 8: Bad Debt Check (Reply ID = 8)

**Reply handler:** No-op (reply_on_error only, so failures don't revert the liquidation)

The `bad_debt_check()` function:

1. Queries `Collateral.GetPositionValue` for remaining collateral
2. Queries `Debt.GetDebtPositions` for remaining debt
3. If `collateral_value < $1` AND `remaining_debt > 0`: this is bad debt

Bad debt triggers:
- `Auction.StartAuction` with `repayment_position_info` pointing to the underwater position
- If `config.debt_auction` is configured, routes there

## Bad Debt Resolution (LTV Disco)

When bad debt is identified:

1. **`LTV_Disco.AddBadDebt`**: Freezes the affected asset in `PENDING_BAD_DEBT` map. The asset's queue is frozen -- no new deposits, no withdrawals
2. **`Auction.StartMBRNSale`**: Starts an auction (no CDT funds attached -- the auction sources MBRN)
3. **`LTV_Disco.SendMBRNForSale`**: Pulls MBRN from Disco by slashing slots in **descending LTV order** (riskiest first). Reduces `total_deposit_tokens` without reducing `total_vault_tokens`, effectively diluting remaining depositors. The slashed MBRN is sent to the auction recipient
4. **`Debt.FulfillBadDebt`**: CDT from the auction is used to pay off the bad debt position

The `SendMBRNForSale` slashing has a **recursion max depth of 3** to prevent gas exhaustion on deeply nested bad debt events.

5. **`ClearBadDebtFreeze`**: Once CDT is received to cover the bad debt, removes the asset from `PENDING_BAD_DEBT` and unfreezes the queue
