use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Uint128, Addr};
use cw20::Cw20ReceiveMsg;

use crate::{
    governance::{ProposalMessage, ProposalVoteOption},
    types::{Allocation, Asset, VestingPeriod},
};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub initial_allocation: Uint128,
    pub labs_addr: String,
    pub mbrn_denom: String,
    pub osmosis_proxy: String,
    pub staking_contract: String,
}


//To decrease Allocations, you need to upgrade the contract
//This is so there is a level of permanance in the vesting contract
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    AddRecipient {
        recipient: String,
    },
    RemoveRecipient {
        recipient: String,
    },
    AddAllocation {
        recipient: String,
        allocation: Uint128,
        vesting_period: Option<VestingPeriod>, //If an existing recipient is using this to divvy their allocation, the vesting period can't be changed.
    },
    WithdrawUnlocked {},
    //Claim fees from MBRN staking for contract. This is called to distribute rewards for "ClaimFeesforReceiver".
    ClaimFeesforContract {},
    //Claim fees pro rata to recipient allcoation.
    ClaimFeesforRecipient {},
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
        additional_allocation: Option<Uint128>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {},
    Allocation { recipient: String },
    UnlockedTokens { recipient: String },
    Recipient { recipient: String },
    Recipients {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Config {
    pub owner: Addr, //Governance Contract
    pub total_allocation: Uint128,
    pub mbrn_denom: String,
    pub osmosis_proxy: Addr,
    pub staking_contract: Addr,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AllocationResponse {
    pub amount: Uint128,
    pub amount_withdrawn: Uint128,
    pub start_time_of_allocation: u64, //block time of allocation in seconds
    pub vesting_period: VestingPeriod,    //In days
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct UnlockedResponse {
    pub unlocked_amount: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RecipientResponse {
    pub recipient: String,
    pub allocation: Option<Allocation>,
    pub claimables: Vec<Asset>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RecipientsResponse {
    pub recipients: Vec<RecipientResponse>,
}

impl RecipientsResponse {
    
    pub fn get_total_vesting(&self) -> Uint128 {

        let mut total_vesting = Uint128::zero();

        for recipient in self.clone().recipients {
            if let Some(allocation) = recipient.allocation{
                total_vesting += allocation.amount;
            }
        }

        total_vesting
    }
}
