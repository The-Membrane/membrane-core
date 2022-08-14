# Positions

The Positions contract implements the logic for Collateralized Debt Positions (CDPs), through which users can receive debt tokens against their deposited collateral.\
\
Collateral parameters are held in the cAsset object, which also holds the address needed for its oracle in the Oracle Contract.

The contract also contains the logic for initiating liquidations of CDPs and the sell wall but external debt repayment logic goes through the **Queue** and **Stability Pool** contracts.

## InstantiateMsg

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub oracle_time_limit: u64, //in seconds until oracle failure is acceoted
    pub debt_minimum: Decimal, //Debt minimum value per position
    pub liq_fee: Decimal,
//Contracts
    pub stability_pool: Option<String>,
    pub dex_router: Option<String>,
    pub liq_fee_collector: Option<String>,
    pub interest_revenue_collector: Option<String>,
    pub osmosis_proxy: Option<String>,
    pub debt_auction: Option<String>,
//Basket Creation
    pub collateral_types: Option<Vec<cAsset>>,
    pub credit_asset: Option<Asset>,
    pub credit_price: Option<Decimal>,
    pub credit_interest: Option<Decimal>,
}


pub struct Asset{
    pub info: AssetInfo,
    pub amount: Uint128,
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

| Key                 | Type         | Description                                                        |
| ------------------- | ------------ | ------------------------------------------------------------------ |
| `*owner`            | String       | Contract owner that defaults to info.sender                        |
| `oracle-time-limit` | u64          | Limit in seconds that the oracle has before the values are invalid |
| `debt_minimum`      | Decimal      | Minimum value in debt per position                                 |
| `liq_fee`           | Decimal      | Fee that goes to the protocol during liquidations                  |
| `*stability_pool`   | String       | Stability Pool Contract                                            |
| `*dex_router`       | String       | DEX Router Contract                                                |
| `*fee_collector`    | String       | Address that is sent liq\_fees                                     |
| `*osmosis_proxy`    | String       | Osmosis Proxy contract to use SDK modules                          |
| `*debt_auction`     | String       | Auction Contract that sells protocol tokens to repay debt          |
| `*collateral_types` | Vec\<cAsset> | Accepted cAssets for an initial basket                             |
| `*credit_asset`     | Asset        | Credit asset for an initial basket                                 |
| `*credit_price`     | Decimal      | Credit price for an initial basket                                 |
| `*credit_interest`  | Decimal      | Credit interest for an initial basket                              |

\* = optional

## ExecuteMsg

### `UpdateConfig`

Update Config by the current config.owner

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    UpdateConfig {
        owner: Option<String>,
        stability_pool: Option<String>,
        dex_router: Option<String>,
        osmosis_proxy: Option<String>,
        debt_auction: Option<String>,
        liq_fee_collector: Option<String>,
        interest_revenue_collector: Option<String>,
        liq_fee: Option<Decimal>,
        debt_minimum: Option<Uint128>,
        oracle_time_limit: Option<u64>,
    }
}
```

| Key                             | Type    | Description                        |
| ------------------------------- | ------- | ---------------------------------- |
| `*owner`                        | String  | Owner of contract                  |
| `*stability_pool`               | String  | Stability Pool contract            |
| `*dex_router`                   | String  | Dex Router contract                |
| `*osmosis_proxy`                | String  | Osmosis Proxy contract             |
| `*debt_auction`                 | String  | Debt Auction contract              |
| `*liq_`_`fee_`_`collector`      | String  | Liquidation fee collector address  |
| `*interest_`_`fee_`_`collector` | String  | CDP interest fee collector address |
| _`*liq_fee`_                    | Decimal | Liquidation fee                    |
| `*debt_minimum`                 | Uint128 | Debt minimum in terms of value     |
| `*oracle_`_`time_`_`limit`      | u64     | Oracle expiration time limit       |

&#x20;\* = optional

### `Receive`

Can be called during a CW20 token transfer when the Positions contract is the recipient. Allows the token transfer to execute a [Receive Hook](positions.md#receive-hook) as a subsequent action within the same transaction.

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

| Key      | Type    | Description                                                             |
| -------- | ------- | ----------------------------------------------------------------------- |
| `sender` | String  | Sender of the token transfer                                            |
| `amount` | Uint128 | Amount of tokens received                                               |
| `msg`    | Binary  | Base64-encoded string of JSON of [Receive Hook](positions.md#undefined) |

### `Deposit`

{% hint style="info" %}
Used for depositing native assets as collateral. For depositing Cw20 collateral to a CDP, you need to use the [Receive Hook variant](positions.md#undefined).
{% endhint %}

Deposits basket accepted collateral to a new or existing position.

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Deposit {
        assets: Vec<AssetInfo>,
        basket_id: Uint128,
        position_id: Option<Uint128>,
        position_owner: Option<String>,
    },
}
```

