# Stability Pool Contract

The Stability Pool contract is used to pool debt tokens that act as a second layer for system liquidations after the Liquidation Queue & before market sales


## ExecuteMsg

### `deposit`

- Simple function that adds deposited asset to the AssetPool total and a Deposit object to it's list of deposits

/// Warning: Don't deposit twice separately in the same tx. Because deposit ids aren't used, deposits with the same owner, amount & time will be deleted together if one is used for liquidation.

### `withdraw`

- Unstake & withdraw deposited assets
- The primary call unstakes, the 2ndary call after the unstake time will withdraw
- If withdrawing no more than deposited, corresponding Deposits are given an unstake_time
- Incentives from unstaked Deposits are allotted to the user's claimables
- Since each Deposit is separate, withdrawals can span multiple Deposit objects so a replacement Deposit is made in order to allow more granular unstaking

### `restake`

- Deposits that were unstaked, i.e. withdrawn once, can be restaked 
- This allows users to restake without waiting for the unstake process to finish, where they would have lost incentives during that period
- It goes through multiple Deposits to attempt to restake but won't create new objects for partial restakes

### `liquidate`

- When called by the Positions contract, repay an at risk loan

### `distribute_funds` 

- After a liquidation, the Position's contract will distribute liquidated assets + the liquidation fee
- Assert the distributed funds are valid but trust the Position contract to send the correct amounts
- Using the repaid debt amount, parse through the list of Deposits in the AssetPool to create a list of depositors to distribute to
- If necessary, incentives will also be allotted during this time
- Use the ratios of the depositors in the distribution list to allocate distributed funds
- Assets are distributed FIFO instead of pro-rata to ease the UX for selling 

### `repay`

- Use a user's funds to repay for their own Position in the process of getting liquidated
- Similar logic to 'withdraw' outside of the CDP::Repay Msg at the end

### `claim`

- Claim ALL assets allocated to the user from incentives or liquidations
- This will recalculate accrued incentives so the total is up-to-date

