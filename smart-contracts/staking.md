---
description: MBRN Staking contract
---

# Staking

## InstantiateMsg

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub positions_contract: Option<String>,
    pub builders_contract: Option<String>,
    pub osmosis_proxy: Option<String>,    
    pub staking_rate: Option<Decimal>,
    pub mbrn_denom: String,
    pub dex_router: Option<String>,
    pub max_spread: Option<Decimal>,
}
```

| Key                   | Type    | Description                                   |
| --------------------- | ------- | --------------------------------------------- |
| `*owner`              | String  | Contract owner                                |
| `*positions_contract` | String  | Positions contract address                    |
| `*builders_contract`  | String  | Builder's Vesting contract address            |
| `*osmosis_proxy`      | String  | Osmosis Proxy contract address                |
| `*staking_rate`       | Decimal | Desired staking rate, defaults to 10%         |
| `mbrn_denom`          | String  | MBRN full denom                               |
| `*dex_router`         | String  | DEX Router contract address                   |
| `*max_spread`         | Decimal | Max spread for asset routing, defaults to 10% |

&#x20;\* = optional

## ExecuteMsg

### `Receive`

Can be called during a CW20 token transfer when the Positions contract is the recipient. Allows the token transfer to execute a [Receive Hook](staking.md#receive-hook) as a subsequent action within the same transaction.

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Receive(Cw20ReceiveMsg)
}

pub struct Cw20ReceiveMsg {
    pub sender: String,
    pub amount: Uint128,
    pub msg: Binary,
}
```

### `UpdateConfig`

Update Config

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    UpdateConfig {
        owner: Option<String>,
        positions_contract: Option<String>,
        builders_contract: Option<String>,
        osmosis_proxy: Option<String>,
        mbrn_denom: Option<String>,  
        staking_rate: Option<Decimal>,        
        dex_router: Option<String>,
        max_spread: Option<Decimal>,
    }
}
```
