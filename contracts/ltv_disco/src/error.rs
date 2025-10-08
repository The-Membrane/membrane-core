use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Invalid asset")]
    InvalidAsset {},

    #[error("Too many assets sent, only {valid} allowed")]
    TooManyAssets { valid: String },

    #[error("Invalid LTV")]
    InvalidLTV {},

    #[error("Invalid deposit amount")]
    InvalidDepositAmount {},

    #[error("Deposit not found")]
    DepositNotFound {},

    #[error("Insufficient deposits")]
    InsufficientDeposits {},

    #[error("Queue not found")]
    QueueNotFound {},

    #[error("Slot not found")]
    SlotNotFound {},

    #[error("Invalid withdrawal amount")]
    InvalidWithdrawal { minimum: cosmwasm_std::Uint128 },

    #[error("Custom error: {val}")]
    CustomError { val: String },

    #[error("Invalid percent to disperse")]
    InvalidPercentToDisperse {},

    #[error("Dispersal not active")]
    DispersalNotActive {},

    #[error("Invalid dispersal window")]
    InvalidDispersalWindow {},
}
