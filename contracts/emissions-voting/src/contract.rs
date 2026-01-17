use std::str::FromStr;

use cosmwasm_std::{
    entry_point, to_json_binary, Addr, Binary, CosmosMsg, Decimal, Deps, DepsMut, Env,
    MessageInfo, Order, QueryRequest, Response, StdResult, Uint128, WasmMsg, WasmQuery,
};
use cw2::set_contract_version;
use cw_storage_plus::Bound;

use membrane::emissions_voting::{
    AllVotesResponse, Config, ConfigResponse, CurrentResultResponse, DecimalGraph, ExecuteMsg,
    Graph, GraphResponse, GraphType, GraphsResponse, HasAnyVotesResponse, InstantiateMsg, MigrateMsg,
    PeriodHistoryResponse, PeriodResult, QueryMsg, Uint128Graph, UserVote, UserVoteDecimal,
    UserVoteResponse, UserVoteUint128, VoteInfo,
};
use membrane::ltv_disco::{QueryMsg as LtvDiscoQueryMsg, LockedDepositsResponse};
use membrane::staking::{QueryMsg as StakingQueryMsg, StakerResponse};
use membrane::types::VotingResultMsg;

use crate::error::ContractError;
use crate::state::{
    CONFIG, GRAPHS, GRAPH_TOTALS, PERIOD_HISTORY, USER_VOTES, WEIGHTED_SUMS_DECIMAL,
    WEIGHTED_SUMS_UINT128,
};

// Contract name and version for migration
const CONTRACT_NAME: &str = "emissions_voting";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

// Constants
const SECONDS_PER_DAY: u64 = 86_400;
const DEFAULT_LIMIT: u32 = 10;
const MAX_LIMIT: u32 = 30;
const MAX_HISTORY_SIZE: usize = 100;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let owner = msg
        .owner
        .map(|o| deps.api.addr_validate(&o))
        .transpose()?
        .unwrap_or(info.sender);

    let config = Config {
        owner,
        ltv_disco_contract: deps.api.addr_validate(&msg.ltv_disco_contract)?,
        staking_contract: deps.api.addr_validate(&msg.staking_contract)?,
    };

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("action", "instantiate")
        .add_attribute("owner", config.owner.to_string()))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::CreateGraph {
            label,
            graph_type,
            range_min,
            range_max,
            period_days,
            callback_contract,
        } => execute_create_graph(
            deps,
            env,
            info,
            label,
            graph_type,
            range_min,
            range_max,
            period_days,
            callback_contract,
        ),
        ExecuteMsg::Vote {
            graph_label,
            vote_value,
        } => execute_vote(deps, env, info, graph_label, vote_value),
        ExecuteMsg::EndVoting { graph_label } => execute_end_voting(deps, env, graph_label),
        ExecuteMsg::UpdateConfig {
            owner,
            ltv_disco_contract,
            staking_contract,
        } => execute_update_config(deps, info, owner, ltv_disco_contract, staking_contract),
        ExecuteMsg::UpdateGraph {
            label,
            period_days,
            callback_contract,
        } => execute_update_graph(deps, info, label, period_days, callback_contract),
        ExecuteMsg::RemoveGraph { label } => execute_remove_graph(deps, info, label),
        ExecuteMsg::RemoveVote { graph_label } => execute_remove_vote(deps, env, info, graph_label),
    }
}

