# LTV Disco

> **Organ:** LTV Disco

## Purpose & Role

LTV Disco is a deposit vault where MBRN holders choose risk tiers (LTV slots) for each collateral asset. Revenue is distributed proportionally to risk using an inverse square root weighting formula, and bad debt is absorbed starting from the riskiest deposits. It is the core mechanism that gives MBRN its value.

## State

| State Key | Type | Description |
|-----------|------|-------------|
| `CONFIG` | `Config` | Contract configuration |
| `ASSET_QUEUES` | `Map<String, AssetQueue>` | Per-asset queues, each containing `Vec<Slot>` |
| `BACKING_DEPOSITS` | `Map<String, BackingDeposit>` | Keyed as `"asset:slot:user:deposit_id"` |
| `USER_DEPOSITS` | Index | User -> deposits lookup |
| `MANAGED_DEPOSITS` | Index | Manager -> managed deposits lookup |
| `MANAGER_FEE` | `Map<Addr, Decimal>` | Manager fee percentages |
| `AFFILIATES` | `Map<Addr, Vec<AffiliateData>>` | Affiliate data, limit 10 per user |
| `REVENUE_EVENTS` | `Map<(String, String), Vec<RevenueEvent>>` | Per (asset, slot) revenue event history |
| `REVENUE_TRACKING` | Capped (100) | Revenue tracking snapshots |
| `PENDING_BAD_DEBT` | `Map<String, Uint128>` | Per-asset pending bad debt (freezes withdrawals) |
| `RATE_ASSURANCE` | `Map<(String, String), Uint128>` | Per-slot VT rate snapshots |
| `UNSTAKE_REQUESTS` | Map | All unstake requests |
| `USER_UNSTAKE_REQUESTS` | Map | Per-user unstake requests |
| `USER_TOTAL_DEPOSITS` | Map | Aggregate deposit value per user |
| `USER_LIFETIME_REVENUE` | Capped (100) | Lifetime revenue tracking per user |
| `DAILY_TVL_TRACKER` | Capped (100) | Daily TVL snapshots |
| `DAILY_DEPOSIT_TRACKER` | Capped (100) | Daily deposit flow snapshots |

## Constants

| Constant | Value | Description |
|----------|-------|-------------|
| `REVENUE_TRACKING_LIMIT` | 100 | Max revenue tracking entries |
| `LIFETIME_REVENUE_LIMIT` | 100 | Max lifetime revenue entries per user |
| `TVL_TRACKER_LIMIT` | 100 | Max daily TVL snapshots |
| `DEPOSIT_TRACKER_LIMIT` | 100 | Max daily deposit snapshots |
| `ONE_DAY_SECONDS` | 86,400 | — |
| `COMPOUND_SWAP_REPLY_ID` | 3 | Reply ID for compound swap handling |
| `DEFAULT_VAULT_TOKENS_PER_STAKED_BASE_TOKEN` | 1,000,000 | Initial VT mint ratio |
| `AFFILIATE_LIMIT` | 10 | Max affiliates per user |
| `MAX_LIMIT` (query) | 32 | Max items per query response |
| Default `unstaking_period` | 172,800 (2 days) | Unstake cooldown |
| Default `affiliate_fee` | 1% | Affiliate referral fee |
| Default `max_management_fee` | 5% | Manager fee cap |
| Rate assurance precision | 1 trillion VT | `base_tokens_per_trillion_vault_tokens` |
| Revenue weight scale | 10^18 | Scale factor for weight calculations |
| Compound swap `max_slippage` | 90% | Slippage tolerance for compound swaps |

## Key Concepts

### Asset Queues & Slots

Each collateral asset has its own queue with slots at LTV intervals:

```
Asset Queue: ATOM
|-- Slot 50% LTV (safest)  -> Low revenue weight, last slashed
|-- Slot 51% LTV
|-- ...
|-- Slot 89% LTV
|-- Slot 90% LTV (riskiest) -> High revenue weight, first slashed
```

Each slot tracks:
- `total_vault_tokens`: Sum of all VTs in this slot
- `total_deposit_tokens`: Sum of underlying deposit tokens backing the VTs

The VT rate for a slot = `total_deposit_tokens / total_vault_tokens`.

### Backing Deposits

Each deposit is a `BackingDeposit`:

```rust
BackingDeposit {
    deposit_id: Uint128,
    vault_tokens: Uint128,          // User's share of the slot
    deposit_owner: Addr,
    manager: Option<Addr>,           // Can move but not withdraw
    revenue_destination: Option<Addr>, // Custom revenue recipient
    affiliate: Option<Addr>,
}
```

Deposits are stored in a map keyed as `"asset:slot:user:deposit_id"`.

### Slot Weight Formula (calculate_slot_weights)

```
1. Filter to active slots only
2. total_deposits = sum of all active slot deposits
3. avg_slot_size = total_deposits / non_empty_active_count  (min 1)
4. Iterate slots DESCENDING by LTV (riskiest first)
5. Track deposits_above = 0 (cumulative from riskier slots)
6. For each non-empty slot:
     weight = 10^18 / isqrt(deposits_above + avg_slot_size)
     deposits_above += slot_deposits
7. total_weight = sum of all raw weights
8. final_weight[slot] = raw_weight[slot] / total_weight
```

