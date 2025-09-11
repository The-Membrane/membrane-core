use cosmwasm_std::{entry_point, to_json_binary, Binary, Decimal, Deps, DepsMut, Env, MessageInfo, Response, StdResult};
use serde::Deserialize;
use std::collections::HashMap;

use membrane::race_engine::{MigrateMsg, TrainingConfig};
use membrane::rps_engine::{InstantiateMsg, ExecuteMsg, QueryMsg, ConfigResponse, GetQResponseEntry, GetQResponse, GetHistoryResponse, GetTickHistoryResponse};
use membrane::types::{ActionSelectionStrategy, RpsRewardConfig, SeriesMode, TickRecord};

use crate::error::ContractError; 
use crate::state::{get_config, get_q_values, push_tick_results, set_config, set_q_values, Config, CONFIG, MATCH_HISTORY, TICK_HISTORY};
use crate::state::push_tick_records;

// Actions
pub const ACTION_ROCK: usize = 0;
pub const ACTION_PAPER: usize = 1;
pub const ACTION_SCISSORS: usize = 2;

// Outcomes (from perspective of the player)
pub const OUTCOME_LOSE: u8 = 0;
pub const OUTCOME_DRAW: u8 = 1;
pub const OUTCOME_WIN: u8 = 2;

const DEFAULT_REWARDS: RpsRewardConfig = RpsRewardConfig { 
    win_points: 3, 
    lose_penalty: -3, 
    draw_points: 0, 
    series_win_points: 10
};

// State encoding
// state_id in 0..=8 => (opp_last_move in 0..=2, last_outcome in 0..=2)
// 9 => initial (no prior move/outcome)
pub const INITIAL_STATE_ID: u8 = 9;

// Q-learning params and clamp
const ALPHA: f32 = 0.3; // learning rate - increased for faster learning
const GAMMA: f32 = 0.95; // discount factor - increased for better long-term planning
const Q_MIN: i32 = -128; // expanded range for more nuanced Q-values
const Q_MAX: i32 = 127;

const MATCH_HISTORY_LIMIT: u32 = 100;

// Q-value cache for efficiency during series
type QValueCache = HashMap<(u128, u8), [i8; 3]>;

// Separate caches per player in a series
struct SeriesCaches {
    a: QValueCache,
    b: QValueCache,
}

// Cache management functions
fn get_cached_q_values(cache: &mut QValueCache, storage: &dyn cosmwasm_std::Storage, car_id: u128, state_id: u8) -> [i8; 3] {
    if let Some(&cached) = cache.get(&(car_id, state_id)) {
        cached
    } else {
        let q_values = get_q_values(storage, car_id, state_id).unwrap_or_else(|_| {
            // Initialize with small random values instead of zeros
            let seed = (car_id as u32) ^ (state_id as u32) ^ 12345;
            [
                (pseudo_random(seed, 21) as i8) - 10, // -10 to 10
                (pseudo_random(seed.wrapping_add(1), 21) as i8) - 10,
                (pseudo_random(seed.wrapping_add(2), 21) as i8) - 10,
            ]
        });
        cache.insert((car_id, state_id), q_values);
        q_values
    }
}

// cw721 owner_of query response
#[derive(Deserialize)]
struct OwnerOfResponse { owner: String }

#[entry_point]
pub fn instantiate(deps: DepsMut, _env: Env, _info: MessageInfo, msg: InstantiateMsg) -> Result<Response, ContractError> {
    let admin = deps.api.addr_validate(&msg.admin)?.to_string();
    let car_contract = deps.api.addr_validate(&msg.car_contract)?.to_string();

    let cfg = Config {
        admin,
        car_contract,
        max_ticks: msg.max_ticks.unwrap_or(100),
        match_history_limit: msg.match_history_limit.unwrap_or(MATCH_HISTORY_LIMIT),
        tick_history_limit: msg.tick_history_limit.unwrap_or(500),
    };
    set_config(deps.storage, cfg)?;
    Ok(Response::new().add_attribute("action", "instantiate"))
}

