---
description: >-
  Proxy to Osmosis SDK module functions. Enforces max supplies for tokens
  created by Membrane.
---

# Osmosis Proxy

## InstantiateMsg

```
pub struct InstantiateMsg {}
```

## ExecuteMsg

### `CreateDenom`

Create native asset denom using Osmosis' tokenfactory

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    CreateDenom {
        subdenom: String,
        basket_id: String,
        max_supply: Option<Uint128>,
        liquidity_multiplier: Option<Decimal>,
    }
}
```

| Key                     | Type    | Description                               |
| ----------------------- | ------- | ----------------------------------------- |
| `subdenom`              | String  | Subdenom for native asset                 |
| `basket_id`             | String  | For Positions contract replies to save    |
| `*max_supply`           | Uint128 | Token max supply enforced by the contract |
| `*liquidity_multiplier` | Decimal | For Positions contract replies to save    |

&#x20;\* = optional

### `ChangeAdmin`

Change Admin for owned tokenfactory denom

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    ChangeAdmin {
        denom: String,
        new_admin_address: String,
    }
}
```

| Key                 | Type   | Description                           |
| ------------------- | ------ | ------------------------------------- |
| `denom`             | String | Owned denom to edit                   |
| `new_admin_address` | String | Admin address to migrate ownership to |

### `MintTokens`

Mint tokens for owned token denoms

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    MintTokens {
        denom: String,
        amount: Uint128,
        mint_to_address: String,
    }
}
```

| Key               | Type    | Description               |
| ----------------- | ------- | ------------------------- |
| `denom`           | String  | Token denom to mint       |
| `amount`          | Uint128 | Amount to mint            |
| `mint_to_address` | String  | Address to mint tokens to |

### `BurnTokens`

Burn tokens&#x20;

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    BurnTokens {
        denom: String,
        amount: Uint128,
        burn_from_address: String,
    }
}
```

| Key                 | Type    | Description                 |
| ------------------- | ------- | --------------------------- |
| `denom`             | String  | Token denom to burn         |
| `amount`            | Uint128 | Amount to mint              |
| `burn_from_address` | String  | Address to burn tokens from |

### `EditTokenMaxSupply`

Edit contract enforced token max supply

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    EditTokenMaxSupply {
        denom: String,
        max_supply: Uint128,
    }
}
```

| Key          | Type    | Description         |
| ------------ | ------- | ------------------- |
| `denom`      | String  | Denom's max to edit |
| `max_supply` | Uint128 | New max supply      |

### `UpdateConfig`

Update the contract configuration

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    UpdateConfig {
        owner: Option<String>,
        add_owner: bool,
        debt_auction: Option<String>,
    }
}
```

| Key             | Type   | Descripiton             |
| --------------- | ------ | ----------------------- |
| `*owner`        | String | New contract owner      |
| `add_owner`     | bool   | Add or remove new owner |
| `*debt_auction` | String | Debt Auction address    |

&#x20; \* = optional

## QueryMsg

### `GetDenom`

Returns full denom of the tokenfactory token representing the subdenom

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    GetDenom {
        creator_address: String,
        subdenom: String,
    }
}

pub struct GetDenomResponse {
    pub denom: String,
}
```

| Key               | Type   | Description                  |
| ----------------- | ------ | ---------------------------- |
| `creator_address` | String | Admin address of token denom |
| `subdenom`        | String | Token subdenom               |

### PoolState

Returns a list all tokens traded on it with current liquidity (spot) for a given pool ID.  As well as the total number of LP shares and their denom.

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    PoolState { id: u64 }
}

pub struct PoolStateResponse {
    /// The various assets that be swapped. Including current liquidity.
    pub assets: Vec<Coin>,
    /// The number of lp shares and their amount
    pub shares: Coin,
}
```

| Key  | Type | Description     |
| ---- | ---- | --------------- |
| `id` | u64  | Pool identifier |

### `ArithmeticTwapToNow`

Returns the accumulated historical TWAP of the given base asset and quote asset.

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    ArithmeticTwapToNow {
        id: u64,
        quote_asset_denom: String,
        base_asset_denom: String,
        start_time: i64,
    }
}

pub struct ArithmeticTwapToNowResponse {
    pub twap: Decimal,
}
```

| Key                 | Type   | Description                              |
| ------------------- | ------ | ---------------------------------------- |
| `id`                | u64    | Pool identifier                          |
| `quote_asset_denom` | String | Quote assert denom                       |
| `base_asset_denom`  | String | Base asset denom                         |
| `start_time`        | i64    | TWAP start time in Unix time millisecond |

### `GetTokenInfo`

Returns current supply and max supply for token denoms created with the contract

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    GetTokenInfo {
        denom: String,
    }
}

pub struct TokenInfoResponse {
    pub denom: String,
    pub current_supply: Uint128,
    pub max_supply: Uint128,
}
```

| Key     | Type   | Description  |
| ------- | ------ | ------------ |
| `denom` | String | Token denom  |

### `Config`

Returns the contract configuration

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config { }
}

pub struct Config {
    pub owners: Vec<Addr>,
    pub debt_auction: Option<Addr>,
}
```
