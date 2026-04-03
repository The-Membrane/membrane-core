# Auction

> **Organ:** Revenue

## Purpose

The Auction contract runs three types of auctions: fee auctions (swap non-CDT assets at discount), debt auctions (sell MBRN to cover bad debt), and MBRN sales (pull-model for LTV Disco). All auctions use a time-based discount curve.

## Key Concepts

### Discount Curve

All auctions share the same discount curve formula:

```
delay = mbrn_auction ? 0 : delay_window_minutes * 60
elapsed = max(0, block_time - start_time - delay)
discount_mult = elapsed / timeframe
current_discount = min(initial + discount_mult * increase, 1.0)
ratio = max(1.0 - current_discount, 0.01)
```

- **MBRN auctions**: No delay (`delay = 0`).
- **Fee auctions**: Configurable delay before discount begins.
- **Floor**: Ratio never drops below 0.01 (99% max discount).

**Enforced parameter ranges:**

| Parameter | Min | Max |
|-----------|-----|-----|
| `initial` | 1% | 10% |
| `increase` | 1% | 5% |
| `timeframe` | 10 seconds | 300 seconds |

### Fee Auctions (SwapForFee)

Users swap one asset to receive another asset held by the auction at a discount.

1. User sends `desired_asset` denomination and provides payment.
2. Oracle prices both the payment asset and the desired asset.
3. User receives desired tokens at the current discount ratio.

**Routing of proceeds based on desired asset:**

| Desired Asset | Proceeds Destination | Call |
|---------------|---------------------|------|
| MBRN | LTV Disco | `AddDepositTokenRevenue` |
| CDT | Revenue Distributor | `SetPromises` |
| Other (if `send_to_stakers`) | Staking | `DepositFee` |
| Other (default) | Governance | Direct send |

### Debt Auctions (StartMBRNSale / BuySuppliedMBRN)

When bad debt exists in the system, MBRN is sold to cover it.

**StartMBRNSale:**

- Only callable by `ltv_disco`.
- No funds required (pull model).
- Creates `AssetBadDebtAllocation` entries per asset with bad debt.

**BuySuppliedMBRN:**

1. User sends CDT to buy MBRN at the current discount.
2. CDT is split evenly across allocations.
3. Each allocation is capped (cannot overpay).
4. MBRN is pulled from Disco via `SendMBRNForSale`.
5. CDT proceeds go to `Debt.FulfillBadDebt`.
6. Exhausted allocations trigger `ClearBadDebtFreeze` on the affected contract.
7. **Recursion max depth**: 3 (for splitting logic).

### MBRN Sale State

The `MBRN_SALE` state tracks an active MBRN sale with per-asset bad debt allocations and the discount curve parameters.

## State

| Key | Type | Description |
|-----|------|-------------|
| `CONFIG` | `Item<Config>` | Contract configuration |
| `DEBT_AUCTION` | `Item<DebtAuction>` | Active debt auction state |
| `FEE_AUCTIONS` | `Map<String, FeeAuction>` | Per-asset fee auction state |
| `MBRN_SALE` | `Item<MBRNSale>` | Active MBRN sale with per-asset allocations |

## User Flows

### Participate in a Fee Auction

1. User sees non-CDT assets available in fee auctions (forwarded from staking).
2. User calls `SwapForFee` specifying which asset they want and sending payment.
3. Oracle prices both assets.
4. User receives desired asset at current discount (discount increases over time).
5. Payment is routed based on the desired asset type (see routing table above).

### Cover Bad Debt via MBRN Purchase

1. LTV Disco detects bad debt and calls `StartMBRNSale`.
2. Auction creates per-asset bad debt allocations.
3. Users call `BuySuppliedMBRN` sending CDT.
4. CDT is split across allocations and used to `FulfillBadDebt`.
5. Users receive MBRN (pulled from Disco) at the current discount.
6. When an allocation is fully covered, `ClearBadDebtFreeze` is called.

## Cross-Contract Interactions

| Direction | Contract | Call | Purpose |
|-----------|----------|------|---------|
| Incoming | Staking | Forward non-CDT | Assets for fee auctions |
| Incoming | LTV Disco | `StartMBRNSale` | Initiate bad debt auction |
| Outgoing | LTV Disco | `SendMBRNForSale` | Pull MBRN for debt auction buyers |
| Outgoing | LTV Disco | `AddDepositTokenRevenue` | Route MBRN swap proceeds |
| Outgoing | Debt | `FulfillBadDebt` | Cover bad debt with CDT |
| Outgoing | Revenue Distributor | `SetPromises` | Route CDT swap proceeds |
| Outgoing | Staking | `DepositFee` | Route other swap proceeds to stakers |
| Outgoing | Affected contract | `ClearBadDebtFreeze` | Unfreeze after debt covered |
| Query | Oracle | Price queries | Price both sides of swaps |

## Important Invariants

1. Discount ratio never drops below 0.01 (1% of value) -- prevents zero-price sales.
2. MBRN auctions have zero delay; fee auctions have configurable delay before discounting begins.
3. `StartMBRNSale` is restricted to `ltv_disco` -- no other contract can initiate MBRN sales.
4. `BuySuppliedMBRN` has a max recursion depth of 3 to prevent stack overflow during allocation splitting.
5. Fee auction parameter ranges are enforced (initial 1-10%, increase 1-5%, timeframe 10-300s).
6. Exhausted bad debt allocations trigger `ClearBadDebtFreeze` -- this is critical for unblocking affected protocol functions.
