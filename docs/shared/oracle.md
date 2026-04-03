# Oracle

> **Used by:** CDP (Collateral, Debt), Transmuter, LTV Disco, Auction, System Discounts

## Purpose

The Oracle contract provides USD price feeds for all protocol assets using a three-tier fallback system: Pyth EMA prices, Osmosis geometric TWAP, and medianized pool-based USD par pricing.

## Key Concepts

### Three-Tier Pricing

Prices are resolved in priority order. If a higher tier succeeds, lower tiers are skipped.

#### Tier 1: Pyth USD Direct

- Queries `pyth_price_feed` for the asset.
- Uses **EMA price** (exponential moving average), NOT spot price.
- Call: `get_ema_price_no_older_than`.
- Returns immediately if successful.

#### Tier 2: Osmosis Geometric TWAP

- Uses `GeometricTwapToNow` from Osmosis pool queries.
- Chains through intermediate assets to resolve to OSMO.
- Converts OSMO to USD via Pyth OSMO/USD feed.
- **Fallback**: If Pyth OSMO/USD fails, uses `pools_for_usd_par_twap` with medianization.

#### Tier 3: Medianized Pool USD Par

- Used when both Pyth and direct TWAP fail.
- Collects prices from multiple pool sources.
- **Medianization**: Sort values, even count = average of middle two, odd count = middle value.

### Special Asset Types

**`is_usd_par` assets:**

- Price is capped at $1.00.
- Used for stablecoins that should never be priced above par.

**LP token prices:**

- Query pool state to get underlying asset composition.
- Price each underlying asset individually.
- Combine for total LP token value.

**Vault tokens:**

- Query `VaultTokenUnderlying` to get the underlying asset amount.
- Price the underlying asset and scale to vault token ratio.

### Query Limits

- Maximum **50 assets** per query call.
- Designed for batch pricing efficiency.

## User Flows

### Query a Price

1. Caller sends `QueryPrice` with asset info.
2. Oracle attempts Tier 1 (Pyth EMA).
3. If Tier 1 fails, falls back to Tier 2 (Osmosis TWAP via intermediaries to OSMO, then OSMO/USD).
4. If Tier 2 fails, falls back to Tier 3 (medianized pool prices).
5. Returns `PriceResponse` with USD price and timestamp.

### Batch Price Query

1. Caller sends `QueryPrices` with up to 50 assets.
2. Each asset is priced independently through the tier system.
3. Returns a vector of price responses.

## Technical Details

### TWAP Chain Resolution

For assets without direct Pyth feeds, the oracle chains through configured intermediate assets:

```
Asset → Pool1 → IntermediateA → Pool2 → OSMO → Pyth OSMO/USD → USD
```

Each hop uses `GeometricTwapToNow` for manipulation resistance.

### Medianization Algorithm

```
values.sort()
if values.len() % 2 == 0:
    median = (values[mid-1] + values[mid]) / 2
else:
    median = values[mid]
```

Used for USD par TWAP fallback to filter outliers from multiple pool sources.

## Cross-Contract Interactions

| Direction | Contract | Call | Purpose |
|-----------|----------|------|---------|
| Queried by | CDP | `QueryPrice` | Collateral and debt valuation |
| Queried by | Auction | `QueryPrice` | Price both sides of swaps |
| Queried by | LTV Disco | `QueryPrice` | Asset valuation for deposits |
| Queried by | Transmuter | `QueryPrice` | Swap rate calculation |
| Queried by | System Discounts | `QueryPrice` | MBRN valuation for discount calc |
| Query | Pyth | `PriceFeed` | EMA price data |
| Query | Osmosis | `GeometricTwapToNow` | Pool TWAP data |
| Query | Osmosis | Pool state | LP token underlying composition |
| Query | Vaults | `VaultTokenUnderlying` | Vault token conversion ratio |

## Important Invariants

1. EMA price is always preferred over spot price (Tier 1) for manipulation resistance.
2. `is_usd_par` assets are hard-capped at $1.00 -- they can never be priced above par.
3. Maximum 50 assets per batch query to prevent gas exhaustion.
4. Medianization requires at least one price source; with even count, the two middle values are averaged.
5. The tier system is strict: Tier 1 success short-circuits all fallbacks.
6. LP and vault token prices are derived from underlying assets, not from secondary market prices.
