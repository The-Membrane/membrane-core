use cosmwasm_std::StdError;
use thiserror::Error;
use racing::types::TournamentStatus;

#[derive(Error, Debug)]
pub enum TournamentError {
    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Tournament not found: {tournament_id}")]
    TournamentNotFound { tournament_id: String },

    #[error("Invalid participant count: {count}. Must be between 2 and 32")]
    InvalidParticipantCount { count: u32 },

    #[error("Insufficient participants: required {required}, actual {actual}")]
    InsufficientParticipants { required: u32, actual: u32 },

    #[error("Tournament not in progress. Current status: {status:?}")]
    TournamentNotInProgress { status: TournamentStatus },

    #[error("All rounds completed. Current: {current}, Total: {total}")]
    AllRoundsCompleted { current: u32, total: u32 },

    #[error("No matches found for round: {round}")]
    NoMatchesForRound { round: u32 },

    #[error("Tournament not completed. Current status: {status:?}")]
    TournamentNotCompleted { status: TournamentStatus },

    #[error("No final results available")]
    NoFinalResults {},

    #[error("Invalid match data")]
    InvalidMatchData {},

    #[error("Race simulation failed")]
    RaceSimulationFailed {},

    #[error("{0}")]
    Std(#[from] StdError),
}

pub type TournamentResult<T> = Result<T, TournamentError>; 