use cosmwasm_std::{OverflowError, StdError};
use thiserror::Error;

/// ## Description
/// This enum describes Assembly contract errors!
#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Duplicate basket_id for this asset: {basket_id:?}")]
    DuplicateOracle { basket_id: String },

    #[error("Asset in use in the Positions contract: {asset:?}")]
    AssetInUse { asset: String },

    #[error("Custom Error val: {val:?}")]
    CustomError { val: String },
}

impl From<OverflowError> for ContractError {
    fn from(o: OverflowError) -> Self {
        StdError::from(o).into()
    }
}
