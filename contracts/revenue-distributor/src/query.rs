use cosmwasm_std::{Deps, Env, StdResult, to_json_binary, Uint128};
use membrane::revenue_distributor::{QueryMsg, CurrentEpochRevenueResponse, EpochCountdownResponse};

use crate::state::{
    CONFIG,
    PROMISES,
    FAILED_DISTRIBUTIONS,
    DISTRIBUTION_PROP,
    EPOCH_REVENUE_ACCUMULATION,
    LAST_DISTRIBUTION_TIME,
};

/// Query contract configuration
pub fn query_config(deps: Deps) -> StdResult<cosmwasm_std::Binary> {
    to_json_binary(&CONFIG.load(deps.storage)?)
}

/// Query current promises
pub fn query_promises(deps: Deps) -> StdResult<cosmwasm_std::Binary> {
    to_json_binary(&PROMISES.load(deps.storage)?)
}

/// Query failed distributions
pub fn query_failed_distributions(deps: Deps) -> StdResult<cosmwasm_std::Binary> {
    let failed_distributions: Vec<(String, Uint128)> = FAILED_DISTRIBUTIONS
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .map(|item| {
            let (key, value) = item.unwrap();
            (key, Uint128::from(value))
        })
        .collect();
    to_json_binary(&failed_distributions)
}

/// Query pending distributions
pub fn query_pending_distributions(deps: Deps) -> StdResult<cosmwasm_std::Binary> {
    to_json_binary(&DISTRIBUTION_PROP.load(deps.storage)?)
}

/// Query current epoch revenue accumulation
pub fn query_current_epoch_revenue(deps: Deps) -> StdResult<cosmwasm_std::Binary> {
    let revenue: Vec<(String, Uint128)> = EPOCH_REVENUE_ACCUMULATION
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .map(|item| item.unwrap())
        .collect();
    to_json_binary(&CurrentEpochRevenueResponse { revenue })
}

/// Query epoch countdown information
pub fn query_epoch_countdown(deps: Deps, env: Env) -> StdResult<cosmwasm_std::Binary> {
    let config = CONFIG.load(deps.storage)?;
    let current_time = env.block.time.seconds();
    let last_distribution = LAST_DISTRIBUTION_TIME.may_load(deps.storage)?.unwrap_or(0u64);
    
    let (epoch_start, epoch_end) = if let Some(window_days) = config.revenue_dispersal_window {
        let window_seconds = window_days * 24 * 60 * 60;
        let start = if last_distribution == 0 {
            // First epoch: use contract instantiation time or current time as fallback
            current_time
        } else {
            last_distribution
        };
        let end = start + window_seconds;
        (start, end)
    } else {
        // No window configured, return current time for both
        (current_time, current_time)
    };
    
    let seconds_remaining = if epoch_end > current_time {
        epoch_end - current_time
    } else {
        0
    };
    
    to_json_binary(&EpochCountdownResponse {
        seconds_remaining,
        epoch_start,
        epoch_end,
        current_time,
    })
}


