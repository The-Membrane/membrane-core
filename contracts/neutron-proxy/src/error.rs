use cosmwasm_std::{StdError, Uint128};
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum TokenFactoryError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Invalid subdenom: {subdenom:?}")]
    InvalidSubdenom { subdenom: String },

    #[error("Invalid denom: {denom:?} {message:?}")]
    InvalidDenom { denom: String, message: String },

    #[error("denom does not exist: {denom:?}")]
    DenomDoesNotExist { denom: String },

    #[error("address is not supported yet, was: {address:?}")]
    BurnFromAddressNotSupported { address: String },

    #[error("amount was zero, must be positive")]
    ZeroAmount {},

    #[error("Mint sends address over its unique cap")]
    MintCapped {},

    #[error("Address is already a contract owner")]
    AlreadyOwner {},

    #[error("Custom Error val: {val:?}")]
    CustomError { val: String },

    #[error("Astroport pair not found for assets: {asset_infos:?}")]
    PairNotFound { asset_infos: Vec<String> },

    #[error("Astroport router not configured")]
    RouterNotConfigured {},

    #[error("Invalid route configuration: {reason}")]
    InvalidRouteConfig { reason: String },

    #[error("No liquidity available on either DEX")]
    NoLiquidityAvailable {},

    #[error("Dynamic routing is disabled")]
    DynamicRoutingDisabled {},

    #[error("Invalid PCL parameter {field}: {message}")]
    InvalidPclParam { field: String, message: String },

    #[error("Slippage exceeded: expected {expected}, got {actual}")]
    SlippageExceeded { expected: Uint128, actual: Uint128 },

    #[error("Duplicate assets in pair")]
    DuplicateAssets {},
}
