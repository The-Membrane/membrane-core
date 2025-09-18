use cosmwasm_std::{
    entry_point, to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult,
    CosmosMsg, WasmMsg, Uint128, Decimal, BankMsg, Coin
};
use std::str::FromStr;

use crate::error::TournamentError;
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
use membrane::tournament::MigrateMsg;
use crate::state::{
    get_tournament_state, set_tournament_state, 
    get_participants, set_participants, get_tournament_results, set_tournament_results, 
    get_tournament_matches, set_tournament_matches, get_pre_registrations, set_pre_registrations,
    get_car_stats, set_car_stats, get_config, set_config,
    get_scheduled_tournament, set_scheduled_tournament
};
use membrane::types::{TournamentCriteria, TournamentStatus, TournamentMatch, TournamentRanking};
use membrane::tournament::{Registration, CarStats, ScheduledTournament};

// Tournament constants
const MAX_PARTICIPANTS: u32 = 32;
const MIN_PARTICIPANTS: u32 = 2;

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, TournamentError> {
    let admin = deps.api.addr_validate(&msg.admin)?;
    let race_engine = deps.api.addr_validate(&msg.race_engine)?;
    let byte_minter = deps.api.addr_validate(&msg.byte_minter)?;
    let car_contract = deps.api.addr_validate(&msg.car_contract)?;
    
    // ADMIN.save(deps.storage, &admin)?;
    // RACE_ENGINE.save(deps.storage, &race_engine)?;
    // BYTE_MINTER.save(deps.storage, &byte_minter)?;
    // CAR_CONTRACT.save(deps.storage, &car_contract)?;

    // Query byte_minter config to get mint information
    let byte_minter_config: membrane::byte_minter::Config = deps.querier.query_wasm_smart(
        &byte_minter,
        &membrane::byte_minter::QueryMsg::GetConfig {}
    )?;

    // Save config with mint information
    let config = membrane::tournament::Config {
        admin: admin.to_string(),
        race_engine: race_engine.to_string(),
        byte_minter: byte_minter.to_string(),
        car_contract: car_contract.to_string(),
        tokenfactory_denom: byte_minter_config.tokenfactory_denom,
        mint_amount_per_round_win: byte_minter_config.mint_amount_per_round_win
            .unwrap_or(Uint128::from(1000u128)), // Default if not set
        reward_round_scalar: byte_minter_config.reward_round_scalar
            .unwrap_or(Decimal::from_str("1.5").unwrap()), // Default if not set
        allow_free_registration: Some(true), // Default to true
    };
    set_config(deps.storage, config)?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("admin", admin)
        .add_attribute("race_engine", race_engine)
        .add_attribute("byte_minter", byte_minter)
        .add_attribute("car_contract", car_contract))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, TournamentError> {
    match msg {
        ExecuteMsg::StartTournament {
            criteria,
            track_id,
            max_participants,
            registration_payment_options,
            max_ticks,
        } => execute_start_tournament(deps, env, info, criteria, track_id, max_participants, registration_payment_options, max_ticks),
        ExecuteMsg::RegisterForTournament { car_id } => execute_register_for_tournament(deps, env, info, car_id),
        ExecuteMsg::RunNextMatch {} => execute_run_next_match(deps, env),
        ExecuteMsg::EndTournament {} => execute_end_tournament(deps, env),
        ExecuteMsg::UpdateConfig { race_engine, byte_minter, car_contract, tokenfactory_denom, mint_amount_per_round_win, reward_round_scalar, allow_free_registration } => execute_update_config(deps, info, race_engine, byte_minter, car_contract, tokenfactory_denom, mint_amount_per_round_win, reward_round_scalar, allow_free_registration),
        ExecuteMsg::EnableWeeklyTournaments { criteria, track_id, max_participants, registration_payment_options, max_ticks } => execute_enable_weekly_tournaments(deps, info, criteria, track_id, max_participants, registration_payment_options, max_ticks),
        ExecuteMsg::DisableWeeklyTournaments {} => execute_disable_weekly_tournaments(deps, info),
        ExecuteMsg::CheckAndStartScheduledTournament {} => execute_check_and_start_scheduled_tournament(deps, env),
    }
}

