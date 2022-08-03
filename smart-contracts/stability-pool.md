---
description: >-
  Inspired by Liquity's Stability Pool   -
  https://docs.liquity.org/faq/stability-pool-and-liquidations
---

# Stability Pool

The Stability Pool (SP) is the second line of defense in Positions contract liquidations. It acts as a pool of liquidity that can be used to repay insolvent positions in return for discounted collateral assets. \
\
When a position is liquidated, the pool repays its debt in exchange for assets sent by the Positions contract for successful repayments. In contrast to Liquity's pro rata model, this SP is **F**irst In **F**irst **O**ut when it comes to rewarding liquidations to pool liquidity providers.\
\
Due to how the liquidation model calculates liquidations, there will always be something for the SP to liquidate, meaning its advantageous for the first bidder at every liquidation and not just the one's the Liq Queue can't fulfill.\
\
Pro-rata distributions, like the Liq Queue and Liquity's SP are better than FIFO at attracting large capital, but FIFO has direct incentives for competitive replenishes which is better for a pool that isn't prioritized but needs quick refills in a situation its primarily used.\
\
We want this phase of the mechanism to be reactive when low while not taking too much potential capital from the Liq Queue which will likely liquidate collateral for lower premiums a majority of the time, which is better for the users.

## InitMsg

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub asset_pool: Option<AssetPool>,
    pub owner: Option<String>,
}

pub struct AssetPool {
    pub credit_asset: Asset,
    pub liq_premium: Decimal,
    pub deposits: Vec<Deposit>
}

pub struct Asset{
    pub info: AssetInfo,
    pub amount: Uint128,
}

pub struct Deposit {
    pub user: Addr,
    pub amount: Decimal,
}
```

| Key           | Type      | Description                                    |
| ------------- | --------- | ---------------------------------------------- |
| `*asset_pool` | AssetPool | Initial Asset Pool for the contract            |
| `*owner`      | String    | Owner of the contract, defaults to info.sender |

\* = optional

## ExecuteMsg

### `Receive`

Can be called during a CW20 token transfer when the Positions contract is the recipient. Allows the token transfer to execute a [Receive Hook](stability-pool.md#receive-hook) as a subsequent action within the same transaction.

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

| Key      | Type    | Description                                                                  |
| -------- | ------- | ---------------------------------------------------------------------------- |
| `sender` | String  | Sender of the token transfer                                                 |
| `amount` | Uint128 | Amount of tokens received                                                    |
| `msg`    | Binary  | Base64-encoded string of JSON of [Receive Hook](stability-pool.md#undefined) |

### `Deposit`

Deposit accepted credit assets to corresponding Asset Pools

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Deposit { //Deposit a list of accepted assets
        user: Option<String>
    }
}
```

| Key     | Type   | Description                            |
| ------- | ------ | -------------------------------------- |
| `*user` | String | Address with claim over the deposit(s) |

\* = optional

### `Withdraw`

Withdraw caller owned deposits from corresponding Asset Pools

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Withdraw { //Withdraw a list of accepted assets 
        assets: Vec<Asset>
    }
}
```

| Key      | Type        | Description            |
| -------- | ----------- | ---------------------- |
| `assets` | Vec\<Asset> | Assets to be withdrawn |

### `Liquidate`

Use Asset Pool assets to repay for a [Position ](positions.md#getposition)and earn discounted assets

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Liquidate { //Use assets from an Asset pool to liquidate for a Position (Positions Contract)
        credit_asset: LiqAsset
    }
}

pub struct LiqAsset {
    pub info: AssetInfo,
    pub amount: Decimal,
}
```

| Key            | Type     | Description        |
| -------------- | -------- | ------------------ |
| `credit_asset` | LiqAsset | Asset to be repaid |

### `Claim`

Claim all discounted assets received from liquidations, for the caller&#x20;

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Claim { 
        claim_as_native: Option<String>,
        claim_as_cw20: Option<String>,
        deposit_to: Option<PositionUserInfo>, 
    }
}

