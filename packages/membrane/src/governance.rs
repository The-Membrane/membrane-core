use cosmwasm_std::{Addr, CosmosMsg, Decimal, StdError, StdResult, Uint128, Uint64};
use cosmwasm_schema::cw_serde;
use std::fmt::{Display, Formatter, Result};
use std::ops::RangeInclusive;

use self::helpers::is_safe_link;

pub const MINIMUM_PROPOSAL_REQUIRED_THRESHOLD_PERCENTAGE: u64 = 51;
pub const MAX_PROPOSAL_REQUIRED_THRESHOLD_PERCENTAGE: u64 = 100;
pub const MAX_PROPOSAL_REQUIRED_QUORUM_PERCENTAGE: u64 = 100;
pub const MINIMUM_PROPOSAL_REQUIRED_QUORUM_PERCENTAGE: u64 = 33;
pub const VOTING_PERIOD_INTERVAL: RangeInclusive<u64> = 14400..=14 * 14400; //1 to 14 days in blocks (6 seconds per block)
pub const DELAY_INTERVAL: RangeInclusive<u64> = 0..=14400; // from 0 to 1 day in blocks (6 seconds per block)
pub const EXPIRATION_PERIOD_INTERVAL: RangeInclusive<u64> = 14400..=100800; //1 to 14 days in blocks (6 seconds per block)
pub const STAKE_INTERVAL: RangeInclusive<u128> = 1000000000..=5000000000; // from 1000 to 5000 $MBRN

/// Proposal validation attributes
const MIN_TITLE_LENGTH: usize = 4;
const MAX_TITLE_LENGTH: usize = 64;
const MIN_DESC_LENGTH: usize = 4;
const MAX_DESC_LENGTH: usize = 1024;
const MIN_LINK_LENGTH: usize = 12;
const MAX_LINK_LENGTH: usize = 128;

/// Special characters that are allowed in proposal text
const SAFE_TEXT_CHARS: &str = "!&?#()*+'-./\"";

/// This structure holds the parameters used for creating a Governance contract.
#[cw_serde]
pub struct InstantiateMsg {
    /// MBRN Staking contract to query MBRN denom
    pub mbrn_staking_contract_addr: String,
    /// Address of the vesting contract
    pub vesting_contract_addr: String,
    ///Multiplier for vesting allocation voting power
    pub vesting_voting_power_multiplier: Decimal,
    /// Proposal voting period
    pub proposal_voting_period: u64,
    /// Expedited Proposal voting period
    pub expedited_proposal_voting_period: u64,
    /// Proposal effective delay
    pub proposal_effective_delay: u64,
    /// Proposal expiration period
    pub proposal_expiration_period: u64,
    /// Proposal required stake
    pub proposal_required_stake: Uint128,
    /// Proposal required quorum
    pub proposal_required_quorum: String,
    /// Proposal required threshold for executable proposals
    pub proposal_required_threshold: String,
    /// Whitelisted links
    pub whitelisted_links: Vec<String>,
}//u64 fields are block units

/// This enum describes all execute functions available in the contract.
#[cw_serde]
pub enum ExecuteMsg {
    /// Submit a new proposal in the Governance contract
    SubmitProposal {
        title: String,
        description: String,
        link: Option<String>,
        messages: Option<Vec<ProposalMessage>>,
        //If from the vesting contract
        recipient: Option<String>,
        //Expedited toggle
        expedited: bool,
    },
    /// Cast a vote for an active proposal
    CastVote {
        /// Proposal identifier
        proposal_id: u64,
        /// Vote option
        vote: ProposalVoteOption,
        //If from the vesting contract
        recipient: Option<String>,
    },
    /// Set the status of a proposal that expired
    EndProposal {
        /// Proposal identifier
        proposal_id: u64,
    },
    /// Check messages execution
    CheckMessages {
        /// messages
        messages: Vec<ProposalMessage>,
    },
    /// The last endpoint which is executed only if all proposal messages have been passed
    CheckMessagesPassed {},
    /// Execute a successful proposal
    ExecuteProposal {
        /// Proposal identifier
        proposal_id: u64,
    },
    /// Remove a proposal that was already executed (or failed/expired)
    RemoveCompletedProposal {
        /// Proposal identifier
        proposal_id: u64,
    },
    /// Update parameters in the Governance contract
    /// ## Executor
    /// Only the Governance contract is allowed to update its own parameters
    UpdateConfig(UpdateConfig),
}