pub fn execute_start_tournament(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    criteria: TournamentCriteria,
    track_id: String,
    max_participants: Option<u32>,
    registration_payment_options: Vec<cosmwasm_std::Coin>,
    max_ticks: u32,
) -> Result<Response, TournamentError> {
    // Check admin authorization
    let config = get_config(deps.storage)?;
    let admin = deps.api.addr_validate(&config.admin)?;
    if info.sender != admin {
        return Err(TournamentError::Unauthorized {});
    }

    // Validate max participants
    let max_participants = max_participants.unwrap_or(MAX_PARTICIPANTS);
    if max_participants < MIN_PARTICIPANTS {
        return Err(TournamentError::InvalidParticipantCount { count: max_participants });
    }

    // Generate tournament ID
    let tournament_id = format!("tournament_{}", env.block.time.seconds());
    
    // Get pre-registrations
    let registrations = get_pre_registrations(deps.storage)?;
    let participants: Vec<u128> = registrations.iter().map(|r| r.car_id).collect();
    
    if participants.len() < MIN_PARTICIPANTS as usize {
        return Err(TournamentError::InsufficientParticipants { 
            required: MIN_PARTICIPANTS, 
            actual: participants.len() as u32 
        });
    }

    // Clear pre-registrations for next tournament
    set_pre_registrations(deps.storage, vec![])?;

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
        registration_payment_options,
        max_ticks,
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

pub fn execute_register_for_tournament(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    car_id: u128,
) -> Result<Response, TournamentError> {
    // Check if tournament is already in progress
    if let Ok(tournament_state) = get_tournament_state(deps.storage) {
        if tournament_state.status == TournamentStatus::InProgress {
            return Err(TournamentError::TournamentNotInProgress { 
                status: tournament_state.status 
            });
        }
    }

    // Get current registrations
    let registrations = get_pre_registrations(deps.storage)?;
    
    // Check if car is already registered
    if registrations.iter().any(|r| r.car_id == car_id) {
        return Err(TournamentError::Std(cosmwasm_std::StdError::generic_err("Car already registered")));
    }

    // Check max participants limit
    let max_participants = tournament_state.max_participants.unwrap_or(MAX_PARTICIPANTS);
    if registrations.len() >= max_participants as usize {
        return Err(TournamentError::InvalidParticipantCount { count: registrations.len() as u32 + 1 });
    }

    // Handle payment
    let config = get_config(deps.storage)?;
    let payment_options = tournament_state.registration_payment_options.clone();
    let payment_option = if config.allow_free_registration.unwrap_or(false) && payment_options.is_empty() {
        None // Free registration
    } else {
        // Check payment
        let sent = &info.funds;
        let paid_ok = payment_options.iter().any(|coin| {
            sent.iter().any(|c| c.denom == coin.denom && c.amount >= coin.amount)
        });
        
        if !paid_ok {
            return Err(TournamentError::Std(cosmwasm_std::StdError::generic_err("Insufficient payment")));
        }

        // Find the payment option used
        payment_options.iter().find(|coin| {
            sent.iter().any(|c| c.denom == coin.denom && c.amount >= coin.amount)
        }).cloned()
    };

    // Add registration
    let mut updated_registrations = registrations.clone();
    updated_registrations.push(Registration {
        car_id,
        registered_at: env.block.time.seconds(),
        payment_option: payment_option.clone(),
    });

    set_pre_registrations(deps.storage, updated_registrations.clone())?;

    // Send payment to car contract
    let mut messages = vec![];
    if let Some(payment) = payment_option.clone() {
        let config = get_config(deps.storage)?;
        let car_contract = deps.api.addr_validate(&config.car_contract)?;
        messages.push(CosmosMsg::Bank(cosmwasm_std::BankMsg::Send {
            to_address: car_contract.to_string(),
            amount: vec![payment],
        }));
    }

    Ok(Response::new()
        .add_messages(messages)
        .add_attribute("method", "register_for_tournament")
        .add_attribute("car_id", car_id.to_string())
        .add_attribute("total_registrations", updated_registrations.len().to_string()))
}

pub fn execute_run_next_match(
    deps: DepsMut,
    _env: Env,
) -> Result<Response, TournamentError> {
    let tournament_state = get_tournament_state(deps.storage)?;
    
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

    // Find the next incomplete match for the current round
    let next_match_index = current_matches.iter().position(|m| {
        m.match_id.starts_with(&format!("match_{}_", tournament_state.current_round)) && !m.completed
    });

    if let Some(match_index) = next_match_index {
        // We have a match to run
        let match_data = current_matches[match_index].clone();
        
        // Simulate the single match using race engine
        let winner = simulate_match_with_race_engine(
            deps.as_ref(), 
            &tournament_state.track_id, 
            &match_data,
            tournament_state.max_ticks,
        )?;
        
        // Update car stats for winner and loser
        let winner_stats = get_car_stats(deps.storage, winner)?;
        let updated_winner_stats = CarStats {
            car_id: winner_stats.car_id,
            round_wins: winner_stats.round_wins + 1,
            round_losses: winner_stats.round_losses,
            tournament_wins: winner_stats.tournament_wins,
        };
        set_car_stats(deps.storage, winner, updated_winner_stats)?;
        
        let loser = if match_data.car1 == winner { match_data.car2 } else { match_data.car1 };
        let loser_stats = get_car_stats(deps.storage, loser)?;
        let updated_loser_stats = CarStats {
            car_id: loser_stats.car_id,
            round_wins: loser_stats.round_wins,
            round_losses: loser_stats.round_losses + 1,
            tournament_wins: loser_stats.tournament_wins,
        };
        set_car_stats(deps.storage, loser, updated_loser_stats)?;
        
        // Mark match as completed and set winner
        current_matches[match_index].winner = Some(winner);
        current_matches[match_index].completed = true;
        
        // Save updated matches
        set_tournament_matches(deps.storage, &tournament_id, current_matches)?;
        
        // Mint rewards for match winner
        let config = get_config(deps.storage)?;
        let mint_msg = create_round_winner_mint_msg(&config, winner)?;

        return Ok(Response::new()
            .add_message(mint_msg)
            .add_attribute("method", "run_next_match")
            .add_attribute("tournament_id", tournament_id)
            .add_attribute("match_id", match_data.match_id)
            .add_attribute("winner", winner.to_string())
            .add_attribute("round", tournament_state.current_round.to_string()));
    } else {
        // No more matches in current round, check if we need to advance to next round
        let round_matches: Vec<&TournamentMatch> = current_matches.iter()
            .filter(|m| m.match_id.starts_with(&format!("match_{}_", tournament_state.current_round)))
        .collect();

    if round_matches.is_empty() {
        return Err(TournamentError::NoMatchesForRound { 
            round: tournament_state.current_round 
        });
    }

        // All matches in current round are completed, advance to next round
        let round_winners: Vec<u128> = round_matches.iter()
            .filter_map(|m| m.winner)
            .collect();

        if round_winners.is_empty() {
            return Err(TournamentError::NoMatchesForRound { 
                round: tournament_state.current_round 
            });
    }

    // Generate next round matches if not the final round
    if tournament_state.current_round < tournament_state.total_rounds {
        let next_round_matches = generate_next_round_matches(&round_winners, tournament_state.current_round + 1)?;
        current_matches.extend(next_round_matches);
        set_tournament_matches(deps.storage, &tournament_id, current_matches)?;
            let mut updated_state = tournament_state.clone();
            updated_state.current_round += 1;
            let new_round = updated_state.current_round;
            set_tournament_state(deps.storage, updated_state)?;
            
            return Ok(Response::new()
                .add_attribute("method", "run_next_match")
                .add_attribute("tournament_id", tournament_id)
                .add_attribute("round_advanced", new_round.to_string())
                .add_attribute("message", "Round completed, advanced to next round"));
    } else {
            // Final round completed - determine winner
        if round_winners.len() == 1 {
                let winner = round_winners[0];
                
                // Update tournament winner stats
                let winner_stats = get_car_stats(deps.storage, winner)?;
                let updated_winner_stats = CarStats {
                    car_id: winner_stats.car_id,
                    round_wins: winner_stats.round_wins,
                    round_losses: winner_stats.round_losses,
                    tournament_wins: winner_stats.tournament_wins + 1,
                };
                set_car_stats(deps.storage, winner, updated_winner_stats)?;
                
            let final_rankings = vec![
                TournamentRanking {
                    car_id: winner,
                    rank: 1,
                    wins: 1,
                    losses: 0,
                }
            ];
            set_tournament_results(deps.storage, &tournament_id, final_rankings)?;
                let mut updated_state = tournament_state.clone();
                updated_state.status = TournamentStatus::Completed;
                set_tournament_state(deps.storage, updated_state)?;
                
                // Mint tournament winner reward (scaled by rounds)
                let config = get_config(deps.storage)?;
                let tournament_winner_msg = create_tournament_winner_mint_msg(&config, winner, tournament_state.total_rounds)?;
                
                return Ok(Response::new()
                    .add_message(tournament_winner_msg)
                    .add_attribute("method", "run_next_match")
                    .add_attribute("tournament_id", tournament_id)
                    .add_attribute("tournament_completed", "true")
                    .add_attribute("winner", winner.to_string()));
            } else {
                return Err(TournamentError::NoFinalResults {});
            }
        }
    }
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

    let winner = final_rankings.first().map(|r| r.car_id);

    Ok(Response::new()
        .add_attribute("method", "end_tournament")
        .add_attribute("tournament_id", tournament_id)
        .add_attribute("winner", winner.unwrap_or(0).to_string())
        .add_attribute("total_participants", final_rankings.len().to_string()))
}

pub fn execute_update_config(
    deps: DepsMut,
    info: MessageInfo,
    race_engine: Option<String>,
    byte_minter: Option<String>,
    car_contract: Option<String>,
    tokenfactory_denom: Option<String>,
    mint_amount_per_round_win: Option<Uint128>,
    reward_round_scalar: Option<Decimal>,
    allow_free_registration: Option<bool>,
) -> Result<Response, TournamentError> {
    // Check admin authorization
    let current_config = get_config(deps.storage)?;
    let admin = deps.api.addr_validate(&current_config.admin)?;
    if info.sender != admin {
        return Err(TournamentError::Unauthorized {});
    }

    // Load current config
    let mut config = current_config;

    // Check which fields are being updated before moving the values
    let race_engine_updated = race_engine.is_some();
    let byte_minter_updated = byte_minter.is_some();
    let car_contract_updated = car_contract.is_some();
    let tokenfactory_denom_updated = tokenfactory_denom.is_some();
    let mint_amount_updated = mint_amount_per_round_win.is_some();
    let reward_scalar_updated = reward_round_scalar.is_some();
    let _allow_free_registration_updated = allow_free_registration.is_some();

    // Update config fields if provided
    if let Some(race_engine_addr) = race_engine {
        let _validated_addr = deps.api.addr_validate(&race_engine_addr)?;
        config.race_engine = race_engine_addr;
    }

    if let Some(byte_minter_addr) = byte_minter {
        let _validated_addr = deps.api.addr_validate(&byte_minter_addr)?;
        config.byte_minter = byte_minter_addr;
    }

    if let Some(car_contract_addr) = car_contract {
        let _validated_addr = deps.api.addr_validate(&car_contract_addr)?;
        config.car_contract = car_contract_addr;
    }

    if let Some(denom) = tokenfactory_denom {
        config.tokenfactory_denom = denom;
    }

    if let Some(amount) = mint_amount_per_round_win {
        config.mint_amount_per_round_win = amount;
    }

    if let Some(scalar) = reward_round_scalar {
        config.reward_round_scalar = scalar;
    }

    if let Some(allow_free) = allow_free_registration {
        config.allow_free_registration = Some(allow_free);
    }

    // Save updated config
    set_config(deps.storage, config)?;

    Ok(Response::new()
        .add_attribute("method", "update_config")
        .add_attribute("updated_fields", format!("race_engine:{}, byte_minter:{}, car_contract:{}, tokenfactory_denom:{}, mint_amount:{}, reward_scalar:{}, allow_free_registration:{}", 
            race_engine_updated, byte_minter_updated, car_contract_updated, tokenfactory_denom_updated, mint_amount_updated, reward_scalar_updated, _allow_free_registration_updated)))
}

pub fn execute_enable_weekly_tournaments(
    deps: DepsMut,
    info: MessageInfo,
    criteria: TournamentCriteria,
    track_id: String,
    max_participants: Option<u32>,
    registration_payment_options: Vec<cosmwasm_std::Coin>,
    max_ticks: u32,
) -> Result<Response, TournamentError> {
    // Check admin authorization
    let config = get_config(deps.storage)?;
    let admin = deps.api.addr_validate(&config.admin)?;
    if info.sender != admin {
        return Err(TournamentError::Unauthorized {});
    }

    // Validate max participants
    let max_participants = max_participants.unwrap_or(MAX_PARTICIPANTS);
    if max_participants < MIN_PARTICIPANTS {
        return Err(TournamentError::InvalidParticipantCount { count: max_participants });
    }

    // Create scheduled tournament configuration
    let scheduled_tournament = ScheduledTournament {
        enabled: true,
        criteria,
        track_id: track_id.clone(),
        max_participants: Some(max_participants),
        registration_payment_options,
        max_ticks,
        last_sunday_start: None,
    };

    set_scheduled_tournament(deps.storage, scheduled_tournament)?;

    Ok(Response::new()
        .add_attribute("method", "enable_weekly_tournaments")
        .add_attribute("enabled", "true")
        .add_attribute("track_id", track_id)
        .add_attribute("max_participants", max_participants.to_string()))
}

pub fn execute_disable_weekly_tournaments(
    deps: DepsMut,
    info: MessageInfo,
) -> Result<Response, TournamentError> {
    // Check admin authorization
    let config = get_config(deps.storage)?;
    let admin = deps.api.addr_validate(&config.admin)?;
    if info.sender != admin {
        return Err(TournamentError::Unauthorized {});
    }

    // Disable scheduled tournaments
    let scheduled_tournament = get_scheduled_tournament(deps.storage)?;
    let mut updated_scheduled = scheduled_tournament;
    updated_scheduled.enabled = false;
    set_scheduled_tournament(deps.storage, updated_scheduled)?;

    Ok(Response::new()
        .add_attribute("method", "disable_weekly_tournaments")
        .add_attribute("enabled", "false"))
}

pub fn execute_check_and_start_scheduled_tournament(
    deps: DepsMut,
    env: Env,
) -> Result<Response, TournamentError> {
    let scheduled_tournament = get_scheduled_tournament(deps.storage)?;
    
    // Check if scheduled tournaments are enabled
    if !scheduled_tournament.enabled {
        return Ok(Response::new()
            .add_attribute("method", "check_and_start_scheduled_tournament")
            .add_attribute("status", "disabled"));
    }

    // Check if there's already a tournament in progress
    let tournament_state = get_tournament_state(deps.storage)?;
    if tournament_state.status == TournamentStatus::InProgress {
        return Ok(Response::new()
            .add_attribute("method", "check_and_start_scheduled_tournament")
            .add_attribute("status", "tournament_already_in_progress"));
    }

    // Check if it's Sunday and we haven't started a tournament this Sunday yet
    let current_time = env.block.time.seconds();
    let current_sunday_start = get_current_sunday_start(current_time);
    
    if let Some(last_start) = scheduled_tournament.last_sunday_start {
        if last_start >= current_sunday_start {
            return Ok(Response::new()
                .add_attribute("method", "check_and_start_scheduled_tournament")
                .add_attribute("status", "already_started_this_sunday"));
        }
    }

    // Check if we have enough participants
    let registrations = get_pre_registrations(deps.storage)?;
    if registrations.len() < MIN_PARTICIPANTS as usize {
        return Ok(Response::new()
            .add_attribute("method", "check_and_start_scheduled_tournament")
            .add_attribute("status", "insufficient_participants")
            .add_attribute("participants", registrations.len().to_string()));
    }

    // Start the tournament using the scheduled configuration
    let participants: Vec<u128> = registrations.iter().map(|r| r.car_id).collect();
    let total_rounds = calculate_total_rounds(participants.len() as u32);
    let initial_matches = generate_bracket(&participants)?;

    let tournament_id = format!("scheduled_tournament_{}", current_time);
    
    let tournament_state = crate::state::TournamentState {
        tournament_id: tournament_id.clone(),
        status: TournamentStatus::InProgress,
        current_round: 1,
        total_rounds,
        track_id: scheduled_tournament.track_id.clone(),
        criteria: scheduled_tournament.criteria.clone(),
        max_participants: scheduled_tournament.max_participants,
        created_at: current_time,
        registration_payment_options: scheduled_tournament.registration_payment_options.clone(),
        max_ticks: scheduled_tournament.max_ticks,
    };

    // Save tournament state and data
    set_tournament_state(deps.storage, tournament_state)?;
    set_participants(deps.storage, &tournament_id, participants.clone())?;
    set_tournament_matches(deps.storage, &tournament_id, initial_matches)?;

    // Clear pre-registrations for next tournament
    set_pre_registrations(deps.storage, vec![])?;

    // Update scheduled tournament with this Sunday's start time
    let mut updated_scheduled = scheduled_tournament;
    updated_scheduled.last_sunday_start = Some(current_sunday_start);
    set_scheduled_tournament(deps.storage, updated_scheduled)?;

    Ok(Response::new()
        .add_attribute("method", "check_and_start_scheduled_tournament")
        .add_attribute("status", "tournament_started")
        .add_attribute("tournament_id", tournament_id)
        .add_attribute("participants", participants.len().to_string())
        .add_attribute("total_rounds", total_rounds.to_string())
        .add_attribute("sunday_start", current_sunday_start.to_string()))
}


/// Get the start of the current Sunday (00:00:00 UTC)
fn get_current_sunday_start(current_time: u64) -> u64 {
    // Unix timestamp of January 1, 1970 00:00:00 UTC was a Thursday (day 4)
    // So we need to calculate days since epoch and find the current Sunday
    let seconds_per_day = 24 * 60 * 60; // 86400 seconds
    let days_since_epoch = current_time / seconds_per_day;
    
    // January 1, 1970 was a Thursday (day 4 in week, where Sunday = 0)
    let day_of_week = (days_since_epoch + 4) % 7;
    
    // Calculate days to subtract to get to Sunday
    let days_to_subtract = if day_of_week == 0 { 0 } else { day_of_week };
    
    // Get the start of current Sunday
    let sunday_start_day = days_since_epoch - days_to_subtract;
    sunday_start_day * seconds_per_day
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
fn generate_bracket(participants: &[u128]) -> Result<Vec<TournamentMatch>, TournamentError> {
    let mut matches = vec![];
    let shuffled = participants.to_vec();
    
    // Simple shuffle (in real implementation, use proper randomization)
    for i in 0..shuffled.len() / 2 {
        let idx1 = i * 2;
        let idx2 = i * 2 + 1;
        if idx2 < shuffled.len() {
            matches.push(TournamentMatch {
                match_id: format!("match_1_{}", i + 1),
                car1: shuffled[idx1],
                car2: shuffled[idx2],
                winner: None,
                completed: false,
            });
        }
    }

    Ok(matches)
}

/// Generate matches for next round
fn generate_next_round_matches(
    winners: &[u128],
    round: u32,
) -> Result<Vec<TournamentMatch>, TournamentError> {
    let mut matches = vec![];
    
    for i in 0..winners.len() / 2 {
        let idx1 = i * 2;
        let idx2 = i * 2 + 1;
        if idx2 < winners.len() {
            matches.push(TournamentMatch {
                match_id: format!("match_{}_{}", round, i + 1),
                car1: winners[idx1],
                car2: winners[idx2],
                winner: None,
                completed: false,
            });
        }
    }

    Ok(matches)
}

/// Create mint message for round winner
fn create_round_winner_mint_msg(config: &membrane::tournament::Config, winner: u128) -> Result<CosmosMsg, TournamentError> {
    let mint_msg = CosmosMsg::Bank(BankMsg::Send {
        to_address: winner.to_string(),
        amount: vec![Coin {
            denom: config.tokenfactory_denom.clone(),
            amount: config.mint_amount_per_round_win,
        }],
    });
    
    Ok(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.byte_minter.clone(),
        msg: to_json_binary(&membrane::byte_minter::ExecuteMsg::TokenfactoryPassthrough {
            msgs: vec![mint_msg],
        })?,
        funds: vec![],
    }))
}

