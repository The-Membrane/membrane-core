# Positions Contract

The Positions contract is used to create multi-collateral backed positions to take out debt & hosts the logic to liquidate when insolvent

---


## ExecuteMsg
- Functions deposit, repay & increase_debt assert state is saved correctly at the end of the function
- A frozen basket only halts withdraw & increase_debt
- Accrue rates before all Position functions

### `deposit`

- Allows deposits of Basket accepted collateral
- Assets are validated before the deposit fn in contract.rs
- Anyone can deposit to any Position
- If an id is passed & no Position is found, a new position won't be created in case of a user mistake
- Supply & Debt Caps are enforced 
- Accruals are used to update repayment price, rate indicies & credit amount

### `withdraw`

- Position owners can withdraw assets as long as the Position remains solvent
- Expunged assets, assets whose supply cap is set to 0 must be withdraw in full before withdrawing other assets. Otherwise the protocol will be left with the asset instead of getting rid of it
- Supply caps aren't enforced to soften withdraw restrictions
- Debt caps are enforced at the end of the withdrawal & take more logic to calculate bc debt per asset changes for each withdraw
- The Withdraw Propagation checks to make sure the withdrawal was valid & didn't take more than request or more than the user owns

### `repay`

- Anyone can repay outstanding debt for a position
- Excess repayments are allowed & will be sent back to the sender or a designated address
- The resulting loan needs to be above the debt minimum so that Positions are always attractive to liquidate. The router is the only address that bypasses this so that liquidations cover as much debt as possible
- The assets used to repay are split to send revenue to stakers with the remaining burnt
- Then finally the repaid debt is subtracted from the debt per asset tallies

### `liq_repay`

- This function is used by the Stability Pool (SP) contract to receive asset distributions for repaid debt
- Aftering calling the previous repay(), collateral is split by its ratio in the Position & sent to the Stability Pool with the added fee
- Each asset sent is removed from the user's Position 
- The SP needs to know details on what's being sent to make distribution easier

### `increase_debt`

- Position owners can mint debt up to their Position's avg max borrow LTV
- The amount variable is passed as a integer or an LTV
- Debt can't be minted below the minimum debt
- Debt Caps are enforced

### `close_position`

- Position owners can close their position by selling remaining collateral to cover outstanding debt
- The max_spread is added to the amount sold to guarantee this is only a single swap. Because of this, users may get excess debt token along with their remaining collateral
- Can't attempt to sell more than the position owns of said collateral, an issue introduced by the max_spread logic
- LPs are split into pool assets
- Sales are a SubMsg sent through the configuration's router contract with a hook msg to Repay with the debt token that was just bought
- On success, the contract attempts to withdraw the remaining collateral. If the sales didn't purchase enough debt token to repay fully, the withdrawal will error

### `liquidate`

- This function validates insolvencies & calculates how much collateral gets liquidated per mechanism
- Loads & updates Position state
- Confirms Position insolvency
- Get repay value & amount
- Calculate per asset obligations which includes sending msg caller & protocol fees and sending collateral to the Liquidation Queue
- If the remaining collateral can't cover the debt repayment + the Stability Pool (SP) fee, then the collateral will get sold through the configuration's router. If not, it gets liquidated in the SP
- Any leftover collateral to be liquidated from the SP also gets sent to the router
- If there is outstanding debt to liquidate for & there is no more collateral to sell for the router, then all collateral gets sold, none going to the liquidation pools
- A CallbackMsg to check for bad debt is added to the end of the list of SubMsgs

Replies
- Due to SubMsg semantics, LQ message replies will return first. This allows us to handle all errors and leftover repayments in the SP reply. If the SP has available funds outstanding debt will be fulfilled by the SP, if not then to the router


### `create_basket`

- Create or edit the contract's sole basket
- Assert credit asset is a native token
- Add credit asset pools to the liquidity contract
- Set basket variables

 ### `edit_basket`

- Edit the contract's sole basket
- Edit basket variables. Can't edit basket_id, current_position_id, credit_price or credit_asset.

cAsset Addition Logic

- cAsset borrow LTV has to be lower than the max LTV & 100%. Realistically with debt minimums the highest will be ~90%.
- LP cAsset's need their pool assets added beforehand
- An oracle needs to exist for the cAsset in the configuration's oracle contract
- Liquidation queue's are added with a max premium at 5% below the cAsset's LTV buffer (90% max_LTV = 10% buffer).
The 5% should be set as the max liquidation caller_fee during high traffic periods to guarantee users in the LQ will be in premium slots whose liquidations wouldn't automatically error to the router due to solvency limits.
The default fee is the Stability Pool's liquidation fee since if the LQ doesn't liquidate the SP will at 10% and lower fees are better for the user & ecosystem. 
- Add cAsset to collateral supply caps
- Can't add the Basket's credit asset or the staking contract's MBRN denom
- No duplicate assets


 
