# Oracle Contract

Oracles are used to retrieve pricing data for collateral assets. The contract has 2 price sourcing branches depending on if the OSMO/USD price feed is available. If not avilable, it queries Geometric TWAPs from Osmosis for USD-par denominations. If available, it queries TWAPs for OSMO denominated prices to then convert into USD from the Pyth oracle system. As new robust price sources are deployed they can be added to reduce concentration of one of the protocol's core tools.