/// Create a new voting graph
fn execute_create_graph(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    label: String,
    graph_type: GraphType,
    range_min: String,
    range_max: String,
    period_days: u64,
    callback_contract: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only owner can create graphs
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    // Validate period days
    if period_days < 1 {
        return Err(ContractError::InvalidPeriodDays {});
    }

    // Check if graph already exists
    if GRAPHS.may_load(deps.storage, &label)?.is_some() {
        return Err(ContractError::GraphAlreadyExists { label });
    }

    let callback_addr = deps.api.addr_validate(&callback_contract)?;
    let current_time = env.block.time.seconds();

    let graph = match graph_type {
        GraphType::Uint128 => {
            let min = Uint128::from_str(&range_min).map_err(|_| ContractError::InvalidGraphType {
                msg: "Invalid Uint128 range_min".to_string(),
            })?;
            let max = Uint128::from_str(&range_max).map_err(|_| ContractError::InvalidGraphType {
                msg: "Invalid Uint128 range_max".to_string(),
            })?;

            if min >= max {
                return Err(ContractError::InvalidRange {});
            }

            // Initialize weighted sum for Uint128
            WEIGHTED_SUMS_UINT128.save(deps.storage, &label, &0u128)?;

            Graph::Uint128(Uint128Graph {
                label: label.clone(),
                range_min: min,
                range_max: max,
                period_days,
                current_period_start: current_time,
                callback_contract: callback_addr,
            })
        }
        GraphType::Decimal => {
            let min =
                Decimal::from_str(&range_min).map_err(|_| ContractError::InvalidGraphType {
                    msg: "Invalid Decimal range_min".to_string(),
                })?;
            let max =
                Decimal::from_str(&range_max).map_err(|_| ContractError::InvalidGraphType {
                    msg: "Invalid Decimal range_max".to_string(),
                })?;

            if min >= max {
                return Err(ContractError::InvalidRange {});
            }

            // Initialize weighted sum for Decimal (stored as string)
            WEIGHTED_SUMS_DECIMAL.save(deps.storage, &label, &"0".to_string())?;

            Graph::Decimal(DecimalGraph {
                label: label.clone(),
                range_min: min,
                range_max: max,
                period_days,
                current_period_start: current_time,
                callback_contract: callback_addr,
            })
        }
    };

    GRAPHS.save(deps.storage, &label, &graph)?;
    GRAPH_TOTALS.save(deps.storage, &label, &0u128)?;

    Ok(Response::new()
        .add_attribute("action", "create_graph")
        .add_attribute("label", label)
        .add_attribute("graph_type", format!("{:?}", graph_type))
        .add_attribute("period_days", period_days.to_string()))
}

