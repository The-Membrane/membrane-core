use thiserror::Error;
use cosmwasm_std::StdError;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Event closed")]
    EventClosed {},

    #[error("Already won in this window")]
    AlreadyWon {},

    #[error("{0}")]
    Std(#[from] StdError),
} 