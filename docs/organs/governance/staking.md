# Staking

> **Organ:** Governance

## Purpose

The Staking contract holds MBRN deposits, distributes protocol revenue as fee events, manages delegation relationships, and runs the buyback-and-burn mechanism. It is the central hub for MBRN-based economic activity.

## Key Concepts

### Stake Deposits

Each user has a `Vec<StakeDeposit>` stored in the `STAKED` map. A deposit contains:

- `amount`: Uint128
- `stake_time`: block timestamp at deposit
- `unstake_start_time`: `None` until unstaking begins
- `last_accrued`: timestamp of last reward claim
- `locked`: optional lock info with `locked_until`, `intended_lock_days`, and optional `perpetual_lock`

**Minimum stake**: 1 MBRN (1,000,000 micro-units).

### Locking

- Lock days validated: `lock_days <= lock_duration_ceiling` (365 days).
- `intended_lock_days` is set at stake time.
- Perpetual locks auto-refresh via `refresh_deposit_lock`: extends `locked_until` to `current_time + perpetual_days`, capped at `lock_duration_ceiling`.

### Staking Totals

```rust
STAKING_TOTALS: Item<Totals>
// Totals { stakers: Uint128, vesting_contract: Uint128 }
```

Incremented on stake, decremented on unstake. `vesting_contract` is updated from vesting queries during fee distribution.

### Fee Distribution (DepositFee)

Only callable by `positions_contract` or `auction_contract`.

1. Non-CDT assets are forwarded to the auction contract.
2. For CDT:
   - Query vesting total from vesting contract's `Recipients` endpoint.
   - `effective_vesting = raw_vesting * vesting_rev_multiplier` (default 20%).
   - `total = effective_vesting + stakers_total`.
   - For each asset: `amount_per_mbrn = asset_amount / total`.
   - Push a new `FeeEvent { time_of_event, fee: LiqAsset { amount_per_mbrn } }`.

### Reward Claiming

`get_deposit_claimables` calculates rewards per deposit:

1. Filter `FEE_EVENTS` where `event.time > deposit.last_accrued` (or `stake_time` if never accrued) and `event.time <= now`.
2. **Commission subtraction**:
   - `total_delegated_ratio = min(total_delegated_to / total_rewarding_stake, 1.0)`
   - `per_deposit_commission = total_delegated_ratio * weighted_commission_rate`
   - `effective_deposit = deposit_amount * (1 - commission)`
3. Claimable per event: `event.fee.amount * effective_deposit`.
4. Commission portion accumulates in `DELEGATE_CLAIMS` for the delegate to claim.

### MBRN Inflation

```
accumulate_interest = stake * rate * elapsed / SECONDS_PER_YEAR
```

- `SECONDS_PER_YEAR = 31,536,000`
- Rate drops to 0 when `now - schedule_start > duration * SECONDS_PER_DAY`.
- Default: 9% rate, 240-day duration.
- **Vesting gets NO MBRN inflation** -- only stakers receive inflation rewards.

### Unstaking

Checks before unstaking:

1. **Active emissions votes**: Queries `emissions_voting` contract. Blocks if user has active votes.
2. **Active governance proposals**: Blocks if user has active proposals or voted "For" on proposals with executables.
3. **Refresh locked deposits**: All perpetual locks refreshed first.

Withdrawal process (`withdraw_from_state`):

1. **Completed unstaking**: Finds deposits where `unstake_start_time` is set and `elapsed >= unstaking_period * SECONDS_PER_DAY` (default 4 days). These are fully withdrawable.
2. **Active staked deposits**: Processes remaining deposits.
3. **Early withdrawal from locked deposits**:
   - `early_withdrawal_ratio = fulfilled_days / intended_days` (capped at 1.0)
   - `effective_withdrawal = amount * ratio`
   - **Lost portion** (`amount - effective`) is added to the contract's own stake deposits (protocol keeps it).
4. Claims all pending rewards before saving state.

### Buyback and Burn

Permissionless execution. Default state: enabled (`true`).

1. Saves current MBRN balance to `LAST_MBRN_TOTAL_BALANCE`.
2. Swaps all CDT held by the contract for MBRN via `OsmosisProxy` with 90% slippage tolerance.
3. Reply handler (`BURN_REPLY_ID = 1`): `burn_amount = new_balance - pre_balance`.
4. Burns the purchased MBRN via `OsmosisProxy`.

