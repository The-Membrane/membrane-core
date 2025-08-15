use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TrackManagerError {
    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Track not found: {track_id}")]
    TrackNotFound { track_id: String },

    #[error("Track already exists: {track_id}")]
    TrackAlreadyExists { track_id: String },

    #[error("Invalid track dimensions: width={width}, height={height}")]
    InvalidTrackDimensions { width: u8, height: u8 },

    #[error("Track must have at least one finish tile")]
    NoFinishTile {},

    #[error("Track must have at least one start tile")]
    NoStartTile {},

    #[error("Track must have at least one accessible path to finish")]
    NoAccessiblePath {},

    #[error("Track too small: width={width}, height={height}. Minimum size is 3x3")]
    TrackTooSmall { width: u8, height: u8 },

    #[error("Track too large: width={width}, height={height}. Maximum size is 50x50")]
    TrackTooLarge { width: u8, height: u8 },

    #[error("{0}")]
    Std(#[from] StdError),
}

pub type TrackManagerResult<T> = Result<T, TrackManagerError>; 