| Key                | Type            | Description                                                              |
| ------------------ | --------------- | ------------------------------------------------------------------------ |
| `assets`           | Vec\<AssetInfo> | Asset info of sent assets                                                |
| `basket_id`        | Uint128         | Basket ID to deposit to.                                                 |
| \*`position_id`    | Uint128         | Position ID to deposit to. If none is passed, a new position is created. |
| \*`position_owner` | String          | Owner of the position, defaults to info.sender                           |

\* = optional

### `IncreaseDebt`

Increase debt of a position. Only callable by the position owner and limited by the position's max borrow LTV.

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    IncreaseDebt { 
        basket_id: Uint128,
        position_id: Uint128,
        amount: Uint128,
    }, 
}
```

| Key           | Type    | Description                      |
| ------------- | ------- | -------------------------------- |
| `basket_id`   | Uint128 | ID of basket the position is in  |
| `position_id` | Uint128 | ID of position                   |
| `amount`      | Uint128 | Amount to increase debt by       |

### `Withdraw`

Withdraw assets from the caller's position as long as it leaves the position solvent in relation to the max borrow LTV

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Withdraw {
        basket_id: Uint128,
        position_id: Uint128,
        assets: Vec<Asset>,
    },
}
```

| Key           | Type        | Description                          |
| ------------- | ----------- | ------------------------------------ |
| `basket_id`   | Uint128     | ID of basket the position is in      |
| `position_id` | Uint128     | ID of position                       |
| `assets`      | Vec\<Asset> | Assets to withdraw from the position |

### `Repay`

{% hint style="info" %}
Used for repaying native assets as collateral. For repaying Cw20 credit assets, you need to use the [Receive Hook variant](positions.md#undefined).
{% endhint %}

Repay outstanding debt for a position, not exclusive to the position owner.

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Repay {
        basket_id: Uint128,
        position_id: Uint128,
        position_owner: Option<String>, 
        credit_asset: Asset,
    },
}
```

| Key               | Type    | Description                     |
| ----------------- | ------- | ------------------------------- |
| `basket_id`       | Uint128 | ID of basket the Position is in |
| `position_id`     | Uint128 | ID of Position                  |
| `*position_owner` | String  | Owner of Position to repay      |
| `credit_asset`    | Asset   | Asset object for repayment info |

\* = optional

### `LiqRepay`

Repay function for the liquidation contracts the CDP uses ([Queue ](liquidation-queue.md)and [Stability Pool](stability-pool.md)). Used to repay insolvent positions and distribute liquidated funds to said contracts.

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    LiqRepay {
        credit_asset: Asset,
        collateral_asset: Option<Asset>,
        fee_ratios: Option<Vec<RepayFee>>, 
    },
}

pub struct RepayFee {
    pub fee: Decimal,
    pub ratio: Decimal,
}
```

| Key                 | Type           | Description                                                                                         |
| ------------------- | -------------- | --------------------------------------------------------------------------------------------------- |
| `credit_asset`      | Asset          | Asset object for repayment info                                                                     |
| `*collateral_asset` | Asset          | Collateral asset to specify for distribution, used by the [Liquidation Queue](liquidation-queue.md) |
| `*fee_ratios`       | Vec\<RepayFee> | List of fee ratios used by the [Liquidation Queue](liquidation-queue.md)                            |

