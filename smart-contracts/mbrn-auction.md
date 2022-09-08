# MBRN Auction

## InstantiateMsg

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub oracle_contract: String,
    pub osmosis_proxy: String,
    pub positions_contract: String,
    pub twap_timeframe: u64,
    pub mbrn_denom: String,
    pub initial_discount: Decimal,
    pub discount_increase_timeframe: u64,
    pub discount_increase: Decimal,
}
```

| Key                           | Type    | Description                                                    |
| ----------------------------- | ------- | -------------------------------------------------------------- |
| `*owner`                      | String  | Contract owner                                                 |
| `oracle_contract`             | String  | Oracle contract address                                        |
| `osmosis_proxy`               | String  | Osmosis Proxy address                                          |
| `positions_contract`          | String  | Position's contract address                                    |
| `twap_timeframe`              | u64     | Timeframe for TWAPs                                            |
| `mbrn_denom`                  | String  | MBRN token full denom                                          |
| `initial_discount`            | Decimal | Starting discount of auctions                                  |
| `discount_increase_timeframe` | u64     | Timeframe in which the discount is increased                   |
| `discount_increase`           | Decimal | Increase in discount per unit of discount\_increase\_timeframe |

&#x20;\* = optional

## ExecuteMsg

### `StartAuction`

Start or add to existing auction

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    StartAuction {
        repayment_position_info: UserInfo,
        debt_asset: Asset,
    }
}

pub struct UserInfo {
    pub basket_id: Uint128,
    pub position_id: Uint128,
    pub position_owner: String,
}

pub struct Asset{
    pub info: AssetInfo,
    pub amount: Uint128,
}
```

| Key                       | Type     | Description                           |
| ------------------------- | -------- | ------------------------------------- |
| `repayment_position_info` | UserInfo | Position info that holds the bad debt |
| `debt_asset`              | Asset    | Asset info + amount to swap for MBRN  |

### `SwapForMBRN`

Swap for MBRN with any open auction's debt\_asset

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    SwapForMBRN { }
}
```

### `UpdateConfig`

Update contract configurations

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    UpdateConfig {
        owner: Option<String>,
        oracle_contract: Option<String>,
        osmosis_proxy: Option<String>,
        mbrn_denom: Option<String>,
        positions_contract: Option<String>,
        twap_timeframe: Option<u64>,
        initial_discount: Option<Decimal>,
        discount_increase_timeframe: Option<u64>, 
        discount_increase: Option<Decimal>, 
    },
}
```

| Key                            | Type    | Description                                                     |
| ------------------------------ | ------- | --------------------------------------------------------------- |
| `*owner`                       | String  | Contract owner                                                  |
| `*oracle_contract`             | String  | Oracle contract address                                         |
| `*osmosis_proxy`               | String  | Osmosis Proxy address                                           |
| `*mbrn_denom`                  | String  | MBRN denom                                                      |
| `*positions_contract`          | String  | Positions contract address                                      |
| `*twap_timeframe`              | u64     | TWAP timeframe for oracle queries                               |
| `*initial_discount`            | Decimal | Initial auction MBRN price discount                             |
| `*discount_increase_timeframe` | u64     | Timeframe in which the multiple of discount\_increase increases |
| `*discount_increase`           | Decimal | Increase in discount per unit of discount\_increase\_timeframe  |

&#x20;\* = optional

## QueryMsg

### `Config`

Return Config

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {}
}

pub struct Config {    
    pub owner: Addr,
    pub oracle_contract: Addr,
    pub osmosis_proxy: Addr,
    pub mbrn_denom: String,
    pub positions_contract: Addr,
    pub twap_timeframe: u64,
    pub initial_discount: Decimal,
    pub discount_increase_timeframe: u64, //in seconds
    pub discount_increase: Decimal, //% increase
}
```

### `OngoingAuctions`

Returns list of ongoing Auctions

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    OngoingAuctions {
        debt_asset: Option<AssetInfo>,
        limit: Option<u64>,
        start_without: Option<AssetInfo>,
    }
}

//Returns list of AuctionResponse
pub struct AuctionResponse {
    pub remaining_recapitalization: Uint128,
    pub repayment_positions: Vec<RepayPosition>,  
    pub auction_start_time: u64,
    pub basket_id_price_source: Uint128,
}

pub struct RepayPosition {  
    pub repayment: Uint128,
    pub position_info: UserInfo,
}
```

| Key              | Type      | Description                               |
| ---------------- | --------- | ----------------------------------------- |
| `*debt_asset`    | AssetInfo | Specific auction to return, list of 1     |
| `*limit`         | u64       | Limit to the amount of returned responses |
| `*start_without` | AssetInfo | Asset to filter out of responses          |

&#x20;\* = optional

### `ValidDebtAssets`&#x20;

Returns assets that have or have had auctions

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    ValidDebtAssets {
        debt_asset: Option<AssetInfo>,
        limit: Option<u64>,
        start_without: Option<AssetInfo>,
    }
}

//Returns list of AssetInfo
pub enum AssetInfo {
    Token{
        address: Addr,
    },
    NativeToken{
        denom: String,
    },
}
```

| Key              | Type      | Description                            |
| ---------------- | --------- | -------------------------------------- |
| `*debt_asset`    | AssetInfo | Specific asset to check for, list of 1 |
| `*limit`         | u64       | Limit to the amount of returned assets |
| `*start_without` | AssetInfo | Asset to filter out of responses       |

&#x20;\* = optional