/// Create mint message for tournament winner (scaled by rounds)
fn create_tournament_winner_mint_msg(config: &membrane::tournament::Config, winner: u128, rounds_to_win: u32) -> Result<CosmosMsg, TournamentError> {
    // Calculate scaled amount: scalar * rounds_to_win * mint_amount_per_round_win
    let rounds_decimal = Decimal::from_str(&rounds_to_win.to_string())
        .map_err(|e| TournamentError::Std(cosmwasm_std::StdError::generic_err(format!("Invalid rounds: {}", e))))?;
    let mint_amount_decimal = Decimal::from_str(&config.mint_amount_per_round_win.to_string())
        .map_err(|e| TournamentError::Std(cosmwasm_std::StdError::generic_err(format!("Invalid mint amount: {}", e))))?;
    let scaled_amount = config.reward_round_scalar
        .checked_mul(rounds_decimal)
        .map_err(|_| TournamentError::Std(cosmwasm_std::StdError::generic_err("Overflow in reward calculation")))?
        .checked_mul(mint_amount_decimal)
        .map_err(|_| TournamentError::Std(cosmwasm_std::StdError::generic_err("Overflow in reward calculation")))?
        .to_uint_ceil();
    
    let mint_msg = CosmosMsg::Bank(BankMsg::Send {
        to_address: winner.to_string(),
        amount: vec![Coin {
            denom: config.tokenfactory_denom.clone(),
            amount: scaled_amount,
        }],
    });
    
    Ok(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.byte_minter.clone(),
        msg: to_json_binary(&membrane::byte_minter::ExecuteMsg::TokenfactoryPassthrough {
            msgs: vec![mint_msg],
        })?,
        funds: vec![],
    }))
}

