use cosmwasm_std::{Deps, StdResult, Uint128, Env, Addr};
use cw_storage_plus::Bound;
use membrane::staking::{TotalStakedResponse, FeeEventsResponse, StakerResponse, RewardsResponse, StakedResponse, DelegationResponse};
use membrane::types::{FeeEvent, StakeDeposit, DelegationInfo};

use crate::contract::{get_deposit_claimables, SECONDS_PER_DAY};
use crate::state::{STAKING_TOTALS, FEE_EVENTS, STAKED, CONFIG, INCENTIVE_SCHEDULING, DELEGATIONS};

const DEFAULT_LIMIT: u32 = 32u32;

/// Returns total of staked tokens for a given staker, includes unstaking tokens
pub fn query_user_stake(deps: Deps, staker: String) -> StdResult<StakerResponse> {
    let config = CONFIG.load(deps.storage)?;    
    let valid_addr = deps.api.addr_validate(&staker)?;

    if config.vesting_contract.is_some() && valid_addr == config.vesting_contract.unwrap() {
        return Ok(StakerResponse {
            staker: valid_addr.to_string(),
            total_staked: STAKING_TOTALS.load(deps.storage)?.vesting_contract,
            deposit_list: vec![],
        })
    }

    let staker_deposits: Vec<StakeDeposit> = STAKED.load(deps.storage, valid_addr.clone())?;

    let deposit_list = staker_deposits
        .clone()
        .into_iter()
        .map(|deposit| (deposit.amount, deposit.stake_time))
        .collect::<Vec<(Uint128, u64)>>();

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

/// Returns claimable assets for a given staker
pub fn query_staker_rewards(deps: Deps, env: Env, staker: String) -> StdResult<RewardsResponse> {
    //Load state
    let config = CONFIG.load(deps.storage)?;
    let incentive_schedule = INCENTIVE_SCHEDULING.load(deps.storage)?;
    //Validate address
    let valid_addr = deps.api.addr_validate(&staker)?;
    //Load state
    let staker_deposits: Vec<StakeDeposit> = STAKED.load(deps.storage, valid_addr.clone())?;
    let fee_events = FEE_EVENTS.load(deps.storage)?;
    let DelegationInfo { delegated, delegated_to, commission: _ } = DELEGATIONS.load(deps.storage, valid_addr.clone())?;

    //Calc total deposits past fee wait period
    let total_rewarding_stake: Uint128 = staker_deposits.clone()
        .into_iter()
        .filter(|deposit| deposit.stake_time + (config.fee_wait_period * SECONDS_PER_DAY) <= env.block.time.seconds())
        .map(|deposit| deposit.amount)
        .sum();

    let mut claimables = vec![];
    let mut accrued_interest = Uint128::zero();
    for deposit in staker_deposits {
        let (claims, incentives) = get_deposit_claimables(
            deps.storage, 
            config.clone(), 
            incentive_schedule.clone(), 
            env.clone(), 
            fee_events.clone(), 
            deposit,
            delegated.clone(),
            delegated_to.clone(),
            total_rewarding_stake,
        )?;
        claimables.extend(claims);
        accrued_interest += incentives;
    }

    Ok(RewardsResponse {
        claimables,
        accrued_interest,
    })
}

/// Returns stake deposits
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

    let mut stakers: Vec<StakeDeposit> = vec![];
    
    let _iter = STAKED
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .map(|item| {
            let stakers_in_loop = item.unwrap_or_else(|_| (Addr::unchecked("null"), vec![])).1;

            let stakers_in_loop = stakers_in_loop.clone()
                .into_iter()
                .filter(|deposit| deposit.stake_time >= start_after && deposit.stake_time < end_before)
                .take(limit as usize)
                .collect::<Vec<StakeDeposit>>();

            stakers.extend(stakers_in_loop);
        });

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

/// Returns historical fee events
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

/// Return staked tokens totals
pub fn query_totals(deps: Deps) -> StdResult<TotalStakedResponse> {
    let totals = STAKING_TOTALS.load(deps.storage)?;

    Ok(TotalStakedResponse {
        total_not_including_vested: totals.stakers,
        vested_total: totals.vesting_contract,
    })
}

/// Returns DelegationInfo
pub fn query_delegations(
    deps: Deps,
    limit: Option<u32>,
    start_after: Option<String>,
    user: Option<String>,
) -> StdResult<Vec<DelegationResponse>> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    let start = start_after.map(|user| Bound::ExclusiveRaw(user.into_bytes()));

    if let Some(user) = user {
        let user = deps.api.addr_validate(&user)?;
        let delegation = DELEGATIONS.load(deps.storage, user.clone())?;

        return Ok(vec![DelegationResponse {
            user,
            delegation_info: delegation,
        }])
    }

    DELEGATIONS
        .range(deps.storage, start, None, cosmwasm_std::Order::Ascending)
        .take(limit as usize)
        .map(|item| {
            let (user, delegation) = item?;

            Ok(DelegationResponse {
                user,
                delegation_info: delegation,
            })
        })
        .collect()
}