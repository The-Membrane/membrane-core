use cosmwasm_std::{StdError, Uint128};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Position is solvent (not liquidatable)")]
    PositionSolvent {},

    #[error("Position {id} has no debt")]
    NoDebt { id: Uint128 },

    #[error("Asset is frozen by circuit breaker: {denom}")]
    AssetFrozen { denom: String },

    #[error("Liquidation timer active, expires at {expiry}")]
    LiquidationTimerActive { expiry: u64 },

    #[error("Liquidation stat limit reached")]
    StatLimitReached {},

    #[error("Custom error: {val}")]
    CustomError { val: String },
}
