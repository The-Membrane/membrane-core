use cosmwasm_schema::{cw_serde, QueryResponses};

use crate::types::{TournamentCriteria, TournamentStatus, TournamentMatch, TournamentRanking};

#[cw_serde]
pub struct InstantiateMsg {
    pub admin: String,
    pub race_engine: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    StartTournament {
        criteria: TournamentCriteria,
        track_id: String,
        max_participants: Option<u32>,
    },
    RunNextRound {},
    EndTournament {},
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(GetCurrentBracketResponse)]
    GetCurrentBracket {},
    #[returns(GetTournamentResultsResponse)]
    GetTournamentResults {},
    #[returns(IsParticipantResponse)]
    IsParticipant { car_id: String },
    #[returns(GetTournamentStateResponse)]
    GetTournamentState {},
}

#[cw_serde]
pub struct GetCurrentBracketResponse {
    pub round: u32,
    pub matches: Vec<TournamentMatch>,
    pub participants: Vec<String>,
}

#[cw_serde]
pub struct GetTournamentResultsResponse {
    pub tournament_id: String,
    pub winner: Option<String>,
    pub final_rankings: Vec<TournamentRanking>,
    pub total_participants: u32,
}

#[cw_serde]
pub struct IsParticipantResponse {
    pub car_id: String,
    pub is_participant: bool,
}

#[cw_serde]
pub struct GetTournamentStateResponse {
    pub tournament_id: String,
    pub status: TournamentStatus,
    pub current_round: u32,
    pub total_rounds: u32,
    pub participants: Vec<String>,
    pub track_id: String,
} 