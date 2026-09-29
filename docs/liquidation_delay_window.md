# Liquidation Delay Window Analysis

Historically each asset's (BTC, ATOM, OSMO) breach rate & recovery rate optimize at a 4% max 1hr window without extending the window so large that the protocol takes insane risk. Neither breaches the 4% window much in the hour either so we can ensure users have the full hour delay 99% of the time.

## 1-Hour Window Summary

| W%  | BTC Saved% | ATOM Saved% | OSMO Saved% | BTC Breach | ATOM Breach | OSMO Breach |
|-----|-----------|------------|------------|-----------|------------|------------|
| 1%  | 41.1%     | 8.0%       | 24.2%      | 4.94%     | 14.79%     | 14.17%     |
| 3%  | 41.3%     | 10.6%      | 25.6%      | 0.17%     | 2.42%      | 0.96%      |
| 4%  | 41.3%     | 10.9%      | 25.6%      | 0.03%     | 0.87%      | 0.50%      |
| 5%  | 41.3%     | 11.0%      | 25.6%      | 0.00%     | 0.47%      | 0.23%      |

## Multi-Hour Sweet Spot at W=4%

| Window | BTC Saved% | ATOM Saved% | OSMO Saved% |
|--------|-----------|------------|------------|
| 1hr    | 44.7%     | 20.8%      | 33.7%      |
| 4hr    | 47.0%     | 34.7%      | 39.4%      |
| 8hr    | 47.9%     | 39.0%      | 41.0%      |
| 12hr   | 48.5%     | 40.9%      | 39.9%      |
| 24hr   | 48.7%     | 39.6%      | 38.9%      |

The tradeoff is clear now -- ATOM and OSMO peak at 8-12hr delay windows for Saved%. Beyond that, breach rate climbs faster than recovery improves, and Saved% starts declining. BTC is more forgiving and keeps climbing through 24hr.
