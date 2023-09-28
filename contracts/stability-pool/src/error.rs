use cosmwasm_std::{StdError, Uint128};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Can't have duplicate withdrawal assets in assets field")]
    DuplicateWithdrawalAssets {},

    #[error("Cw20Msg Error")]
    Cw20MsgError {},

    #[error("Distributed funds are less than repaid funds")]
    InsufficientFunds {},

    #[error("Asset pool hasn't been added for this asset yet")]
    InvalidAsset {},

    #[error("Deposit is too small, minimum is {min:?}")]
    MinimumDeposit { min: Uint128 },

    #[error("Asset that was passed in has uncongruent object field & deposit amounts")]
    InvalidAssetObject {},

    #[error("Invalid withdrawal")]
    InvalidWithdrawal {},

    #[error("Invalid function parameters")]
    InvalidParameters {},

    #[error("Variable overflow due to mismanaged state")]
    MismanagedState {},

    #[error("Custom Error val: {val:?}")]
    CustomError { val: String },
    // Add any other custom errors you like here.
    // Look at https://docs.rs/thiserror/1.0.21/thiserror/ for details.
}
