use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Uint128, Decimal};

use crate::types::{TournamentCriteria, TournamentStatus, TournamentMatch, TournamentRanking};

#[cw_serde]
pub struct InstantiateMsg {
    pub admin: String,
    pub race_engine: String,
    pub byte_minter: String,
    pub car_contract: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    StartTournament {
        criteria: TournamentCriteria,
        track_id: String,
        max_participants: Option<u32>,
        registration_payment_options: Vec<cosmwasm_std::Coin>,
        max_ticks: u32,
    },
    RegisterForTournament {
        car_id: u128,
    },
    RunNextMatch {},
    EndTournament {},
    UpdateConfig {
        race_engine: Option<String>,
        byte_minter: Option<String>,
        car_contract: Option<String>,
        tokenfactory_denom: Option<String>,
        mint_amount_per_round_win: Option<Uint128>,
        reward_round_scalar: Option<Decimal>,
        allow_free_registration: Option<bool>,
    },
    EnableWeeklyTournaments {
        criteria: TournamentCriteria,
        track_id: String,
        max_participants: Option<u32>,
        registration_payment_options: Vec<cosmwasm_std::Coin>,
        max_ticks: u32,
    },
    DisableWeeklyTournaments {},
    CheckAndStartScheduledTournament {},
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(GetCurrentBracketResponse)]
    GetCurrentBracket {},
    #[returns(GetTournamentResultsResponse)]
    GetTournamentResults {},
    #[returns(IsParticipantResponse)]
    IsParticipant { car_id: u128 },
    #[returns(GetTournamentStateResponse)]
    GetTournamentState {},
    #[returns(GetRegistrationsResponse)]
    GetRegistrations {},
    #[returns(GetCarStatsResponse)]
    GetCarStats { car_id: u128 },
    #[returns(GetConfigResponse)]
    GetConfig {},
    #[returns(GetScheduledTournamentResponse)]
    GetScheduledTournament {},
}

#[cw_serde]
pub struct GetCurrentBracketResponse {
    pub round: u32,
    pub matches: Vec<TournamentMatch>,
    pub participants: Vec<u128>,
}

#[cw_serde]
pub struct GetTournamentResultsResponse {
    pub tournament_id: String,
    pub winner: Option<u128>,
    pub final_rankings: Vec<TournamentRanking>,
    pub total_participants: u32,
}

#[cw_serde]
pub struct IsParticipantResponse {
    pub car_id: u128,
    pub is_participant: bool,
}

#[cw_serde]
pub struct GetTournamentStateResponse {
    pub tournament_id: String,
    pub status: TournamentStatus,
    pub current_round: u32,
    pub total_rounds: u32,
    pub participants: Vec<u128>,
    pub track_id: String,
}

#[cw_serde]
pub struct Registration {
    pub car_id: u128,
    pub registered_at: u64,
    pub payment_option: Option<cosmwasm_std::Coin>,
}

#[cw_serde]
pub struct GetRegistrationsResponse {
    pub registrations: Vec<Registration>,
}

#[cw_serde]
pub struct CarStats {
    pub car_id: u128,
    pub round_wins: u32,
    pub round_losses: u32,
    pub tournament_wins: u32,
}

#[cw_serde]
pub struct GetCarStatsResponse {
    pub stats: CarStats,
}

#[cw_serde]
pub struct Config {
    pub admin: String,
    pub race_engine: String,
    pub byte_minter: String,
    pub car_contract: String,
    pub tokenfactory_denom: String,
    pub mint_amount_per_round_win: Uint128,
    pub reward_round_scalar: Decimal,
    pub allow_free_registration: Option<bool>,
}

#[cw_serde]
pub struct GetConfigResponse {
    pub config: Config,
}

#[cw_serde]
pub struct ScheduledTournament {
    pub enabled: bool,
    pub criteria: TournamentCriteria,
    pub track_id: String,
    pub max_participants: Option<u32>,
    pub registration_payment_options: Vec<cosmwasm_std::Coin>,
    pub max_ticks: u32,
    pub last_sunday_start: Option<u64>,
}

#[cw_serde]
pub struct GetScheduledTournamentResponse {
    pub scheduled_tournament: ScheduledTournament,
}

#[cw_serde]
pub struct MigrateMsg {} 