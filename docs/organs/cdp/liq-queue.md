# Liquidation Queue

> **Organ:** CDP

## Purpose

The Liquidation Queue (Liq Queue) is a stability pool where users deposit CDT to purchase liquidated collateral at a discount. It uses a Liquity-style compounding stake distribution model with product/sum snapshots, enabling O(1) claim calculations regardless of how many liquidation events have occurred.

## Key Concepts

### Queue Structure

Each collateral asset has its own queue. A queue contains **premium slots** at integer percentages from 0% up to `max_premium%`. Each premium slot represents a pool of bids willing to buy collateral at that discount.

```
Queue (per collateral asset)
  +-- PremiumSlot[0%]   -- bids, waiting_bids, snapshots
  +-- PremiumSlot[1%]   -- bids, waiting_bids, snapshots
  +-- PremiumSlot[2%]   -- bids, waiting_bids, snapshots
  +-- ...
  +-- PremiumSlot[max%] -- bids, waiting_bids, snapshots
```

Each `PremiumSlot` contains:

| Field | Description |
|-------|-------------|
| `bids` | Active bids participating in liquidations |
| `waiting_bids` | Bids waiting for activation (cooldown period) |
| `liq_premium` | The discount percentage for this slot |
| `sum` | Cumulative sum snapshot (Liquity S) |
| `product` | Cumulative product snapshot (Liquity P) |
| `total_bid_amount` | Total active CDT in this slot |
| `epoch` | Current epoch counter (resets on pool depletion) |
| `scale` | Current scale counter (tracks product magnitude) |
| `residues` | Accumulated rounding residues |

### Liquity-Style Compounding Stake Distribution

The queue uses product and sum snapshots to track each bid's proportional share across multiple liquidation events without iterating over all bids.

**Product (P)**: Tracks the fraction of CDT remaining in the pool after each liquidation. After a liquidation that consumes fraction `f` of the pool:
```
P_new = P_old * (1 - f)
```

**Sum (S)**: Tracks cumulative collateral earned per unit of CDT. After a liquidation distributing `collateral_per_cdt`:
```
S_new = S_old + (collateral_per_cdt / P)
```

**Epoch**: Incremented when the product reaches zero (pool fully depleted). Bids from previous epochs are fully consumed.

**Scale**: Tracks magnitude shifts in the product to maintain numerical precision.

These values are stored in `EPOCH_SCALE_SUM` with composite keys: `"bid_for:premium:epoch:scale"`.

### Bid Lifecycle

1. **Submission**: User places a bid at a chosen premium slot
2. **Activation**: Bid becomes eligible to absorb liquidations
3. **Partial consumption**: Liquidations reduce the bid proportionally
4. **Claim**: User claims earned collateral and any remaining CDT

### Bid Activation Rules

When a bid is submitted to a premium slot:

- **If `slot.total_bid_amount <= bid_threshold`**: Bid activates immediately. Snapshots (`sum`, `product`, `epoch`, `scale`) are copied from the current slot state.
- **If adding the bid would exceed `bid_threshold`**: The bid is **split**:
  - Amount up to the threshold activates immediately
  - Excess goes to `waiting_bids` with `wait_end = block.time + waiting_period`
- **Maximum waiting bids**: Each slot enforces `maximum_waiting_bids`

### Liquidation Execution

Only the Liquidation Engine (positions contract) can trigger liquidations.

For each collateral asset being liquidated:

1. Iterate premium slots from 0% upward (cheapest discount first)
2. At each slot, first activate any expired waiting bids
3. Execute pool liquidation using the Liquity-style compounding model:
   - Calculate the fraction of the pool consumed
   - Update product (P) and sum (S) snapshots
   - Track epoch/scale transitions
4. CDT absorbed during liquidation is burned via `osmosis_proxy.BurnTokens`
5. Any collateral not absorbed by the queue is returned for sell wall processing

### Claim Calculation

When a user claims their bid:

**Remaining CDT** (bid not yet consumed):
```
remaining = initial_amount * (current_product / bid_product_snapshot)
```

