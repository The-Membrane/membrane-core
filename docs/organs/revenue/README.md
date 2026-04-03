# Revenue Organ

The Revenue Organ collects protocol revenue from all sources and distributes it to stakeholders -- MBRN stakers, LTV Disco depositors, affiliates, and other configured destinations. It handles both normal fee distribution and bad-debt auctions.

## Contracts

| Contract | Purpose |
|----------|---------|
| [Revenue Distributor](revenue-distributor.md) | Collects CDT from CDP basket, fulfills promises, distributes to staking/LTV Disco/affiliates |
| [Auction](auction.md) | Fee auctions (swap non-CDT assets at discount), debt auctions (sell MBRN to cover bad debt), MBRN sale for Disco |

## Architecture

```
┌─────────┐    TakeRevenue    ┌───────────────────┐    DepositFee    ┌──────────┐
│  CDP    │──────────────────▶│  Revenue          │────────────────▶│  Staking │
│ Basket  │                   │  Distributor      │                 └──────────┘
└─────────┘                   │                   │    BankMsg::Send
                              │  (Promises +      │────────────────▶ Affiliates
                              │   Distribution)   │
                              │                   │    AddRevenue
                              │                   │────────────────▶ LTV Disco
                              └───────────────────┘

┌──────────┐   Non-CDT fees   ┌───────────────────┐
│  Staking │─────────────────▶│  Auction          │
│          │                  │  (Fee + Debt +     │
└──────────┘                  │   MBRN Sale)      │
                              └───────────────────┘
```

### Revenue Flow

1. **Collection**: Revenue Distributor calls `TakeRevenueFromBasket` to pull CDT from the CDP contract's pending revenue.
2. **Promises**: Users or contracts call `SetPromises` to allocate revenue to specific destinations with per-asset breakdowns.
3. **Distribution**: `DistributePromises` sends CDT to staking (via `DepositFee`), affiliates (via `BankMsg::Send`), and LTV Disco.
4. **Remainder**: Any CDT left after promises is split by `revenue_destinations` ratios.
5. **Non-CDT**: Non-CDT assets arriving at staking are forwarded to the auction contract for fee auctions.

### Auction Types

| Type | Trigger | Mechanism |
|------|---------|-----------|
| **Fee Auction** | Non-CDT assets arrive at staking | Users swap desired assets for fee tokens at a time-based discount |
| **Debt Auction** | Bad debt in the system | MBRN is sold (pulled from Disco) to buyers who pay CDT to cover bad debt |
| **MBRN Sale** | LTV Disco initiates | Pull-model sale of MBRN with CDT proceeds going to debt fulfillment |

## Key Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| `revenue_dispersal_window` | 7 days | Minimum time between distributions |
| Auction `delay_window_minutes` | Varies | Delay before discount starts (0 for MBRN auctions) |
| Auction discount `initial` | 1-10% | Starting discount rate |
| Auction discount `increase` | 1-5% | Maximum additional discount |
| Auction `timeframe` | 10-300 seconds | Time for discount to reach maximum |
