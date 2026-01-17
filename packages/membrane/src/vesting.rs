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
    /// Address receiving pre-launch contributors allocation
    pub pre_launch_contributors: String,
    /// Address receiving pre-launch community allocation
    pub pre_launch_community: Vec<String>,
    /// MBRN denom
    pub mbrn_denom: String,
    /// Osomosis proxy contract address
    pub osmosis_proxy: String,
    /// Staking contract address
    pub staking_contract: String,
    /// Neutron Proxy contract address (for vesting minting)
    pub neutron_proxy: Option<String>,
    /// Old MBRN denom (token received from users)
    pub old_mbrn_denom: Option<String>,
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
        /// Neutron Proxy contract address
        neutron_proxy: Option<String>,
        /// Old MBRN denom
        old_mbrn_denom: Option<String>,
    },
    /// Accept vested transmutation from neutron-proxy (auth required)
    AddVestedTransmutation {
        recipient: String,
        amount_to_mint: Uint128,
        vesting_period: VestingPeriod,
    },
    /// Withdraw unlocked MBRN from weekly vesting schedules
    WithdrawVestedUnlocked {},
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
    /// Query all vesting schedules for a user
    VestingSchedules { user: String },
    /// Query specific schedule
    VestingSchedule { user: String, week_id: u64 },
    /// Query total unlocked across all schedules
    TotalVestedUnlocked { user: String },
    /// Query global vesting stats
    VestingStats {},
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
    /// Neutron Proxy contract address (for vesting minting)
    pub neutron_proxy: Option<Addr>,
    /// Old MBRN denom (token received from users)
    pub old_mbrn_denom: Option<String>,
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
                total_vesting += allocation.amount - allocation.amount_withdrawn;
            }
        }

        total_vesting
    }
}

#[cw_serde]
pub struct MigrateMsg {}

#[cw_serde]
pub struct VestingSchedulesResponse {
    pub schedules: Vec<VestingScheduleInfo>,
}

#[cw_serde]
pub struct VestingScheduleInfo {
    pub week_id: u64,
    pub mbrn_to_mint: Uint128,
    pub amount_withdrawn: Uint128,
    pub start_time: u64,
    pub vesting_period: VestingPeriod,
    pub transmutation_count: u64,
    pub unlocked_amount: Uint128,
}

#[cw_serde]
pub struct VestingStatsResponse {
    pub total_old_mbrn_received: Uint128,
    pub total_schedules: u64,
    pub total_users: u64,
}