---
description: >-
  Inspired by Liquity's Stability Pool   -
  https://docs.liquity.org/faq/stability-pool-and-liquidations
---

# Stability Pool

The Stability Pool (SP) is the second line of defense in Positions contract liquidations. It acts as a pool of liquidity that can be used to repay insolvent positions in return for discounted collateral assets. \
\
When a position is liquidated, the pool repays its debt in exchange for assets sent by the Positions contract after successful repayments. In contrast to [Liquity's ](https://docs.liquity.org/faq/stability-pool-and-liquidations)pro rata model, this SP is **F**irst In **F**irst **O**ut when it comes to rewarding liquidations to pool liquidity providers.\
\
Due to how the [Liquidation Queue](liquidation-queue.md) calculates liquidations, there will always be something for the SP to liquidate, meaning its advantageous for the first bidder at every liquidation and not just the one's the Liq Queue can't fulfill. This has the added benefit of filtering through spam deposits before large liquidatiosn.\
\
Pro-rata distributions, like the Liq Queue and Liquity's SP are better than FIFO at attracting large capital, but FIFO has direct incentives for competitive replenishes which is better for a pool that isn't prioritized but needs quick refills if the situation calls for it.\
\
We want this step of the liquidation mechanism to be reactive when low while not taking too much potential capital from the Liq Queue which will likely liquidate collateral for lower premiums a majority of the time, which is better for [user solvency](https://twitter.com/euler\_mab/status/1537091423748517889).

**Note: Any user funds in the Stability Pool will be used to repay said user's positions if liquidated. Meaning depositing in the SP doesn't increase liquidation risk for the user.**

## InstantiateMsg

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub asset_pool: Option<AssetPool>,
    pub incentive_rate: Option<Decimal>,
    pub max_incentives: Option<Uint128>,
    pub desired_ratio_of_total_credit_supply: Option<Decimal>,
    pub osmosis_proxy: String,
    pub positions_contract: String,
    pub mbrn_denom: String,
    pub dex_router: Option<String>,
    pub max_spread: Option<Decimal>,
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
```

| Key                                     | Type      | Description                                       |
| --------------------------------------- | --------- | ------------------------------------------------- |
| `*asset_pool`                           | AssetPool | Initial Asset Pool for the contract               |
| `*owner`                                | String    | Owner of the contract, defaults to info.sender    |
| `*incentive_rate`                       | Decimal   | Base MBRN incentive rate                          |
| `*max_incentives`                       | Uint128   | Maximum MBRN the Pool can mint for incentives     |
| `*desired_ratio_of_total_credit_supply` | Decimal   | Desired ratio of credit (CDT) in the pool         |
| `osmosis_proxy`                         | String    | Osmosis Proxy contract address                    |
| `positions_contract`                    | String    | CDP contract                                      |
| `mbrn_denom`                            | String    | MBRN denom                                        |
| `*dex_router`                           | String    | DEX Router Contract                               |
| `*max_spread`                           | Decimal   | Max spread for claim\_as() swaps, defaults to 10% |

\* = optional

## ExecuteMsg

### `UpdateConfig`

Update Config if info.sender is config.owner

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    UpdateConfig {
        owner: Option<String>,
        incentive_rate: Option<Decimal>,
        max_incentives: Option<Uint128>,
        desired_ratio_of_total_credit_supply: Option<Decimal>,
        unstaking_period: Option<u64>,
        osmosis_proxy: Option<String>,
        positions_contract: Option<String>,
        mbrn_denom: Option<String>,
        dex_router: Option<String>,
        max_spread: Option<Decimal>,
    }
}
```

| Key                                     | Type     | Description                                   |
| --------------------------------------- | -------- | --------------------------------------------- |
| `*owner`                                | String   | Address of Owner of the contract              |
| `*incentive_rate`                       | Decimal  | Base MBRN incentive rate                      |
| `*max_incentives`                       | UIint128 | Maximum MBRN the Pool can mint for incentives |
| `*desired_ratio_of_total_credit_supply` | Decimal  | Desired ratio of credit (CDT) in the pool     |
| `*unstaking_period`                     | u64      | Unstaking period in days                      |
| `*osmosis_proxy`                        | String   | Osmosis Proxy contract address                |
| `*positions_contract`                   | String   | CDP contract                                  |
| `*mbrn_denom`                           | String   | MBRN denom                                    |
| `*dex_router`                           | String   | Dex Router contract                           |
| `*max_spread`                           | Decimal  | Max spread for ClaimAs swaps                  |

&#x20;\* = optional

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
    Deposit { 
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
    Withdraw { 
        assets: Vec<Asset>
    }
}
```

| Key      | Type        | Description            |
| -------- | ----------- | ---------------------- |
| `assets` | Vec\<Asset> | Assets to be withdrawn |

### `Restake`

Restake unstak(ed/ing) assets

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Restake { 
        restake_asset: LiqAsset,
    }
}
```

| Key             | Type     | Description       |
| --------------- | -------- | ----------------- |
| `restake_asset` | LiqAsset | Asset to restake  |

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

### `Config`

Returns the current Config fields

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {}
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub owner: Addr, //Positions contract address
    pub incentive_rate: Decimal,
    pub max_incentives: Uint128,
    //% of Supply desired in the SP. 
    //Incentives decrease as it gets closer
    pub desired_ratio_of_total_credit_supply: Decimal,
    pub unstaking_period: u64, // in days
    pub mbrn_denom: String,
    pub osmosis_proxy: Addr,
    pub positions_contract: Addr,
    pub dex_router: Option<Addr>,
    pub max_spread: Option<Decimal>, //max_spread for the router, mainly claim_as swaps
}
```

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
    pub deposit_time: u64,
    pub last_accrued: u64,
    pub unstake_time: Option<u64>,
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

### `Rate`

Returns current MBRN incentive rate

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Rate {
        asset_info: AssetInfo,
    }
}

//Returns Decimal
```

| Key          | Type      | Description                                          |
| ------------ | --------- | ---------------------------------------------------- |
| `asset_info` | AssetInfo | Asset info corresponding to an available Asset Pool  |

### `UnclaimedIncentives`

Returns unclaimed incentives for a user in an AssetPool

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    UnclaimedIncentives {
        user: String,
        asset_info: AssetInfo,
    }
}

//Returns Uint128
```

| Key          | Type      | Description                                          |
| ------------ | --------- | ---------------------------------------------------- |
| `user`       | String    | User address                                         |
| `asset_info` | AssetInfo | Asset info corresponding to an available Asset Pool  |

### `CapitalAheadOfDeposit`

Returns capital ahead of each user Deposit in an AssetPool

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    CapitalAheadOfDeposit {
        user: String,
        asset_info: AssetInfo,
    }
}

pub struct DepositPositionResponse {
    pub deposit: Deposit,
    pub capital_ahead: Decimal,
}

pub struct Deposit {
    pub user: Addr,
    pub amount: Decimal,
    pub deposit_time: u64,
    pub last_accrued: u64,
    pub unstake_time: Option<u64>,
}
```

| Key          | Type      | Description                                          |
| ------------ | --------- | ---------------------------------------------------- |
| `user`       | String    | User address                                         |
| `asset_info` | AssetInfo | Asset info corresponding to an available Asset Pool  |
