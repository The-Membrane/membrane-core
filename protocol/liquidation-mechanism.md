---
description: >-
  TLDR: Membrane's liquidation mechanism has 3 layers designed to have minimal
  effect on a collateral's market price while democratizing access to the
  discounted assets that come from liquidations.
---

# Liquidation Mechanism

There are 3 layers to the liquidation mechanism: **Liquidation Queue**, **Stability Pool (SP)** and the **Sell Wall.**\
****\
****The **Queue** allows users to bid on specific collateral assets at a range of premium rates.&#x20;

The **SP** acts as a backstop for the entire CDP system, its funds being used to liquidate for any collateral at a set premium.

As a final measure, any collateral positions that can't get liquidated by the first 2 steps will be sold on the market to avoid the protocol accruing bad debt. In the case it does, pending revenue and MBRN auctions will cover it, similar to [MakerDAO's Debt Auctions](https://docs.makerdao.com/keepers/the-auctions-of-the-maker-protocol).\
\
_In the case of errors repaying from the liquidation contracts, the error will trigger the collateral to go through the sell wall to ensure all liquidations can be executed by 1 external call of the initial liquidation function._

Having pools of CDT on standby to liquidate increases the efficiency of the market liquidity in smoothing our liquidation process. Instead of bots having to either hold CDT or buy it on the spot, with slippage relative to the pool, they can simply worry about executing the call. This provides the mechanism a larger buffer for harsh liquidation periods and reduces the effect on the collateral and CDT markets.

**Note: Any user funds in the Stability Pool will be used to repay said user's positions if liquidated. Meaning depositing in the SP doesn't increase liquidation risk for the user.**

Additional Sources:&#x20;

1\) [https://docs.makerdao.com/keepers/the-auctions-of-the-maker-protocol](https://docs.makerdao.com/keepers/the-auctions-of-the-maker-protocol)\
2\) [https://docs.liquity.org/faq/stability-pool-and-liquidations](https://docs.liquity.org/faq/stability-pool-and-liquidations)\
3\) [https://docs.anchorprotocol.com/protocol/loan-liquidation](https://docs.anchorprotocol.com/protocol/loan-liquidation)

### Liquidator Fees

Smart contracts aren't autonomous so they need to be called by an external source. These calls will be incentivized by a liquidation fee determined by free market mechanics. The more the target position is insolvent the larger the fee will be to the caller.

_Ex: If a position's liquidation point is 80% LTV and the position gets to 81%, the caller's fee would be 1% of the liquidated collateral._

The fee will keep increasing until a liquidator deems its profitable/desirable to liquidate, but if one waits too long it may lose the chance to capture the fee. This mechanism finds the lowest viable liquidation fee which benefits the user and the overall market.&#x20;

**Note: There is a minimum fee that goes to MBRN stakers**\


The lower the fee, the longer the user's position can stay solvent to avoid additional liquidations as well as less of the asset getting sold on the open market, assuming the bot doesn't hold it, which protects against market wide cascades.

Additional Sources:&#x20;

1\)[ ](https://docs.euler.finance/developers/architecture#front-running-protection)[https://docs.euler.finance/developers/architecture#front-running-protection](https://docs.euler.finance/developers/architecture#front-running-protection)\
2\) [https://twitter.com/euler\_mab/status/1537091423748517889](https://twitter.com/euler\_mab/status/1537091423748517889)

