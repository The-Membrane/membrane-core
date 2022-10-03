use cosmwasm_std::{Deps, StdResult, Uint128, Env};
use membrane::staking::{TotalStakedResponse, FeeEventsResponse, StakerResponse, RewardsResponse, StakedResponse};
use membrane::types::{FeeEvent, StakeDeposit};

use crate::contract::get_deposit_claimables;
use crate::state::{TOTALS, FEE_EVENTS, STAKED, CONFIG};

const DEFAULT_LIMIT: u32 = 32u32;

pub fn query_user_stake(deps: Deps, staker: String) -> StdResult<StakerResponse> {
    let valid_addr = deps.api.addr_validate(&staker)?;

    let staker_deposits: Vec<StakeDeposit> = STAKED
        .load(deps.storage)?
        .into_iter()
        .filter(|deposit| deposit.staker == valid_addr)
        .collect::<Vec<StakeDeposit>>();

    let deposit_list = staker_deposits
        .clone()
        .into_iter()
        .map(|deposit| (deposit.amount.to_string(), deposit.stake_time.to_string()))
        .collect::<Vec<(String, String)>>();

    let total_staker_deposits: Uint128 = staker_deposits
        .into_iter()
        .map(|deposit| deposit.amount)
        .collect::<Vec<Uint128>>()
        .into_iter()
        .sum();

    Ok(StakerResponse {
        staker: valid_addr.to_string(),
        total_staked: total_staker_deposits,
        deposit_list,
    })
}

pub fn query_staker_rewards(deps: Deps, env: Env, staker: String) -> StdResult<RewardsResponse> {
    let config = CONFIG.load(deps.storage)?;

    let valid_addr = deps.api.addr_validate(&staker)?;

    let staker_deposits: Vec<StakeDeposit> = STAKED
        .load(deps.storage)?
        .into_iter()
        .filter(|deposit| deposit.staker == valid_addr)
        .collect::<Vec<StakeDeposit>>();

    let fee_events = FEE_EVENTS.load(deps.storage)?;

    let mut claimables = vec![];
    let mut accrued_interest = Uint128::zero();
    for deposit in staker_deposits {
        let (claims, incentives) = get_deposit_claimables(config.clone(), env.clone(), fee_events.clone(), deposit)?;
        claimables.extend(claims);
        accrued_interest += incentives;
    }

    Ok(RewardsResponse {
        claimables,
        accrued_interest,
    })
}

pub fn query_staked(
    deps: Deps,
    env: Env,
    limit: Option<u32>,
    start_after: Option<u64>,
    end_before: Option<u64>,
    unstaking: bool,
) -> StdResult<StakedResponse> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    let start_after = start_after.unwrap_or(0u64);
    let end_before = end_before.unwrap_or_else(|| env.block.time.seconds() + 1u64);

    let mut stakers = STAKED
        .load(deps.storage)?
        .into_iter()
        .filter(|deposit| deposit.stake_time >= start_after && deposit.stake_time < end_before)
        .take(limit as usize)
        .collect::<Vec<StakeDeposit>>();

    //Filter out unstakers
    if !unstaking {
        stakers = stakers
            .clone()
            .into_iter()
            .filter(|deposit| deposit.unstake_start_time.is_none())
            .collect::<Vec<StakeDeposit>>();
    }

    Ok(StakedResponse { stakers })
}

pub fn query_fee_events(
    deps: Deps,
    limit: Option<u32>,
    start_after: Option<u64>,
) -> StdResult<FeeEventsResponse> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    let start_after = start_after.unwrap_or(0u64);

    let fee_events = FEE_EVENTS
        .load(deps.storage)?
        .into_iter()
        .filter(|event| event.time_of_event >= start_after)
        .take(limit as usize)
        .collect::<Vec<FeeEvent>>();

    Ok(FeeEventsResponse { fee_events })
}

pub fn query_totals(deps: Deps) -> StdResult<TotalStakedResponse> {
    let totals = TOTALS.load(deps.storage)?;

    Ok(TotalStakedResponse {
        total_not_including_builders: totals.stakers.to_string(),
        builders_total: totals.builders_contract.to_string(),
    })
}
