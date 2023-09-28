use cosmwasm_std::{OverflowError, StdError};
use thiserror::Error;

/// ## Description
/// This enum describes Assembly contract errors!
#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Proposal not active!")]
    ProposalNotActive {},

    #[error("Voting period ended!")]
    VotingPeriodEnded {},

    #[error("User already voted!")]
    UserAlreadyVoted {},

    #[error("You don't have any voting power!")]
    NoVotingPower {},

    #[error("Voting period not ended yet!")]
    VotingPeriodNotEnded {},

    #[error("Proposal expired!")]
    ExecuteProposalExpired {},

    #[error("Insufficient stake!")]
    InsufficientStake {},

    #[error("Proposal not passed!")]
    ProposalNotPassed {},

    #[error("Proposal can't be removed!")]
    CantRemove {},

    #[error("Proposal delay not ended!")]
    ProposalDelayNotEnded {},

    #[error("Contract can't be migrated!")]
    MigrationError {},

    #[error("Whitelist cannot be empty!")]
    WhitelistEmpty {},

    #[error("Messages check passed. Nothing was committed to the blockchain")]
    MessagesCheckPassed {},

    #[error("Total staked amount isn't greater than {minimum}")]
    InsufficientTotalStake { minimum: u128 },
    
    #[error("No need for further alignment to activate the proposal")]
    AlignmentReached {},
}

impl From<OverflowError> for ContractError {
    fn from(o: OverflowError) -> Self {
        StdError::from(o).into()
    }
}