/// Cast a vote on a graph for any value within the range
fn execute_vote(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    graph_label: String,
    vote_value: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let graph = GRAPHS
        .load(deps.storage, &graph_label)
        .map_err(|_| ContractError::GraphNotFound {
            label: graph_label.clone(),
        })?;

    // Get user's voting power
    let voting_power = query_voting_power(deps.as_ref(), &env, &config, &info.sender)?;
    if voting_power.is_zero() {
        return Err(ContractError::NoVotingPower {});
    }

    let user_addr = info.sender.to_string();
    let current_period = graph.current_period_start();

    // Remove old vote if exists in this period
    if let Some(existing_vote) = USER_VOTES.may_load(deps.storage, (&graph_label, &user_addr))? {
        if existing_vote.period_start() == current_period {
            // Subtract old vote from totals
            let old_power = existing_vote.voting_power();
            let graph_total = GRAPH_TOTALS
                .may_load(deps.storage, &graph_label)?
                .unwrap_or(0);
            GRAPH_TOTALS.save(
                deps.storage,
                &graph_label,
                &graph_total.saturating_sub(old_power.u128()),
            )?;

            // Subtract from weighted sum
            match &existing_vote {
                UserVote::Uint128(v) => {
                    let old_weighted = v.voted_value.u128() * v.voting_power.u128();
                    let current_sum = WEIGHTED_SUMS_UINT128
                        .may_load(deps.storage, &graph_label)?
                        .unwrap_or(0);
                    WEIGHTED_SUMS_UINT128.save(
                        deps.storage,
                        &graph_label,
                        &current_sum.saturating_sub(old_weighted),
                    )?;
                }
                UserVote::Decimal(v) => {
                    let old_weighted = v.voted_value * Decimal::from_atomics(v.voting_power, 6)
                        .map_err(|_| ContractError::InvalidVoteValue {
                            msg: "Failed to convert voting power".to_string(),
                        })?;
                    let current_sum_str = WEIGHTED_SUMS_DECIMAL
                        .may_load(deps.storage, &graph_label)?
                        .unwrap_or_else(|| "0".to_string());
                    let current_sum = Decimal::from_str(&current_sum_str).unwrap_or(Decimal::zero());
                    let new_sum = current_sum.saturating_sub(old_weighted);
                    WEIGHTED_SUMS_DECIMAL.save(deps.storage, &graph_label, &new_sum.to_string())?;
                }
            }
        }
    }

    // Validate and parse vote value, create new vote
    let user_vote = match &graph {
        Graph::Uint128(g) => {
            let value = Uint128::from_str(&vote_value).map_err(|_| {
                ContractError::InvalidVoteValue {
                    msg: format!("Invalid Uint128 value: {}", vote_value),
                }
            })?;

            // Validate range
            if value < g.range_min || value > g.range_max {
                return Err(ContractError::VoteOutOfRange {
                    value: vote_value,
                    min: g.range_min.to_string(),
                    max: g.range_max.to_string(),
                });
            }

            // Add to weighted sum
            let weighted_contribution = value.u128() * voting_power.u128();
            let current_sum = WEIGHTED_SUMS_UINT128
                .may_load(deps.storage, &graph_label)?
                .unwrap_or(0);
            WEIGHTED_SUMS_UINT128.save(
                deps.storage,
                &graph_label,
                &(current_sum + weighted_contribution),
            )?;

            UserVote::Uint128(UserVoteUint128 {
                voted_value: value,
                voting_power,
                period_start: current_period,
            })
        }
        Graph::Decimal(g) => {
            let value = Decimal::from_str(&vote_value).map_err(|_| {
                ContractError::InvalidVoteValue {
                    msg: format!("Invalid Decimal value: {}", vote_value),
                }
            })?;

            // Validate range
            if value < g.range_min || value > g.range_max {
                return Err(ContractError::VoteOutOfRange {
                    value: vote_value,
                    min: g.range_min.to_string(),
                    max: g.range_max.to_string(),
                });
            }

            // Add to weighted sum
            let power_decimal = Decimal::from_atomics(voting_power, 6).map_err(|_| {
                ContractError::InvalidVoteValue {
                    msg: "Failed to convert voting power".to_string(),
                }
            })?;
            let weighted_contribution = value * power_decimal;
            let current_sum_str = WEIGHTED_SUMS_DECIMAL
                .may_load(deps.storage, &graph_label)?
                .unwrap_or_else(|| "0".to_string());
            let current_sum = Decimal::from_str(&current_sum_str).unwrap_or(Decimal::zero());
            let new_sum = current_sum + weighted_contribution;
            WEIGHTED_SUMS_DECIMAL.save(deps.storage, &graph_label, &new_sum.to_string())?;

            UserVote::Decimal(UserVoteDecimal {
                voted_value: value,
                voting_power,
                period_start: current_period,
            })
        }
    };

    // Save user vote
    USER_VOTES.save(deps.storage, (&graph_label, &user_addr), &user_vote)?;

    // Update graph total
    let graph_total = GRAPH_TOTALS
        .may_load(deps.storage, &graph_label)?
        .unwrap_or(0);
    GRAPH_TOTALS.save(
        deps.storage,
        &graph_label,
        &(graph_total + voting_power.u128()),
    )?;

    Ok(Response::new()
        .add_attribute("action", "vote")
        .add_attribute("graph_label", graph_label)
        .add_attribute("vote_value", vote_value)
        .add_attribute("voting_power", voting_power.to_string())
        .add_attribute("voter", info.sender.to_string()))
}

