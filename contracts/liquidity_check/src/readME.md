# Liquidity Check Contract

The Liquidity Check contract is used to fetch total debt token liquidity


## ExecuteMsg

### `add_asset`

- Save pool_ids for an asset

### `edit_asset`

- Extend current list of pool_ids

### `remove_asset`

- Remove asset info from state

## QueryMsg

### `liquidity`

- Query pooled assets for all of an asset's pool_ids
- Return the total of the asset in corresponding pools
