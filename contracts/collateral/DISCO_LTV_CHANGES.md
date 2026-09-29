# Disco Contract Changes Needed for Dynamic LTV Updates

## Overview

The Collateral contract's dynamic LTV updater queries the Disco contract for weighted average max LTV values. The Disco contract needs to implement the following query and response type.

---

## Required Query

Add to `packages/membrane/src/ltv_disco.rs` QueryMsg (already added):

```rust
/// Get weighted average max LTV for assets (queried by Collateral contract)
GetAverageLTVs {
    assets: Vec<String>,
},
```

## Required Response Type

Add to `packages/membrane/src/ltv_disco.rs` (already added):

```rust
#[cw_serde]
pub struct AverageLTVsResponse {
    /// Weighted average max_LTV from Disco deposits
    pub average_max_ltv: Decimal,
}
```

**Note:** There is no `average_borrow_ltv` field. The Collateral contract derives borrow LTV as `max_LTV - config.ltv_borrow_distance`.

---

## Implementation Requirements

The Disco contract query handler must:

1. Accept a list of asset denoms (the Collateral contract currently queries one at a time).
2. Compute a **deposit-weighted average** of `max_LTV` across all Disco slots/deposits for that asset.
3. Return `average_max_ltv = Decimal::zero()` if there are no deposits for the asset (this tells the Collateral contract to skip the update).

### Weighted Average Calculation

```
average_max_ltv = sum(deposit_i.amount * deposit_i.max_ltv) / sum(deposit_i.amount)
```

Where `deposit_i` iterates over all active Disco deposits for the requested asset.

### Example

If Disco has three deposits for `uatom`:
- Deposit A: 1000 uatom at max_LTV 80%
- Deposit B: 2000 uatom at max_LTV 70%
- Deposit C: 500 uatom at max_LTV 90%

```
average_max_ltv = (1000 * 0.80 + 2000 * 0.70 + 500 * 0.90) / (1000 + 2000 + 500)
                = (800 + 1400 + 450) / 3500
                = 2650 / 3500
                = 0.7571... (75.71%)
```

---

## Package Types Already Added

The message types and response struct have been added to `packages/membrane/src/ltv_disco.rs`:
- `QueryMsg::GetAverageLTVs { assets: Vec<String> }`
- `AverageLTVsResponse { average_max_ltv: Decimal }`

The Disco contract (`contracts/ltv_disco/`) needs to add the query handler implementation in its `query()` function.
