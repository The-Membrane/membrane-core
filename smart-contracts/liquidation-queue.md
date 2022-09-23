---
description: Fork of Anchor Protocol's implementation with slight modifications
---

# Liquidation Queue

\
The Liquidation contract enables users to submit CDP token bids for a Cw20 or native sdk token. Bidders can submit a bid to one of the bid pools; each of the pools deposited funds are used to buy the liquidated collateral at different discount rates. There are 21 slots per collateral, from 0% to 20%; users can bid on one or more slots.

Upon execution of a bid, collateral tokens are allocated to the bidder, while the bidder's bid tokens are sent to the repay the liquidated position.

Bids are consumed from the bid pools in increasing order of premium rate (e.g 2% bids are only consumed after 0% and 1% pools are emptied). The liquidated collateral is then allocatedto the bidders in the affected pools in proportion to their bid amount. The respective collateral should be claimed by the bidders.

To prevent bots from sniping loans, submitted bids are only activated after `wait_period` has expired, unless the total bid amount falls under the `bid_threshold`, in which case bids will be directly activated upon submission.

### Source

[https://docs.anchorprotocol.com/smart-contracts/liquidations/liquidation-queue-contract](https://docs.anchorprotocol.com/smart-contracts/liquidations/liquidation-queue-contract)\
[https://github.com/Anchor-Protocol/money-market-contracts/tree/main/contracts/liquidation\_queue](https://github.com/Anchor-Protocol/money-market-contracts/tree/main/contracts/liquidation\_queue)

### Modifications

* Automatic activation after `wait_period` elaspes. This increases computation time in return for less reliance on external contract calls.
* Liquidations send the [RepayMsg ](positions.md#repay)for the position in the Positions contract
* Prices are taken from input by the Positions contract, the messages are guaranteed the same block so the price will be block\__time +_[ __ Position's config](positions.md#config) oracle\__time\__limit second's old.
* The position is assumed insolvent since called by the Positions contract, ie there is no additional solvency check in this contract.
* ExecuteMsg::Liquidate doesn't take any assets up front, instead receiving assets in the Reply fn of the [Positions ](positions.md)contract

## InstantiateMsg

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub positions_contract: String,
    pub waiting_period: u64, //seconds
    pub basket_id: Option<Uint128>,
    pub bid_asset: Option<AssetInfo>,
}
```

| Key                  | Type      | Description                                    |
| -------------------- | --------- | ---------------------------------------------- |
| `*owner`             | String    | Owner of the contract, defaults to info.sender |
| `positions_contract` | String    | CDP contract                                   |
| `waiting_period`     | u64       | Waiting period for bids (secs)                 |
| `*basket_id`         | Uint128   | Basket ID for contract's bid\_asset            |
| `*bid_asset`         | AssetInfo | Bid asset direct input                         |

&#x20;\* = optional

## ExecuteMsg

### `Receive`

Used to receive cw20 tokens

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Receive ( Cw20ReceiveMsg )
}
```

### `SubmitBid`

Submit a Bid alongside an accepted native SDK asset to a Queue and corresponding PremiumSlot

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    SubmitBid { //Deposit a list of accepted assets
        bid_input: BidInput,
        bid_owner: Option<String>,
    }
}

pub struct BidInput{
    pub bid_for: AssetInfo,
    pub liq_premium: u8, //Premium within range of Queue
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

| Key          | Type     | Description                                              |
| ------------ | -------- | -------------------------------------------------------- |
| `bid_input`  | BidInput | Information on what asset to bid for and at what premium |
| `*bid_owner` | String   | Owner of the bid, defaults to info.sender                |

&#x20;\* = optional

### `RetractBid`

Withdraw bid

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    RetractBid { //Withdraw a list of accepted assets 
        bid_id: Uint128,
        bid_for: AssetInfo,
        amount: Option<Uint256>, //If none, retracts full bid
    }
}
```

| Key       | Type      | Description                  |
| --------- | --------- | ---------------------------- |
| `bid_id`  | Uint128   | ID of bid to withdraw        |
| `bid_for` | AssetInfo | Asset queue to withdraw from |
| `*amount` | Uint256   | Amount to withdraw           |

&#x20;\* = optional

### `Liquidate`

Repays for a Position and earns discounted collateral for the bidders. Only used by the owner (ie Positions contract)

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Liquidate { //Use bids to fulfll liquidation of Position Contract basket. Called by Positions
    //From Positions Contract
        credit_price: Decimal, 
        collateral_price: Decimal,
        collateral_amount: Uint256,
        bid_for: AssetInfo,
        bid_with: AssetInfo,  
    //For Repayment 
        basket_id: Uint128,
        position_id: Uint128,
        position_owner: String, 
    }
}
```

| Key                 | Type      | Description                    |
| ------------------- | --------- | ------------------------------ |
| `credit_price`      | Decimal   | Credit repayment price         |
| `collateral_price`  | Decimal   | Collateral TWAP price          |
| `collateral_amount` | Uint256   | Collateral amount to liquidate |
| `bid_for`           | AssetInfo | Collateral asset info          |
| `bid_with`          | AssetInfo | Bid asset info                 |
| `basket_id`         | Uint128   | Position Info                  |
| `position_id`       | Uint128   | Position Info                  |
| `position_owner`    | String    | Position Info                  |

### `ClaimLiquidations`

Claim liquidations for a list of bids

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    ClaimLiquidations {
        bid_for: AssetInfo,
        bid_ids: Option<Vec<Uint128>>, //None = All bids in the queue
    }
}
```

| Key        | Type          | Description                                                             |
| ---------- | ------------- | ----------------------------------------------------------------------- |
| `bid_for`  | AssetInfo     | Info of asset queue to claim from                                       |
| `*bid_ids` | Vec\<Uint128> | List of bids to claim from, if `None` it claims all available user bids |

&#x20;\* = optional

### `AddQueue`

Add Queue to the contract

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    AddQueue{    
        bid_for: AssetInfo,
        bid_asset: AssetInfo, //This should always be the same credit_asset but will leave open for flexibility
        max_premium: Uint128, //A slot for each premium is created when queue is created
        bid_threshold: Uint256, //Minimum bid amount. Unlocks waiting bids if total_bids is less than.
    }
}
```

| Key             | Type      | Description                                               |
| --------------- | --------- | --------------------------------------------------------- |
| `bid_for`       | AssetInfo | Asset to bid for                                          |
| `bid_asset`     | AssetInfo | Asset to denominate slots in                              |
| `max_premium`   | Uint128   | Max premium for the Queue's range                         |
| `bid_threshold` | Uint256   | Minimum total bids before waiting bids unlock immediately |

### `UpdateQueue`

Update existing Queue

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    UpdateQueue{
        bid_for: AssetInfo, //To signla which queue to edit. You can't edit the bid_for asset.
        max_premium: Option<Uint128>, 
        bid_threshold: Option<Uint256>, 
    },

```

| Key              | Type      | Description                                               |
| ---------------- | --------- | --------------------------------------------------------- |
| `bid_for`        | AssetInfo | Asset to bid for                                          |
| `*max_premium`   | Uint128   | Max premium for the Queue's range                         |
| `*bid_threshold` | Uint256   | Minimum total bids before waiting bids unlock immediately |

&#x20;\* = optional

### UpdateConfig

Update Config

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    UpdateConfig{
        owner: Option<String>,
        positions_contract: Option<String>,
        waiting_period: Option<u64>,
        basket_id: Option<Uint128>,
    }
}
```

| Key                   | Type    | Description                         |
| --------------------- | ------- | ----------------------------------- |
| `*owner`              | String  | Owner of the contract               |
| `*positions_contract` | String  | CDP contract                        |
| `*waiting_period`     | u64     | Bid waiting period in seconds       |
| `*basket_id`          | Uint128 | Basket ID for contract's bid\_asset |

&#x20;\* = optional

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
    pub waiting_period: u64,
    pub added_assets: Vec<AssetInfo>,
}
```

### `Bid`

Returns a Bid&#x20;

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Bid {
        bid_for: AssetInfo, 
        bid_id: Uint128, 
    }
}

pub struct BidResponse {
    pub user: String,
    pub id: Uint128,
    pub amount: Uint256,
    pub liq_premium: u8,
    pub product_snapshot: Decimal256,
    pub sum_snapshot: Decimal256,
    pub pending_liquidated_collateral: Uint256,
    pub wait_end: Option<u64>,
    pub epoch_snapshot: Uint128,
    pub scale_snapshot: Uint128,
}
```

