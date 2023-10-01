Fork of Anchor Protocol's implementation with slight modifications

Source: 
https://docs.anchorprotocol.com/smart-contracts/liquidations/liquidation-queue-contract
https://github.com/Anchor-Protocol/money-market-contracts/tree/main/contracts/liquidation_queue

# Liquidation Queue Contract

The Liquidation Queue contract is used to pool debt tokens to liquidate specific collateral assets, acting as the 1st layer in the liquidation system. Each collateral type has a list of viable liquidation premium slots that users can bid within. Slots are emptied starting at the lowest premium to increase the efficiency of liquidations. During a successful liquidation, collateral is distributed pro-rata to each user in the premium slot. Its expected to attract bots but the goal is to distribute liquidated collateral to less technical users as well. For this, there is a waiting period that forces any just in time bids to wait if the liquidation slot is filled past a configurable threshold. Specific to Membrane, any collateral not liquidated in the Queue, which is at least the premium %, is sent further down the filter to the 2nd layer, the Stability Pool.

Modifications

- Automatic activation after wait_period elapses. This increases computation time in return for less reliance on external contract calls.
- Liquidations send BurnMsg for the debt to the Osmosis Proxy
- Prices are taken from input by the Positions contract, the messages are guaranteed the same block so the price will be block_time + Position's config oracle_time_limit second's old.
- The position is assumed insolvent since called by the Positions contract, ie there is no additional solvency check in this contract.
- ExecuteMsg::Liquidate doesn't take any assets up front, instead receiving assets in the Reply fn of the Positions contract
- Removed bid_with, instead saving the bid_asset from the Positions contract
- Don't error if the full collateral amount isn't liquidated, just update the returning attribute
- bid_for is a String in functions that require .as_bytes() to allow LP tokens to work

To Pass tests:
- Comment bid_asset in instantiate msg & add below:
let bid_asset = AssetInfo::NativeToken { denom: String::from("cdt") };
