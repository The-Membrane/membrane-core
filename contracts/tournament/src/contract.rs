use cosmwasm_std::{
    entry_point, to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult,
};

use crate::error::TournamentError;
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
use crate::state::{ADMIN, RACE_ENGINE, get_tournament_state, set_tournament_state, get_participants, set_participants, get_tournament_results, set_tournament_results, get_tournament_matches, set_tournament_matches};
use racing::types::{TournamentCriteria, TournamentStatus, TournamentMatch, TournamentRanking};

// Tournament constants
const MAX_PARTICIPANTS: u32 = 32;
const MIN_PARTICIPANTS: u32 = 2;
const MAX_ROUNDS: u32 = 5; // 2^5 = 32 max participants

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, TournamentError> {
    let admin = deps.api.addr_validate(&msg.admin)?;
    let race_engine = deps.api.addr_validate(&msg.race_engine)?;
    
    ADMIN.save(deps.storage, &admin)?;
    RACE_ENGINE.save(deps.storage, &race_engine)?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("admin", admin)
        .add_attribute("race_engine", race_engine))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, TournamentError> {
    match msg {
        ExecuteMsg::StartTournament {
            criteria,
            track_id,
            max_participants,
        } => execute_start_tournament(deps, _env, criteria, track_id, max_participants),
        ExecuteMsg::RunNextRound {} => execute_run_next_round(deps, _env),
        ExecuteMsg::EndTournament {} => execute_end_tournament(deps, _env),
    }
}

pub fn execute_start_tournament(
    deps: DepsMut,
    env: Env,
    criteria: TournamentCriteria,
    track_id: String,
    max_participants: Option<u32>,
) -> Result<Response, TournamentError> {
    // Validate max participants
    let max_participants = max_participants.unwrap_or(MAX_PARTICIPANTS);
    if max_participants < MIN_PARTICIPANTS || max_participants > MAX_PARTICIPANTS {
        return Err(TournamentError::InvalidParticipantCount { count: max_participants });
    }

    // Generate tournament ID
    let tournament_id = format!("tournament_{}", env.block.time.seconds());
    
    // Select participants based on criteria
    let participants = select_participants(&criteria, max_participants)?;
    
    if participants.len() < MIN_PARTICIPANTS as usize {
        return Err(TournamentError::InsufficientParticipants { 
            required: MIN_PARTICIPANTS, 
            actual: participants.len() as u32 
        });
    }

    // Calculate total rounds needed
    let total_rounds = calculate_total_rounds(participants.len() as u32);
    
    // Generate initial bracket
    let initial_matches = generate_bracket(&participants)?;

    let tournament_state = crate::state::TournamentState {
        tournament_id: tournament_id.clone(),
        status: TournamentStatus::InProgress,
        current_round: 1,
        total_rounds,
        track_id,
        criteria,
        max_participants: Some(max_participants),
        created_at: env.block.time.seconds(),
    };

    // Save tournament state and data
    set_tournament_state(deps.storage, tournament_state)?;
    set_participants(deps.storage, &tournament_id, participants.clone())?;
    set_tournament_matches(deps.storage, &tournament_id, initial_matches)?;

    Ok(Response::new()
        .add_attribute("method", "start_tournament")
        .add_attribute("tournament_id", tournament_id)
        .add_attribute("participants", participants.len().to_string())
        .add_attribute("total_rounds", total_rounds.to_string()))
}

