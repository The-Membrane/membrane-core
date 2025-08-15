use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Invalid car count: expected {expected}, got {actual}")]
    InvalidCarCount { expected: u32, actual: u32 },

    #[error("Invalid action: {action}")]
    InvalidAction { action: usize },

    #[error("Track not found: {track_id}")]
    TrackNotFound { track_id: String },

    #[error("Car not found: {car_id}")]
    CarNotFound { car_id: String },

    #[error("Race not found: {race_id}")]
    RaceNotFound { race_id: String },

    #[error("Invalid race configuration")]
    InvalidRaceConfig,

    #[error("Simulation error: {message}")]
    SimulationError { message: String },

    #[error("Q-learning update error: {message}")]
    QLearningError { message: String },

    #[error("{0}")]
    Std(#[from] StdError),
} 