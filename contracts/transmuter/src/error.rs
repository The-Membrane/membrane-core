use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Std error: {0}")]
    Std(#[from] StdError),

    #[error("Invalid funds: {reason}")]
    InvalidFunds { reason: String },

    #[error("Slippage exceeded")]
    SlippageExceeded {},

    #[error("Invalid asset: {denom}")]
    InvalidAsset { denom: String },

    #[error("Insufficient liquidity for {0}")]
    InsufficientLiquidity(String),

    #[error("Validation error: {0}")]
    Validation(String),
}
