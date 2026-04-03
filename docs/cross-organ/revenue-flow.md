# Revenue Flow

How protocol revenue moves from interest accrual and swap fees through the Revenue Distributor to stakers, LTV Disco depositors, and affiliates.

## Sources of Revenue

### 1. CDP Interest (CDT)

Debt positions accrue interest continuously via rate indices in the Debt contract:

```
accumulated_interest = principal * rate * elapsed / SECONDS_PER_YEAR
```

This accumulates as `pending_revenue` in the basket. The Revenue Distributor collects it via `TakeRevenue`.

### 2. Transmuter Swap Fees (CDT or paired asset)

Every non-allowlisted swap through the Transmuter incurs a `usage_fee` (default 1%). The fee is split:

- **80%** stays in the Transmuter contract (accrues to LP/VT holders via vault token rate appreciation)
- **20%** (`revenue_distributor_fee_percentage`, configurable) is sent to the Revenue Distributor via `SetPromises`

The Transmuter handles CDT fees directly. Paired-asset fees are accumulated in `PENDING_REVENUE` state and transmuted to CDT (1:1) when CDT balance is available, then forwarded.

The `usage_fee_utilization_threshold` exists in config (defaults to 80%) but the utilization check is dead code -- fees apply to all non-allowlisted swaps regardless of utilization.

### 3. Non-CDT Revenue

Any non-CDT asset sent via `AddNonCdtRevenue` is routed to the Auction contract via `StartAuction`, which converts it to CDT through the auction mechanism.

## Revenue Distributor Contract

**Source:** `contracts/revenue-distributor/src/contract.rs`

### State

| Key | Type | Purpose |
|-----|------|---------|
| `PROMISES` | `Item<Vec<RevenuePromise>>` | Pending affiliate/destination promises |
| `LTV_DISCO_DISTRIBUTION` | `Map<String, Uint128>` | Per-asset CDT amounts accumulated for LTV Disco |
| `EPOCH_REVENUE_ACCUMULATION` | `Map<String, Uint128>` | Per-asset revenue in current epoch (cleared on distribution) |
| `LAST_DISTRIBUTION_TIME` | `Item<u64>` | Timestamp of last distribution |
| `PRE_TAKE_REVENUE_BALANCE` | `Item<Uint128>` | CDT balance snapshot before TakeRevenue call |
| `PRE_TAKE_REVENUE_PER_ASSET` | `Item<Vec<Asset>>` | Per-asset ratios saved before TakeRevenue |

### Entry Points

| Message | Description |
|---------|-------------|
| `TakeRevenueFromBasket` | Triggers CDP revenue collection. Saves CDT balance snapshot, calls CDP `TakeRevenue` as SubMsg |
| `ExecuteRevenueDistribution` | Orchestrates: TakeRevenue -> SetPromises -> DistributePromises |
| `SetPromises` | Receives CDT + affiliate promises + LTV Disco distribution ratios. Accumulates into `LTV_DISCO_DISTRIBUTION` map |
| `DistributePromises` | Distributes accumulated revenue to all destinations |
| `AddNonCdtRevenue` | Routes non-CDT revenue to Auction via `StartAuction` |

## Revenue Collection Flow

### Step 1: TakeRevenue (Reply ID = 3)

```
Revenue Distributor                          CDP / Debt
       |                                        |
       |-- Save PRE_TAKE_REVENUE_BALANCE ------->|
       |-- Save PRE_TAKE_REVENUE_PER_ASSET ----->|
       |-- SubMsg: CDP.TakeRevenue ------------->|
       |                                        |
       |<---- Reply (TAKE_REVENUE_REPLY_ID=3) ---|
       |                                        |
       |-- actual_revenue = balance - pre_bal ---|
       |-- Self-call: SetPromises -------------->|
       |-- Self-call: DistributePromises ------->|
```

The reply handler (`handle_take_revenue_reply`) computes `actual_revenue = current_balance - pre_balance` to determine exactly how much CDT was received, then chains `SetPromises` and `DistributePromises` as ordinary messages (not SubMsgs).

### Step 2: SetPromises

Receives CDT funds along with:
- `promises`: affiliate fee entries (`address`, `amount`)
- `ltv_disco_distribution`: per-asset CDT amounts (e.g., how much revenue came from ATOM collateral vs USDC collateral)

Actions:
1. Validates sent amount >= total promised
2. Accumulates `ltv_disco_distribution` entries into `LTV_DISCO_DISTRIBUTION` map (additive)
3. Accumulates into `EPOCH_REVENUE_ACCUMULATION` for frontend queries
4. Merges new promises into existing `PROMISES` (aggregates by address)

