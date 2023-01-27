Fork of Anchor Protocol's implementation with slight modifications

Source: 
https://docs.anchorprotocol.com/smart-contracts/liquidations/liquidation-queue-contract
https://github.com/Anchor-Protocol/money-market-contracts/tree/main/contracts/liquidation_queue

# Liquidation Queue Contract

The Liquidation Queue contract is used to pool debt tokens used to liquidate specific collateral assets, acting as the first layer in the liquidation system

Modifications

- Automatic activation after wait_period elapses. This increases computation time in return for less reliance on external contract calls.
- Liquidations send the RepayMsg for the position in the Positions contract
- Prices are taken from input by the Positions contract, the messages are guaranteed the same block so the price will be block_time + Position's config oracle_time_limit second's old.
- The position is assumed insolvent since called by the Positions contract, ie there is no additional solvency check in this contract.
- ExecuteMsg::Liquidate doesn't take any assets up front, instead receiving assets in the Reply fn of the Positions contract
- Removed bid_with, instead saving the bid_asset from the Positions contract

To Pass tests:
- Comment bid_asset in instantiate msg & add below:
let bid_asset = AssetInfo::NativeToken { denom: String::from("cdt") };
