use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Custom error: {val}")]
    CustomError { val: String },
}

