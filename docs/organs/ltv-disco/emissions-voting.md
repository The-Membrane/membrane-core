# Emissions Voting

> **Organ:** LTV Disco

## Purpose & Role

Emissions Voting enables MBRN holders (via LTV Disco deposits and Staking) to vote on protocol parameters through configurable weighted graphs. Results are sent to a callback contract for parameter updates. This creates decentralized, continuous governance over emission rates, multipliers, and other protocol variables.

## Key Concepts

### Voting Graphs

Each votable parameter is a **graph** with a type, range, and label:

| Graph Type | Value Type | Example Use |
|-----------|-----------|-------------|
| `Uint128` | Whole numbers | MBRN emission amounts, caps |
| `Decimal` | Decimal numbers | Multipliers, ratios, percentages |

Each graph has:
- `label`: Identifies the parameter (e.g., "retention_multiplier")
- `min_value` / `max_value`: Valid range for votes
- `period_duration`: How long each voting period lasts
- `callback_contract`: Where results are sent when period ends

### Voting Power

Voting power is derived from:
1. **LTV Disco deposits**: User's vault tokens across all slots and assets
2. **Staked MBRN**: User's staked balance (weighted by lock duration)

Power is queried from both contracts and summed for the voter.

### Voting Periods

Each graph has independent voting periods:
- Period starts when the graph is created
- Anyone can call `EndVoting` when the period elapses
- Weighted average of all votes becomes the result
- Result sent to `callback_contract` via `ReceiveVotingResult`
- New period starts immediately

### Persistent Votes

Votes carry across periods. Once a user votes, their vote persists until they change it or remove it:
- Active participants do not need to re-vote every period
- Passive holders maintain their influence
- New votes can override at any time

### Unstaking Restriction

Deposits with active emissions votes **cannot** be unstaked. Users must `RemoveVote` before requesting unstake from LTV Disco.

## User Flows

### Flow 1: Voting on a Parameter

1. User has MBRN in Disco and/or Staking
2. Calls `Vote { graph_id, value }` with their preferred value within range
3. Vote recorded with current voting power
4. Vote persists across future periods until changed

### Flow 2: Period End & Result

1. Period elapses for a graph
2. Anyone calls `EndVoting { graph_id }`
3. Weighted average computed across all votes
4. Result sent to callback contract via `ReceiveVotingResult`
5. New period starts

## Technical Details

### Messages

#### ExecuteMsg

| Message | Auth | Description |
|---------|------|-------------|
| `CreateGraph` | Owner | Create new votable parameter graph |
| `Vote` | Anyone with voting power | Cast vote on a graph |
| `RemoveVote` | Voter | Remove vote from a graph |
| `EndVoting` | Permissionless | End period and send result |
| `UpdateConfig` | Owner | Update contract configuration |

#### QueryMsg

| Query | Returns | Description |
|-------|---------|-------------|
| `Config` | `Config` | Contract configuration |
| `Graphs` | Graph list | All voting graphs with current state |
| `UserVotes` | Vote list | User's active votes |
| `VotingPower` | Power amount | User's current voting power |
| `PeriodResults` | Historical results | Past period outcomes |

## Cross-Contract Interactions

### Queries (Outgoing)

| Contract | Query | Purpose |
|----------|-------|---------|
| **LTV Disco** | User deposits | Voting power from Disco VTs |
| **Staking** | User stake | Voting power from staked MBRN |

### Calls (Outgoing)

| Contract | Message | Purpose |
|----------|---------|---------|
| **Callback Contract** | `ReceiveVotingResult` | Send period result for parameter update |

### Called By (Incoming)

| Contract | Message | Purpose |
|----------|---------|---------|
| **Users** | `Vote`, `RemoveVote` | Cast or remove votes |
| **Anyone** | `EndVoting` | Trigger period end (permissionless) |

## Important Invariants & Edge Cases

- **Voting power is live**: Power is queried at vote time and at period end. If a user unstakes between voting and period end, their power may change
- **Persistent votes simplify UX**: Users do not need to actively participate every period
- **Range enforcement**: Votes outside `min_value` / `max_value` are rejected
- **Permissionless period end**: Anyone can trigger `EndVoting`, ensuring periods resolve even if no single party is responsible
- **Blocks unstaking**: Deposits with active votes in emissions voting cannot be unstaked from LTV Disco until votes are removed