/// End the voting period and send results (permissionless)
fn execute_end_voting(
    deps: DepsMut,
    env: Env,
    graph_label: String,
) -> Result<Response, ContractError> {
    let mut graph = GRAPHS
        .load(deps.storage, &graph_label)
        .map_err(|_| ContractError::GraphNotFound {
            label: graph_label.clone(),
        })?;

    let current_time = env.block.time.seconds();
    let period_end = graph.current_period_start() + (graph.period_days() * SECONDS_PER_DAY);

    // Check if period has ended
    if current_time < period_end {
        return Err(ContractError::PeriodNotEnded { ends_at: period_end });
    }

    // Get total voting power
    let total_power = GRAPH_TOTALS
        .may_load(deps.storage, &graph_label)?
        .unwrap_or(0);
    if total_power == 0 {
        return Err(ContractError::NoVotes {});
    }

    // Calculate weighted average
    let (result_uint128, result_decimal) = match &graph {
        Graph::Uint128(_) => {
            let weighted_sum = WEIGHTED_SUMS_UINT128
                .may_load(deps.storage, &graph_label)?
                .unwrap_or(0);
            let result = Uint128::from(weighted_sum / total_power);
            (Some(result), None)
        }
        Graph::Decimal(_) => {
            let weighted_sum_str = WEIGHTED_SUMS_DECIMAL
                .may_load(deps.storage, &graph_label)?
                .unwrap_or_else(|| "0".to_string());
            let weighted_sum = Decimal::from_str(&weighted_sum_str).unwrap_or(Decimal::zero());
            let total_power_decimal =
                Decimal::from_atomics(Uint128::from(total_power), 6).unwrap_or(Decimal::one());
            let result = weighted_sum / total_power_decimal;
            (None, Some(result))
        }
    };

    // Save result to history
    let result = PeriodResult {
        period_start: graph.current_period_start(),
        period_end,
        result_uint128,
        result_decimal,
        total_voting_power: Uint128::from(total_power),
    };

    let mut history = PERIOD_HISTORY
        .may_load(deps.storage, &graph_label)?
        .unwrap_or_default();
    history.push(result.clone());
    // Enforce max history size - remove oldest entries if needed
    if history.len() > MAX_HISTORY_SIZE {
        history = history.split_off(history.len() - MAX_HISTORY_SIZE);
    }
    PERIOD_HISTORY.save(deps.storage, &graph_label, &history)?;

    // Clear all votes for this graph
    clear_graph_votes(deps.storage, &graph_label, &graph)?;

    // Start new period
    graph.set_period_start(current_time);
    GRAPHS.save(deps.storage, &graph_label, &graph)?;

    // Build callback message
    let callback_msg = VotingResultMsg::ReceiveVotingResult {
        label: graph_label.clone(),
        result_uint128,
        result_decimal,
    };

    let msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: graph.callback_contract().to_string(),
        msg: to_json_binary(&callback_msg)?,
        funds: vec![],
    });

    Ok(Response::new()
        .add_message(msg)
        .add_attribute("action", "end_voting")
        .add_attribute("graph_label", graph_label)
        .add_attribute("total_voting_power", total_power.to_string())
        .add_attribute(
            "result",
            result_uint128
                .map(|r| r.to_string())
                .or(result_decimal.map(|r| r.to_string()))
                .unwrap_or_default(),
        ))
}

/// Clear all votes for a graph when period ends
fn clear_graph_votes(
    storage: &mut dyn cosmwasm_std::Storage,
    graph_label: &str,
    graph: &Graph,
) -> Result<(), ContractError> {
    // Clear user votes for this graph
    let user_keys: Vec<_> = USER_VOTES
        .prefix(graph_label)
        .keys(storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<_>>>()?;

    for user in user_keys {
        USER_VOTES.remove(storage, (graph_label, &user));
    }

    // Reset graph total
    GRAPH_TOTALS.save(storage, graph_label, &0u128)?;

    // Reset weighted sums
    match graph {
        Graph::Uint128(_) => {
            WEIGHTED_SUMS_UINT128.save(storage, graph_label, &0u128)?;
        }
        Graph::Decimal(_) => {
            WEIGHTED_SUMS_DECIMAL.save(storage, graph_label, &"0".to_string())?;
        }
    }

    Ok(())
}

/// Update contract configuration
fn execute_update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    ltv_disco_contract: Option<String>,
    staking_contract: Option<String>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;

    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    if let Some(owner) = owner {
        config.owner = deps.api.addr_validate(&owner)?;
    }
    if let Some(ltv_disco_contract) = ltv_disco_contract {
        config.ltv_disco_contract = deps.api.addr_validate(&ltv_disco_contract)?;
    }
    if let Some(staking_contract) = staking_contract {
        config.staking_contract = deps.api.addr_validate(&staking_contract)?;
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("action", "update_config"))
}

/// Update graph parameters
fn execute_update_graph(
    deps: DepsMut,
    info: MessageInfo,
    label: String,
    period_days: Option<u64>,
    callback_contract: Option<String>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let mut graph = GRAPHS
        .load(deps.storage, &label)
        .map_err(|_| ContractError::GraphNotFound { label: label.clone() })?;

    match &mut graph {
        Graph::Uint128(g) => {
            if let Some(days) = period_days {
                if days < 1 {
                    return Err(ContractError::InvalidPeriodDays {});
                }
                g.period_days = days;
            }
            if let Some(callback) = callback_contract {
                g.callback_contract = deps.api.addr_validate(&callback)?;
            }
        }
        Graph::Decimal(g) => {
            if let Some(days) = period_days {
                if days < 1 {
                    return Err(ContractError::InvalidPeriodDays {});
                }
                g.period_days = days;
            }
            if let Some(callback) = callback_contract {
                g.callback_contract = deps.api.addr_validate(&callback)?;
            }
        }
    }

    GRAPHS.save(deps.storage, &label, &graph)?;

    Ok(Response::new()
        .add_attribute("action", "update_graph")
        .add_attribute("label", label))
}

