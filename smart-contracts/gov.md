---
description: Membrane Governance contract
---

# Gov

**Notes:** Governance can execute any arbitrary message. Voting power is based on power when a proposal was created.

## InstantiateMsg

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub mbrn_staking_contract: String,
    pub builders_contract_addr: String,
    pub builders_voting_power_multiplier: Decimal,
    pub proposal_voting_period: u64,
    pub proposal_effective_delay: u64,
    pub proposal_expiration_period: u64,
    pub proposal_required_stake: Uint128,
    pub proposal_required_quorum: String,
    pub proposal_required_threshold: String,
    pub whitelisted_links: Vec<String>,
}

```

| Key                                | Type         | Description                                      |
| ---------------------------------- | ------------ | ------------------------------------------------ |
| `mbrn_staking_contract`            | String       | MBRN staking contract                            |
| `builders_contract_addr`           | String       | Builder's Vesting contract                       |
| `builders_voting_power_multiplier` | Decimal      | Multiplier for Builders' allocation voting power |
| `proposal_voting_period`           | u64          | Proposal voting period                           |
| `proposal_effective_delay`         | u64          | Proposal effective delay                         |
| `proposal_expiration_period`       | u64          | Proposal expiration period                       |
| `proposal_required_stake`          | Uint128      | Proposal required stake                          |
| `proposal_required_quorum`         | String       | Proposal required quorum                         |
| `proposal_required_threshold`      | String       | Proposal required threshold                      |
| `whitelisted_links`                | Vec\<String> | Whitelisted links                                |

## ExecuteMsg

### `SubmitProposal`

Submit a new proposal in the Governance contract

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    SubmitProposal {
        title: String,
        description: String,
        link: Option<String>,
        messages: Option<Vec<ProposalMessage>>,
        //If from the builder's contract
        receiver: Option<String>,
    }
}
```

| Key           | Type                  | Description                                      |
| ------------- | --------------------- | ------------------------------------------------ |
| `title`       | String                | Proposal title                                   |
| `description` | String                | Proposal description                             |
| `*link`       | String                | Proposal whitelisted link                        |
| `messages`    | Vec\<ProposalMessage> | Proposal executeble messages                     |
| `receiver`    | String                | If from Builder's contract, add Receiver address |

&#x20;\* = optional

### `CastVote`

Cast vote on an active proposal

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    CastVote {
        proposal_id: u64,
        vote: ProposalVoteOption,
        receiver: Option<String>,
    }
}

pub enum ProposalVoteOption {
    For,
    Against,
}
```

| Key           | Type               | Description                                      |
| ------------- | ------------------ | ------------------------------------------------ |
| `proposal_id` | u64                | Proposal identifier                              |
| `vote`        | ProposalVoteOption | Vote Option                                      |
| `*receiver`   | String             | If from Builder's contract, add Receiver address |

&#x20;\* = optional

### `EndProposal`

Set the status of a proposal that has expired

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    EndProposal {
        proposal_id: u64,
    }
}
```

| Key           | Type | Description         |
| ------------- | ---- | ------------------- |
| `proposal_id` | u64  | Proposal Identifier |

### `CheckMessages`

Check messages execution

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    CheckMessages {
        messages: Vec<ProposalMessage>,
    }
}

//If passed you'll get returned 'ContractError::MessagesCheckPassed {}'
#[error("Messages check passed. Nothing was committed to the blockchain")]
    MessagesCheckPassed {},
```

| Key        | Type                  | Description       |
| ---------- | --------------------- | ----------------- |
| `messages` | Vec\<ProposalMessage> | Messages to check |

### `CheckMessagesPassed`

The last endpoint which is executed only if all proposal messages have been passed. Called by the contract at the end of CheckMessages.

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    CheckMessagesPassed {}
}
```

### `ExecuteProposal`

Execute a successful proposal

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    ExecuteProposal{
        proposal_id: u64,
    }
}
```

| Key           | Type | Description         |
| ------------- | ---- | ------------------- |
| `proposal_id` | u64  | Proposal Identifier |

### `RemoveCompletedProposal`

Remove a proposal that was already executed (or failed/expired)

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    RemoveCompletedProposal {
        proposal_id: u64,
    }
}
```

| Key           | Type | Description         |
| ------------- | ---- | ------------------- |
| `proposal_id` | u64  | Proposal Identifier |

### `UpdateConfig`

