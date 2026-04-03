# Revenue Distributor

> **Organ:** Revenue

## Purpose

The Revenue Distributor collects CDT revenue from the CDP basket, manages distribution promises, and sends revenue to staking, LTV Disco, affiliates, and other configured destinations. It operates on a 7-day dispersal window.

## Key Concepts

### Promises

Promises are commitments to send specific amounts of revenue to specific addresses. They are set via `SetPromises` and fulfilled via `DistributePromises`.

**Setting promises** (`SetPromises`):

- Requires 1 CDT in sent funds.
- Total promised amount must be `<= sent amount`.
- `ltv_disco_distribution` accumulates per-asset breakdowns.
- Promises to the same address are merged (amounts combined).

### Distribution Flow

`DistributePromises` executes the following:

1. **Window check**: Enforces `revenue_dispersal_window` (7 days = `7 * 86400` seconds) since `LAST_DISTRIBUTION_TIME`.
2. **LTV Disco promises**: Skipped (removed from the promise list) -- they are handled separately via `ltv_disco_add_revenue_msgs`.
3. **Revenue destination promises**: Sent to staking via `StakingExecuteMsg::DepositFee` as `SubMsg` with `reply_always`.
4. **Affiliate promises**: Sent via `BankMsg::Send` as `SubMsg` with `reply_always`.
5. **Remainder**: Any CDT balance remaining after promises is split according to `revenue_destinations` ratios.
6. **LTV Disco revenue**: Sent via `ltv_disco_add_revenue_msgs`.
7. **Cleanup**: Updates `LAST_DISTRIBUTION_TIME`, clears `EPOCH_REVENUE_ACCUMULATION`.

### Taking Revenue from CDP

`TakeRevenueFromBasket`:

1. Saves current CDT balance to `PRE_TAKE_REVENUE_BALANCE`.
2. Queries basket's `pending_revenue.per_asset_rev` for per-asset breakdown.
3. Sends `SubMsg` to CDP's `TakeRevenue` with `TAKE_REVENUE_REPLY_ID = 3`.
4. Reply handler calculates `actual_revenue = current_balance - pre_balance`.
5. Self-calls `SetPromises` + `DistributePromises` in sequence.

### Reply Handlers

**DISTRIBUTION_REPLY_ID (1):**

- Pops from `DISTRIBUTION_PROP` FIFO queue.
- On success: removes the promise from state, sends affiliate points (if applicable).
- On failure: moves the promise to `FAILED_DISTRIBUTIONS` map for later retry.

**TAKE_REVENUE_REPLY_ID (3):**

- Calculates actual revenue received: `current_CDT_balance - PRE_TAKE_REVENUE_BALANCE`.
- Chains to `SetPromises` (with per-asset revenue data) then `DistributePromises`.

## State

| Key | Type | Description |
|-----|------|-------------|
| `CONFIG` | `Item<Config>` | Contract configuration |
| `PROMISES` | `Item<Vec<Promise>>` | Pending distribution promises |
| `FAILED_DISTRIBUTIONS` | `Map<String, Promise>` | Promises that failed to distribute |
| `DISTRIBUTION_PROP` | `Item<Vec<DistProp>>` | FIFO queue for reply handling |
| `LTV_DISCO_DISTRIBUTION` | `Map<String, Uint128>` | Per-asset accumulation for LTV Disco |
| `EPOCH_REVENUE_ACCUMULATION` | `Map<String, Uint128>` | Per-asset revenue tracking for current epoch |
| `LAST_DISTRIBUTION_TIME` | `Item<u64>` | Timestamp of last distribution |
| `PRE_TAKE_REVENUE_BALANCE` | `Item<Uint128>` | CDT balance before TakeRevenue call |
| `PRE_TAKE_REVENUE_PER_ASSET` | `Item<Vec<(String, Uint128)>>` | Per-asset balances before TakeRevenue |

## Default Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `revenue_dispersal_window` | 7 days | Minimum time between distributions |

## User Flows

### Trigger Revenue Collection and Distribution

1. Anyone calls `TakeRevenueFromBasket` (permissionless).
2. Contract queries CDP basket for pending revenue.
3. Sends `TakeRevenue` to CDP as SubMsg.
4. Reply calculates actual CDT received.
5. Auto-chains to `SetPromises` (allocates revenue) then `DistributePromises` (sends it out).

### Manual Promise Setting

1. Authorized caller sends CDT with `SetPromises` specifying destinations and amounts.
2. Promises are merged by address (duplicate destinations combine amounts).
3. LTV Disco distributions accumulate per-asset in `LTV_DISCO_DISTRIBUTION`.

### Handle Failed Distributions

- Failed promises are stored in `FAILED_DISTRIBUTIONS` keyed by address.
- Can be retried in subsequent distribution cycles.

## Cross-Contract Interactions

| Direction | Contract | Call | Purpose |
|-----------|----------|------|---------|
| Outgoing | CDP | `TakeRevenue` | Pull pending CDT revenue from basket |
| Outgoing | Staking | `DepositFee` | Send CDT revenue for staker distribution |
| Outgoing | LTV Disco | `AddRevenue` | Send per-asset revenue to Disco |
| Outgoing | Affiliates | `BankMsg::Send` | Direct CDT transfers to affiliate addresses |
| Outgoing | Points System | Affiliate points | Award points on successful distribution |
| Query | CDP | `Basket` | Get pending revenue per-asset breakdown |

## Important Invariants

1. `revenue_dispersal_window` enforces a minimum 7-day gap between distributions.
2. Total promised in `SetPromises` must be `<= sent CDT amount`.
3. LTV Disco promises are removed from the main promise list and handled via separate messages.
4. Reply handlers use a FIFO queue (`DISTRIBUTION_PROP`) -- order matters for matching replies to promises.
5. Failed distributions are preserved in state and not silently dropped.
6. `EPOCH_REVENUE_ACCUMULATION` is cleared after each distribution cycle.
