# Liquidation Mechanism

There are 3 main steps to the liquidation mechanism: **Liquidation Queue**, **Stability Pool (SP)** and the **Sell Wall.**\
****\
****The **Queue** allows users to bid on specific collateral assets at a range of premium rates.&#x20;

The **SP** acts as a backstop for the entire CDP system, its funds being used to liquidate for any collateral at a set premium.

Then as a final measure, any collateral positions that can't get liquidated by the first 2 steps will be sold on the market to avoid the protocol accruing bad debt. In the case it does, there will be MBRN auctions to cover it, similar to MakerDAO's Debt Auctions.\
\
_In the case of errors repaying from the liquidation contracts, the error will trigger the collateral to go through the sell wall to ensure all liquidations can be executed by 1 external call of the initial liquidation function._

Additional Sources:&#x20;

1\) [https://docs.makerdao.com/keepers/the-auctions-of-the-maker-protocol](https://docs.makerdao.com/keepers/the-auctions-of-the-maker-protocol)\
2\) [https://docs.liquity.org/faq/stability-pool-and-liquidations](https://docs.liquity.org/faq/stability-pool-and-liquidations)\
3\) [https://docs.anchorprotocol.com/protocol/loan-liquidation](https://docs.anchorprotocol.com/protocol/loan-liquidation)

### Bot Fees

Smart contracts aren't autonomous so they need to be called by an external source. These calls will be incentivized by a liquidation fee determined by free market mechanics. The further the target position is insolvent the larger the fee will be to the caller.

_Ex: If a position's liquidation point is 80% LTV and the position gets to 81%, the caller's fee would be 1% of the liquidated collateral._

The fee will keep increasing until a bot deems its profitable/desirable to liquidate, but if 1 bot waits too long it may lose the chance to capture the fee. This mechanism finds the lowest viable liquidation fee which benefits the user and the overall market.\
\
_Bots can get up to a 2x bonus on the current fee based on MBRN staked proportional to the liquidation amount. This boost won't push fees far enough to negatively impact users but instead it'll give bots who obtain the boost liquidation priority due to increased speed into its profitability window._&#x20;

The lower the fee, the longer the user's position can stay solvent to avoid additional liquidations as well as less of the asset getting sold on the open market, assuming the bot doesn't hold it, which protects against market wide cascades.

Additional Sources:&#x20;

1\)[ ](https://docs.euler.finance/developers/architecture#front-running-protection)[https://docs.euler.finance/developers/architecture#front-running-protection](https://docs.euler.finance/developers/architecture#front-running-protection)\
2\) [https://twitter.com/euler\_mab/status/1537091423748517889](https://twitter.com/euler\_mab/status/1537091423748517889)



