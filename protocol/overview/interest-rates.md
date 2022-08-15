# Interest Rates

In v1, there are variable rates that follow a two-slope model similar to AAVE.&#x20;

As the debt cap utilization for an asset increases, the variable rate rises linearly towards the "max" rate for the collateral type. Once the utilization gets to the configurations desired debt cap util, the rate is multiplied by each % increase of utilization. This is to incentivize users using those assets to pay back debts as the protocol's risk in that asset increases.

**Ex:** With a desired util of 90%, current util of 89% and "max" rate of 3%, the variable rate would equal 2.67%.&#x20;

To enact the 2nd slope directly past the desired util, the multiplier will start at 1 + the % increase. So w/ the same parameters but w/ a current util at 91%, the variable rate would be 5.4% or 3 \* .9 \* (1+1).

The debt caps are equal to the _asset's ratio of TVL_ times the _credit's liquidity_ \* the _configuration's liquidity multiplier._