| Key       | Type      | Description          |
| --------- | --------- | -------------------- |
| `bid_for` | AssetInfo | Asset the bid is for |
| `bid_id`  | Uint128   | Bid ID               |

### `BidsByUser`

Returns Bids by user in a Queue

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    BidsByUser{
        bid_for: AssetInfo,
        user: String,
        limit: Option<u8>,
    }
}

pub struct BidResponse {
    pub user: String,
    pub id: Uint128,
    pub amount: Uint256,
    pub liq_premium: u8,
    pub product_snapshot: Decimal256,
    pub sum_snapshot: Decimal256,
    pub pending_liquidated_collateral: Uint256,
    pub wait_end: Option<u64>,
    pub epoch_snapshot: Uint128,
    pub scale_snapshot: Uint128,
}
```

| Key       | Type      | Description                    |
| --------- | --------- | ------------------------------ |
| `bid_for` | AssetInfo | Asset the Queue is bidding for |
| `user`    | String    | Bid owner                      |
| `*limit`  | u8        | Limit to returned bids         |

&#x20;\* = optional

### `Queue`

Returns Queue&#x20;

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Queue {
        bid_for: AssetInfo,
    }
}

pub struct QueueResponse {
    pub bid_asset: String,
    pub max_premium: String, 
    pub slots: Vec<PremiumSlot>,
    pub current_bid_id: String,
    pub bid_threshold: String,
}

pub struct PremiumSlot {
    pub bids: Vec<Bid>,
    pub liq_premium: Decimal256, //
    pub sum_snapshot: Decimal256,
    pub product_snapshot: Decimal256,
    pub total_bid_amount: Uint256,
    pub last_total: u64, //last time the bids have been totaled
    pub current_epoch: Uint128,
    pub current_scale: Uint128,
    pub residue_collateral: Decimal256,
    pub residue_bid: Decimal256,
}
```

