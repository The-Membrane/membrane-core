use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum CarError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Car not found: {car_id}")]
    CarNotFound { car_id: u128 },

    #[error("Unauthorized: only owner can perform this action")]
    Unauthorized {},

    #[error("Car already exists: {car_id}")]
    CarAlreadyExists { car_id: u128 },

    #[error("Car has no owners: {car_id}")]
    CarHasNoOwners { car_id: u128 },

    #[error("Invalid car ID format: {car_id}")]
    InvalidCarId { car_id: String },

    #[error("Q-table not found for car: {car_id} and state: {state_hash}")]
    QTableNotFound { car_id: u128, state_hash: String },

    #[error("Decal is not custom and cannot be edited")]
    NotCustomDecal {},
}

pub type CarResult<T> = Result<T, CarError>; 