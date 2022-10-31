---
description: Looped leverage contract that interacts w/ the Positions contract for the user
---

# Margin Proxy

## InstantiateMsg

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub apollo_router_contract: String,
    pub positions_contract: String,
    pub max_slippage: Decimal,
}
```

| Key                      | Type    | Description                             |
| ------------------------ | ------- | --------------------------------------- |
| `*owner`                 | String  | Contract owner, defaults to info.sender |
| `apollo_router_contract` | String  | Apollo DEX router contract              |
| `positions_contract`     | String  | Positions contract                      |
| `max_slippage`           | Decimal | Max slippage for asset swaps            |

&#x20;\* = optional

## ExecuteMsg

### `Deposit`

Deposits asset into a contract owned Position in the Positions contract

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Deposit {
        basket_id: Uint128,
        position_id: Option<Uint128>, 
    }
}
```

| Key            | Type    | Description                                     |
| -------------- | ------- | ----------------------------------------------- |
| `basket_id`    | Uint128 | Basket ID                                       |
| `*position_id` | Uint128 | Position ID, creates new position if not passed |

&#x20;\* = optional

### `Loop`

Loop credit sales to buy more collateral on leverage

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Loop {
        basket_id: Uint128,
        position_id: Uint128,
        num_loops: Option<u64>,
        target_LTV: Decimal,
    }
}
```

| Key           | Type    | Description                  |
| ------------- | ------- | ---------------------------- |
| `basket_id`   | Uint128 | Basket ID                    |
| `position_id` | Uint128 | Position ID                  |
| `*num_loops`  | u64     | Max number of loops          |
| `target_LTV`  | Decimal | LTV to use to loop position  |

&#x20;\* = optional

### `ClosePosition`

Close position and send excess credit and leftover collateral to the owner

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    ClosePosition {
        basket_id: Uint128,
        position_id: Uint128,
        max_spread: Decimal,
    }
}
```

| Key           | Type    | Description                                                   |
| ------------- | ------- | ------------------------------------------------------------- |
| `basket_id`   | Uint128 | Basket ID                                                     |
| `position_id` | Uint128 | Position ID                                                   |
| `max_spread`  | Decimal | Spread used to ensure collateral sales repay the debt amount  |

### `UpdateConfig`

Update contract configuration

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    UpdateConfig {
        owner: Option<String>,
        positions_contract: Option<String>,
        apollo_router_contract: Option<String>,
        max_slippage: Option<Decimal>,
    }
}
```

| Key                       | Type    | Description                                    |
| ------------------------- | ------- | ---------------------------------------------- |
| `*owner`                  | String  | Contract owner                                 |
| `*positions_contract`     | String  | Positions contract                             |
| `*apollo_router_contract` | String  | Apollo DEX router contract                     |
| `*max_slippage`           | Decimal | Max slippage for collateral sales when looping |

&#x20;\* = optional

## QueryMsg

### `Config`

Returns contract configuration

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {}
}

pub struct Config {
    pub owner: Addr,
    pub positions_contract: Addr,
    pub apollo_router_contract: Addr,
    pub max_slippage: Decimal,
}
```

### `GetUserPositions`

Returns user's positions hosted by this contract

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    GetUserPositions { user: String }
}

pub struct PositionResponse {
    pub position_id: Uint128,
    pub collateral_assets: Vec<cAsset>,pub cAsset_ratios: Vec<Decimal>,
    pub credit_amount: Uint128,
    pub basket_id: Uint128,
    pub avg_borrow_LTV: Decimal,
    pub avg_max_LTV: Decimal,
}
```

| Key    | Type   | Description                          |
| ------ | ------ | ------------------------------------ |
| `user` | String | User address that owns the positions |
