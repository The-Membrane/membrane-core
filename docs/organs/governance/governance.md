# Governance

> **Organ:** Governance

## Purpose

The Governance contract manages on-chain proposals for protocol parameter changes and contract upgrades. It implements quadratic voting, alignment mechanics, and expedited proposals for vesting recipients.

## Key Concepts

### Proposal Submission

**Requirements:**

- `non_vested_total >= minimum_total_stake` (default 1,000,000 MBRN = `1_000_000_000_000` micro-units).
- Quadratic voting is NOT applied for submission threshold checks.
- Proposals with executable messages have a minimum voting period of 7 days.

**Expedited proposals:**

- Only vesting recipients can submit expedited proposals. The submitter passes `recipient=sender` to governance.
- Governance forces `expedited=false` if `recipient=None`.
- Expedited end time: `now + voting_period * 6` (in seconds).
- If an expedited proposal fails to reach quorum, it extends to the normal voting period instead of being rejected.

**Alignment check at submission:**

- If the submitter's voting power `VP >= required_stake`: `aligned = required_stake + sqrt(VP - required_stake)`.
- If `VP < required_stake`: the proposal goes to `PENDING_PROPOSALS` with a 1-day end time. It must gather enough alignment votes to proceed.

### Voting Power Calculation

`calc_voting_power` queries staking deposits:

1. Load all deposits with `stake_time < proposal.start_time` and `unstake_start_time = None`.
2. Sum deposit amounts.
3. **Vesting voting power**: `VP = (allocation - withdrawn) * multiplier`, **capped at 19% of `non_vested_total`** (pre-quadratic).
4. **Quadratic scaling**: `sqrt(total).ceil()`.
5. **Delegations**: Each delegation is sqrt'd individually, then summed.

### Casting Votes

- Cannot vote on own proposal.
- VP calculated at `proposal.start_time` with quadratic scaling (sqrt).
- Removes any previous vote before applying new one.
- **Alignment vote**: VP is **squared** before adding to `aligned_power` (not sqrt'd). Once the threshold is crossed, only the excess beyond the threshold is sqrt'd.

### Ending Proposals

Anyone can call `EndProposal` after the voting period ends.

**Quorum calculation** (with quadratic voting enabled):

- `aligned_power` is adjusted: subtract `required_stake`, sqrt the remainder, add back.
- `quorum = (total_votes + aligned_power) / total_voting_power`.
- Aligned power IS included in the quorum calculation.

**Threshold checks:**

```
for_ratio       = for_votes / total_votes
amend_ratio     = (for_votes + amend_votes) / total_votes
removal_ratio   = removal_votes / total_votes
```

- Proposals **without executables**: threshold drops to 50%.
- Proposals **with executables**: uses the configured threshold (default higher).

**Results:**

| Condition | Result |
|-----------|--------|
| Quorum met + `for >= threshold` | `Passed` |
| Quorum met + `amend >= threshold` | `AmendmentDesired` |
| Quorum met + `removal >= threshold` | `Rejected` |
| Quorum NOT met + expedited | Extends to normal period |
| Quorum NOT met + not expedited | `Rejected` |

### Message Validation (CheckMessages)

Dry-run with intentional revert to validate executable messages before proposal execution.

`msg_switch` values for testing:

| Value | Target |
|-------|--------|
| 0 | CDP contract test |
| 1 | Staking contract test |
| 2 | Liquidation Queue test |

### Freeze Positions

Hardcoded founder-only function. Only the address `osmo1988s5h45qwkaqch8km4ceagw2e08vdw28mwk4n` can call `FreezePositions`.

## State

| Key | Type | Description |
|-----|------|-------------|
| `CONFIG` | `Item<Config>` | Contract configuration |
| `PROPOSAL_COUNT` | `Item<u64>` | Auto-incrementing proposal ID |
| `PROPOSALS` | `Map<String, Proposal>` | All proposals by ID |
| `PENDING_PROPOSALS` | `Item<Vec<Proposal>>` | Proposals below alignment threshold awaiting support |

## Default Configuration

| Parameter | Default |
|-----------|---------|
| `minimum_total_stake` | 1,000,000 MBRN (1_000_000_000_000 micro) |
| `quadratic_voting` | `true` |

## User Flows

### Submit a Proposal

1. User calls `SubmitProposal` with title, description, optional messages, and optional `recipient` (for expedited).
2. Contract queries staking for `non_vested_total` and validates it meets `minimum_total_stake`.
3. If the user's VP meets the alignment threshold, the proposal is created with a voting period.
4. If VP is below threshold, it goes to `PENDING_PROPOSALS` with a 1-day alignment window.
5. Proposals with executables get a minimum 7-day voting period.

### Vote on a Proposal

1. User calls `CastVote` with proposal ID and vote option (For, Against, Amend, Remove, Align).
2. Contract calculates VP at `proposal.start_time` using quadratic scaling.
3. Previous vote (if any) is removed and the new vote is applied.
4. Alignment votes use squared VP to accelerate threshold crossing.

### End a Proposal

1. Anyone calls `EndProposal` after voting period expires.
2. Contract tallies votes, checks quorum and thresholds.
3. If passed with executables, messages are executed on-chain.
4. If expedited and no quorum, extends to normal period.

## Cross-Contract Interactions

| Direction | Contract | Call | Purpose |
|-----------|----------|------|---------|
| Query | Staking | `UserStake` | Get deposits for voting power calculation |
| Query | Staking | `Totals` | Get total staked for quorum denominator |
| Query | Vesting | `Recipients` | Get vesting allocations for VP calculation |
| Execute | Any | Proposal messages | Execute passed proposals |

## Important Invariants

1. Quadratic voting applies sqrt to individual deposit sums, not to each deposit separately (except delegations which are sqrt'd individually).
2. Vesting VP is hard-capped at 19% of non-vested total before quadratic scaling.
3. Alignment votes use squared VP (reverse of quadratic) to make alignment more impactful.
4. Proposals with executables require longer voting periods (minimum 7 days) and higher thresholds.
5. Expedited proposals are exclusive to vesting recipients -- there is no other path to expedited submission.
6. The `FreezePositions` function is hardcoded to a single founder address and cannot be changed by governance.
