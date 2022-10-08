---
description: MBRN Staking contract
---

# Staking

* The Positions contract uses this contract's [DepositFee ](staking.md#depositfee)to allocate liquidation fees to stakers staked at the time of the liquidation.&#x20;
* Rewards are earned in the unstaking period but have no voting power, as its primary use is to restrict sales prompted by an activated Debt Auction.&#x20;
* Stakers can restake after starting to unstake if **MBRN** hasn't been withdrawn.&#x20;
* The [Builder's Vesting](<staking (1).md>) contract doesn't receive inflationary **MBRN** rewards.
* Because [Governance ](gov.md)can submit arbitrary messages, the staking rate is hard capped at 20%&#x20;

## InstantiateMsg

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub positions_contract: Option<String>,
    pub builders_contract: Option<String>,
    pub osmosis_proxy: Option<String>,    
    pub staking_rate: Option<Decimal>,
    pub fee_wait_period: Option<u64>,  
    pub unstaking_period: Option<u64>,    
    pub mbrn_denom: String,
    pub dex_router: Option<String>,
    pub max_spread: Option<Decimal>,
}
```

| Key                   | Type    | Description                                            |
| --------------------- | ------- | ------------------------------------------------------ |
| `*owner`              | String  | Contract owner                                         |
| `*positions_contract` | String  | Positions contract address                             |
| `*builders_contract`  | String  | Builder's Vesting contract address                     |
| `*osmosis_proxy`      | String  | Osmosis Proxy contract address                         |
| `*staking_rate`       | Decimal | Desired staking rate, defaults to 10%                  |
| `*fee_wait_period`    | u64     | Waiting period before stakers earn fees from FeeEvents |
| `*unstaking_period`   | u64     | Unstaking period in days, defaults to 3 days           |
| `mbrn_denom`          | String  | MBRN full denom                                        |
| `*dex_router`         | String  | DEX Router contract address                            |
| `*max_spread`         | Decimal | Max spread for asset routing, defaults to 10%          |

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
        fee_wait_period: Option<u64>,     
        unstaking_period: Option<u64>,            
        dex_router: Option<String>,
        max_spread: Option<Decimal>,
    }
}
```

| Key                   | Type    | Description                                            |
| --------------------- | ------- | ------------------------------------------------------ |
| `*owner`              | String  | Contract owner                                         |
| `*positions_contract` | String  | Positions contract address                             |
| `*builders_contract`  | String  | Builder's Vesting contract address                     |
| `*osmosis_proxy`      | String  | Osmosis Proxy contract address                         |
| `*staking_rate`       | Decimal | Desired staking rate                                   |
| `*fee_wait_period`    | u64     | Waiting period before stakers earn fees from FeeEvents |
| `*unstaking_period`   | u64     | Unstaking period in days                               |
| `*mbrn_denom`         | String  | MBRN full denom                                        |
| `*dex_router`         | String  | DEX Router contract address                            |
| `*max_spread`         | Decimal | Max spread for asset routing                           |

&#x20;\* = optional

### `Stake`

Stake MBRN for user

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Stake { 
        user: Option<String>,
    }
}
```

| Key     | Type   | Description                                |
| ------- | ------ | ------------------------------------------ |
| `*user` | String | User to stake for, defaults to info.sender |

&#x20;\* = optional

### `Unstake`

Withdraw desired stake for info.sender

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Unstake { 
        mbrn_amount: Option<Uint128>,
    }
}
```

| Key            | Type    | Description                                     |
| -------------- | ------- | ----------------------------------------------- |
| `*mbrn_amount` | Uint128 | MBRN amount to unstake, defaults to total stake |

&#x20;\* = optional

### `Restake`

Restake unstak(ed/ing) MBRN

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Restake { 
        mbrn_amount: Uint128,
    }
}
```

| Key           | Type    | Description            |
| ------------- | ------- | ---------------------- |
| `mbrn_amount` | Uint128 | MBRN amount to restake |

### `ClaimRewards`

Claim all staking rewards for info.sender

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    ClaimRewards { 
        claim_as_native: Option<String>, //Native FullDenom
        claim_as_cw20: Option<String>, //Contract Address
        send_to: Option<String>,
        restake: bool,
    }
}
```

| Key                | Type   | Description                              |
| ------------------ | ------ | ---------------------------------------- |
| `*claim_as_native` | String | Native token full denom to claim fees as |
| `*claim_as_cw20`   | String | Cw20 token address to claim fees as      |
| `*send_to`         | String | Address to send rewards to               |
| `restake`          | bool   | Restake MBRN toggle                      |

&#x20;\* = optional

### `DepositFee`

Positions contract deposit's liquidation fees to be distributed to stakers

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    DepositFee { }
}
```

## ReceiveHook

### `DepositFee`

Positions contract deposit's liquidation fees to be distributed to stakers

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cw20HookMsg {
    DepositFee { }
}
```

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
    pub positions_contract: String,
    pub builders_contract: String,
    pub osmosis_proxy: String,    
    pub staking_rate: String,
    pub fee_wait_period: String,
    pub unstaking_period: String,    
    pub mbrn_denom: String,
    pub dex_router: String,
    pub max_spread: String, 
}
```

### `UserStake`

Returns Staker information

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    UserStake { 
        staker: String,    
    }
}

pub struct StakerResponse {
    pub staker: String,
    pub total_staked: Uint128,
    pub deposit_list: Vec<(String, String)>, //Amount and timestamp of each deposit
}
```

| Key      | Type   | Description      |
| -------- | ------ | ---------------- |
| `staker` | String | Staker's address |

### `StakerRewards`

Returns Staker rewards

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    StakerRewards { 
        staker: String,    
    }
}

pub struct RewardsResponse {
    pub claimables: Vec<Asset>,
    pub accrued_interest: Uint128,
}
```

| Key      | Type   | Description      |
| -------- | ------ | ---------------- |
| `staker` | String | Staker's address |

### `Staked`

Returns list of all Staked

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Staked {
        limit: Option<u32>,
        start_after: Option<u64>, //Timestamp in seconds
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct StakedResponse {
    pub stakers: Vec<StakeDeposit>,
}

pub struct StakeDeposit {
    pub staker: Addr,
    pub amount: Uint128,
    pub deposit_time: u64,
}
```

| Key            | Type | Description                         |
| -------------- | ---- | ----------------------------------- |
| `*limit`       | u32  | Limit # of entries returned         |
| `*start_after` | u64  | Start after a block time in seconds |

&#x20;\* = optional

### `FeeEvents`

Returns list of all Fee Events

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    FeeEvents {
        limit: Option<u32>,
        start_after: Option<u64>, //Timestamp in seconds
    }
}
```

| Key            | Type | Description                         |
| -------------- | ---- | ----------------------------------- |
| `*limit`       | u32  | Limit # of entries returned         |
| `*start_after` | u64  | Start after a block time in seconds |

&#x20;\* = optional

### `TotalStaked`

Returns total MBRN staked

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    TotalStaked { }
}

pub struct TotalStakedResponse {
    pub total_not_including_builders: String,
    pub builders_total: String,
}
```
