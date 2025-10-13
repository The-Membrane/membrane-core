use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Invalid amount: sent {sent}, promised {promised}")]
    InvalidAmount { sent: u128, promised: u128 },

    #[error("No promises set")]
    NoPromisesSet {},

    #[error("Invalid revenue destination: {destination}")]
    InvalidRevenueDestination { destination: String },
}