The riskiest slot (highest LTV, processed first with `deposits_above = 0`) always gets the highest weight. Weights decrease via inverse square root as cumulative deposits grow.

### Revenue Distribution

**CDT Revenue** (`AddRevenue`):
1. CDT sent to the contract for a specific asset queue
2. Slot weights calculated via the formula above
3. `RevenueEvent` created per slot with `amount_per_vt = (total_revenue * slot_weight) / slot_total_vt`
4. Users claim by iterating all events since their last claim index
5. Manager fee deducted if manager is set (capped at `max_management_fee`, default 5%)

**Deposit Token Revenue** (`AddDepositTokenRevenue`, auction only):
- Directly adds to `slot.total_deposit_tokens` proportionally by each deposit's size
- No `RevenueEvent` created, no claiming needed
- Silently increases VT rate for all depositors in that slot

### Bad Debt Slashing

**Phase 1 — Freeze** (`AddBadDebt` from CDP):
- `PENDING_BAD_DEBT[asset] += amount`
- Freezes withdrawals and unstaking for this asset
- Starts MBRN auction at the auction contract

**Phase 2 — Slash** (`SendMBRNForSale` from auction):
- Iterates slots **descending by LTV** (riskiest first)
- For each slot: `slash = min(remaining, slot.total_deposit_tokens)`
- Reduces `total_deposit_tokens` WITHOUT reducing `total_vault_tokens`
- Effect: VT rate drops for slashed slots
- Sends slashed deposit tokens (MBRN) to auction buyer

**Phase 3 — Unfreeze** (`ClearBadDebtFreeze` from auction):
- Reduces `PENDING_BAD_DEBT[asset]`
- When it reaches zero, the asset queue is unfrozen

### Manager System

- Depositors assign a manager who can `MoveDeposit` between slots
- Managers earn a fee on claimed revenue (set via `SetManagerFee`, capped at `max_management_fee`)
- Managers **cannot** withdraw deposits or change the deposit owner

### Rate Assurance

Per-slot check that `base_tokens_per_trillion_vault_tokens` hasn't decreased during operations. Stored in `RATE_ASSURANCE` map keyed by `(asset, slot)`. Prevents economic exploits during deposit/withdrawal/move operations.

### Unstaking (2-Step)

| Step | Action | Notes |
|------|--------|-------|
| 1 | `RequestUnstake` | Creates request with `unlock_time = now + unstaking_period`. Deposit keeps earning. VTs NOT removed yet. Blocked if `PENDING_BAD_DEBT > 0` or has emissions votes |
| 2 | `CompleteUnstake` | After cooldown: calculates `base_tokens` from current VT ratio (may be reduced by slashing). Sends deposit tokens to user |
| Cancel | `CancelUnstake` | Cancels pending request |

### Compound

`CompoundAction` flow:
1. Claimed CDT sent to `chain_proxy.ExecuteSwaps` with 90% max slippage
2. Reply handler (`COMPOUND_SWAP_REPLY_ID = 3`) calculates received deposit tokens
3. Creates `SubmitDeposit` self-calls to top up each contributing deposit

### MoveDeposit

Moves a deposit between slots without actual token transfer:
1. Converts source VT to base tokens via source slot's ratio
2. Converts base tokens to new VT via destination slot's ratio
3. Creates new deposit with fresh `deposit_id` in destination slot
4. Preserves metadata (owner, manager, revenue_destination, affiliate)

## Execute Messages

| Message | Auth | Description |
|---------|------|-------------|
| `CreateQueue` | Owner | Create asset queue with LTV range |
| `UpdateQueue` | Owner | Expand/contract LTV range |
| `SubmitDeposit` | Anyone | Deposit into asset + slot. If `deposit_id` provided, tops up existing deposit (auto-claims revenue first) |
| `RequestUnstake` | Deposit owner | Start unstaking cooldown |
| `CompleteUnstake` | Deposit owner | Withdraw after cooldown at current VT rate |
| `CancelUnstake` | Deposit owner | Cancel pending unstake request |
| `MoveDeposit` | Owner or Manager | Move deposit between slots (VT conversion at current rates) |
| `UpdateDeposit` | Deposit owner | Change owner, manager, revenue_destination |
| `AddBadDebt` | CDP | Set pending bad debt, freeze asset queue |
| `AddRevenue` | Revenue Dist. | Distribute CDT revenue per slot weights |
| `AddDepositTokenRevenue` | Auction only | Directly add deposit tokens to slots (no events) |
| `ClaimRevenueForUser` | Anyone | Claim user's accumulated CDT revenue, optional compound |
| `SetManagerFee` | Manager | Set management fee percentage (capped at `max_management_fee`) |
| `SetAffiliate` | User | Set affiliate/referrer |
| `SendMBRNForSale` | Auction | Slash MBRN from slots for bad debt sale |
| `ClearBadDebtFreeze` | Auction | Unfreeze asset queue after bad debt resolved |
| `RateAssurance` | Self | Verify per-slot VT rate stability |

