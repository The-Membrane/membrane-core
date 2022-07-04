# Asset Bundles

Our CDP systems allows users to "bundle" any accepted assets in a single position. Bundling averages asset liquidation parameters as a way to allow further risk customization.\
\
The averaging effects interest rates, LTV and debt cap distribution but for simplicity we'll use an example with only LTV.\
\
Ex: A position with 2 assets, each 50% of the position with 30% - 40% and 40% - 53% max LTV ranges respectively will result in position parameters of 35% max borrow LTV and 46.5% max LTV.\
\
**Note:** _All positions are liquidated proportionally. Users can open multiple positions so if a proportional liquidation is undesired, the deposits should be in separate positions._\
__\
__