#[entry_point]
pub fn execute(deps: DepsMut, env: Env, info: MessageInfo, msg: ExecuteMsg) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::PlaySeries { car_id, opponent_id, train, training_config, reward_config, mode } => {
            execute_play_series(deps, env, info, car_id, opponent_id.unwrap_or(0), train, training_config, reward_config, mode)
        }
        ExecuteMsg::UpdateConfig { max_ticks, match_history_limit, tick_history_limit } => {
            let mut cfg = get_config(deps.storage)?;
            if info.sender.as_str() != cfg.admin { return Err(ContractError::Unauthorized {}); }
            if let Some(v) = max_ticks { cfg.max_ticks = v; }
            if let Some(v) = match_history_limit { cfg.match_history_limit = v; }
            if let Some(v) = tick_history_limit { cfg.tick_history_limit = v; }
            set_config(deps.storage, cfg)?;
            Ok(Response::new().add_attribute("action", "update_config"))
        }
        // ExecuteMsg::PurgeCar { car_id: _ } => {
        //     // Anyone can purge; removes Q-table and history for the car
        //     // let id = car_id.u128();
        //     // // remove q-table entries
        //     // let prefix = Q_TABLE.prefix(id);
        //     // let keys: Vec<u8> = prefix
        //     //     .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        //     //     .map(|r| r.unwrap().0)
        //     //     .collect();
        //     // for key in keys { Q_TABLE.remove(deps.storage, (id, key)); }
        //     // // remove history
        //     // crate::state::MATCH_HISTORY.remove(deps.storage, id);
        //     Ok(Response::new().add_attribute("action", "purge_car"))
        // }
    }
}

fn decimal_to_f32(d: Decimal) -> f32 {
    let num = d.atomics().u128() as f64;
    let denom = 1e18f64;
    (num / denom) as f32
}

fn default_training_config(train: bool) -> TrainingConfig {
    TrainingConfig { training_mode: train, epsilon: Decimal::percent(30), temperature: Decimal::zero(), enable_epsilon_decay: true }
}

fn choose_action(
    storage: &dyn cosmwasm_std::Storage,
    car_id: u128,
    state_id: u8,
    strategy: ActionSelectionStrategy,
    seed: u32,
    cache: Option<&mut QValueCache>,
) -> Result<(usize, [i8; 3]), ContractError> {
    let q_values = if let Some(cache) = cache {
        get_cached_q_values(cache, storage, car_id, state_id)
    } else {
        get_q_values(storage, car_id, state_id).unwrap_or_else(|_| {
            // Initialize with small random values instead of zeros
            let seed = (car_id as u32) ^ (state_id as u32) ^ 12345;
            [
                (pseudo_random(seed, 21) as i8) - 10, // -10 to 10
                (pseudo_random(seed.wrapping_add(1), 21) as i8) - 10,
                (pseudo_random(seed.wrapping_add(2), 21) as i8) - 10,
            ]
        })
    };
    let action_count = 3u32;

    let pick_best = || -> usize {
        let mut best_idxs = vec![0];
        let mut best = q_values[0];
        for (i, &v) in q_values.iter().enumerate().skip(1) {
            if v > best { best = v; best_idxs.clear(); best_idxs.push(i); }
            else if v == best { best_idxs.push(i); }
        }
        if best_idxs.len() == 1 { best_idxs[0] } else { (pseudo_random(seed, best_idxs.len() as u32) as usize) % best_idxs.len() }
    };

    let action = match strategy {
        ActionSelectionStrategy::Best => pick_best(),
        ActionSelectionStrategy::Random => pseudo_random(seed, action_count) as usize,
        ActionSelectionStrategy::EpsilonGreedy(eps) => {
            let threshold = (eps * 100.0) as u32;
            if pseudo_random(seed, 100) < threshold { pseudo_random(seed.wrapping_add(1), action_count) as usize } else { pick_best() }
        }
        ActionSelectionStrategy::EpsilonDecay { initial_epsilon, final_epsilon, current_tick, total_ticks } => {
            let progress = if total_ticks == 0 { 1.0 } else { current_tick as f32 / total_ticks as f32 };
            let eps = initial_epsilon - (initial_epsilon - final_epsilon) * progress;
            let threshold = (eps * 100.0) as u32;
            if pseudo_random(seed, 100) < threshold { pseudo_random(seed.wrapping_add(1), action_count) as usize } else { pick_best() }
        },
        ActionSelectionStrategy::Softmax(temp) => {
            let t = if temp <= 0.0 { 1.0 } else { temp };
            let mut max_q = q_values[0];
            for &q in q_values.iter().skip(1) { if q > max_q { max_q = q; } }
            let exps: Vec<f32> = q_values.iter().map(|&q| ((q as f32 - max_q as f32)/t).exp()).collect();
            let sum: f32 = exps.iter().sum();
            let sample = (pseudo_random(seed, 10000) as f32) / 10000.0;
            let mut acc = 0.0;
            for (i, &exp_val) in exps.iter().enumerate() {
                acc += exp_val / sum;
                if sample < acc { return Ok((i, q_values)); }
            }
            action_count as usize - 1
        }
    };

    Ok((action, q_values))
}


