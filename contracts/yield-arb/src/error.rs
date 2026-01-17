use thiserror::Error;
use cosmwasm_std::{StdError, Uint128};

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("Std error: {0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized,

    #[error("Insufficient transmuter balance: required {required}, available {available}")]
    InsufficientTransmuterBalance {
        required: Uint128,
        available: Uint128,
    },

    #[error("Custom error: {msg}")]
    CustomError {
        msg: String,
    },
}