pub fn execute_run_next_round(
    deps: DepsMut,
    env: Env,
) -> Result<Response, TournamentError> {
    let mut tournament_state = get_tournament_state(deps.storage)?;
    
    // Check if tournament is in progress
    if tournament_state.status != TournamentStatus::InProgress {
        return Err(TournamentError::TournamentNotInProgress { 
            status: tournament_state.status 
        });
    }

    // Check if we've completed all rounds
    if tournament_state.current_round > tournament_state.total_rounds {
        return Err(TournamentError::AllRoundsCompleted { 
            current: tournament_state.current_round, 
            total: tournament_state.total_rounds 
        });
    }

    // Get current matches for this round
    let tournament_id = tournament_state.tournament_id.clone();
    let mut current_matches = get_tournament_matches(deps.storage, &tournament_id)
        .unwrap_or(vec![]);

    // Filter matches for current round (using match_id to determine round)
    let round_matches: Vec<TournamentMatch> = current_matches.clone()
        .into_iter()
        .filter(|m| {
            // Extract round from match_id format "match_round_number"
            m.match_id.starts_with(&format!("match_{}_", tournament_state.current_round))
        })
        .collect();

    if round_matches.is_empty() {
        return Err(TournamentError::NoMatchesForRound { 
            round: tournament_state.current_round 
        });
    }

    // Simulate all matches in this round
    let mut round_winners = vec![];
    for match_data in round_matches {
        let winner = simulate_match(deps.as_ref(), &tournament_state.track_id, &match_data)?;
        round_winners.push(winner);
    }

    // Generate next round matches if not the final round
    if tournament_state.current_round < tournament_state.total_rounds {
        let next_round_matches = generate_next_round_matches(&round_winners, tournament_state.current_round + 1)?;
        current_matches.extend(next_round_matches);
        set_tournament_matches(deps.storage, &tournament_id, current_matches)?;
    } else {
        // Final round - determine winner
        if round_winners.len() == 1 {
            let winner = round_winners[0].clone();
            let final_rankings = vec![
                TournamentRanking {
                    car_id: winner,
                    rank: 1,
                    wins: 1,
                    losses: 0,
                }
            ];
            set_tournament_results(deps.storage, &tournament_id, final_rankings)?;
            tournament_state.status = TournamentStatus::Completed;
        }
    }

    // Update tournament state
    tournament_state.current_round += 1;
    set_tournament_state(deps.storage, tournament_state.clone())?;

    Ok(Response::new()
        .add_attribute("method", "run_next_round")
        .add_attribute("tournament_id", tournament_id)
        .add_attribute("round", tournament_state.current_round.to_string())
        .add_attribute("matches_played", round_winners.len().to_string()))
}

pub fn execute_end_tournament(
    deps: DepsMut,
    _env: Env,
) -> Result<Response, TournamentError> {
    let tournament_state = get_tournament_state(deps.storage)?;
    
    // Check if tournament is completed
    if tournament_state.status != TournamentStatus::Completed {
        return Err(TournamentError::TournamentNotCompleted { 
            status: tournament_state.status 
        });
    }

    // Get final results
    let tournament_id = tournament_state.tournament_id.clone();
    let final_rankings = get_tournament_results(deps.storage, &tournament_id)
        .unwrap_or(vec![]);

    if final_rankings.is_empty() {
        return Err(TournamentError::NoFinalResults {});
    }

    let winner = final_rankings.first().map(|r| r.car_id.clone());

    Ok(Response::new()
        .add_attribute("method", "end_tournament")
        .add_attribute("tournament_id", tournament_id)
        .add_attribute("winner", winner.unwrap_or_default())
        .add_attribute("total_participants", final_rankings.len().to_string()))
}

/// Select participants based on criteria
fn select_participants(
    criteria: &TournamentCriteria,
    max_participants: u32,
) -> Result<Vec<String>, TournamentError> {
    // In a real implementation, this would query other contracts
    // For now, generate mock participants based on criteria
    let mut participants = vec![];
    
    match criteria {
        TournamentCriteria::Random => {
            // Generate random car IDs
            for i in 0..max_participants {
                participants.push(format!("car_{}", i + 1));
            }
        }
        TournamentCriteria::TopTrained { min_training_updates } => {
            // Query trainer contract for cars with sufficient training
            // In a real implementation, this would query the trainer contract
            // For now, generate mock participants
            for i in 0..max_participants {
                participants.push(format!("trained_car_{}", i + 1));
            }
        }
        TournamentCriteria::AllCars => {
            // Query car contract for all available cars
            // In a real implementation, this would query the car contract
            // For now, generate mock participants
            for i in 0..max_participants {
                participants.push(format!("all_car_{}", i + 1));
            }
        }
    }

    Ok(participants)
}

/// Calculate total rounds needed for tournament
fn calculate_total_rounds(participant_count: u32) -> u32 {
    let mut rounds = 0;
    let mut remaining = participant_count;
    
    while remaining > 1 {
        remaining = (remaining + 1) / 2; // Ceiling division
        rounds += 1;
    }
    
    rounds
}

/// Generate initial bracket
fn generate_bracket(participants: &[String]) -> Result<Vec<TournamentMatch>, TournamentError> {
    let mut matches = vec![];
    let mut shuffled = participants.to_vec();
    
    // Simple shuffle (in real implementation, use proper randomization)
    for i in 0..shuffled.len() / 2 {
        let idx1 = i * 2;
        let idx2 = i * 2 + 1;
        if idx2 < shuffled.len() {
            matches.push(TournamentMatch {
                match_id: format!("match_1_{}", i + 1),
                car1: shuffled[idx1].clone(),
                car2: shuffled[idx2].clone(),
                winner: None,
                completed: false,
            });
        }
    }

    Ok(matches)
}