fn make_strategy(tc: &TrainingConfig, tick: u32, total: u32) -> ActionSelectionStrategy {
    if !tc.training_mode { return ActionSelectionStrategy::Best; }
    let eps = decimal_to_f32(tc.epsilon);
    let temp = decimal_to_f32(tc.temperature);
    if temp > 0.0 { ActionSelectionStrategy::Softmax(temp) }
    else if tc.enable_epsilon_decay { ActionSelectionStrategy::EpsilonDecay { initial_epsilon: eps, final_epsilon: 0.01, current_tick: tick, total_ticks: total } }
    else if eps > 0.0 { ActionSelectionStrategy::EpsilonGreedy(eps) }
    else { ActionSelectionStrategy::Best }
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetConfig {} => {
            let cfg = CONFIG.load(deps.storage)?;
            to_json_binary(&ConfigResponse {
                admin: cfg.admin,
                car_contract: cfg.car_contract,
                max_ticks: cfg.max_ticks,
                match_history_limit: cfg.match_history_limit,
                tick_history_limit: cfg.tick_history_limit,
            })
        }
        QueryMsg::GetQ { car_id, state_id } => {
            let entries = if let Some(s) = state_id { vec![GetQResponseEntry { state_id: s, action_values: get_q_values(deps.storage, car_id, s).unwrap_or([0,0,0]) }] } else {
                let mut v = vec![];
                for s in 0u8..=INITIAL_STATE_ID { if let Ok(vals) = get_q_values(deps.storage, car_id, s) { v.push(GetQResponseEntry { state_id: s, action_values: vals }); } }
                v
            };
            to_json_binary(&GetQResponse { car_id, q_values: entries })
        }
        QueryMsg::GetHistory { car_id } => {
            let history = MATCH_HISTORY.load(deps.storage, car_id).unwrap_or_default();
            to_json_binary(&GetHistoryResponse { car_id, history })
        }
        QueryMsg::GetTickHistory { car_id } => {
            let ticks = crate::state::TICK_HISTORY.load(deps.storage, car_id).unwrap_or_default();
            to_json_binary(&GetTickHistoryResponse { car_id, ticks })
        }
    }
}

fn encode_state(opp_last_move: Option<u8>, last_outcome: Option<u8>) -> u8 {
    match (opp_last_move, last_outcome) {
        (Some(m), Some(o)) => m * 3 + o, // 0..=8
        _ => INITIAL_STATE_ID,
    }
}

fn rps_outcome(a: usize, b: usize) -> (u8, u8) {
    if a == b { return (OUTCOME_DRAW, OUTCOME_DRAW); }
    // returns (outcome_for_a, outcome_for_b)
    match (a, b) {
        (ACTION_ROCK, ACTION_SCISSORS) => (OUTCOME_WIN, OUTCOME_LOSE),
        (ACTION_SCISSORS, ACTION_PAPER) => (OUTCOME_WIN, OUTCOME_LOSE),
        (ACTION_PAPER, ACTION_ROCK) => (OUTCOME_WIN, OUTCOME_LOSE),
        _ => (OUTCOME_LOSE, OUTCOME_WIN),
    }
}

