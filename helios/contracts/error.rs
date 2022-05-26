use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Invalid Collateral")]
    InvalidCollateral {},

    #[error("Makes position insolvent")]
    PositionInsolvent {},

    #[error("Position doesn't exist")]
    NonExistentPosition {},

    #[error("Invalid Withdrawal")]
    InvalidWithdrawal {},

    #[error("No repayment price set for this basket")]
    NoRepaymentPrice {},

    #[error("Invalid function parameters")]
    InvalidParameters {},

    #[error("Repayment exceeds outstanding credit")]
    ExcessRepayment {},

    #[error("Repayment exceeds outstanding credit")]
    Cw20MsgError {},

    #[error("Custom Error val: {val:?}")]
    CustomError {
        val: String,
    },
    // Add any other custom errors you like here.
    // Look at https://docs.rs/thiserror/1.0.21/thiserror/ for details.
}