Adjusted for epoch transitions (remaining = 0 if epoch advanced past bid's snapshot epoch).

**Earned collateral**:
```
collateral = initial_amount * (sum_at_bid_scale - bid_sum_snapshot) / bid_product_snapshot
```

With cross-scale accumulation when the scale has advanced, and epoch-aware lookups from `EPOCH_SCALE_SUM`.

Residues from rounding are accumulated in the slot's `residues` field and distributed to claimants.

Bids with `remaining <= 1` (1 micro-unit) are considered fully consumed and removed.

## User Flows

### Place a Bid

1. User sends CDT with `SubmitBid { collateral_asset, premium_slot }`
2. Contract locates the queue for the collateral asset
3. Contract checks the target premium slot:
   - If below threshold: activates immediately with current snapshots
   - If at/above threshold: splits, partial activation + waiting queue
4. Waiting bids receive `wait_end = block.time + waiting_period`
5. Bid is stored with snapshot values for later claim calculation

### Claim Liquidation Proceeds

1. User calls `ClaimLiquidation { bid_id }` (or claims all bids)
2. For each bid:
   - Calculate remaining CDT from product snapshot ratio
   - Calculate earned collateral from sum/epoch/scale snapshots
   - Accumulate residues
3. Transfer earned collateral to user
4. Return any remaining CDT to user
5. Remove fully consumed bids (remaining <= 1)

### Retract a Bid

1. User calls `RetractBid { bid_id }`
2. If bid is in waiting queue: remove and refund full amount
3. If bid is active: calculate remaining CDT, refund it, update slot totals

## Technical Details

### State

| Key | Type | Description |
|-----|------|-------------|
| `QUEUES` | `Map` per collateral asset | Queue with premium slots, each containing bids, waiting bids, and Liquity snapshots |
| `EPOCH_SCALE_SUM` | `Map` | Keyed by `"bid_for:premium:epoch:scale"`, stores cumulative sum values for cross-epoch/scale claim calculations |

### Premium Slot Fields

| Field | Type | Description |
|-------|------|-------------|
| `bids` | `Vec<Bid>` | Active bids in this slot |
| `waiting_bids` | `Vec<Bid>` | Bids awaiting activation |
| `liq_premium` | `u64` | Integer discount percentage |
| `sum` | `Decimal` | Cumulative Liquity S snapshot |
| `product` | `Decimal` | Cumulative Liquity P snapshot |
| `total_bid_amount` | `Uint128` | Total active CDT in slot |
| `epoch` | `u64` | Reset counter for pool depletion |
| `scale` | `u64` | Magnitude tracking for product precision |
| `residues` | `Decimal` | Accumulated rounding leftovers |

### Key Parameters

| Parameter | Description |
|-----------|-------------|
| `max_premium` | Maximum discount percentage (highest slot index) |
| `bid_threshold` | CDT amount below which new bids activate immediately |
| `waiting_period` | Seconds before a waiting bid can be activated |
| `maximum_waiting_bids` | Max waiting bids per slot |

## Cross-Contract Interactions

| Direction | Target | Message | Purpose |
|-----------|--------|---------|---------|
| Inbound | Liq Engine | `Liquidate` (per asset) | Execute liquidation against bids in this queue |
| Inbound | Liq Engine | Collateral transfer | Receive collateral to distribute to bidders |
| Outbound | Osmosis Proxy | `BurnTokens` | Burn CDT that was used to purchase liquidated collateral |

## Important Invariants

1. **Slot ordering**: Liquidations always fill from the lowest premium (0%) upward. Bidders at lower premiums get filled first but receive less discount.
2. **Snapshot consistency**: A bid's claim is fully determined by its initial snapshots vs. current slot snapshots. No iteration over liquidation history is needed.
3. **Epoch boundary**: When a pool is fully depleted (product reaches 0), the epoch increments. Bids from previous epochs have zero remaining CDT.
4. **Activation threshold**: Bids only activate immediately when the slot is below `bid_threshold`. This prevents front-running of known liquidation events.
5. **Waiting period**: Bids in the waiting queue cannot participate in liquidations until `block.time >= wait_end`.
6. **Dust removal**: Bids with remaining amount <= 1 micro-unit are considered fully consumed and cleaned up on claim.
7. **Permissioned liquidation**: Only the positions contract (Liquidation Engine) can call `Liquidate`. User bids are passive.