\* = optional

### `Liquidate`

Assert's the position is insolvent and calculates the distribution of repayment to the various liquidation modules. Does a bad debt check at the end of the procedure that starts a[ MBRN auction](mbrn-auction.md) if necessary.

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Liquidate {
        basket_id: Uint128,
        position_id: Uint128,
        position_owner: String,
    },
}
```

| Key              | Type    | Description                     |
| ---------------- | ------- | ------------------------------- |
| `basket_id`      | Uint128 | ID of basket the Position is in |
| `position_id`    | Uint128 | ID of Position                  |
| `position_owner` | String  | Owner of Position               |

### `MintRevenue`

Mint pending revenue from chosen basket, only usable by config or basket.owner

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    MintRevenue {
        basket_id: Uint128,
        send_to: Option<String>, //Defaults to config.interest_revenue_collector
        repay_for: Option<UserInfo>, //Repay for a position w/ the revenue
        amount: Option<Uint128>,
    },
}

pub struct UserInfo {
    pub basket_id: Uint128,
    pub position_id: Uint128,
    pub position_owner: String,
}
```

| Key          | Type     | Description                                                                 |
| ------------ | -------- | --------------------------------------------------------------------------- |
| `basket_id`  | Uint128  | ID of Basket                                                                |
| `*send_to`   | String   | Address to send revenue to, defaults to config.interest\_revenue\_collector |
| `*repay_for` | UserInfo | Position Info to repay for w/ revenue. To be used for BadDebt situations.   |
| `*amount`    | Uint128  | Amount to mint, defaults to all                                             |

&#x20;\* = optional

### `CreateBasket`

Add Basket to the Position's contract, only callable by the contract owner.

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    CreateBasket {
        owner: Option<String>,
        collateral_types: Vec<cAsset>,
        credit_asset: Asset,
        credit_price: Option<Decimal>,
        credit_interest: Option<Decimal>,
    },
}
```

| Key                | Type         | Description                               |
| ------------------ | ------------ | ----------------------------------------- |
| `*owner`           | String       | Basket owner, defaults to info.sender     |
| `collateral_type`  | Vec\<cAsset> | List of accepted cAssets                  |
| `credit_asset`     | Asset        | Asset info for Basket's credit asset      |
| `*credit_price`    | Decimal      | Price of credit in basket                 |
| `*credit_interest` | Decimal      | Interest rate of credit's repayment price |

\* = optional

### `EditBasket`

Add cAsset, change owner and/or change credit\_interest of an existing Basket.

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    EditBasket {
        basket_id: Uint128,
        added_cAsset: Option<cAsset>,
        owner: Option<String>,
        credit_interest: Option<Decimal>,
    }, 
}
```

| Key                | Type    | Description                                     |
| ------------------ | ------- | ----------------------------------------------- |
| `basket_id`        | Uint128 | ID of existing Basket                           |
| `*added_cAsset`    | cAsset  | cAsset object to add to accepted basket objects |
| `*owner`           | String  | New owner of Basket                             |
| `*credit_interest` | Decimal | Credit repayment price interest                 |

\* = optional

### `EditAdmin`

Edit contract owner.

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    EditAdmin {
        owner: String,
    },
}
```

| Key     | Type   | Description              |
| ------- | ------ | ------------------------ |
| `owner` | String | Positions contract owner |

### `Callback`

Messages usable only by the contract to enable functionality in line with message semantics

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Callback(CallbackMsg)
}

pub enum CallbackMsg {
    BadDebtCheck {
        basket_id: Uint128,
        position_id: Uint128,
        position_owner: Addr,
    },
}
```

## Receive Hook

### `Deposit`

