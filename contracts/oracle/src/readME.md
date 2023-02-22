# Oracle Contract

The Oracle contract is used to fetch asset prices for the system


## ExecuteMsg

### `add_asset`

- Save OracleInfo for an asset

### `edit_asset`

- Replace the OracleInfo saved for an asset
- Oracles are saved under basket_ids so that Baskets' can have different denominations

## QueryMsg

### `price`

- Query TWAP prices from Osmosis Pools and multiply results to get desired price denomination
- Find the median of price sources (currently only one price source)


Comment lines 346-357 to pass tests