pub struct PositionUserInfo{
    pub basket_id: Uint128,
    pub position_id: Uint128,
}
```

| Key                | Type             | Description                                               |
| ------------------ | ---------------- | --------------------------------------------------------- |
| `*claim-as-native` | String           | Claim all assets as a native token                        |
| `*claim-as-cw20`   | String           | Claim all assets as a CW20 token                          |
| `*deposit_to`      | PositionUserInfo | Deposit to Position in [Positions ](positions.md)contract |

\* = optional

### `AddPool`

Contract owner can add an Asset Pool&#x20;

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    AddPool { //Adds an asset pool 
        asset_pool: AssetPool 
    }
}
```

| Key          | Type      | Description                                           |
| ------------ | --------- | ----------------------------------------------------- |
| `asset_pool` | AssetPool | AssetPool object to add to state as an available pool |

### `Distribute`

Called by the Positions contract. Distributes liquidated funds to the users whose Deposits were used to repay the debt.

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Distribute { //Distributes liquidated funds to users
        distribution_assets: Vec<Asset>,
        distribution_asset_ratios: Vec<Decimal>,
        credit_asset: AssetInfo,
        distribute_for: Uint128,
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

| Key                         | Type          | Description                                                      |
| --------------------------- | ------------- | ---------------------------------------------------------------- |
| `distribution_assets`       | Vec\<Asset>   | Assets to be distributed to users                                |
| `distribution-asset-ratios` | Vec\<Decimal> | Ratios of distribution assets                                    |
| `credit_asset`              | AssetInfo     | AssetInfo corresponding to the Asset Pool that was used to repay |
| `credit_price`              | Decimal       | Redemption price of `credit_asset`                               |

## Receive Hook

### `Distribute`

Called by the Positions contract. Distributes liquidated funds to the users whose Deposits were used to repay the debt.

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cw20HookMsg {
    Distribute { //Distributes liquidated funds to users
        distribution_assets: Vec<Asset>,
        distribution_asset_ratios: Vec<Decimal>,
        credit_asset: AssetInfo,
        distribute_for: Uint128,
    } 
} 

```

| Key                         | Type          | Description                                                      |
| --------------------------- | ------------- | ---------------------------------------------------------------- |
| `distribution_assets`       | Vec\<Asset>   | Assets to be distributed to users                                |
| `distribution-asset-ratios` | Vec\<Decimal> | Ratios of distribution assets                                    |
| `credit_asset`              | AssetInfo     | AssetInfo corresponding to the Asset Pool that was used to repay |
| `credit_price`              | Decimal       | Redemption price of `credit_asset`                               |

## QueryMsg

### `CheckLiquidatible`

Returns the amount of said asset that isn't liquidatible (i.e. leftover)

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    CheckLiquidatible { 
        asset: LiqAsset 
    }
}

pub struct LiquidatibleResponse {
    pub leftover: Decimal,
}
```

| Key     | Type     | Description                        |
| ------- | -------- | ---------------------------------- |
| `asset` | LiqAsset | Asset info and amount to check for |

### `AssetDeposits`

Returns User deposits in a single Asset Pool

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    AssetDeposits{ 
        user: String, 
        asset_info: AssetInfo 
    }
}

pub struct DepositResponse {
    pub asset: AssetInfo,
    pub deposits: Vec<Deposit>,
}

pub struct Deposit {
    pub user: Addr,
    pub amount: Decimal,
}
```

| Key          | Type      | Description                             |
| ------------ | --------- | --------------------------------------- |
| `user`       | String    | User whose deposits to query            |
| `asset_info` | AssetInfo | Asset info of the desired pool to query |

### `UserClaims`

Returns the `user`'s claimable assets

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    UserClaims{ 
        user: String 
    }
}

pub struct ClaimsResponse {
    pub claims: Vec<Asset>,
}
```

| Key    | Type   | Description                    |
| ------ | ------ | ------------------------------ |
| `user` | String | The user whose claims to check |

### `AssetPool`

Returns Asset Pool info

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    AssetPool{ 
        asset_info: AssetInfo 
    }
}

pub struct PoolResponse {
    pub credit_asset: Asset,
    pub liq_premium: Decimal,
    pub deposits: Vec<Deposit>
}
```

| Key          | Type      | Description                                          |
| ------------ | --------- | ---------------------------------------------------- |
| `asset_info` | AssetInfo | Asset info corresponding to an available Asset Pool  |