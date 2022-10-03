---
description: Contract that holds logic for vested MBRN in the builder's allocation
---

# Builder's Vesting

Builder's staked allocations can vote with a Governance determined % of their voting power and receive normal revenue minus **MBRN** inflationary rewards. Voting and proposal creation by allocaiton receivers is done through this contract.

## InstantiateMsg

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub initial_allocation: Uint128,
    pub mbrn_denom: String,
    pub osmosis_proxy: String,
    pub staking_contract: String,
}
```

| Key                  | Type    | Description                     |
| -------------------- | ------- | ------------------------------- |
| `*owner`             | String  | Owner of contract               |
| `initial_allocation` | Uint129 | Builder's Allocation            |
| `mbrn_denom`         | String  | MBRN token denom                |
| `osmosis_proxy`      | String  | Osmosis Proxy  contract address |
| `staking_contract`   | String  | MBRN Staking contract address   |

&#x20;\* = optional

## ExecuteMsg

### `Receive`

Used to Receive Cw20 tokens

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Receiver( Cw20ReceiveMsg )
}

pub struct Cw20ReceiveMsg {
    pub sender: String,
    pub amount: Uint128,
    pub msg: Binary,
}
```

### `AddReceiver`

Add address eligible to receiver a MBRN allocation

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    AddReceiver {
        receiver: String,
    }
}
```

| Key        | Type   | Decription          |
| ---------- | ------ | ------------------- |
| `receiver` | String | Address of Receiver |

### `RemoveReceiver`

Remove Receiver and any allocations

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    RemoveReceiver {
        receiver: String,
    }
}
```

| Key        | Type   | Description         |
| ---------- | ------ | ------------------- |
| `receiver` | String | Address of Receiver |

### `AddAllocation`

Add MBRN allocation to an eligible Receiver

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    AddAllocation {
        receiver: String,
        allocation: Uint128,
        vesting_period: VestingPeriod,
    }
}

pub struct VestingPeriod {
    pub cliff: u64, //In days
    pub linear: u64, //In days
}
```

| Key              | Type          | Description                             |
| ---------------- | ------------- | --------------------------------------- |
| `receiver`       | String        | Address of Receiver                     |
| `allocation`     | Uint128       | Allocation amount                       |
| `vesting_period` | VestingPeriod | VestingPeriod for receiver's allocation |

### `DecreaseAllocation`

Decrease MBRN allocated to a Receiver

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    DecreaseAllocation {
        receiver: String,
        allocation: Uint128,
    }
}
```

| Key          | Type    | Description         |
| ------------ | ------- | ------------------- |
| `receiver`   | String  | Address of Receiver |
| `allocation` | Uint128 | Allocation amount   |

### `WithdrawUnlocked`

Withdraw unlocked tokens for info.sender if address is a Receiver

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    WithdrawUnlocked { }
}
```

### `ClaimFeesForContract`

Claim staking liquidation fee rewards for the contract which distributes it to Receivers with an allocation&#x20;

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    ClaimFeesforContract { }
}
```

### `ClaimFeesForReceiver`

If info.sender is a Receiver, claim allocated rewards

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    ClaimFeesforContract { }
}
```

### `SubmitProposal`

Submit MBRN Governance proposal&#x20;

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    SubmitProposal {
        title: String,
        description: String,
        link: Option<String>,
        messages: Option<Vec<ProposalMessage>>,
        expedited: bool,
    }
}

pub struct ProposalMessage {
    /// Order of execution of the message
    pub order: Uint64,
    /// Execution message
    pub msg: CosmosMsg,
}
```

| Key           | Type                  | Description                   |
| ------------- | --------------------- | ----------------------------- |
| `title`       | String                | Proposal title                |
| `description` | String                | Proposal description          |
| `*link`       | String                | Proposal link                 |
| `*messages`   | Vec\<ProposalMessage> | Proposal executeable messages |
| `expedited`   | bool                  | Expedited Proposal toggle     |

&#x20;\* = optional

### `CastVote`

Vote for MBRN proposal

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    CastVote {
        proposal_id: u64,
        vote: ProposalVoteOption,
    }
}

pub enum ProposalVoteOption {
    For,
    Against,
}
```

| Key           | Type               | Description          |
| ------------- | ------------------ | -------------------- |
| `proposal_id` | u64                | Proposal identifier  |
| `vote`        | ProposalVoteOption | Proposal vote option |

### `UpdateConfig`

Update Config if contract owner

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    UpdateConfig {
        owner: Option<String>,
        mbrn_denom: Option<String>,
        osmosis_proxy: Option<String>,
        staking_contract: Option<String>,
    }
}
```

| Key                 | Type   | Description                 |
| ------------------- | ------ | --------------------------- |
| `*owner`            | String | Contract owner              |
| `*mbrn_denom`       | String | MBRN full denom             |
| `*osmosis_proxy`    | String | Osmosis Proxy contract addr |
| `*staking_contract` | String | MBRN staking contract addr  |

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

pub struct Config {
    pub owner: Addr, 
    pub initial_allocation: Uint128,
    pub mbrn_denom: String,
    pub osmosis_proxy: Addr,
    pub staking_contract: Addr,
}
```

### `Receivers`

Returns list of Receivers

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Receivers {}
}

//Vec<ReceiverResponse>
pub struct ReceiverResponse {
    pub receiver: String,
    pub allocation: Option<Allocation>,
    pub claimables: Vec<Asset>,
}
```

### `Allocation`

Returns allocation for a Receiver

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Allocation {
        receiver: String,
    }
}

pub struct AllocationResponse {
    pub amount: String,
    pub amount_withdrawn: String,
    pub start_time_of_allocation: String, //block time of allocation in seconds
    pub vesting_period: VestingPeriod,  //In days
}
```

| Key        | Type   | Description      |
| ---------- | ------ | ---------------- |
| `receiver` | String | Receiver address |

### `UnlockedTokens`

Returns a Receiver's unlocked allocation

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    UnlockedTokens {
        receiver: String,
    }
}
```

| Key        | Type   | Description      |
| ---------- | ------ | ---------------- |
| `receiver` | String | Receiver address |

### `Receiver`

Returns a Receiver

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Receiver {
        receiver: String,
    }
}

pub struct ReceiverResponse {
    pub receiver: String,
    pub allocation: Option<Allocation>,
    pub claimables: Vec<Asset>,
}
```

| Key        | Type   | Description      |
| ---------- | ------ | ---------------- |
| `receiver` | String | Receiver address |
