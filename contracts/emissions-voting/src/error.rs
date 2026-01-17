use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Graph with label '{label}' already exists")]
    GraphAlreadyExists { label: String },

    #[error("Graph with label '{label}' not found")]
    GraphNotFound { label: String },

    #[error("Vote value {value} is outside the allowed range [{min}, {max}]")]
    VoteOutOfRange { value: String, min: String, max: String },

    #[error("Voting period has not ended yet. Ends at {ends_at}")]
    PeriodNotEnded { ends_at: u64 },

    #[error("Invalid range: min must be less than max")]
    InvalidRange {},

    #[error("Period days must be at least 1")]
    InvalidPeriodDays {},

    #[error("User has no voting power")]
    NoVotingPower {},

    #[error("No votes have been cast in this period")]
    NoVotes {},

    #[error("Invalid graph type: {msg}")]
    InvalidGraphType { msg: String },

    #[error("Invalid vote value: {msg}")]
    InvalidVoteValue { msg: String },
}

