use cosmwasm_std::{StdResult, Storage, Coin};
use cw_storage_plus::{Item, Map};
use serde::{Deserialize, Serialize};

use membrane::types::{TournamentStatus, TournamentMatch, TournamentRanking, TournamentCriteria};
use membrane::tournament::{Registration, CarStats, Config, ScheduledTournament};

// pub const ADMIN: Item<Addr> = Item::new("admin");
// pub const RACE_ENGINE: Item<Addr> = Item::new("race_engine");
// pub const BYTE_MINTER: Item<Addr> = Item::new("byte_minter");
// pub const CAR_CONTRACT: Item<Addr> = Item::new("car_contract");
pub const CONFIG: Item<Config> = Item::new("config");

// Current tournament state
pub const TOURNAMENT_STATE: Item<TournamentState> = Item::new("tournament_state");

// Tournament participants: tournament_id -> Vec<car_id>
pub const PARTICIPANTS: Map<&str, Vec<u128>> = Map::new("participants");

// Tournament matches: tournament_id -> Vec<TournamentMatch>
pub const TOURNAMENT_MATCHES: Map<&str, Vec<TournamentMatch>> = Map::new("tournament_matches");

// Tournament results: tournament_id -> Vec<TournamentRanking>
pub const TOURNAMENT_RESULTS: Map<&str, Vec<TournamentRanking>> = Map::new("tournament_results");

// Pre-registrations for next tournament
pub const PRE_REGISTRATIONS: Item<Vec<Registration>> = Item::new("pre_registrations");

// Car statistics: car_id -> CarStats
pub const CAR_STATS: Map<u128, CarStats> = Map::new("car_stats");

// Scheduled tournament configuration
pub const SCHEDULED_TOURNAMENT: Item<ScheduledTournament> = Item::new("scheduled_tournament");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TournamentState {
    pub tournament_id: String,
    pub status: TournamentStatus,
    pub current_round: u32,
    pub total_rounds: u32,
    pub track_id: String,
    pub criteria: TournamentCriteria,
    pub max_participants: Option<u32>,
    pub created_at: u64,
    pub allow_free_registration: bool,
    pub registration_payment_options: Vec<Coin>,
    pub max_ticks: u32,
}

impl Default for TournamentState {
    fn default() -> Self {
        Self {
            tournament_id: String::new(),
            status: TournamentStatus::NotStarted,
            current_round: 0,
            total_rounds: 0,
            track_id: String::new(),
            criteria: TournamentCriteria::Random,
            max_participants: None,
            created_at: 0,
            allow_free_registration: false,
            registration_payment_options: vec![],
            max_ticks: 1000,
        }
    }
}

pub fn get_tournament_state(storage: &dyn Storage) -> StdResult<TournamentState> {
    TOURNAMENT_STATE.load(storage)
}

pub fn set_tournament_state(storage: &mut dyn Storage, state: TournamentState) -> StdResult<()> {
    TOURNAMENT_STATE.save(storage, &state)
}

pub fn get_participants(storage: &dyn Storage, tournament_id: &str) -> StdResult<Vec<u128>> {
    PARTICIPANTS.load(storage, tournament_id)
}

pub fn set_participants(
    storage: &mut dyn Storage,
    tournament_id: &str,
    participants: Vec<u128>,
) -> StdResult<()> {
    PARTICIPANTS.save(storage, tournament_id, &participants)
}

pub fn get_tournament_matches(
    storage: &dyn Storage,
    tournament_id: &str,
) -> StdResult<Vec<TournamentMatch>> {
    TOURNAMENT_MATCHES.load(storage, tournament_id)
}

pub fn set_tournament_matches(
    storage: &mut dyn Storage,
    tournament_id: &str,
    matches: Vec<TournamentMatch>,
) -> StdResult<()> {
    TOURNAMENT_MATCHES.save(storage, tournament_id, &matches)
}

pub fn get_tournament_results(
    storage: &dyn Storage,
    tournament_id: &str,
) -> StdResult<Vec<TournamentRanking>> {
    TOURNAMENT_RESULTS.load(storage, tournament_id)
}

pub fn set_tournament_results(
    storage: &mut dyn Storage,
    tournament_id: &str,
    results: Vec<TournamentRanking>,
) -> StdResult<()> {
    TOURNAMENT_RESULTS.save(storage, tournament_id, &results)
}

pub fn get_pre_registrations(storage: &dyn Storage) -> StdResult<Vec<Registration>> {
    Ok(PRE_REGISTRATIONS.load(storage).unwrap_or_else(|_| vec![]))
}

pub fn set_pre_registrations(storage: &mut dyn Storage, registrations: Vec<Registration>) -> StdResult<()> {
    PRE_REGISTRATIONS.save(storage, &registrations)
}

pub fn get_car_stats(storage: &dyn Storage, car_id: u128) -> StdResult<CarStats> {
    Ok(CAR_STATS.load(storage, car_id).unwrap_or_else(|_| CarStats {
        car_id,
        round_wins: 0,
        round_losses: 0,
        tournament_wins: 0,
    }))
}

pub fn set_car_stats(storage: &mut dyn Storage, car_id: u128, stats: CarStats) -> StdResult<()> {
    CAR_STATS.save(storage, car_id, &stats)
}

pub fn get_config(storage: &dyn Storage) -> StdResult<Config> {
    CONFIG.load(storage)
}

pub fn set_config(storage: &mut dyn Storage, config: Config) -> StdResult<()> {
    CONFIG.save(storage, &config)
}

pub fn get_scheduled_tournament(storage: &dyn Storage) -> StdResult<ScheduledTournament> {
    Ok(SCHEDULED_TOURNAMENT.load(storage).unwrap_or_else(|_| ScheduledTournament {
        enabled: false,
        criteria: TournamentCriteria::Random,
        track_id: String::new(),
        max_participants: None,
        allow_free_registration: false,
        registration_payment_options: vec![],
        max_ticks: 1000,
        last_sunday_start: None,
    }))
}

pub fn set_scheduled_tournament(storage: &mut dyn Storage, scheduled_tournament: ScheduledTournament) -> StdResult<()> {
    SCHEDULED_TOURNAMENT.save(storage, &scheduled_tournament)
} 