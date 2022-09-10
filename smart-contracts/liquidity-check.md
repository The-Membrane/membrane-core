---
description: Calculates total Osmosis liquidity for its list of assets
---

# Liquidity Check

## InstantiateMsg

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub osmosis_proxy: String,
    pub positions_contract: String,
}
```

| Key                  | Type   | Description                             |
| -------------------- | ------ | --------------------------------------- |
| `*owner`             | String | Contract owner, defaults to info.sender |
| `osmosis_proxy`      | String | Osmosis Proxy contract address          |
| `positions_contract` | String | Positions contract address              |

&#x20;\* = optional

## ExecuteMsg

### `AddAsset`

Add asset and its query info to contract state

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    AddAsset {
        asset: LiquidityInfo,
    }
}

pub struct LiquidityInfo {  
    pub asset: AssetInfo,
    pub pool_ids: Vec<u64>,
}
```

| Key     | Type          | Description                                                    |
| ------- | ------------- | -------------------------------------------------------------- |
| `asset` | LiquidityInfo | Info needed to save to state and query liquidity for the asset |

### `EditAsset`

Replaces existing LiquidityInfo for an asset

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    EditAsset {
        asset: LiquidityInfo,
    }
}
```

| Key     | Type          | Description                                                    |
| ------- | ------------- | -------------------------------------------------------------- |
| `asset` | LiquidityInfo | Info needed to save to state and query liquidity for the asset |

### RemoveAsset

Remove asset from state

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    RemoveAsset {
        asset: AssetInfo,
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
```

| Key     | Type      | Description       |
| ------- | --------- | ----------------- |
| `asset` | AssetInfo | Asset information |

### `UpdateConfig`

Update contract configuration

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

|                       |        |                                |
| --------------------- | ------ | ------------------------------ |
| `*owner`              | String | Contract owner                 |
| `*osmosis_proxy`      | String | Osmosis Proxy contract address |
| `*positions_contract` | String | Positions contract address     |

&#x20;\* = optional

## QueryMsg

### `Config`

Returns contract configuration

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {}
}

pub struct Config {    
    pub owner: Addr,
    pub osmosis_proxy: Addr,
    pub positions_contract: Addr,
}
```

### `Assets`

Returns asset(s) available to query liquidity for

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Assets {
        asset_info: Option<AssetInfo>,
        limit: Option<u64>,
        start_after: Option<AssetInfo>,
    }
}

//Returns list of AssetInfo
```

| Key            | Type      | Description                                |
| -------------- | --------- | ------------------------------------------ |
| `*asset_info`  | AssetInfo | Specific asset to query, returns list of 1 |
| `*limit`       | u64       | Response limit                             |
| `*start_after` | AssetInfo | Start after asset for response list        |

&#x20;\* = optional

### `Liquidity`

Returns Osmosis liquidity for an asset

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Liquidity {
        asset: AssetInfo,
    }
}

//Returns Uint128
```

| Key     | Type      | Description |
| ------- | --------- | ----------- |
| `asset` | AssetInfo | Asset info  |