/// Remove a graph
fn execute_remove_graph(
    deps: DepsMut,
    info: MessageInfo,
    label: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let graph = GRAPHS
        .load(deps.storage, &label)
        .map_err(|_| ContractError::GraphNotFound { label: label.clone() })?;

    // Clear all votes
    clear_graph_votes(deps.storage, &label, &graph)?;

    // Remove graph and history
    GRAPHS.remove(deps.storage, &label);
    PERIOD_HISTORY.remove(deps.storage, &label);

    // Remove weighted sum storage
    match &graph {
        Graph::Uint128(_) => {
            WEIGHTED_SUMS_UINT128.remove(deps.storage, &label);
        }
        Graph::Decimal(_) => {
            WEIGHTED_SUMS_DECIMAL.remove(deps.storage, &label);
        }
    }

    Ok(Response::new()
        .add_attribute("action", "remove_graph")
        .add_attribute("label", label))
}

/// Remove a user's vote from a graph (allows user to unstake/withdraw after)
fn execute_remove_vote(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    graph_label: String,
) -> Result<Response, ContractError> {
    let graph = GRAPHS
        .load(deps.storage, &graph_label)
        .map_err(|_| ContractError::GraphNotFound {
            label: graph_label.clone(),
        })?;

    let user_addr = info.sender.to_string();
    let current_period = graph.current_period_start();

    // Load existing vote
    let existing_vote = USER_VOTES
        .may_load(deps.storage, (&graph_label, &user_addr))?
        .ok_or_else(|| ContractError::NoVotes {})?;

    // Only remove if vote is from current period
    if existing_vote.period_start() != current_period {
        return Err(ContractError::NoVotes {});
    }

    // Subtract vote from totals
    let old_power = existing_vote.voting_power();
    let graph_total = GRAPH_TOTALS
        .may_load(deps.storage, &graph_label)?
        .unwrap_or(0);
    GRAPH_TOTALS.save(
        deps.storage,
        &graph_label,
        &graph_total.saturating_sub(old_power.u128()),
    )?;

    // Subtract from weighted sum
    match &existing_vote {
        UserVote::Uint128(v) => {
            let old_weighted = v.voted_value.u128() * v.voting_power.u128();
            let current_sum = WEIGHTED_SUMS_UINT128
                .may_load(deps.storage, &graph_label)?
                .unwrap_or(0);
            WEIGHTED_SUMS_UINT128.save(
                deps.storage,
                &graph_label,
                &current_sum.saturating_sub(old_weighted),
            )?;
        }
        UserVote::Decimal(v) => {
            let old_weighted = v.voted_value * Decimal::from_atomics(v.voting_power, 6)
                .map_err(|_| ContractError::InvalidVoteValue {
                    msg: "Failed to convert voting power".to_string(),
                })?;
            let current_sum_str = WEIGHTED_SUMS_DECIMAL
                .may_load(deps.storage, &graph_label)?
                .unwrap_or_else(|| "0".to_string());
            let current_sum = Decimal::from_str(&current_sum_str).unwrap_or(Decimal::zero());
            let new_sum = current_sum.saturating_sub(old_weighted);
            WEIGHTED_SUMS_DECIMAL.save(deps.storage, &graph_label, &new_sum.to_string())?;
        }
    }

    // Remove the vote
    USER_VOTES.remove(deps.storage, (&graph_label, &user_addr));

    Ok(Response::new()
        .add_attribute("action", "remove_vote")
        .add_attribute("graph_label", graph_label)
        .add_attribute("user", user_addr))
}