/// Thie enum describes all the queries available in the contract.
#[cw_serde]
pub enum QueryMsg {
    /// Return the contract's configuration
    Config {},
    /// Return the current list of proposals
    Proposals {
        /// Id from which to start querying
        start: Option<u64>,
        /// The amount of proposals to return
        limit: Option<u32>,
    },
    PendingProposals {
        /// Id from which to start querying
        start: Option<u64>,
        /// The amount of proposals to return
        limit: Option<u32>,
    },
    /// Return proposal voters of specified proposal
    ProposalVoters {
        /// Proposal unique id
        proposal_id: u64,
        /// Proposal vote option
        vote_option: ProposalVoteOption,
        /// Index from which to start querying
        start: Option<u64>,
        /// The amount of voters to return
        limit: Option<u32>,
        /// Specific user to query for
        specific_user: Option<String>,
    },
    /// Return information about a specific proposal
    Proposal { proposal_id: u64 },
    /// Return information about the votes cast on a specific proposal
    ProposalVotes { proposal_id: u64 },
    /// Return user voting power for a specific proposal
    UserVotingPower {
        user: String,
        proposal_id: u64,
        vesting: bool,
    },
    /// Return total voting power for a specific proposal
    TotalVotingPower { proposal_id: u64 },
}

/// This structure stores general parameters for the Governance contract.
#[cw_serde]
pub struct Config {
    /// MBRN native token fulldenom
    pub mbrn_denom: String,
    /// Minimum total stake required to submit a proposal
    pub minimum_total_stake: Uint128,
    ///MBRN staking contract
    pub staking_contract_addr: Addr,
    /// Address of the vesting contract
    pub vesting_contract_addr: Addr,
    ///Multiplier for vesting allocation voting power
    pub vesting_voting_power_multiplier: Decimal,
    /// Proposal voting period
    pub proposal_voting_period: u64,
    /// Expedited Proposal voting period
    pub expedited_proposal_voting_period: u64,
    /// Proposal effective delay
    pub proposal_effective_delay: u64,
    /// Proposal expiration period
    pub proposal_expiration_period: u64,
    /// Proposal required stake
    pub proposal_required_stake: Uint128,
    /// Proposal required quorum
    pub proposal_required_quorum: Decimal,
    /// Proposal required threshold for executable proposals
    pub proposal_required_threshold: Decimal,
    /// Whitelisted links
    pub whitelisted_links: Vec<String>,
    /// Toggle quadratic voting
    pub quadratic_voting: bool,
}

impl Config {
    pub fn validate(&self) -> StdResult<()> {
        if self.proposal_required_threshold
            > Decimal::percent(MAX_PROPOSAL_REQUIRED_THRESHOLD_PERCENTAGE)
            || self.proposal_required_threshold
                < Decimal::percent(MINIMUM_PROPOSAL_REQUIRED_THRESHOLD_PERCENTAGE)
        {
            return Err(StdError::generic_err(format!(
                "The required threshold for a proposal cannot be lower than {}% or higher than {}%",
                MINIMUM_PROPOSAL_REQUIRED_THRESHOLD_PERCENTAGE,
                MAX_PROPOSAL_REQUIRED_THRESHOLD_PERCENTAGE
            )));
        }

        if self.proposal_required_quorum > Decimal::percent(MAX_PROPOSAL_REQUIRED_QUORUM_PERCENTAGE)
            || self.proposal_required_quorum
                < Decimal::percent(MINIMUM_PROPOSAL_REQUIRED_QUORUM_PERCENTAGE)
        {
            return Err(StdError::generic_err(format!(
                "The required quorum for a proposal cannot be lower than {}% or higher than {}%",
                MINIMUM_PROPOSAL_REQUIRED_QUORUM_PERCENTAGE,
                MAX_PROPOSAL_REQUIRED_QUORUM_PERCENTAGE
            )));
        }

        if !DELAY_INTERVAL.contains(&self.proposal_effective_delay) {
            return Err(StdError::generic_err(format!(
                "The effective delay for a proposal cannot be lower than {} or higher than {}",
                DELAY_INTERVAL.start(),
                DELAY_INTERVAL.end()
            )));
        }

        if !EXPIRATION_PERIOD_INTERVAL.contains(&self.proposal_expiration_period) {
            return Err(StdError::generic_err(format!(
                "The expiration period for a proposal cannot be lower than {} or higher than {}",
                EXPIRATION_PERIOD_INTERVAL.start(),
                EXPIRATION_PERIOD_INTERVAL.end()
            )));
        }

        if !VOTING_PERIOD_INTERVAL.contains(&self.proposal_voting_period) {
            return Err(StdError::generic_err(format!(
                "The voting period for a proposal should be more than {} or less than {} blocks.",
                VOTING_PERIOD_INTERVAL.start(),
                VOTING_PERIOD_INTERVAL.end()
            )));
        }

        if !STAKE_INTERVAL.contains(&self.proposal_required_stake.u128()) {
            return Err(StdError::generic_err(format!(
                "The required deposit for a proposal cannot be lower than {}",
                STAKE_INTERVAL.start(),
            )));
        }

        Ok(())
    }
}

