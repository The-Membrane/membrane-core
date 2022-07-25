use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Not enough bids to execute this liquidation")]
    InsufficientBids {},

    #[error("Queue hasn't been added for this asset")]
    InvalidAsset {},

    #[error("Bids aren't denominated in this asset")]
    InvalidBidAsset {},

    #[error("Premium greater than max premium for this asset queue")]
    InvalidPremium {},

    #[error("A queue for this asset already exists")]
    DuplicateQueue {},

    #[error("Asset that was passed in has uncongruent object field & deposit amounts")]
    InvalidAssetObject {},

    #[error("Invalid withdrawal")]
    InvalidWithdrawal {},

    #[error("A bid with this bid id doesn't exist in the queue")]
    InvalidBidID {},

    #[error("Invalid function parameters")]
    InvalidParameters {},

    #[error("Variable overflow due to mismanaged state")]
    MismanagedState {},


    #[error("Custom Error val: {val:?}")]
    CustomError { val: String },
    // Add any other custom errors you like here.
    // Look at https://docs.rs/thiserror/1.0.21/thiserror/ for details.
}
