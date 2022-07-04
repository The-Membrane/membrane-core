# Risk Management

### Collateral

Each collateral has a debt cap (CDL supply cap). The cap is based off available liquidity for the asset in the accepted markets, initially just Osmosis. The max cap will be 20% of time-weighted average liquidity (TWAL) to account for price volatility and LP providers removing liquidity occasionally/during downturns.\
\
Collateral deemed as riskier will have lower % of TWAL for their debt cap, as well as lower minimum collateral ratios (i.e. lower LTVs).\
\
Risk is determined by:\
\- Liquidity -> Debt Caps\
\- Volatility -> Collateral Ratio\
\- Centralization risk (supply or regulatory)\
\- Value reflexivity\
\
An amalgamation of them all will factor into the interest rate ranges decided by governance.

### Debt Token

The supply limit for CDL is 5x available liquidity, aka liquidity is minimum 20% of total MC.&#x20;