fn pseudo_random(seed: u32, modulus: u32) -> u32 { // simple LCG
    let a: u32 = 1103515245; let c: u32 = 12345; a.wrapping_mul(seed).wrapping_add(c) % modulus.max(1)
}

fn owner_check_for_training(deps: Deps, car_contract: String, info: &MessageInfo, car_id: u128) -> Result<(), ContractError> {
    if car_id == 0 { return Ok(()); }
    let resp: OwnerOfResponse = deps.querier.query_wasm_smart(
        car_contract,
        &membrane::car::QueryMsg::OwnerOf { token_id: car_id.to_string(), include_expired: None }
    ).map_err(|_| ContractError::Unauthorized {})?;
    if resp.owner != info.sender { return Err(ContractError::Unauthorized {}); }
    Ok(())
}

fn execute_play_series(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    car_id: u128,
    mut opponent_id: u128,
    train: bool,
    training_config: Option<TrainingConfig>,
    reward_config: Option<RpsRewardConfig>,
    mode: SeriesMode,
) -> Result<Response, ContractError> {
    let cfg = get_config(deps.storage)?;
    if train { owner_check_for_training(deps.as_ref(), cfg.car_contract.clone(), &info, car_id)?; }
    if train { opponent_id = 0; }

    let tc = training_config.unwrap_or_else(|| default_training_config(train));
    let rewards = reward_config.unwrap_or(DEFAULT_REWARDS);

    // Initialize separate Q-value caches for each player
    let mut caches = SeriesCaches { a: HashMap::new(), b: HashMap::new() };

    // per-player traces
    let mut a_opp_last: Option<u8> = None; // last opponent move for A
    let mut b_opp_last: Option<u8> = None; // last opponent move for B
    let mut a_last_outcome: Option<u8> = None;
    let mut b_last_outcome: Option<u8> = None;

    let mut a_trace: Vec<(u8, usize, i32)> = vec![]; // (state, action, reward)
    let mut b_trace: Vec<(u8, usize, i32)> = vec![];
    // per-player per-tick action history
    let mut a_ticks: Vec<TickRecord> = vec![];
    let mut b_ticks: Vec<TickRecord> = vec![];

    let mut a_wins: u32 = 0;
    let mut b_wins: u32 = 0;
    let mut rounds: u32 = 0;

    let max_ticks = cfg.max_ticks;

    let mut target_wins: Option<u32> = None;
    let mut max_rounds: u32 = max_ticks;
    match mode {
        SeriesMode::FixedTicks { ticks } => { max_rounds = ticks; }
        SeriesMode::BestOf { wins_target } => { target_wins = Some(wins_target); }
    }

    // Play rounds
    loop {
        let total = max_rounds.max(1);
        let strat_a = make_strategy(&tc, rounds, total);
        let strat_b = make_strategy(&tc, rounds, total);

        let a_state = encode_state(a_opp_last, a_last_outcome);
        let b_state = encode_state(b_opp_last, b_last_outcome);

        let seed = (env.block.height as u32) ^ (env.block.time.seconds() as u32) ^ (rounds as u32);
        let (a_action, _a_q) = choose_action(deps.storage, car_id, a_state, strat_a, seed.wrapping_add(1), Some(&mut caches.a))?;
        let (b_action, _b_q) = choose_action(deps.storage, opponent_id, b_state, strat_b, seed.wrapping_add(2), Some(&mut caches.b))?;

        let (a_out, b_out) = rps_outcome(a_action, b_action);
        let a_reward = match a_out { OUTCOME_WIN => rewards.win_points, OUTCOME_LOSE => rewards.lose_penalty, _ => rewards.draw_points } as i32;
        let b_reward = match b_out { OUTCOME_WIN => rewards.win_points, OUTCOME_LOSE => rewards.lose_penalty, _ => rewards.draw_points } as i32;

        a_trace.push((a_state, a_action, a_reward));
        b_trace.push((b_state, b_action, b_reward));
        // record actions and outcome for this tick
        a_ticks.push(TickRecord { my_action: a_action as u8, opp_action: b_action as u8, outcome: a_out });
        b_ticks.push(TickRecord { my_action: b_action as u8, opp_action: a_action as u8, outcome: b_out });

        // update last for next state
        a_opp_last = Some(b_action as u8);
        b_opp_last = Some(a_action as u8);
        a_last_outcome = Some(a_out);
        b_last_outcome = Some(b_out);

        if a_out == OUTCOME_WIN { a_wins += 1; }
        if b_out == OUTCOME_WIN { b_wins += 1; }
        rounds += 1;

        // Stop conditions
        match target_wins {
            Some(tw) => {
                // Best of: draws don't count; keep going until someone reaches tw
                if a_wins >= tw || b_wins >= tw { break; }
                if rounds >= max_rounds { break; } // safety
            }
            None => {
                if rounds >= max_rounds { break; }
            }
        }
    }

    // If fixed ticks and tie, add sudden-death until winner (with safety cap)
    if target_wins.is_none() && a_wins == b_wins {
        let safety_cap = max_rounds + 100; // extra cushion
        while a_wins == b_wins && rounds < safety_cap {
            let total = safety_cap;
            let strat_a = make_strategy(&tc, rounds, total);
            let strat_b = make_strategy(&tc, rounds, total);
            let a_state = encode_state(a_opp_last, a_last_outcome);
            let b_state = encode_state(b_opp_last, b_last_outcome);
            let seed = (env.block.height as u32) ^ (env.block.time.seconds() as u32) ^ (rounds as u32);
            let (a_action, _a_q) = choose_action(deps.storage, car_id, a_state, strat_a, seed.wrapping_add(3), Some(&mut caches.a))?;
            let (b_action, _b_q) = choose_action(deps.storage, opponent_id, b_state, strat_b, seed.wrapping_add(4), Some(&mut caches.b))?;
            let (a_out, b_out) = rps_outcome(a_action, b_action);
            let a_reward = match a_out { OUTCOME_WIN => rewards.win_points, OUTCOME_LOSE => rewards.lose_penalty, _ => rewards.draw_points } as i32;
            let b_reward = match b_out { OUTCOME_WIN => rewards.win_points, OUTCOME_LOSE => rewards.lose_penalty, _ => rewards.draw_points } as i32;
            a_trace.push((a_state, a_action, a_reward));
            b_trace.push((b_state, b_action, b_reward));
            // record sudden-death tick with outcome
            a_ticks.push(TickRecord { my_action: a_action as u8, opp_action: b_action as u8, outcome: a_out });
            b_ticks.push(TickRecord { my_action: b_action as u8, opp_action: a_action as u8, outcome: b_out });
            a_opp_last = Some(b_action as u8);
            b_opp_last = Some(a_action as u8);
            a_last_outcome = Some(a_out);
            b_last_outcome = Some(b_out);
            if a_out == OUTCOME_WIN { a_wins += 1; }
            if b_out == OUTCOME_WIN { b_wins += 1; }
            rounds += 1;
        }
    }

    let a_won_series = a_wins > b_wins;
    let b_won_series = b_wins > a_wins;

    // add series bonus to last step for each winner
    if let Some(last) = a_trace.last_mut() { if a_won_series { last.2 += rewards.series_win_points as i32; } }
    if let Some(last) = b_trace.last_mut() { if b_won_series { last.2 += rewards.series_win_points as i32; } }

    // Apply Q-learning updates if training
    if train {
        apply_q_learning_updates(deps.storage, car_id, &a_trace)?;
        apply_q_learning_updates(deps.storage, opponent_id, &b_trace)?; // train car 0 as well
    }

    // Update histories - now tracking per-tick results instead of per-match
    push_tick_results(deps.storage, car_id, &a_ticks)?;
    push_tick_results(deps.storage, opponent_id, &b_ticks)?;
    // Save per-tick action histories (overwrites previous match)
    push_tick_records(deps.storage, car_id, a_ticks)?;
    push_tick_records(deps.storage, opponent_id, b_ticks)?;

    Ok(Response::new()
        .add_attribute("action", "play_series")
        .add_attribute("car_id", car_id.to_string())
        .add_attribute("opponent_id", opponent_id.to_string())
        .add_attribute("rounds", rounds.to_string())
        .add_attribute("winner", if a_won_series { car_id.to_string() } else { opponent_id.to_string() }))
}

