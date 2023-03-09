# Debt Auction Contract

The Debt Auction contract is used to sell MBRN for debt tokens in the event of an insolvency that can't be covered by pending revenue


## ExecuteMsg

### `start_auction`

- Start or add to an existing auction
- Fulfilled auctions will automatically repay or send debt tokens to the designated address
- Starting an auction w/ a debt token is the way to add said token to the contract's asset list

### `remove_auction`

- Removes existing auction 

### `swap_for_mbrn`

- With the debt token of an ongoing auction, swap for MBRN at a discount
- Discount is incremented on a timescale determined by the contract configuration
- Allotted MBRN is minted to the sender
- If there is enough debt token in the contract to fulfill a repayment, it is done. Generic sends are done incrementally.
- Excess repayments are sent back to the sender to allow users to focus on speed of recapitaliziation rather than correctness

