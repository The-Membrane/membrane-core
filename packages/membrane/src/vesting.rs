use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Uint128, Addr};

use crate::{
    governance::{ProposalMessage, ProposalVoteOption},
    types::{Allocation, Asset, VestingPeriod},
};

#[cw_serde]
pub struct InstantiateMsg {
    /// Contract owner, defaults to info.sender
    pub owner: Option<String>,
    /// Initial allocation
    pub initial_allocation: Uint128,
    /// Labs address
    pub labs_addr: String,
    /// MBRN denom
    pub mbrn_denom: String,
    /// Osomosis proxy contract address
    pub osmosis_proxy: String,
    /// Staking contract address
    pub staking_contract: String,
}


//To decrease Allocations, you need to upgrade the contract
//This is so there is a level of permanance in the vesting contract
#[cw_serde]
pub enum ExecuteMsg {
    /// Add a new recipient
    AddRecipient {
        /// Recipient address
        recipient: String,
    },
    /// Remove a recipient
    RemoveRecipient {
        /// Recipient address
        recipient: String,
    },
    /// Add allocation to a recipient
    AddAllocation {
        /// Recipient address
        recipient: String,
        /// Additional allocation
        allocation: Uint128,
        /// Vesting period.
        /// If an existing recipient is using this to divvy their allocation, the vesting period can't be changed.
        vesting_period: Option<VestingPeriod>,
    },
    /// Withdraw unlocked tokens
    WithdrawUnlocked {},
    /// Claim fees from MBRN staking for contract. 
    /// This is called to distribute rewards before "ClaimFeesforReceiver".
    ClaimFeesforContract {},
    /// Claim fees pro rata to recipient allocation
    ClaimFeesforRecipient {},
    /// Submit a proposal
    SubmitProposal {
        /// Proposal title
        title: String,
        /// Proposal description
        description: String,
        /// Proposal link
        link: Option<String>,
        /// Proposal messages
        messages: Option<Vec<ProposalMessage>>,
        /// Toggle for expedited proposal
        expedited: bool,
    },
    /// Vote on a proposal
    CastVote {
        /// Proposal identifier
        proposal_id: u64,
        /// Vote option
        vote: ProposalVoteOption,
    },
    /// Update contract config
    UpdateConfig {
        /// Contract owner
        owner: Option<String>,
        /// MBRN denom
        mbrn_denom: Option<String>,
        /// Osmosis Proxy contract address
        osmosis_proxy: Option<String>,
        /// Staking contract address
        staking_contract: Option<String>,
        /// Additional allocation for the contract to distribute
        additional_allocation: Option<Uint128>,
    },
}

#[cw_serde]
pub enum QueryMsg {
    /// Return contract config
    Config {},
    /// Return allocation for a recipient
    Allocation {
        /// Recipient address
        recipient: String 
    },
    /// Return unlocked tokens
    UnlockedTokens {
        /// Recipient address
        recipient: String
    },
    /// Returns RecipientResponse
    Recipient {
        /// Recipient address
        recipient: String
    },
    /// Returns all recipients
    Recipients {},
}

#[cw_serde]
pub struct Config {
    /// Contract owner
    pub owner: Addr,
    /// Total allocation able to be distributed
    pub total_allocation: Uint128,
    /// MBRN denom
    pub mbrn_denom: String,
    /// Osmosis Proxy contract address
    pub osmosis_proxy: Addr,
    /// Staking contract address
    pub staking_contract: Addr,
}

#[cw_serde]
pub struct AllocationResponse {
    /// Amount allocated
    pub amount: Uint128,
    /// Amount withdrawn
    pub amount_withdrawn: Uint128,
    /// Start time of allocation in seconds
    pub start_time_of_allocation: u64,
    /// Vesting period
    pub vesting_period: VestingPeriod,
}

#[cw_serde]
pub struct UnlockedResponse {
    /// Amount unlocked
    pub unlocked_amount: Uint128,
}

#[cw_serde]
pub struct RecipientResponse {
    /// Recipient address
    pub recipient: String,
    /// Allocation
    pub allocation: Option<Allocation>,
    /// Claimable rewards
    pub claimables: Vec<Asset>,
}

#[cw_serde]
pub struct RecipientsResponse {
    /// Recipients
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
