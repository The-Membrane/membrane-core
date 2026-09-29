use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Std error: {0}")]
    Std(#[from] StdError),

    #[error("Custom error: {val}")]
    CustomError { val: String },

    #[error("Invalid funds: {reason}")]
    InvalidFunds { reason: String },

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Lockdrop not active")]
    LockdropNotActive {},

    #[error("Deposit period ended")]
    DepositPeriodEnded {},

    #[error("Withdrawal period ended")]
    WithdrawalPeriodEnded {},

    #[error("Not in deposit period")]
    NotInDepositPeriod {},

    #[error("Not in withdrawal period")]
    NotInWithdrawalPeriod {},

    #[error("Locks not completed")]
    LocksNotCompleted {},

    #[error("Pending claims exist")]
    PendingClaimsExist {},
}

