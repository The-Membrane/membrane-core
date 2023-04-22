use cosmwasm_std::{StdError, Decimal};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Invalid Collateral")]
    InvalidCollateral {},

    #[error("Invalid Repayment Asset")]
    InvalidCredit {},

    #[error("Basket withdrawals & debt increases are frozen temporarily")]
    Frozen {},

    #[error("Position is solvent and shouldn't be liquidated")]
    PositionSolvent {},

    #[error("Makes position insolvent")]
    PositionInsolvent {},

    #[error("User has no positions in this basket")]
    NoUserPositions {},

    #[error("Position doesn't exist")]
    NonExistentPosition {},

    #[error("Basket doesn't exist")]
    NonExistentBasket {},

    #[error("Invalid Withdrawal")]
    InvalidWithdrawal {},

    #[error("No repayment price set for this basket")]
    NoRepaymentPrice {},

    #[error("Invalid function parameters")]
    InvalidParameters {},

    #[error("Repayment exceeds outstanding credit")]
    ExcessRepayment {},

    #[error("Position's debt is below minimum")]
    BelowMinimumDebt {},

    #[error("Cw20Msg Error")]
    Cw20MsgError {},

    #[error("Config ID wasn't previously incremented")]
    ConfigIDError {},

    #[error("Info.sender is not the config.owner")]
    NotContractOwner {},

    #[error("Info.sender is not the basket.owner")]
    NotBasketOwner {},

    #[error("{msg}")]
    FaultyCalc { msg: String },

    #[error("Invalid target_LTV for debt increase: {target_LTV}")]
    InvalidLTV { target_LTV: Decimal },

    #[error("Invalid Max LTV")]
    InvalidMaxLTV { max_LTV: Decimal },

    #[error("Maximum position number reached")]
    MaxPositionsReached {},

    #[error("Custom Error val: {val:?}")]
    CustomError { val: String },
    // Add any other custom errors you like here.
    // Look at https://docs.rs/thiserror/1.0.21/thiserror/ for details.
}
