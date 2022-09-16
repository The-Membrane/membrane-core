# Risk Management

### Collateral

Each unique asset has a **debt cap** (CDT mint cap) and a **supply cap**. The **debt cap** is based off the proportional value of the given asset within Membrane's TVL. The **supply cap** is a TVL ratio for each asset set by governance. LP shares don't receive a unique **debt cap** because its assets are added to their respective totals but can be directly limited through a **supply cap**. This allows a large diversification of token risk, minimizing the risk effects of bad debt or a malicious token attack to the caps assigned to the asset.\
\
Collateral deemed riskier will have lower % of TVL supply cap which in turn effects its possible debt cap. As well as having lower minimum collateral ratios (i.e. lower LTVs).\
\
Risk type -> Solved By:\
\- Systemic or Protocol Risk -> Debt Caps\
\- Volatility Risk -> Collateral Ratio\
\- Liquidity Risk -> Stability Pool\
\
An amalgamation of them all will factor into the interest rate ranges decided by governance. In general, rates increase when caps are near their respective set ratios to incentivize correction and vice versa.

### Safety Fund

Funded by protocol revenue, the safety fund is used to cover bad debt generated due to inefficient liquidations during harsh market conditions. This fund isn't a separate contract, instead its the quantity of pending revenue earned by the system that is yet to be "redeemed".\
\
While not covering liabilities, it acts as a living safety net to give user extra insurance that the protocol will remain solvent. In the long term, it can also be used to allow the protocol to make riskier loans and expand to unusual collateral types.

### MBRN Auction

If there is ever a shortfall event that results in more bad debt than pending revenue in the Safety Fund, **MBRN** will be auctioned off for a discount to recapitalize the system.