Update parameters in the Governance contract. Only the Governance contract is allowed to update its own parameters through a successful proposal.

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    UpdateConfig( UpdateConfig )
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct UpdateConfig {
    pub mbrn_denom: Option<String>,
    pub staking_contract: Option<String>,
    pub builders_contract_addr: Option<String>,
    pub builders_voting_power_multiplier: Option<Decimal>, 
    pub proposal_voting_period: Option<u64>,
    pub proposal_effective_delay: Option<u64>,
    pub proposal_expiration_period: Option<u64>,
    pub proposal_required_stake: Option<u128>,
    pub proposal_required_quorum: Option<String>,
    pub proposal_required_threshold: Option<String>,
    pub whitelist_remove: Option<Vec<String>>,
    pub whitelist_add: Option<Vec<String>>,
}
```

| Key                                 | Type         | Description                                      |
| ----------------------------------- | ------------ | ------------------------------------------------ |
| `*mbrn_denom`                       | String       | MBRN native token fulldenom                      |
| `*staking_contract_addr`            | String       | MBRN staking contract                            |
| `*builders_contract_addr`           | String       | Builders' contract address                       |
| `*builders_voting_power_multiplier` | Decimal      | Multiplier for Builders' allocation voting power |
| `*porposal_voting_period`           | u64          | Proposal voting period                           |
| `*proposal_effective_delay`         | u64          | Proposal effective delay                         |
| `*proposal_expiration_period`       | u64          | Proposal expiration period                       |
| `*proposal_required_stake`          | u128         | Proposal required stake                          |
| `*proposal_required_quorum`         | String       | Proposal required quorum                         |
| `*proposal_required_threshold`      | String       | Proposal required threshold                      |
| `*whitelist_remove`                 | Vec\<String> | Links to remove from whitelist                   |
| `*whitelist_add`                    | Vec\<String> | Links to add to whitelist                        |

&#x20;\* = optional

## QueryMsg

### `Config`

Return the contract's configuration

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {}
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    /// MBRN native token fulldenom
    pub mbrn_denom: String,
    ///MBRN staking contract
    pub staking_contract_addr: Addr,
    /// Address of the builder unlock contract
    pub builders_contract_addr: Addr,
    ///Multiplier for Builders' allocation voting power
    pub builders_voting_power_multiplier: Decimal,
    /// Proposal voting period
    pub proposal_voting_period: u64,
    /// Proposal effective delay
    pub proposal_effective_delay: u64,
    /// Proposal expiration period
    pub proposal_expiration_period: u64,
    /// Proposal required stake
    pub proposal_required_stake: Uint128,
    /// Proposal required quorum
    pub proposal_required_quorum: Decimal,
    /// Proposal required threshold
    pub proposal_required_threshold: Decimal,
    /// Whitelisted links
    pub whitelisted_links: Vec<String>,
}

```

### `Proposals`

Return the current list of proposals

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Proposals {
        start: Option<u64>,
        limit: Option<u32>,
    }
}

pub struct ProposalResponse {
    /// Unique proposal ID
    pub proposal_id: Uint64,
    /// The address of the proposal submitter
    pub submitter: Addr,
    /// Status of the proposal
    pub status: ProposalStatus,
    /// `For` power of proposal
    pub for_power: Uint128,
    /// `Against` power of proposal
    pub against_power: Uint128,
    /// Start block of proposal
    pub start_block: u64,
    /// Start time of proposal
    pub start_time: u64,
    /// End block of proposal
    pub end_block: u64,
    /// Delayed end block of proposal
    pub delayed_end_block: u64,
    /// Expiration block of proposal
    pub expiration_block: u64,
    /// Proposal title
    pub title: String,
    /// Proposal description
    pub description: String,
    /// Proposal messages
    pub messages: Option<Vec<ProposalMessage>>,
    /// Proposal link
    pub link: Option<String>,
}
```

| Key      | Type | Description                       |
| -------- | ---- | --------------------------------- |
| `*start` | u64  | Id from which to start querying   |
| `*limit` | u32  | The amount of proposals to return |

&#x20;\* = optional

### `ProposalVoters`

Return proposal voters of specified proposal

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    ProposalVoters {
        proposal_id: u64,
        vote_option: ProposalVoteOption,
        start: Option<u64>,
        limit: Option<u32>,
    }
}

Returns Vec<Addr>
```

| Key           | Type               | Description                       |
| ------------- | ------------------ | --------------------------------- |
| `proposal_id` | u64                | Proposal unique id                |
| `vote_option` | ProposalVoteOption | Proposal vote option              |
| `*start`      | u64                | Id from which to start querying   |
| `*limit`      | u32                | The amount of proposals to return |

&#x20;\* = optional

### `Proposal`

Return information about a specific proposal

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Proposal { proposal_id: u64 }
}

pub struct ProposalResponse {
    /// Unique proposal ID
    pub proposal_id: Uint64,
    /// The address of the proposal submitter
    pub submitter: Addr,
    /// Status of the proposal
    pub status: ProposalStatus,
    /// `For` power of proposal
    pub for_power: Uint128,
    /// `Against` power of proposal
    pub against_power: Uint128,
    /// Start block of proposal
    pub start_block: u64,
    /// Start time of proposal
    pub start_time: u64,
    /// End block of proposal
    pub end_block: u64,
    /// Delayed end block of proposal
    pub delayed_end_block: u64,
    /// Expiration block of proposal
    pub expiration_block: u64,
    /// Proposal title
    pub title: String,
    /// Proposal description
    pub description: String,
    /// Proposal messages
    pub messages: Option<Vec<ProposalMessage>>,
    /// Proposal link
    pub link: Option<String>,
}
```

| Key           | Type | Description        |
| ------------- | ---- | ------------------ |
| `proposal_id` | u64  | Proposal unique id |

### `ProposalVotes`

Return information about the votes cast on a specific proposal

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    ProposalVotes { proposal_id: u64 }
}

pub struct ProposalVotesResponse {
    /// Proposal identifier
    pub proposal_id: u64,
    /// Total amount of `for` votes for a proposal
    pub for_power: Uint128,
    /// Total amount of `against` votes for a proposal.
    pub against_power: Uint128,
}
```

| Key           | Type | Description        |
| ------------- | ---- | ------------------ |
| `proposal_id` | u64  | Proposal unique id |

### `UserVotingPower`

Return user voting power for a specific proposal

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    UserVotingPower { 
        user: String, 
        proposal_id: u64, 
        builders: bool, 
    }
}

Returns Uint128
```

| Key           | Type   | Description                                   |
| ------------- | ------ | --------------------------------------------- |
| `user`        | String | User's voting power to query                  |
| `proposal_id` | u64    | Proposal's unique id                          |
| `builders`    | bool   | If user is a receiver from Builder's contract |

### `TotalVotingPower`

Return total voting power for a specific proposal

```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    TotalVotingPower { proposal_id: u64 }
}

Returns Uint128
```

| Key           | Type | Description        |
| ------------- | ---- | ------------------ |
| `proposal_id` | u64  | Proposal unique id |
