use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error(transparent)]
    Std(#[from] StdError),

    #[error("unauthorized")]
    Unauthorized {},

    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("invalid action: {action}")]
    InvalidAction { action: usize },
}