{% hint style="info" %}
Used for depositing `CW20` assets as collateral. For depositing native assets collateral to a CDP, you need to use the [ExecuteMsg variant](positions.md#deposit)
{% endhint %}

Deposits basket accepted collateral to a new or existing position.

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Deposit {
        basket_id: Uint128,
        position_owner: Option<String>,
        position_id: Option<Uint128>,
    },
}
```

| Key                | Type    | Description                                                             |
| ------------------ | ------- | ----------------------------------------------------------------------- |
| `basket_id`        | Uint128 | Basket ID to deposit to                                                 |
| \*`position_owner` | String  | Owner of the position, defaults to info.sender                          |
| \*`position_id`    | Uint128 | Position ID to deposit to. If none is passed, a new position is created |

\* = optional

## CallbackMsg

### `BadDebtCheck`

After liquidations, this checks for bad debt in the liquidated position.

```
BadDebtCheck {
        basket_id: Uint128,
        position_id: Uint128,
        position_owner: Addr,
}
```

| Key              | Type    | Description                     |
| ---------------- | ------- | ------------------------------- |
| `basket_id`      | Uint128 | ID of basket the Position is in |
| `position_id`    | Uint128 | ID of Position                  |
| `position_owner` | Addr    | Owner of Position               |

## QueryMsg

### `Config`

Returns Config

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {}
}

pub struct ConfigResponse {
    pub owner: String,
    pub current_basket_id: Uint128,
    pub stability_pool: String,
    pub dex_router: String, //Apollo's router, will need to change msg types if the router changes most likely.
    pub liq_fee_collector: String,
    pub interest_revenue_collector: String,
    pub osmosis_proxy: String,
    pub debt_auction: String,
    pub liq_fee: Decimal, // 5 = 5%
    pub oracle_time_limit: u64,
    pub debt_minimum: Uint128,
}
```

### `GetUserPositions`

Returns all Positions from a user

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    GetUserPositions { 
        basket_id: Option<Uint128>, 
        user: String
    },
}

pub struct PositionResponse {
    pub position_id: String,
    pub collateral_assets: Vec<cAsset>,
    pub avg_borrow_LTV: String,
    pub avg_max_LTV: String,
    pub credit_amount: String,
    pub basket_id: String,
    
}
```

| Key          | Type    | Description                                                               |
| ------------ | ------- | ------------------------------------------------------------------------- |
| `*basket_id` | Uint128 | ID of Basket to limit positions to, defaults to positions from all Basket |
| `user`       | String  | Position owner to query for                                               |

\* = optional

### `GetPosition`

Returns single Position data

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    GetPosition { 
        position_id: Uint128, 
        basket_id: Uint128, 
        user: String 
    },
}

pub struct PositionResponse {
    pub position_id: String,
    pub collateral_assets: Vec<cAsset>,
    pub credit_amount: String,
    pub basket_id: String,
    
}
```

| Key           | Type    | Description                      |
| ------------- | ------- | -------------------------------- |
| `position_id` | Uint128 | ID of Position                   |
| `basket_id`   | Uint128 | ID of Basket the Position is in  |
| `user`        | String  | User that owns position          |

### `GetBasketPositions`

Returns all positions in a basket with optional limits

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    GetBasketPositions { 
        basket_id: Uint128,
        start_after: Option<String>,
        limit: Option<u32>,
    },
}

pub struct PositionsResponse{
    pub user: String,
    pub positions: Vec<Position>,
}

pub struct Position {
    pub position_id: Uint128,
    pub collateral_assets: Vec<cAsset>,
    pub credit_amount: Decimal,
    pub basket_id: Uint128,
}
```

| Key            | Type    | Description                        |
| -------------- | ------- | ---------------------------------- |
| `basket_id`    | Uint128 | ID of Basket to parse              |
| `*start_after` | String  | User address to start after        |
| `*limit`       | u32     | Limit to # of users parsed through |

\* = optional

### `GetBasket`

Returns Basket parameters

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    GetBasket { basket_id: Uint128 }, 
}

pub struct BasketResponse{
    pub owner: String,
    pub basket_id: String,
    pub current_position_id: String,
    pub collateral_types: Vec<cAsset>, 
    pub collateral_supply_caps: Vec<Uint128>,
    pub credit_asset: Asset, 
    pub credit_price: String,
    pub credit_interest: String,
    pub debt_pool_ids: Vec<u64>,
    pub debt_liquidity_multiplier_for_caps: Decimal, //Ex: 5 = debt cap at 5x liquidity.
    pub liq_queue: String,
    pub base_interest_rate: Decimal, //Enter as percent, 0.02
    pub desired_debt_cap_util: Decimal, //Enter as percent, 0.90
    pub pending_revenue: Uint128, 
}
```

