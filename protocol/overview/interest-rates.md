---
description: How does CDT stabilize?
---

# Interest Rates

In v1, there are variable rates that follow a two-slope model.&#x20;

As the debt cap utilization for an asset increases, the variable rate rises linearly towards the "max" rate for the collateral type. Once the utilization gets to the configurations desired debt cap util, the rate is multiplied by each % increase of utilization. This is to incentivize users using those assets to pay back debts as the protocol's risk in that asset increases.

**Ex:** With a desired util of 90%, current util of 89% and "max" rate of 3%, the variable rate would equal 2.67%.&#x20;

To enact the 2nd slope directly past the desired util, the multiplier will start at 1 + the % increase. So w/ the same parameters but w/ a current util at 91%, the variable rate would be 5.4% or 3 \* .9 \* (1+1).

The debt caps are equal to the _asset's ratio of TVL_ times the _credit's liquidity_ \* the _configuration's liquidity multiplier._

## **Price-aware Rates**

The rates act as desired above when market price of **CDT** is within the margin of error. Outside of said margin, the rates will react to the price to incentivizie **CDT** stability. This means if above peg, rates will trend towards 0 & below peg, rates will increase proportional to the distance from the peg.

The system operates optimal with a stable debt token which is why the system prioritizes stability incentives over revenue.

## Liquidity-aware Rates

Due to the relationship between the debt cap and the liqudity multiplier, as liquidity decreases, so will the system-wide debt cap. This translates to increased rates as liquidity decreases and decreased rates as it increases. This acts as an incentive to tighten the range of **CDT** volatility.

**Note: Because rates are percentages, the incentives they create will have more effect on the CDT market as well-capitalized users enter the system. This means as CDT TVL grows, its stability & volatility will increase and decrease respectively.**