/// Query user's voting power from LTV Disco and Staking contracts
fn query_voting_power(
    deps: Deps,
    env: &Env,
    config: &Config,
    user: &Addr,
) -> Result<Uint128, ContractError> {
    let current_time = env.block.time.seconds();
    let mut total_power = Uint128::zero();

    // Query LTV Disco locked deposits
    let disco_response: StdResult<LockedDepositsResponse> =
        deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: config.ltv_disco_contract.to_string(),
            msg: to_json_binary(&LtvDiscoQueryMsg::GetLockedDeposits {
                user: user.to_string(),
            })?,
        }));

    if let Ok(locked_deposits) = disco_response {
        for locked_deposit in locked_deposits.locked_deposits {
            let deposit = locked_deposit.deposit;
            let deposit_amount = deposit.vault_tokens;

            // Calculate lock multiplier
            let lock_multiplier = calculate_lock_multiplier(&deposit.locked, current_time);

            // Weighted voting power = deposit * (lock_days + 1)
            total_power += deposit_amount * Uint128::from(lock_multiplier);
        }
    }

    // Query Staking deposits
    let staker_response: StdResult<StakerResponse> = deps.querier.query(&QueryRequest::Wasm(
        WasmQuery::Smart {
            contract_addr: config.staking_contract.to_string(),
            msg: to_json_binary(&StakingQueryMsg::UserStake {
                staker: user.to_string(),
            })?,
        },
    ));

    if let Ok(staker) = staker_response {
        for stake_deposit in staker.deposit_list {
            // Skip unstaking deposits
            if stake_deposit.unstake_start_time.is_some() {
                continue;
            }

            let deposit_amount = stake_deposit.amount;

            // Calculate lock multiplier
            let lock_multiplier = calculate_lock_multiplier(&stake_deposit.locked, current_time);

            // Weighted voting power = deposit * (lock_days + 1)
            total_power += deposit_amount * Uint128::from(lock_multiplier);
        }
    }

    Ok(total_power)
}