| Key         | Type    | Description           |
| ----------- | ------- | --------------------- |
| `basket_id` | Uint128 | ID of Basket to parse |

### `GetAllBaskets`

Returns parameters for all Baskets with optional limiters

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    GetAllBaskets { 
        start_after: Option<Uint128>,
        limit: Option<u32>, 
    }, 
}

pub struct BasketResponse{
    pub owner: String,
    pub basket_id: String,
    pub current_position_id: String,
    pub collateral_types: Vec<cAsset>, 
    pub credit_asset: Asset, 
    pub credit_price: String,
    pub credit_interest: String,
}
```

| Key            | Type    | Description                 |
| -------------- | ------- | --------------------------- |
| `*start_after` | Uint128 | User address to start after |
| `*limit`       | u32     | Basket limit                |

&#x20;\* = optional

### `GetBasketDebtCaps`

Returns a basket's debt caps per collateral asset, calculates on every call

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    GetBasketDebtCaps {
        basket_id: Uint128,
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct DebtCapResponse{
    pub caps: Vec<String>,
}
```

| Key         | Type    | Description   |
| ----------- | ------- | ------------- |
| `basket_id` | Uint128 | ID of basket  |

### `GetBasketBadDebt`

Returns a basket's bad debt

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    GetBasketBadDebt {
        basket_id: Uint128,
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct BadDebtResponse{
    pub has_bad_debt: Vec<( PositionUserInfo, Decimal )>,
}
```

| Key         | Type    | Description   |
| ----------- | ------- | ------------- |
| `basket_id` | Uint128 | ID of basket  |

### `GetBasketInsolvency`

Return's any insolvent positions in a basket

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    GetBasketInsolvency {
        basket_id: Uint128,
        start_after: Option<String>,
        limit: Option<u32>,
    }
}

pub struct InsolvencyResponse{
    pub insolvent_positions: Vec<InsolventPosition>,
}

pub struct InsolventPosition {
    pub insolvent: bool,
    pub position_info: UserInfo,
    pub current_LTV: Decimal,
    pub available_fee: Uint128,
}

pub struct UserInfo {
    pub basket_id: Uint128,
    pub position_id: Uint128,
    pub position_owner: String,
}
```

| Key            | Type    | Description                              |
| -------------- | ------- | ---------------------------------------- |
| `basket_id`    | Uint128 | ID of basket                             |
| `*start_after` | String  | Get responses starting after user        |
| `*limit`       | u32     | Limit the number of users parsed through |

&#x20;\* = optional

### `GetPositionInsolvency`

Returns a single position's insolvency info

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    GetPositionInsolvency {
        basket_id: Uint128,
        position_id: Uint128,
        position_owner: String,
    }
}

pub struct InsolvencyResponse{
    pub insolvent_positions: Vec<InsolventPosition>,
}
```

| Key              | Type    | Description           |
| ---------------- | ------- | --------------------- |
| `basket_id`      | Uint128 | Basket ID to query    |
| `position_id`    | Uint128 | Position ID to query  |
| `position_owner` | String  | Owner of the position |

### `Propagation`

Returns `RepayPropagation.`Used internally to test state propagation.

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Propagation {}
}

pub struct PropResponse {
    pub liq_queue_leftovers: Decimal,
    pub stability_pool: Decimal,
    pub sell_wall_distributions: Vec<SellWallDistribution>,
    pub positions_contract: String,
    //So the sell wall knows who to repay to
    pub position_id: Uint128,
    pub basket_id: Uint128,
    pub position_owner: String,
}

pub struct SellWallDistribution {
    pub distributions: Vec<( AssetInfo, Decimal )>,
}
```
