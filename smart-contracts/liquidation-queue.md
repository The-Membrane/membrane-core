---
description: Fork of Anchor Protocol's implementation with slight modifications
---

# Liquidation Queue

\
The Liquidation contract enables users to submit CDP token bids for a Cw20 or native sdk token. Bidders can submit a bid to one of the bid pools; each of the pools deposited funds are used to buy the liquidated collateral at different discount rates. There are 21 slots per collateral, from 0% to 20%; users can bid on one or more slots.

Upon execution of a bid, collateral tokens are allocated to the bidder, while the bidder's bid tokens are sent to the repay the liquidated position.

Bids are consumed from the bid pools in increasing order of premium rate (e.g 2% bids are only consumed after 0% and 1% pools are emptied). The liquidated collateral is then allocatedto the bidders in the affected pools in proportion to their bid amount. The respective collateral should be claimed by the bidders.

To prevent bots from sniping loans, submitted bids are only activated after `wait_period` has expired, unless the total bid amount falls under the `bid_threshold`, in which case bids will be directly activated upon submission.

### Source

[https://docs.anchorprotocol.com/smart-contracts/liquidations/liquidation-queue-contract](https://docs.anchorprotocol.com/smart-contracts/liquidations/liquidation-queue-contract)\
[https://github.com/Anchor-Protocol/money-market-contracts/tree/main/contracts/liquidation\_queue](https://github.com/Anchor-Protocol/money-market-contracts/tree/main/contracts/liquidation\_queue)

### Modifications

* Automatic activation after `wait_period` elaspes. This increases computation time in return for less reliance on external contract calls.
* Liquidations send the [RepayMsg ](positions.md#repay)for the position in the Positions contract
* Prices are taken from input by the Positions contract, the messages are guaranteed the same block so the price will be block\__time +_[ __ Position's config](positions.md#config) oracle\__time\__limit second's old.
* The position is assumed insolvent since called by the Positions contract, ie there is no additional solvency check in this contract.
* ExecuteMsg::Liquidate doesn't take any assets up front, instead receiving assets in the Reply fn of the [Positions ](positions.md)contract
