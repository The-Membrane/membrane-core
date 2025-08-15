use cosmwasm_std::{Addr, StdResult, Storage};
use cw_storage_plus::{Item, Map};
use serde::{Deserialize, Serialize};

use racing::types::{TournamentStatus, TournamentMatch, TournamentRanking, TournamentCriteria};

pub const ADMIN: Item<Addr> = Item::new("admin");
pub const RACE_ENGINE: Item<Addr> = Item::new("race_engine");

// Current tournament state
pub const TOURNAMENT_STATE: Item<TournamentState> = Item::new("tournament_state");

// Tournament participants: tournament_id -> Vec<car_id>
pub const PARTICIPANTS: Map<&str, Vec<String>> = Map::new("participants");

// Tournament matches: tournament_id -> Vec<TournamentMatch>
pub const TOURNAMENT_MATCHES: Map<&str, Vec<TournamentMatch>> = Map::new("tournament_matches");

// Tournament results: tournament_id -> Vec<TournamentRanking>
pub const TOURNAMENT_RESULTS: Map<&str, Vec<TournamentRanking>> = Map::new("tournament_results");

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
        }
    }
}

pub fn get_tournament_state(storage: &dyn Storage) -> StdResult<TournamentState> {
    TOURNAMENT_STATE.load(storage)
}

pub fn set_tournament_state(storage: &mut dyn Storage, state: TournamentState) -> StdResult<()> {
    TOURNAMENT_STATE.save(storage, &state)
}

pub fn get_participants(storage: &dyn Storage, tournament_id: &str) -> StdResult<Vec<String>> {
    PARTICIPANTS.load(storage, tournament_id)
}

pub fn set_participants(
    storage: &mut dyn Storage,
    tournament_id: &str,
    participants: Vec<String>,
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