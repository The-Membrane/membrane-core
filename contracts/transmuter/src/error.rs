use cosmwasm_std::{OverflowError, StdError};
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Std error: {0}")]
    Std(#[from] StdError),

    #[error("Overflow: {0}")]
    Overflow(#[from] OverflowError),

    #[error("Custom error: {val}")]
    CustomError { val: String },

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

    #[error("CDT to paired asset swaps are restricted to the CDP contract")]
    CdtToPairedAssetRestricted {},

    #[error("Duplicate tranche: ({denom}, {junior})")]
    DuplicateTranche { denom: String, junior: bool },

    #[error("Invalid collateral denom: {denom}")]
    InvalidCollateralDenom { denom: String },

    #[error("No tranche positions found for user")]
    NoTranchePositions {},

    #[error("Insufficient tranche balance: have {have}, need {need}")]
    InsufficientTrancheBalance { have: String, need: String },

    #[error("Bad debt amount {amount} exceeds max assignable {max}")]
    BadDebtExceedsMax { amount: String, max: String },

    #[error("Tranche rate decreased: pre {pre_rate}, post {post_rate}")]
    TrancheRateDecreased { pre_rate: String, post_rate: String },
}
