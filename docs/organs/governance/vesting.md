# Vesting

> **Organ:** Governance

## Purpose

The Vesting contract manages linear MBRN vesting for team members and contributors. It provides cliff-then-linear unlock schedules and integrates with staking for synthetic revenue participation at a reduced multiplier.

## Key Concepts

### Vesting Schedule

Each recipient has an allocation with a cliff period and a linear vesting period.

**Unlock formula:**

```
During cliff (time < cliff_end):
  unlocked = 0

After cliff, during linear period:
  time_past_cliff = current_time - cliff_end
  unlocked = (time_past_cliff / linear_seconds) * total_allocation - already_withdrawn

After cliff + linear period:
  unlocked = total_allocation (fully vested)
```

- Nothing unlocks during the cliff period.
- After the cliff, tokens unlock linearly over the configured linear duration.
- `already_withdrawn` is tracked to prevent double-claiming.

### Synthetic Staking Participation

The vesting contract does NOT directly stake MBRN. Instead:

1. Staking queries the vesting contract's `Recipients` endpoint to get total remaining allocations.
2. Staking creates a synthetic deposit using `vesting_rev_multiplier` (default 20%).
3. `effective_vesting = raw_vesting_total * 0.20`.
4. This effective amount is included in the fee distribution denominator alongside regular stakers.
5. Vesting recipients receive fee revenue proportional to their effective vesting amount.
6. **Vesting receives NO MBRN inflation** -- only CDT/asset revenue.

### Expedited Governance Access

Vesting recipients have a unique privilege: they can submit expedited governance proposals.

- When submitting a proposal, the vesting contract passes `recipient=sender` to the governance contract.
- Governance checks: if `recipient=None`, it forces `expedited=false`.
- Expedited proposals have a voting period of `normal_period * 6` and can extend to normal period if quorum is not met.

### Vested Transmutation (AddVestedTransmutation)

- Only callable by `neutron_proxy`.
- Groups transmutations by `week_id = current_time / 604800` (seconds per week).
- Tracks mint liability vs minted amount with an invariant check to prevent over-minting.

## State

The vesting contract tracks:

- **Recipients**: List of vesting recipients with their allocations, cliff/linear parameters, and withdrawal history.
- **Transmutation tracking**: Weekly buckets for vested transmutation accounting.

## User Flows

### Claim Vested Tokens

1. Recipient calls to claim unlocked tokens.
2. Contract calculates `unlocked` using the linear formula above.
3. Subtracts `already_withdrawn` to get claimable amount.
4. Transfers MBRN to the recipient and updates `already_withdrawn`.

### Receive Revenue

1. CDP/Auction sends CDT to staking via `DepositFee`.
2. Staking queries vesting total and applies the 20% multiplier.
3. Fee events are created with `amount_per_mbrn` based on `effective_vesting + stakers_total`.
4. Vesting's synthetic deposit accrues rewards at the reduced rate.
5. Revenue is claimable through the staking contract using the `VESTING_STAKE_TIME` cursor.

### Submit Expedited Proposal

1. Vesting recipient calls governance's `SubmitProposal` with `recipient=Some(sender)`.
2. Governance validates the recipient exists in the vesting contract.
3. Proposal is created with expedited timing (`period * 6`).

## Cross-Contract Interactions

| Direction | Contract | Call | Purpose |
|-----------|----------|------|---------|
| Queried by | Staking | `Recipients` | Get total vesting for fee split calculation |
| Queried by | Governance | `Recipients` | Validate expedited proposal eligibility, calculate vesting VP |
| Incoming | Neutron Proxy | `AddVestedTransmutation` | Weekly transmutation tracking |

## Important Invariants

1. Nothing unlocks before the cliff ends -- the formula produces zero during the cliff period.
2. The 20% revenue multiplier means vesting recipients earn 1/5th the revenue per MBRN compared to stakers.
3. Vesting VP is capped at 19% of `non_vested_total` in governance calculations (enforced in the governance contract, not here).
4. MBRN inflation from the staking incentive schedule is never distributed to vesting -- only fee revenue.
5. `AddVestedTransmutation` is restricted to `neutron_proxy` and groups by 7-day (604,800 second) windows.
6. The `already_withdrawn` field prevents any double-claiming of vested tokens.