fn apply_q_learning_updates(storage: &mut dyn cosmwasm_std::Storage, car_id: u128, trace: &Vec<(u8, usize, i32)>) -> Result<(), ContractError> {
    if trace.is_empty() { return Ok(()); }
    let n = trace.len();
    for i in (0..n).rev() {
        let (s, a, r) = trace[i];
        let mut q_values = get_q_values(storage, car_id, s).unwrap_or_else(|_| {
            // Initialize with small random values if not found
            let seed = (car_id as u32) ^ (s as u32) ^ 12345;
            [
                (pseudo_random(seed, 21) as i8) - 10,
                (pseudo_random(seed.wrapping_add(1), 21) as i8) - 10,
                (pseudo_random(seed.wrapping_add(2), 21) as i8) - 10,
            ]
        });
        let old = q_values[a] as f32;
        let next_max = if i + 1 < n {
            let (s_next, _a_next, _r_next) = trace[i+1];
            let next = get_q_values(storage, car_id, s_next).unwrap_or_else(|_err| {
                let seed = (car_id as u32) ^ (s_next as u32) ^ 12345;
                [
                    (pseudo_random(seed, 21) as i8) - 10,
                    (pseudo_random(seed.wrapping_add(1), 21) as i8) - 10,
                    (pseudo_random(seed.wrapping_add(2), 21) as i8) - 10,
                ]
            });
            let mut m = next[0] as f32; for &v in next.iter().skip(1) { if (v as f32) > m { m = v as f32; } } m
        } else { 0.0 };
        let target = r as f32 + GAMMA * next_max;
        let updated = (old + ALPHA * (target - old)).round() as i32;
        let clamped = updated.clamp(Q_MIN, Q_MAX) as i8;
        q_values[a] = clamped;
        
        // println!("  State {}, Action {}, Reward {}, Old: {}, Target: {}, Updated: {}, Clamped: {}", 
        //          s, a, r, old, target, updated, clamped);
        
        set_q_values(storage, car_id, s, q_values)?;
    }
    Ok(())
}