/// This structure stores the params used when updating the main Governance contract params.
#[cw_serde]
pub struct UpdateConfig {
    /// MBRN native token fulldenom
    pub mbrn_denom: Option<String>,
    /// Minimum total stake required to submit a proposal
    pub minimum_total_stake: Option<Uint128>,
    /// MBRN staking contract
    pub staking_contract: Option<String>,
    /// vesting' contract address
    pub vesting_contract_addr: Option<String>,
    /// Multiplier for vesting' allocation voting power
    pub vesting_voting_power_multiplier: Option<Decimal>,
    /// Proposal voting period
    pub proposal_voting_period: Option<u64>,
    /// Expedited Proposal voting period
    pub expedited_proposal_voting_period: Option<u64>,
    /// Proposal effective delay
    pub proposal_effective_delay: Option<u64>,
    /// Proposal expiration period
    pub proposal_expiration_period: Option<u64>,
    /// Proposal required stake
    pub proposal_required_stake: Option<u128>,
    /// Proposal required quorum
    pub proposal_required_quorum: Option<String>,
    /// Proposal required threshold for executable proposals
    pub proposal_required_threshold: Option<String>,
    /// Links to remove from whitelist
    pub whitelist_remove: Option<Vec<String>>,
    /// Links to add to whitelist
    pub whitelist_add: Option<Vec<String>>,
    /// Toggle quadratic voting
    pub quadratic_voting: Option<bool>,
}

/// This structure stores data for a proposal.
#[cw_serde]
pub struct Proposal {
    /// Unique proposal ID
    pub proposal_id: Uint64,
    /// The address of the proposal submitter
    pub submitter: Addr,
    /// Status of the proposal
    pub status: ProposalStatus,
    /// Aligned power of proposal
    pub aligned_power: Uint128,
    /// `For` power of proposal
    pub for_power: Uint128,
    /// `Against` power of proposal
    pub against_power: Uint128,
    /// `Amend` power of proposal
    pub amendment_power: Uint128,
    /// `Remove` power of proposal
    pub removal_power: Uint128,
    /// Proposal boosters
    pub aligned_voters: Vec<Addr>,
    /// `For` votes for the proposal
    pub for_voters: Vec<Addr>,
    /// `Against` votes for the proposal
    pub against_voters: Vec<Addr>,
    /// `Amend` votes for the proposal
    pub amendment_voters: Vec<Addr>,
    /// `Remove` votes for the proposal
    pub removal_voters: Vec<Addr>,
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
    /// Proposal link
    pub link: Option<String>,
    /// Proposal messages
    pub messages: Option<Vec<ProposalMessage>>,
}

