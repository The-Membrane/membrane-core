# Asset Bundles

Membrane's CDP system allows users to "bundle" any accepted assets in a single position. Bundling multiple assets together treats their total collateral value and TVL as a single unit, proportional to the internal value and LTV of each underlying asset. This enables users to hedge liquidation risk via bundling with less volatile assets, or uncorrelated assets.\
\
Interest rates, LTV and debt cap distribution are all affected by the bundling process, but for simplicity we'll use an example with only LTV.\
\
**Ex:** A position with 2 assets, each 50% of the position with 30% - 40% and 40% - 53% max LTV ranges respectively will result in position parameters of 35% max borrow LTV and 46.5% max LTV.\
\
**Note:** _All positions are liquidated proportionally. Users can open multiple positions so if a proportional liquidation is undesired, the deposits should be in separate positions._\
__\
__
