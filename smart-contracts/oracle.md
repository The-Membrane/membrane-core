---
description: Contract that holds price fetching information for assets
---

# Oracle

* The v1 Oracle only uses Osmosis TWAPs&#x20;

## InstantiateMsg

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub osmosis_proxy: String,
}
```

| Key             | Type   | Description                             |
| --------------- | ------ | --------------------------------------- |
| `*owner`        | String | Contract owner, defaults to info.sender |
| `osmosis_proxy` | String | Osmosis Proxy contract address          |

&#x20;\* = optional

## ExecuteMsg

### `UpdateConfig`

Updates contract configuration

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    UpdateConfig {
        owner: Option<String>,
        osmosis_proxy: Option<String>,
        positions_contract: Option<String>,
    }
}
```

| Key                   | Type   | Description                    |
| --------------------- | ------ | ------------------------------ |
| `*owner`              | String | Contract owner                 |
| `*osmosis_proxy`      | String | Osmosis Proxy contract address |
| `*positions_contract` | String | Position's contract address    |

&#x20;\* = optional

### `AddAsset`

Add Asset oracle information to the contract

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    AddAsset {
        asset_info: AssetInfo,
        oracle_info: AssetOracleInfo,
    }
}

pub enum AssetInfo {
    Token{
        address: Addr,
    },
    NativeToken{
        denom: String,
    },
}

pub struct AssetOracleInfo {
    pub osmosis_pool_for_twap: TWAPPoolInfo,
}

pub struct TWAPPoolInfo {
    pub pool_id: u64,
    pub base_asset_denom: String,
    pub quote_asset_denom: String,
 }
```

| Key           | Type            | Description                 |
| ------------- | --------------- | --------------------------- |
| `asset_info`  | AssetInfo       | Asset info                  |
| `oracle_info` | AssetOracleInfo | Oracle info for added asset |

### `EditAsset`

Replace existing asset oracle info

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    AddAsset {
        asset_info: AssetInfo,
        oracle_info: AssetOracleInfo,
    }
}
```

| Key           | Type            | Description        |
| ------------- | --------------- | ------------------ |
| `asset_info`  | AssetInfo       | Asset info         |
| `oracle_info` | AssetOracleInfo | Asset oracle info  |

## QueryMsg

### `Config`

Returns contract configuration

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {}
}

pub struct ConfigResponse {
    pub owner: String, 
}
```

### `Price`

Returns list of asset prices and average price&#x20;

```
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Price {
        asset_info: AssetInfo,
        twap_timeframe: u64, 
        basket_id: Option<Uint128>,
    }
}

pub struct PriceResponse {
    pub prices: Vec<PriceInfo>, 
    pub avg_price: Decimal,
}
```

| Key              | Type      | Description                                     |
| ---------------- | --------- | ----------------------------------------------- |
| `asset_info`     | AssetInfo | Asset info                                      |
| `twap_timeframe` | u64       | TWAP timeframe (in days) for TWAP prices        |
| `*basket_id`     | Uint128   | Basket\_id to select oracle quote asset for CDT |

&#x20;\* = optional

### `Prices`

Returns list of PriceResponse for list of assets

```
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Prices {
        asset_infos: Vec<AssetInfo>,
        twap_timeframe: u64,
    }
}

pub struct PriceResponse {
    pub prices: Vec<PriceInfo>, 
    pub avg_price: Decimal,
}
```

| Key              | Type            | Description                              |
| ---------------- | --------------- | ---------------------------------------- |
| `asset_info`     | Vec\<AssetInfo> | Asset infos                              |
| `twap_timeframe` | u64             | TWAP timeframe (in days) for TWAP prices |

### `Asset`

Returns oracle info for an asset

```
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Asset {
        asset_info: AssetInfo,
    }
}

pub struct AssetResponse {
    pub asset_info: AssetInfo,
    pub oracle_info: AssetOracleInfo,
}
```

| Key          | Type      | Description |
| ------------ | --------- | ----------- |
| `asset_info` | AssetInfo | Asset Info  |

### `Assets`

Returns list of oracle info for a list of assets

```
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Assets {
        asset_infos: Vec<AssetInfo>,
    }
}

pub struct AssetResponse {
    pub asset_info: AssetInfo,
    pub oracle_info: AssetOracleInfo,
}
```

| Key          | Type      | Description |
| ------------ | --------- | ----------- |
| `asset_info` | AssetInfo | Asset Info  |
