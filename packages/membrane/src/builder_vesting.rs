use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::Uint128;
use cw20::Cw20ReceiveMsg;

use crate::{
    governance::{ProposalMessage, ProposalVoteOption},
    types::{Allocation, Asset, VestingPeriod},
};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub initial_allocation: Uint128,
    pub mbrn_denom: String,
    pub osmosis_proxy: String,
    pub staking_contract: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Receive(Cw20ReceiveMsg),
    AddReceiver {
        receiver: String,
    },
    RemoveReceiver {
        receiver: String,
    },
    AddAllocation {
        receiver: String,
        allocation: Uint128,
        vesting_period: VestingPeriod,
    },
    DecreaseAllocation {
        receiver: String,
        allocation: Uint128,
    },
    WithdrawUnlocked {},
    //Claim fees from MBRN staking for contract. This is called to distribute rewards for "ClaimFeesforReceiver".
    ClaimFeesforContract {},
    //Claim fees pro rata to receiver allcoation.
    ClaimFeesforReceiver {},
    SubmitProposal {
        title: String,
        description: String,
        link: Option<String>,
        messages: Option<Vec<ProposalMessage>>,
        expedited: bool,
    },
    CastVote {
        /// Proposal identifier
        proposal_id: u64,
        /// Vote option
        vote: ProposalVoteOption,
    },
    UpdateConfig {
        owner: Option<String>,
        mbrn_denom: Option<String>,
        osmosis_proxy: Option<String>,
        staking_contract: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {},
    Allocation { receiver: String },
    UnlockedTokens { receiver: String },
    Receiver { receiver: String },
    Receivers {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ConfigResponse {
    pub owner: String,
    pub initial_allocation: String,
    pub mbrn_denom: String,
    pub osmosis_proxy: String,
    pub staking_contract: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AllocationResponse {
    pub amount: String,
    pub amount_withdrawn: String,
    pub start_time_of_allocation: String, //block time of allocation in seconds
    pub vesting_period: VestingPeriod,    //In days
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct UnlockedResponse {
    pub unlocked_amount: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ReceiverResponse {
    pub receiver: String,
    pub allocation: Option<Allocation>,
    pub claimables: Vec<Asset>,
}