| Key       | Type      | Description                    |
| --------- | --------- | ------------------------------ |
| `bid_for` | AssetInfo | Asset the Queue is bidding for |

### Queues

Returns Queues

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Queues{
        start_after: Option<AssetInfo>,
        limit: Option<u8>,
    }
}

pub struct QueueResponse {
    pub bid_asset: String,
    pub max_premium: String, 
    pub slots: Vec<PremiumSlot>,
    pub current_bid_id: String,
    pub bid_threshold: String,
}
```

| Key            | Type      | Description                        |
| -------------- | --------- | ---------------------------------- |
| `*start_after` | AssetInfo | Asset Queue to start after         |
| `*limit`       | u8        | Limit to # of users parsed through |

&#x20;\* = optional

### `CheckLiquidatible`

Check if collateral amount is liquidatible

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    //Check if the amount of said asset is liquidatible
    //Position's contract is sending its basket.credit_price
    CheckLiquidatible { 
        bid_for: AssetInfo,
        collateral_price: Decimal,
        collateral_amount: Uint256,
        credit_info: AssetInfo,
        credit_price: Decimal,
    }
}

pub struct LiquidatibleResponse {
    pub leftover_collateral: String,
    pub total_credit_repaid: String,
}

```

| Key                 | Type      | Description                       |
| ------------------- | --------- | --------------------------------- |
| `bid_for`           | AssetInfo | Asset the Queue is bidding for    |
| `collateral_price`  | Decimal   | Price of collateral being bid for |
| `collateral_amount` | Uint256   | Collateral amount                 |
| `credit_info`       | AssetInfo | Asset being bid with              |
| `credit_price`      | Decimal   | Bid\_with price                   |

### `UserClaims`

Returns a users claimable assets

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
//Check if user has any claimable assets
    UserClaims { user: String }
}

pub struct ClaimsResponse {
    pub bid_for: String,
    pub pending_liquidated_collateral: Uint256
}
```

| Key    | Type     | Description            |
| ------ | -------- | ---------------------- |
| `user` | `String` | User's claims to check |

### `PremiumSlot`

Returns info for a Queue's PremiumSlot

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    PremiumSlot { 
        bid_for: AssetInfo, 
        premium: u64, //Taken as %. 50 = 50%
    }
} 

pub struct SlotResponse {
    pub bids: Vec<Bid>,
    pub liq_premium: String,
    pub sum_snapshot: String,
    pub product_snapshot: String,
    pub total_bid_amount: String,
    pub current_epoch: Uint128,
    pub current_scale: Uint128,
    pub residue_collateral: String,
    pub residue_bid: String,
}

pub struct Bid {
    pub user: Addr,
    pub id: Uint128,
    pub amount: Uint256,
    pub liq_premium: u8,
    pub product_snapshot: Decimal256,
    pub sum_snapshot: Decimal256,
    pub pending_liquidated_collateral: Uint256,
    pub wait_end: Option<u64>,
    pub epoch_snapshot: Uint128,
    pub scale_snapshot: Uint128,
}
```

| Key       | Type      | Description                    |
| --------- | --------- | ------------------------------ |
| `bid_for` | AssetInfo | Asset the Queue is bidding for |
| `premium` | u64       | Which premium slot to query    |

### PremiumSlots

Returns all of a Queue's PremiumSlots

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    PremiumSlots { 
        bid_for: AssetInfo, 
        start_after: Option<u64>, //Start after a premium value taken as a %.( 50 = 50%)
        limit: Option<u8>,
    }
}

pub struct SlotResponse {
    pub bids: Vec<Bid>,
    pub liq_premium: String,
    pub sum_snapshot: String,
    pub product_snapshot: String,
    pub total_bid_amount: String,
    pub current_epoch: Uint128,
    pub current_scale: Uint128,
    pub residue_collateral: String,
    pub residue_bid: String,
}
```

| Key            | Type      | Description                    |
| -------------- | --------- | ------------------------------ |
| `bid_fpr`      | AssetInfo | Asset the Queue is bidding for |
| `*start_after` | AssetInfo | Asset Queue to start after     |
| `*limit`       | u8        | Limit to # of slots returned   |
