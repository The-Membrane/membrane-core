use cosmwasm_std::{StdError, Uint128};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Basket is frozen")]
    Frozen {},

    #[error("No user positions found")]
    NoUserPositions {},

    #[error("Position {id} does not exist")]
    NonExistentPosition { id: Uint128 },

    #[error("Position is insolvent")]
    Insolvent {},

    #[error("Debt below minimum: {minimum}")]
    BelowDebtMinimum { minimum: Uint128 },

    #[error("Cannot borrow: max borrow LTV exceeded")]
    MaxBorrowLTVExceeded {},

    #[error("No debt to repay")]
    NoDebt {},

    #[error("Repay amount exceeds debt")]
    ExcessiveRepay {},

    #[error("Invalid debt split: fractions must sum to 1")]
    InvalidDebtSplit {},

    #[error("Fixed rate cap exceeded for segment {segment}")]
    FixedRateCapExceeded { segment: String },

    #[error("No credit oracle set")]
    NoCreditOracle {},

    #[error("Credit asset not found")]
    NoCreditAsset {},

    #[error("Custom error: {val}")]
    CustomError { val: String },
}