## Query Messages

| Query | Returns | Description |
|-------|---------|-------------|
| `Config` | `Config` | Contract configuration |
| `AssetQueue` / `AssetQueues` | Queue state | Queue info with slots |
| `UserDeposits` | Deposits | All deposits for a user (paginated, max 32) |
| `UserTotalDeposits` | Aggregate | Total deposit value per user |
| `UserRevenue` | Claimable CDT | Pending revenue across deposits |
| `SlotInfo` | Slot state | TVL, VT supply, deposits per slot |
| `UnstakeRequests` | Requests | User's pending unstake requests |
| `QueueAverageLTV` | Weighted avg | Deposit-weighted average LTV (queried by CDP Collateral) |
| `DailyTVL` | Snapshots | Daily TVL tracker (max 100 entries) |
| `ManagerFee` | Fee % | Manager's current fee |
| `UserLifetimeRevenue` | History | Lifetime revenue per user (max 100 entries) |

## Config

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `cdp_contract` | `String` | — | CDP for bad debt calls |
| `deposit_denom` | `DepositDenom` | — | Accepted deposit token (MBRN) |
| `cdt_denom` | `String` | — | CDT for revenue |
| `minimum_deposit` | `Uint128` | — | Min deposit amount |
| `unstaking_period` | `u64` | 172,800 (2 days) | Unstake cooldown in seconds |
| `oracle_contract` | `String` | — | Oracle for price queries |
| `chain_proxy_contract` | `String` | — | DEX router for compound swaps |
| `emissions_voting_contract` | `Option<String>` | — | Emissions voting for deposit power |
| `affiliate_fee` | `Decimal` | 1% | Affiliate referral fee |
| `max_management_fee` | `Decimal` | 5% | Manager fee cap |
| `auction_contract` | `Option<String>` | — | Auction for MBRN bad debt sales |
| `mbrn_denom` | `Option<String>` | — | MBRN denom for slashing/sales |

## Cross-Contract Interactions

### Calls (Outgoing)

| Contract | Message | Purpose |
|----------|---------|---------|
| **Chain Proxy** | `ExecuteSwaps` | Compound revenue into deposit tokens |
| **Auction** | Start MBRN sale | Initiate bad debt auction |
| **Emissions Voting** | Deposit power updates | Update voting weight |

### Called By (Incoming)

| Contract | Message | Purpose |
|----------|---------|---------|
| **Revenue Distributor** | `AddRevenue` | CDT revenue distribution |
| **Auction** | `AddDepositTokenRevenue` | Direct deposit token revenue |
| **Auction** | `SendMBRNForSale` | Slash MBRN for bad debt |
| **Auction** | `ClearBadDebtFreeze` | Unfreeze after bad debt |
| **CDP (Debt)** | `AddBadDebt` | Initiate bad debt process |
| **Acquisition** | `SubmitDeposit` | MBRN rewards deposited with contract as owner, user as manager + revenue_destination |
| **Collateral** | `QueueAverageLTV` query | Dynamic LTV targets |
| **Users** | All deposit/unstake/claim messages | Direct interaction |

## Important Invariants & Edge Cases

- **Slot ordering**: Slots are processed descending by LTV for both revenue weighting and bad debt slashing. The highest LTV slot is always riskiest, earns most, and is slashed first
- **Revenue during unstake**: Deposits continue earning revenue during the cooldown period. VTs are not removed until `CompleteUnstake` executes
- **Unstake blocked during bad debt**: `RequestUnstake` errors if `PENDING_BAD_DEBT > 0` for the asset, or if the deposit has active emissions votes
- **Bad debt reduces rate, not VTs**: Slashing reduces `total_deposit_tokens` without reducing `total_vault_tokens`. This means VT holders in slashed slots receive fewer underlying tokens per VT
- **AddDepositTokenRevenue is silent**: Unlike `AddRevenue`, it creates no events and requires no claiming. It directly increases the VT rate by adding to `total_deposit_tokens`
- **MoveDeposit creates new deposit_id**: Moving between slots creates a fresh deposit with a new ID. The old deposit is removed. Metadata is preserved
- **Compound uses 90% slippage**: The compound swap via chain proxy allows up to 90% slippage. This is intentionally permissive to handle volatile assets
- **Rate assurance per slot**: Each slot independently verifies its VT rate. Uses precision of 1 trillion VT to detect small manipulations
- **Acquisition deposits**: When the Acquisition contract deposits MBRN on behalf of a user, the contract is set as `deposit_owner` and the user is set as `manager` + `revenue_destination`. The user earns revenue during cliff, but cannot withdraw the principal until ownership is transferred
- **Dynamic LTV influence**: `QueueAverageLTV` returns the deposit-weighted average LTV across all depositors in a queue. CDP Collateral uses this to adjust max LTV parameters
- **All tracker maps capped**: Revenue tracking, TVL, deposit trackers, and lifetime revenue are all capped at 100 entries to prevent unbounded state growth
