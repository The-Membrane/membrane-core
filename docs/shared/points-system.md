# Points System

> **Used by:** CDP (repayment points), Transmuter (swap fee points), LTV Disco (revenue claim points), Staking (claim points), Governance (voting points), Liquidation Queue (execution points)

## Purpose

The Points System contract awards points for protocol participation and allows users to claim MBRN rewards. It uses a proxy pattern to intercept transactions, parse reply events for revenue amounts, and award points proportionally.

## Key Concepts

### Proxy Pattern

The Points System wraps existing protocol actions with point-awarding logic. Users call the Points System instead of the target contract directly. The contract sends the actual transaction as a `SubMsg` and parses the reply to determine point awards.

| Proxy Function | Target Contract | Reply ID | Parsed Attribute |
|---------------|----------------|----------|-----------------|
| `RepayAndGivePoints` | CDP `Repay` | 3 (CDP_REPAY) | `"revenue"` |
| `ClaimDiscoRevenueAndGivePoints` | Disco `ClaimRevenueForUser` | 4 (DISCO_CLAIM) | `"revenue_claimed"` |
| `TransmuteAndGivePoints` | Transmuter `Transmute` | 5 (TRANSMUTER) | `"swap_fee"` + `"swap_fee_denom"` |

### Reply IDs

| ID | Constant | Purpose |
|----|----------|---------|
| 1 | `LIQUIDATION` | Liquidation execution points |
| 3 | `CDP_REPAY` | CDP repayment points |
| 4 | `DISCO_CLAIM` | Disco revenue claim points |
| 5 | `TRANSMUTER` | Transmuter swap fee points |
| 6 | `MGMT_POINTS` | Management points award |

### Point Calculation

Points are awarded based on dollar value of the action:

```
points = action_dollar_value * points_per_dollar * multiplier
```

Where `points_per_dollar = 1` and multipliers vary by action type.

### Claiming MBRN

`ClaimMBRN`:

```
mbrn_amount = claimable_points * mbrn_per_point  (ceiling)
```

- Claimed MBRN is **staked for the user** (sent to hardcoded staking address via `Stake` message).
- MBRN is not sent as liquid tokens -- it goes directly to staking.
- `max_mbrn_distribution` check is **COMMENTED OUT** in current code (no cap enforced).

### Governance Voting Points

- Points awarded for casting governance votes.
- **Anti-farming protection**: Proposals less than 1 hour old are skipped (no points for voting on brand-new proposals).
- `ClaimCheck` must be in the same block as the vote to count.

### Management Points

- `MANAGEMENT_POINTS_REWARD = 5` points per management action.
- Awarded via reply ID 6 (`MGMT_POINTS`).

## Default Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `mbrn_per_point` | 1 MBRN | MBRN awarded per point claimed |
| `max_mbrn_distribution` | 100,000 MBRN | Max total MBRN (currently not enforced) |
| `points_per_dollar` | 1 | Base points per dollar of action value |
| `MANAGEMENT_POINTS_REWARD` | 5 points | Points for management actions |

### Multipliers

| Action | Multiplier |
|--------|-----------|
| `interest_rate` | 1x |
| `liquidation_execution` | 1x |
| `liquidation_claims` | 1x |
| `governance_votes` | 3x |
| `transmuter_swap_fees` | 1x |
| `disco_revenue` | 1x |
| `range_bound_vault` | 100x |

## User Flows

### Earn Points via CDP Repayment

1. User calls `RepayAndGivePoints` instead of CDP `Repay` directly.
2. Points System sends `Repay` as SubMsg to CDP.
3. Reply (ID 3) parses the `"revenue"` attribute from the response events.
4. Points are calculated: `revenue_dollars * points_per_dollar * interest_rate_multiplier`.
5. Points are added to user's claimable balance.

### Earn Points via Disco Revenue Claim

1. User calls `ClaimDiscoRevenueAndGivePoints`.
2. Points System sends `ClaimRevenueForUser` to Disco as SubMsg.
3. Reply (ID 4) parses `"revenue_claimed"` from events.
4. Points awarded based on claimed revenue value.

### Earn Points via Transmuter

1. User calls `TransmuteAndGivePoints` instead of Transmuter `Transmute`.
2. Points System sends `Transmute` as SubMsg.
3. Reply (ID 5) parses `"swap_fee"` and `"swap_fee_denom"` from events.
4. Points awarded based on swap fee value.

### Claim MBRN Rewards

1. User calls `ClaimMBRN`.
2. Contract calculates `mbrn = claimable_points * mbrn_per_point` (ceiling rounding).
3. MBRN is staked for the user at the staking contract (not sent as liquid tokens).
4. User's claimable points are reset to zero.

## Cross-Contract Interactions

| Direction | Contract | Call | Purpose |
|-----------|----------|------|---------|
| Outgoing | CDP | `Repay` (SubMsg) | Proxy repayment for points |
| Outgoing | LTV Disco | `ClaimRevenueForUser` (SubMsg) | Proxy claim for points |
| Outgoing | Transmuter | `Transmute` (SubMsg) | Proxy transmute for points |
| Outgoing | Staking | `Stake` | Stake claimed MBRN for user |
| Incoming | Liquidation Engine | Point awards | Liquidation execution points |
| Query | Governance | Proposal age check | Anti-farming for voting points |

## Important Invariants

1. The proxy pattern means users must call the Points System contract, not the target contract, to earn points.
2. Reply parsing depends on specific attribute names (`"revenue"`, `"revenue_claimed"`, `"swap_fee"`) -- if target contracts change these attributes, points break.
3. Claimed MBRN is always staked, never sent as liquid tokens.
4. `max_mbrn_distribution` is defined but not enforced (check is commented out).
5. Governance vote anti-farming: proposals must be at least 1 hour old. `ClaimCheck` must occur in the same block as the vote.
6. `MANAGEMENT_POINTS_REWARD` is a flat 5 points regardless of action value.
7. `range_bound_vault` has a 100x multiplier -- significantly higher than all other actions.