#[entry_point]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    
    MATCH_HISTORY.clear(deps.storage);
    TICK_HISTORY.clear(deps.storage);

    //change config match_history_limit to 100
    let mut config = get_config(deps.storage)?;
    config.match_history_limit = 1000;
    set_config(deps.storage, config)?;

    Ok(Response::new().add_attribute("action", "migrate"))
}

#[cfg(not(target_arch = "wasm32"))]
pub mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};

    #[test]
    fn instantiate_and_play_fixed_ticks() {
        let mut deps = mock_dependencies();
        let info = mock_info("admin", &[]);
        instantiate(
            deps.as_mut(),
            mock_env(),
            info,
            InstantiateMsg { admin: "admin".into(), car_contract: "car_contract".into(), max_ticks: Some(10), match_history_limit: Some(5), tick_history_limit: Some(100) },
        ).unwrap();

        // Play a non-training series to avoid owner checks in test
        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("anyone", &[]),
            ExecuteMsg::PlaySeries { car_id: 1, opponent_id: Some(0), train: false, training_config: None, reward_config: None, mode: SeriesMode::FixedTicks { ticks: 3 } }
        ).unwrap();
        assert_eq!(res.attributes.iter().find(|a| a.key == "action").unwrap().value, "play_series");

        // History recorded for both
        let history_1 = MATCH_HISTORY.load(&deps.storage, 1).unwrap();
        let history_0 = MATCH_HISTORY.load(&deps.storage, 0).unwrap();
        assert_eq!(history_1.len(), 1);
        assert_eq!(history_0.len(), 1);
    }
}


