---
description: Interest Rates and Borrow Fee
---

# Usage Costs

### Interest Rates

Membrane offers fixed and variable interest rates. The fixed rate is always 50% of the current variable rate and the max rate for the asset. The interest rate range will be decided by governance but will be generally be higher for riskier assets. __ \
\
_If an asset has a current rate range of 0-2% then the fixed rate for that asset would be 1%._\
\
The variable rate changes as the debt cap for the collateral asset increases. So a capped asset would have users paying the max rate for new or non-fixed rate positions.

Fixed rates can be lowered by staking **MBRN** at values proportional to the debt amount**.** There will be a minimum time limit decided by governance initially 2 weeks. The formula for the discount would be: _( Value of MBRN / value of debt ) \* ( fixed rate - variable rate )_

_Using the example above, if the user's staked MBRN was 50% of the debt value, their fixed rate would be lowered to .5% instead of 1%._\
__\
_**Note:** Any debt increase will alter the position rates if fixed. The user can choose the current fixed rate or stick to the variable rate._

__

### Borrow Fee

The borrow fee is the fee taken at each increase of debt, initially at 0.5% but mutable by governance.
