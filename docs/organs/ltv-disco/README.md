# LTV Disco Organ

The LTV Disco (Liquidity-Tiered Value Disco) organ is Membrane's **risk-weighted revenue distribution system**. MBRN holders deposit into LTV-designated slots per collateral asset, earning CDT revenue proportional to the risk they absorb. Higher LTV slots earn more revenue but are first in line for bad debt slashing. This organ makes MBRN the "take all risk, get all reward" asset.

## Contracts in this Organ

| Contract | Purpose |
|----------|---------|
| [LTV Disco](ltv-disco.md) | Risk-weighted deposit slots, revenue distribution, bad debt slashing |
| [Emissions Voting](emissions-voting.md) | Weighted voting from Disco + Staking deposits for protocol parameters |

## Architecture

```
                    Revenue Sources
                    (CDP Interest, Transmuter Fees)
                          |
                    +-----v-----+
                    |  Revenue   |
                    | Distributor|
                    +-----+-----+
                          |
              AddRevenue (CDT)    AddDepositTokenRevenue (deposit tokens)
                          |
                    +-----v------------------------------------------+
                    |              LTV DISCO                          |
                    |                                                 |
                    |  Asset: ATOM                                    |
                    |  +----+----+----+----+----+                    |
                    |  |50% |60% |70% |80% |90% |  <-- LTV Slots    |
                    |  |    |    |    |    |    |                    |
                    |  +----+----+----+----+----+                    |
                    |                                                 |
                    |  Revenue weights: isqrt-based, riskiest = most  |
                    |  Bad debt slashing: 90% -> 80% -> ... -> 50%   |
                    |  Unstaking: 2-day cooldown, keeps earning       |
                    +------------------+-----------------------------+
                                       |
                                +------v------+
                                |  Emissions  |
                                |   Voting    |
                                +-------------+
```

## Key User Journey

### Depositing MBRN to Earn Revenue

1. User deposits MBRN into an asset queue (e.g., ATOM) at a chosen LTV slot (e.g., 80%)
2. Receives vault tokens (VT) tracking their share of the slot
3. Revenue distributed to slot proportional to inverse-sqrt risk weight
4. Optional: set a manager who can move deposits between slots (cannot withdraw)
5. Claim CDT revenue or compound into other assets

### Risk Exposure

- If bad debt occurs, deposits are slashed starting from the **highest LTV slot (riskiest)**, iterating downward
- Revenue rewards follow the same ordering: riskier slots earn proportionally more
- Withdrawals blocked when `PENDING_BAD_DEBT` is non-zero for the asset

## Cross-Organ Dependencies

| Dependency | Direction | Purpose |
|-----------|-----------|---------|
| **CDP (Collateral)** | Queries Disco | Dynamic LTV targets via `QueueAverageLTV` |
| **Revenue Distributor** | Inbound | CDT revenue via `AddRevenue`, deposit token revenue via `AddDepositTokenRevenue` |
| **Auction** | Bidirectional | `SendMBRNForSale` pulls MBRN for bad debt sale, `ClearBadDebtFreeze` unfreezes |
| **Emissions Voting** | Outbound | Voting power from deposits |
| **Acquisition** | Inbound | MBRN rewards deposited via `SubmitDeposit` with contract as owner, user as manager + revenue_destination |

## Critical Mechanics

### Revenue Weighting Formula (Actual Implementation)

The `calculate_slot_weights` function uses an inverse square root formula:

1. Filter to active slots only
2. `total_deposits` = sum of all active slot deposits
3. `avg_slot_size` = `total_deposits / non_empty_active_count` (minimum 1)
4. Iterate slots descending by LTV (riskiest first)
5. Track `deposits_above` (cumulative deposits from riskier slots already processed)
6. Skip empty slots (zero VT)
7. For each non-empty slot: `weight = 10^18 / isqrt(deposits_above + avg_slot_size)`
8. Sum all raw weights
9. Normalize: `final_weight = raw_weight / total_weight_sum`

**Effect**: The riskiest slots (`deposits_above = 0`) get the highest weight. As you move to safer slots, `deposits_above` grows and the weight decreases via the inverse square root.

### Bad Debt - Three Phase Process

**Phase 1** (`AddBadDebt` from CDP):
- Sets `PENDING_BAD_DEBT[asset] += amount`
- This freezes withdrawals for the asset
- Starts MBRN auction at the auction contract (no MBRN transferred yet at this stage)

**Phase 2** (`SendMBRNForSale` from auction):
- Iterates slots **descending by LTV** (riskiest first)
- For each slot: `slash = min(remaining_bad_debt, slot.total_deposit_tokens)`
- Reduces `total_deposit_tokens` **WITHOUT reducing `total_vault_tokens`**
- Sends slashed MBRN to auction buyer
- This means VT rate drops for slashed slots (same VTs, fewer underlying tokens)

**Phase 3** (`ClearBadDebtFreeze` from auction):
- Reduces `PENDING_BAD_DEBT`
- When `PENDING_BAD_DEBT` reaches zero, the asset queue is unfrozen

### Unstaking Cooldown

- `RequestUnstake`: creates request with `unlock_time = now + unstaking_period` (default 2 days)
- Deposit **continues earning revenue** during the cooldown period
- Vault tokens are **NOT removed** during the request phase
- Blocks if `PENDING_BAD_DEBT` is non-zero for the asset or if deposit has emissions votes
- `CompleteUnstake`: after cooldown, calculates `base_tokens` from current VT ratio (may be reduced by bad debt slashing), sends tokens to user

### Two Types of Revenue

1. **`AddRevenue` (CDT)**: Distributed per slot weights. Creates `RevenueEvent` per slot with `amount_per_vt`. Users claim by iterating events since their last claim index
2. **`AddDepositTokenRevenue` (from auction only)**: Directly adds to `slot.total_deposit_tokens` proportionally by deposit size. No events created, no claiming needed. Silently increases the VT rate for all depositors in that slot