### Delegation

Stored in `DELEGATIONS: Map<Addr, DelegationInfo>` tracking `delegated` (sent) and `delegated_to` (received) amounts, plus `commission` rate.

- **Fluid**: Delegate can re-delegate to another address via `DelegateFluidDelegations`.
- **Non-fluid**: Cannot be re-delegated.
- **Voting power delegation** (default `true`): Conveys governance voting power.
- **Commission**: Max 10% (`max_commission_rate`). Earned on delegator rewards.
- **Minimum delegation**: 1 MBRN.
- Delegate claims stored in `DELEGATE_CLAIMS: Map<Addr, (Vec<Coin>, Uint128)>` -- coins plus MBRN inflation.

## State

| Key | Type | Description |
|-----|------|-------------|
| `CONFIG` | `Item<Config>` | Contract configuration |
| `STAKING_TOTALS` | `Item<Totals>` | `{ stakers, vesting_contract }` Uint128 each |
| `STAKED` | `Map<Addr, Vec<StakeDeposit>>` | Per-user deposit stack |
| `DELEGATIONS` | `Map<Addr, DelegationInfo>` | Per-user delegation info (delegated/delegated_to/commission) |
| `DELEGATE_CLAIMS` | `Map<Addr, (Vec<Coin>, Uint128)>` | Pending delegate commission claims |
| `FEE_EVENTS` | `Item<Vec<FeeEvent>>` | Fee events with `time_of_event` + `fee` as LiqAsset `amount_per_mbrn` |
| `INCENTIVE_SCHEDULING` | `Item<StakeDistributionLog>` | Inflation schedule (rate, duration, start_time) |
| `DELEGATE_INFO` | `Item<Vec<Delegate>>` | Declared delegate info |
| `BUYBACK_AND_BURN` | `Item<bool>` | Toggle for buyback-and-burn (default `true`) |
| `LAST_MBRN_TOTAL_BALANCE` | `Item<Uint128>` | Pre-swap MBRN balance for burn calculation |
| `VESTING_STAKE_TIME` | `Item<u64>` | Timestamp for vesting contract claims |
| `VESTING_REV_MULTIPLIER` | `Item<Decimal>` | Revenue multiplier for vesting (default 20%) |

## Constants

| Constant | Value | Description |
|----------|-------|-------------|
| `SECONDS_PER_YEAR` | 31,536,000 | Used for inflation calculation |
| `SECONDS_PER_DAY` | 86,400 | Used for unstaking period |
| `BURN_REPLY_ID` | 1 | Reply ID for burn after swap |

## Default Configuration

| Parameter | Default |
|-----------|---------|
| `incentive_schedule.rate` | 9% |
| `incentive_schedule.duration` | 240 days |
| `lock_duration_ceiling` | 365 days |
| `unstaking_period` | 4 days |
| `max_commission_rate` | 10% |
| `vesting_rev_multiplier` | 20% |
| `keep_raw_cdt` | true |
| `BUYBACK_AND_BURN` | true |

## Cross-Contract Interactions

| Direction | Contract | Call | Purpose |
|-----------|----------|------|---------|
| Incoming | CDP (positions_contract) | `DepositFee` | Sends CDT revenue |
| Incoming | Auction | `DepositFee` | Sends fee auction proceeds |
| Outgoing | Auction | `ExecuteMsg` | Forwards non-CDT assets |
| Outgoing | OsmosisProxy | Swap + Burn | Buyback-and-burn flow |
| Query | Vesting | `Recipients` | Get total vesting for fee split |
| Query | Emissions Voting | `HasAnyVotes` | Block unstaking if active votes |
| Query | Governance | `ProposalList` | Block unstaking if active proposals |

## Important Invariants

1. Only `positions_contract` or `auction_contract` can call `DepositFee`.
2. Vesting receives fee revenue at 20% multiplier but zero MBRN inflation.
3. Early withdrawal penalty is redistributed to the protocol's own stake -- it is never burned or lost.
4. Users cannot unstake while they have active emissions votes or governance proposals with executables.
5. `FEE_EVENTS` grow monotonically in time; each deposit tracks its own `last_accrued` cursor.
6. Delegation commission is subtracted from the delegator's rewards, not added on top.