/// This structure describes a proposal response.
#[cw_serde]
pub struct ProposalResponse {
    /// Unique proposal ID
    pub proposal_id: Uint64,
    /// The address of the proposal submitter
    pub submitter: Addr,
    /// Status of the proposal
    pub status: ProposalStatus,
    /// Aligned power of proposal
    pub aligned_power: Uint128,
    /// `For` power of proposal
    pub for_power: Uint128,
    /// `Against` power of proposal
    pub against_power: Uint128,
    /// `Amend` power of proposal
    pub amendment_power: Uint128,
    /// `Remove` power of proposal
    pub removal_power: Uint128,
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

impl Proposal {
    pub fn validate(&self, whitelisted_links: Vec<String>) -> StdResult<()> {
        // Title validation
        if self.title.len() < MIN_TITLE_LENGTH {
            return Err(StdError::generic_err("Title too short!"));
        }
        if self.title.len() > MAX_TITLE_LENGTH {
            return Err(StdError::generic_err("Title too long!"));
        }
        if !self.title.chars().all(|c| {
            c.is_ascii_alphanumeric() || c.is_ascii_whitespace() || SAFE_TEXT_CHARS.contains(c)
        }) {
            return Err(StdError::generic_err(
                "Title is not in alphanumeric format!",
            ));
        }

        // Description validation
        if self.description.len() < MIN_DESC_LENGTH {
            return Err(StdError::generic_err("Description too short!"));
        }
        if self.description.len() > MAX_DESC_LENGTH {
            return Err(StdError::generic_err("Description too long!"));
        }
        if !self.description.chars().all(|c| {
            c.is_ascii_alphanumeric() || c.is_ascii_whitespace() || SAFE_TEXT_CHARS.contains(c)
        }) {
            return Err(StdError::generic_err(
                "Description is not in alphanumeric format",
            ));
        }

        // Link validation
        if let Some(link) = &self.link {
            if link.len() < MIN_LINK_LENGTH {
                return Err(StdError::generic_err("Link too short!"));
            }
            if link.len() > MAX_LINK_LENGTH {
                return Err(StdError::generic_err("Link too long!"));
            }
            if !whitelisted_links.iter().any(|wl| link.starts_with(wl)) {
                return Err(StdError::generic_err("Link is not whitelisted!"));
            }
            if !is_safe_link(link) {
                return Err(StdError::generic_err(
                    "Link is not properly formatted or contains unsafe characters!",
                ));
            }
        }

        Ok(())
    }
}

/// This enum describes available statuses/states for a Proposal.
#[cw_serde]
pub enum ProposalStatus {
    Active,
    Passed,
    AmendmentDesired,
    Rejected,
    Executed,
    Expired,
}

impl Display for ProposalStatus {
    fn fmt(&self, fmt: &mut Formatter) -> Result {
        match self {
            ProposalStatus::Active {} => fmt.write_str("active"),
            ProposalStatus::Passed {} => fmt.write_str("passed"),
            ProposalStatus::AmendmentDesired {} => fmt.write_str("amendment_required"),
            ProposalStatus::Rejected {} => fmt.write_str("rejected"),
            ProposalStatus::Executed {} => fmt.write_str("executed"),
            ProposalStatus::Expired {} => fmt.write_str("expired"),
        }
    }
}

/// This structure describes a proposal message.
#[cw_serde]
pub struct ProposalMessage {
    /// Order of execution of the message
    pub order: Uint64,
    /// Execution message
    pub msg: CosmosMsg,
}

/// This structure describes a proposal vote.
#[cw_serde]
pub struct ProposalVote {
    /// Voted option for the proposal
    pub option: ProposalVoteOption,
    /// Vote power
    pub power: Uint128,
}

/// This enum describes available options for voting on a proposal.
#[cw_serde]
pub enum ProposalVoteOption {
    For,
    Against,
    Amend,
    Remove,
    Align,
}

impl Display for ProposalVoteOption {
    fn fmt(&self, fmt: &mut Formatter) -> Result {
        match self {
            ProposalVoteOption::For {} => fmt.write_str("for"),
            ProposalVoteOption::Against {} => fmt.write_str("against"),
            ProposalVoteOption::Amend {} => fmt.write_str("amend"),
            ProposalVoteOption::Remove {} => fmt.write_str("remove"),
            ProposalVoteOption::Align {} => fmt.write_str("align"),
        }
    }
}

/// This structure describes a proposal vote response.
#[cw_serde]
pub struct ProposalVotesResponse {
    /// Proposal identifier
    pub proposal_id: u64,
    /// Total amount of `for` votes for a proposal
    pub for_power: Uint128,
    /// Total amount of `against` votes for a proposal.
    pub against_power: Uint128,
    /// Total amount of `amend` votes for a proposal.
    pub amendment_power: Uint128,
    /// Total amount of `remove` votes for a proposal.
    pub removal_power: Uint128,
    /// Total amount of `align` votes for a proposal.
    pub aligned_power: Uint128,
}

/// This structure describes a proposal list response.
#[cw_serde]
pub struct ProposalListResponse {
    /// The amount of proposals returned
    pub proposal_count: Uint64,
    /// The list of proposals that are returned
    pub proposal_list: Vec<ProposalResponse>,
}

pub mod helpers {
    use cosmwasm_std::{StdError, StdResult};

    const SAFE_LINK_CHARS: &str = "-_:/?#@!$&()*+,;=.~[]'%";

    /// Checks if the link is valid. Returns a boolean value.
    pub fn is_safe_link(link: &str) -> bool {
        link.chars()
            .all(|c| c.is_ascii_alphanumeric() || SAFE_LINK_CHARS.contains(c))
    }

    /// Validating the list of links. Returns an error if a list has an invalid link.
    pub fn validate_links(links: &[String]) -> StdResult<()> {
        for link in links {
            if !(is_safe_link(link) && link.contains('.') && link.ends_with('/')) {
                return Err(StdError::generic_err(format!(
                    "Link is not properly formatted or contains unsafe characters: {}.",
                    link
                )));
            }
        }

        Ok(())
    }
}