### Step 3: DistributePromises

Enforces a **dispersal window** check: `revenue_dispersal_window` days (default 7) must pass since `LAST_DISTRIBUTION_TIME`. First distribution always allowed.

```
window_seconds = window_days * 24 * 60 * 60
if last_distribution > 0 && current_time < last_distribution + window_seconds:
    return early (window not passed)
```

Distribution targets:

#### Affiliate Promises

Each affiliate promise is distributed via `BankMsg::Send` as a `SubMsg::reply_always` with `DISTRIBUTION_REPLY_ID=1`. On success, the reply handler awards points via `PointsSystem.GivePointsForAffiliateFee` (as `SubMsg::reply_on_error` so points failure does not revert the distribution).

#### LTV Disco Promises

Promises with address matching `config.ltv_disco` are **skipped** during promise distribution (they are removed from PROMISES but no SubMsg is sent). LTV Disco receives its share separately through the destination path.

#### Revenue Destinations (Staking)

Remaining CDT balance (after affiliate promises) is split by `distribution_ratio` across `config.revenue_destinations`. Each destination receives `StakingExecuteMsg::DepositFee` as a `SubMsg::reply_always` with `REVENUE_DESTINATION_REPLY_ID=2`.

#### LTV Disco Revenue Destination

If a revenue destination matches `config.ltv_disco`, the system calls `ltv_disco_add_revenue_msgs` which generates per-asset `AddRevenue` messages. Each message sends CDT proportional to that asset's accumulated share in `LTV_DISCO_DISTRIBUTION`. After sending, the `LTV_DISCO_DISTRIBUTION` map is cleared.

### Reply Handling

| Reply ID | Handler | Purpose |
|----------|---------|---------|
| 1 (`DISTRIBUTION_REPLY_ID`) | `handle_distribution_reply` | Processes affiliate/destination promise result. Success: remove from PROMISES, award points. Failure: move to FAILED_DISTRIBUTIONS |
| 2 (`REVENUE_DESTINATION_REPLY_ID`) | `handle_revenue_destination_reply` | Logs success/failure for DepositFee calls. No retry tracking |
| 3 (`TAKE_REVENUE_REPLY_ID`) | `handle_take_revenue_reply` | Computes actual revenue received, chains SetPromises + DistributePromises |

## Staking Fee Distribution

When `DepositFee` is called on the Staking contract:

1. Calculates `total = totals.vesting_contract + totals.stakers` (if zero, defaults to 1)
2. For each fee asset, creates a `FeeEvent`:
   ```
   FeeEvent {
       time_of_event: block_time,
       fee: LiqAsset {
           info: asset_info,
           amount: asset_amount / total   // amount per staked MBRN
       }
   }
   ```
3. Vesting contract MBRN is weighted by `vesting_rev_multiplier` (default 20%):
   ```
   totals.vesting_contract = vesting_amount * vesting_rev_multiplier
   ```
   This means vesting MBRN earns 20% of the revenue that fully staked MBRN earns.

Users claim accumulated fee events based on which events occurred after their deposit timestamp.

## Transmuter Fee Mechanics (Detail)

The Transmuter's `handle_revenue_distribution` function (`contract.rs` line ~1680):

1. Loads `PENDING_REVENUE` (accumulated paired-asset fees from prior swaps)
2. If current fee is in paired asset: adds to pending. If in CDT: uses directly
3. Calculates `amount_to_send = total_fee_amount * revenue_distributor_fee_percentage`
4. If fee is CDT: sends directly to Revenue Distributor via `SetPromises` with `ltv_disco_distribution` ratios from `config.revenue_distributions`
5. If fee is paired asset: attempts to transmute to CDT (1:1). If CDT balance available, transmutes min(amount_to_send, cdt_balance). Remainder saved as `PENDING_REVENUE`

## Failed Distribution Recovery

- `FAILED_DISTRIBUTIONS`: `Map<String, u128>` tracks cumulative failed amounts per address
- `RetryFailedDistribute`: re-attempts failed distributions
- `ClearFailedDistributions`: admin-only cleanup
- `ClearPendingDistributions`: admin-only emergency reset of `DISTRIBUTION_PROP`

## Epoch Revenue Tracking

`EPOCH_REVENUE_ACCUMULATION` tracks per-asset revenue within the current distribution window. Cleared when `DistributePromises` executes successfully, enabling the frontend to display per-epoch revenue breakdowns.
