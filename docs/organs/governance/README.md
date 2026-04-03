# Governance Organ

The Governance Organ manages protocol ownership through MBRN token staking, on-chain proposal voting, and team vesting. It consists of three contracts that work together to provide decentralized control over all protocol parameters.

## Contracts

| Contract | Purpose |
|----------|---------|
| [Staking](staking.md) | MBRN staking, delegation, fee distribution, buyback-and-burn |
| [Governance](governance.md) | On-chain proposals with quadratic voting, expedited proposals for vesting recipients |
| [Vesting](vesting.md) | Linear vesting with cliff, synthetic staking participation at reduced multiplier |

## Architecture

```
                    ┌──────────────┐
                    │  Governance  │
                    │  (Proposals) │
                    └──────┬───────┘
                           │ queries voting power
                           ▼
┌──────────┐        ┌──────────────┐        ┌──────────┐
│  Vesting │───────▶│   Staking    │◀───────│  CDP /   │
│ (Team)   │ total  │ (MBRN+Fees)  │DepositFee│ Auction │
└──────────┘ query  └──────────────┘        └──────────┘
```

### Key Relationships

- **Staking** tracks vesting totals via query to the vesting contract's `Recipients` endpoint and creates synthetic deposits at a 20% revenue multiplier (`vesting_rev_multiplier`).
- **Governance** queries staking deposits at proposal start time to calculate voting power, applying quadratic scaling (`sqrt`).
- **Vesting** recipients can submit expedited proposals through governance by passing their recipient address. Governance forces `expedited=false` if `recipient=None`.
- **Fee distribution** flows from CDP/Auction into staking as `FeeEvent` entries, distributed proportionally per MBRN staked.

## Token: MBRN

MBRN is the governance and staking token. It accrues protocol revenue through staking and controls all protocol parameters through governance proposals.

- **Staking inflation**: 9% annual rate over 240 days (from schedule start), distributed only to stakers (not vesting).
- **Buyback-and-burn**: Enabled by default. Swaps accumulated CDT for MBRN via OsmosisProxy (90% slippage tolerance) and burns the purchased MBRN.
- **Minimum stake**: 1 MBRN (1,000,000 micro-units).
- **Lock ceiling**: 365 days maximum.
- **Unstaking period**: 4 days.
- **Max commission rate**: 10%.

## Revenue Flow

1. CDP interest and liquidation fees accumulate as pending revenue in the basket.
2. Revenue Distributor calls `TakeRevenueFromBasket` to collect CDT from the CDP contract.
3. Distributor sends CDT to Staking via `DepositFee`.
4. Staking queries vesting total and computes `effective_vesting = raw_vesting * vesting_rev_multiplier` (20%).
5. `total = effective_vesting + stakers_total`.
6. For each asset in the fee: `amount_per_mbrn = asset_amount / total`.
7. A new `FeeEvent` is pushed with `time_of_event` and the per-MBRN rate.
8. Each staker's claimable amount accumulates based on their deposit size, filtered by events where `time > deposit.last_accrued`.

## Governance Flow

1. A user with sufficient stake submits a proposal (requires `non_vested_total >= 1,000,000 MBRN`).
2. Voters cast votes with quadratic-scaled voting power (`sqrt` of their stake).
3. Alignment voting: if voting power exceeds `required_stake`, excess is sqrt'd; below threshold goes to pending with 1-day window.
4. After the voting period, anyone can call `EndProposal` to tally results.
5. Passed proposals with executable messages are executed on-chain.

## Delegation System

Stakers can delegate their MBRN to representatives. Delegations can be:

- **Fluid**: Delegate can re-delegate the MBRN to another address.
- **Non-fluid**: Cannot be re-delegated.
- **Voting power delegation** (default `true`): Conveys governance voting power to the delegate.
- **Commission**: Delegates earn a commission on delegator rewards (max 10%). Calculated as `total_delegated_ratio * weighted_commission_rate` per deposit.
- **Minimum delegation**: 1 MBRN.