/// Simulate a single match using race engine
fn simulate_match_with_race_engine(
    deps: Deps,
    track_id: &str,
    match_data: &TournamentMatch,
    _max_ticks: u32,
) -> Result<u128, TournamentError> {
    let config = get_config(deps.storage)?;
    let race_engine = deps.api.addr_validate(&config.race_engine)?;
    
    // Query the race engine for the race result
    let race_result: membrane::race_engine::RaceResult = deps.querier.query_wasm_smart(
        race_engine,
        &membrane::race_engine::QueryMsg::GetRaceResult {
            track_id: track_id.parse::<u128>().map_err(|_| TournamentError::Std(cosmwasm_std::StdError::generic_err("Invalid track_id")))?,
            race_id: format!("tournament_match_{}", match_data.match_id),
        }
    ).map_err(|_| TournamentError::RaceSimulationFailed {})?;

    // Get the winner from race result
    if let Some(winner_id) = race_result.winner_ids.first() {
        Ok(*winner_id)
    }
    else if let Some(winner_id) = race_result.rankings.first() {
        Ok(winner_id.car_id)
    } else {
        Err(TournamentError::RaceSimulationFailed {})
    }
}

#[entry_point]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, TournamentError> {
    // Get current config
    let mut config = get_config(deps.storage)?;
    
    // Set allow_free_registration to Some(true) if it's not already set
    if config.allow_free_registration.is_none() {
        config.allow_free_registration = Some(true);
        set_config(deps.storage, config)?;
    }
    
    Ok(Response::new()
        .add_attribute("method", "migrate")
        .add_attribute("allow_free_registration", "set_to_true"))
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetCurrentBracket {} => to_json_binary(&query_current_bracket(deps).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::GetTournamentResults {} => to_json_binary(&query_tournament_results(deps).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::IsParticipant { car_id } => to_json_binary(&query_is_participant(deps, car_id).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::GetTournamentState {} => to_json_binary(&query_tournament_state(deps).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::GetRegistrations {} => to_json_binary(&query_registrations(deps).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::GetCarStats { car_id } => to_json_binary(&query_car_stats(deps, car_id).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::GetConfig {} => to_json_binary(&query_config(deps).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::GetScheduledTournament {} => to_json_binary(&query_scheduled_tournament(deps).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
    }
}

pub fn query_current_bracket(deps: Deps) -> Result<membrane::tournament::GetCurrentBracketResponse, TournamentError> {
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
    
    Ok(membrane::tournament::GetCurrentBracketResponse {
        round: tournament_state.current_round,
        matches: current_matches,
        participants,
    })
}

pub fn query_tournament_results(deps: Deps) -> Result<membrane::tournament::GetTournamentResultsResponse, TournamentError> {
    let tournament_state = get_tournament_state(deps.storage)?;
    let results = get_tournament_results(deps.storage, &tournament_state.tournament_id).unwrap_or(vec![]);
    
    let winner = results.first().map(|r| r.car_id);
    
    Ok(membrane::tournament::GetTournamentResultsResponse {
        tournament_id: tournament_state.tournament_id,
        winner,
        final_rankings: results.clone(),
        total_participants: results.len() as u32,
    })
}

pub fn query_is_participant(deps: Deps, car_id: u128) -> Result<membrane::tournament::IsParticipantResponse, TournamentError> {
    let tournament_state = get_tournament_state(deps.storage)?;
    let participants = get_participants(deps.storage, &tournament_state.tournament_id).unwrap_or(vec![]);
    let is_participant = participants.contains(&car_id);
    
    Ok(membrane::tournament::IsParticipantResponse {
        car_id,
        is_participant,
    })
}

pub fn query_tournament_state(deps: Deps) -> Result<membrane::tournament::GetTournamentStateResponse, TournamentError> {
    let tournament_state = get_tournament_state(deps.storage)?;
    let participants = get_participants(deps.storage, &tournament_state.tournament_id).unwrap_or(vec![]);
    
    Ok(membrane::tournament::GetTournamentStateResponse {
        tournament_id: tournament_state.tournament_id,
        status: tournament_state.status,
        current_round: tournament_state.current_round,
        total_rounds: tournament_state.total_rounds,
        participants,
        track_id: tournament_state.track_id,
    })
}

pub fn query_registrations(deps: Deps) -> Result<membrane::tournament::GetRegistrationsResponse, TournamentError> {
    let registrations = get_pre_registrations(deps.storage)?;
    
    Ok(membrane::tournament::GetRegistrationsResponse {
        registrations,
    })
}

pub fn query_car_stats(deps: Deps, car_id: u128) -> Result<membrane::tournament::GetCarStatsResponse, TournamentError> {
    let stats = get_car_stats(deps.storage, car_id)?;
    
    Ok(membrane::tournament::GetCarStatsResponse {
        stats,
    })
}

pub fn query_config(deps: Deps) -> Result<membrane::tournament::GetConfigResponse, TournamentError> {
    let config = get_config(deps.storage)?;
    
    Ok(membrane::tournament::GetConfigResponse {
        config,
    })
}

pub fn query_scheduled_tournament(deps: Deps) -> Result<membrane::tournament::GetScheduledTournamentResponse, TournamentError> {
    let scheduled_tournament = get_scheduled_tournament(deps.storage)?;
    
    Ok(membrane::tournament::GetScheduledTournamentResponse {
        scheduled_tournament,
    })
} 