/// Generate matches for next round
fn generate_next_round_matches(
    winners: &[String],
    round: u32,
) -> Result<Vec<TournamentMatch>, TournamentError> {
    let mut matches = vec![];
    
    for i in 0..winners.len() / 2 {
        let idx1 = i * 2;
        let idx2 = i * 2 + 1;
        if idx2 < winners.len() {
            matches.push(TournamentMatch {
                match_id: format!("match_{}_{}", round, i + 1),
                car1: winners[idx1].clone(),
                car2: winners[idx2].clone(),
                winner: None,
                completed: false,
            });
        }
    }

    Ok(matches)
}

/// Simulate a single match
fn simulate_match(
    deps: Deps,
    track_id: &str,
    match_data: &TournamentMatch,
) -> Result<String, TournamentError> {
    // In a real implementation, this would call the race engine
    // For now, simulate a simple winner selection
    
    // Simulate race by choosing winner based on car ID (simple deterministic logic)
    let car1_score = match_data.car1.chars().map(|c| c as u32).sum::<u32>();
    let car2_score = match_data.car2.chars().map(|c| c as u32).sum::<u32>();
    
    let winner = if car1_score > car2_score {
        match_data.car1.clone()
    } else {
        match_data.car2.clone()
    };

    Ok(winner)
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetCurrentBracket {} => to_json_binary(&query_current_bracket(deps).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::GetTournamentResults {} => to_json_binary(&query_tournament_results(deps).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::IsParticipant { car_id } => to_json_binary(&query_is_participant(deps, car_id).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::GetTournamentState {} => to_json_binary(&query_tournament_state(deps).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
    }
}

pub fn query_current_bracket(deps: Deps) -> Result<crate::msg::GetCurrentBracketResponse, TournamentError> {
    let tournament_state = get_tournament_state(deps.storage)?;
    let participants = get_participants(deps.storage, &tournament_state.tournament_id).unwrap_or(vec![]);
    let matches = get_tournament_matches(deps.storage, &tournament_state.tournament_id).unwrap_or(vec![]);
    
    // Filter matches for current round
    let current_matches: Vec<TournamentMatch> = matches
        .into_iter()
        .filter(|m| {
            // Extract round from match_id format "match_round_number"
            m.match_id.starts_with(&format!("match_{}_", tournament_state.current_round))
        })
        .collect();
    
    Ok(crate::msg::GetCurrentBracketResponse {
        round: tournament_state.current_round,
        matches: current_matches,
        participants,
    })
}

pub fn query_tournament_results(deps: Deps) -> Result<crate::msg::GetTournamentResultsResponse, TournamentError> {
    let tournament_state = get_tournament_state(deps.storage)?;
    let results = get_tournament_results(deps.storage, &tournament_state.tournament_id).unwrap_or(vec![]);
    
    let winner = results.first().map(|r| r.car_id.clone());
    
    Ok(crate::msg::GetTournamentResultsResponse {
        tournament_id: tournament_state.tournament_id,
        winner,
        final_rankings: results.clone(),
        total_participants: results.len() as u32,
    })
}

pub fn query_is_participant(deps: Deps, car_id: String) -> Result<crate::msg::IsParticipantResponse, TournamentError> {
    let tournament_state = get_tournament_state(deps.storage)?;
    let participants = get_participants(deps.storage, &tournament_state.tournament_id).unwrap_or(vec![]);
    let is_participant = participants.contains(&car_id);
    
    Ok(crate::msg::IsParticipantResponse {
        car_id,
        is_participant,
    })
}

pub fn query_tournament_state(deps: Deps) -> Result<crate::msg::GetTournamentStateResponse, TournamentError> {
    let tournament_state = get_tournament_state(deps.storage)?;
    let participants = get_participants(deps.storage, &tournament_state.tournament_id).unwrap_or(vec![]);
    
    Ok(crate::msg::GetTournamentStateResponse {
        tournament_id: tournament_state.tournament_id,
        status: tournament_state.status,
        current_round: tournament_state.current_round,
        total_rounds: tournament_state.total_rounds,
        participants,
        track_id: tournament_state.track_id,
    })
} 