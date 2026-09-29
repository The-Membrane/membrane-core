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

    #[error("Invalid collateral asset")]
    InvalidCollateral {},

    #[error("Withdrawal amount exceeds position balance")]
    ExcessiveWithdrawal {},

    #[error("Supply cap exceeded for asset")]
    SupplyCapExceeded {},

    #[error("Asset is frozen by circuit breaker: {denom}")]
    AssetFrozen { denom: String },

    #[error("Maximum positions per user reached")]
    MaxPositionsReached {},

    #[error("Custom error: {val}")]
    CustomError { val: String },
}