/// Calculate lock multiplier as (lock_days + 1)
/// If perpetual_lock exists without locked_until, use perpetual_lock days
/// If locked_until exists, calculate remaining lock days from current time
/// If no lock, returns 1 (multiplier of 1)
fn calculate_lock_multiplier(locked: &Option<membrane::types::Locked>, current_time: u64) -> u64 {
    match locked {
        Some(lock) => {
            if let Some(perpetual_days) = lock.perpetual_lock {
                // Use perpetual lock days if lock has expired but perpetual is set
                perpetual_days + 1
            } else if lock.locked_until > current_time {
                // Calculate remaining lock days
                let remaining_seconds = lock.locked_until - current_time;
                let remaining_days = remaining_seconds / SECONDS_PER_DAY;
                remaining_days + 1
            } else {
                // Lock has expired
                1
            }
        }
        None => 1, // No lock = multiplier of 1
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        QueryMsg::Graph { label } => to_json_binary(&query_graph(deps, label)?),
        QueryMsg::Graphs { start_after, limit } => {
            to_json_binary(&query_graphs(deps, start_after, limit)?)
        }
        QueryMsg::CurrentResult { label } => to_json_binary(&query_current_result(deps, env, label)?),
        QueryMsg::UserVote { user, graph_label } => {
            to_json_binary(&query_user_vote(deps, env, user, graph_label)?)
        }
        QueryMsg::PeriodHistory { label, limit } => {
            to_json_binary(&query_period_history(deps, label, limit)?)
        }
        QueryMsg::AllVotes {
            graph_label,
            start_after,
            limit,
        } => to_json_binary(&query_all_votes(deps, graph_label, start_after, limit)?),
        QueryMsg::HasAnyVotes { user } => to_json_binary(&query_has_any_votes(deps, user)?),
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    let config = CONFIG.load(deps.storage)?;
    Ok(ConfigResponse { config })
}

fn query_graph(deps: Deps, label: String) -> StdResult<GraphResponse> {
    let graph = GRAPHS.load(deps.storage, &label)?;
    let total_power = GRAPH_TOTALS.may_load(deps.storage, &label)?.unwrap_or(0);
    let period_end = graph.current_period_start() + (graph.period_days() * SECONDS_PER_DAY);

    Ok(GraphResponse {
        graph,
        period_end,
        total_voting_power: Uint128::from(total_power),
    })
}

fn query_graphs(
    deps: Deps,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<GraphsResponse> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as usize;
    let start = start_after.as_deref().map(Bound::exclusive);

    let graphs: Vec<GraphResponse> = GRAPHS
        .range(deps.storage, start, None, Order::Ascending)
        .take(limit)
        .map(|item| {
            let (label, graph) = item?;
            let total_power = GRAPH_TOTALS.may_load(deps.storage, &label)?.unwrap_or(0);
            let period_end = graph.current_period_start() + (graph.period_days() * SECONDS_PER_DAY);

            Ok(GraphResponse {
                graph,
                period_end,
                total_voting_power: Uint128::from(total_power),
            })
        })
        .collect::<StdResult<Vec<_>>>()?;

    Ok(GraphsResponse { graphs })
}

fn query_current_result(deps: Deps, env: Env, label: String) -> StdResult<CurrentResultResponse> {
    let graph = GRAPHS.load(deps.storage, &label)?;
    let total_power = GRAPH_TOTALS.may_load(deps.storage, &label)?.unwrap_or(0);
    let period_end = graph.current_period_start() + (graph.period_days() * SECONDS_PER_DAY);
    let period_ended = env.block.time.seconds() >= period_end;

    if total_power == 0 {
        return Ok(CurrentResultResponse {
            result_uint128: None,
            result_decimal: None,
            total_voting_power: Uint128::zero(),
            period_ended,
        });
    }

    let (result_uint128, result_decimal) = match &graph {
        Graph::Uint128(_) => {
            let weighted_sum = WEIGHTED_SUMS_UINT128
                .may_load(deps.storage, &label)?
                .unwrap_or(0);
            let result = Uint128::from(weighted_sum / total_power);
            (Some(result), None)
        }
        Graph::Decimal(_) => {
            let weighted_sum_str = WEIGHTED_SUMS_DECIMAL
                .may_load(deps.storage, &label)?
                .unwrap_or_else(|| "0".to_string());
            let weighted_sum = Decimal::from_str(&weighted_sum_str).unwrap_or(Decimal::zero());
            let total_power_decimal =
                Decimal::from_atomics(Uint128::from(total_power), 6).unwrap_or(Decimal::one());
            let result = weighted_sum / total_power_decimal;
            (None, Some(result))
        }
    };

    Ok(CurrentResultResponse {
        result_uint128,
        result_decimal,
        total_voting_power: Uint128::from(total_power),
        period_ended,
    })
}

fn query_user_vote(
    deps: Deps,
    env: Env,
    user: String,
    graph_label: String,
) -> StdResult<UserVoteResponse> {
    let config = CONFIG.load(deps.storage)?;
    let user_addr = deps.api.addr_validate(&user)?;

    let vote = USER_VOTES.may_load(deps.storage, (&graph_label, &user))?;

    // Get current voting power
    let current_voting_power =
        query_voting_power(deps, &env, &config, &user_addr).unwrap_or(Uint128::zero());

    Ok(UserVoteResponse {
        vote,
        current_voting_power,
    })
}

fn query_period_history(
    deps: Deps,
    label: String,
    limit: Option<u32>,
) -> StdResult<PeriodHistoryResponse> {
    let history = PERIOD_HISTORY
        .may_load(deps.storage, &label)?
        .unwrap_or_default();
    let limit = limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as usize;

    // Return most recent results first
    let history: Vec<PeriodResult> = history.into_iter().rev().take(limit).collect();

    Ok(PeriodHistoryResponse { history })
}

fn query_all_votes(
    deps: Deps,
    graph_label: String,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<AllVotesResponse> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as usize;
    let start = start_after.as_deref().map(Bound::exclusive);

    let mut total_power = Uint128::zero();
    let votes: Vec<VoteInfo> = USER_VOTES
        .prefix(&graph_label)
        .range(deps.storage, start, None, Order::Ascending)
        .take(limit)
        .map(|item| {
            let (user, vote) = item?;
            total_power += vote.voting_power();
            Ok(VoteInfo {
                user: Addr::unchecked(&user),
                vote,
            })
        })
        .collect::<StdResult<Vec<_>>>()?;

    Ok(AllVotesResponse { votes, total_power })
}

/// Check if user has any active votes across all graphs
fn query_has_any_votes(deps: Deps, user: String) -> StdResult<HasAnyVotesResponse> {
    let user_addr = deps.api.addr_validate(&user)?;
    
    // Iterate through all graphs and check for user votes
    let graphs: Vec<(String, Graph)> = GRAPHS
        .range(deps.storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<_>>>()?;
    
    for (label, graph) in graphs {
        let current_period = graph.current_period_start();
        
        // Check if user has a vote in the current period for this graph
        if let Some(vote) = USER_VOTES.may_load(deps.storage, (&label, user_addr.as_str()))? {
            if vote.period_start() == current_period {
                return Ok(HasAnyVotesResponse { has_votes: true });
            }
        }
    }
    
    Ok(HasAnyVotesResponse { has_votes: false })
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    Ok(Response::default())
}
