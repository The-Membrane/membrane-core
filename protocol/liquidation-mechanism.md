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


The lower the fee, the longer the user's position can stay solvent to avoid additional liquidations as well as less of the asset getting sold on the open market, assuming the bot doesn't hold it, which protects against market wide cascades.

Additional Sources:&#x20;

1\)[ ](https://docs.euler.finance/developers/architecture#front-running-protection)[https://docs.euler.finance/developers/architecture#front-running-protection](https://docs.euler.finance/developers/architecture#front-running-protection)\
2\) [https://twitter.com/euler\_mab/status/1537091423748517889](https://twitter.com/euler\_mab/status/1537091423748517889)

### Technical Implementation Description

The protocol liquidates what collateral it can through the LQ and the SP before selling the remaining on the market. In the case that the modules take enough collateral without fully repaying the liquidation leaving the Sell Wall w/o enough to sell on the market to avoid bad debt, the protocol will skip the discounts and sell for the full amount from the start.

If either of the modules error or don't repay what was queried beforehand, the reply will catch it and sell the collateral on the market to cover. If the LQ has leftovers it will try to use the SP to cover, but if that also errors or is out of funds it'll use the sell wall for both. If the LQ does use the SP and that has errors, it goes to the sell wall.\
\
The last message that gets executed is the [BadDebtCheck ](../smart-contracts/positions.md#baddebtcheck)CallbackMsg. The check is lazy in the sense that it doesn't look for undercollateralized positions, just positions without collateral and debt to repay. This is because the liquidation function market sells collateral once the position value is under the Stability Pool + caller + protocol fee threshold.\
\
On success, i.e. bad debt is true, the contract activates a debt auction through the [Auction ](../smart-contracts/mbrn-auction.md)contract. It being lazy allows it to be added without slowing down liquidation calls and if necessary, auctions can be called manually.

#### Liquidation Function Walkthrough

* Fetch target\_position
* Assert Insolvency
* Calc + add message for per asset fees (caller + protocol)

Sift remaining collateral amount through LQ

* Check if LQ can liquidate the collateral amount and create the SubMsgs to do so
* Keep track of how much value is left in the position

Whatever credit isn't liquidated gets sent to the SP

* Query `sp_liq_fee`
* If: `leftover_position_value` minus fees can't repay the position, then the collateral is market sold. LQ msgs still go thru. Assign `RepayPropagation` fields.
* Else: Check if SP has enough credit to liquidate
* Whatever it can't is sent to the Sell Wall
* Assign `RepayPropagation` fields
* Build SP sub\_msgs
* If `leftover_position_value` isn't enough to repay a potential remaining `credit_repayment_amount`, NULL every sub\_msgs added and SW everything
* Reassign `RepayPropagation` fields

#### Reply Function Walkthrough

_Liquidation Queue:_

* Parse `repaid_amount` from the Response and subtract that from the total allocated to the queue
* Parse `collateral_amount` from the Response and send collateral amount liquidated to the Queue
* Update position claims for user, ie decrease how much they can withdraw\


_Stability Pool:_

* Query leftover credit to repay from `RepayPropagation`
* Send SP and LQ leftovers to the sell wall and add to existing Sell Wall distributions
* If there are none, send only LQ leftovers to the SP



_Sell Wall:_

* If success, update position claims for user.
* NOTE: Sell Wall is a submsg on reply on success only bc we want the msg to revert on error. In the future we can add backup routers and have the SW reply on errors and scroll thru the router options.&#x20;
