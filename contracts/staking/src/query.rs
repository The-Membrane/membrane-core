use cosmwasm_std::{Deps, StdResult, Uint128, Env, Addr, Decimal, StdError};
use cw_storage_plus::Bound;
use membrane::math::decimal_multiplication;
use membrane::staking::{TotalStakedResponse, FeeEventsResponse, StakerResponse, RewardsResponse, StakedResponse, DelegationResponse};
use membrane::types::{Asset, Delegation, DelegationInfo, FeeEvent, StakeDeposit};

use crate::contract::{get_deposit_claimables, get_total_vesting};
use crate::state::{STAKING_TOTALS, FEE_EVENTS, STAKED, CONFIG, INCENTIVE_SCHEDULING, DELEGATIONS, VESTING_STAKE_TIME, DELEGATE_CLAIMS};

const DEFAULT_LIMIT: u32 = 32u32;

/// Returns total of staked tokens for a given staker, includes unstaking tokens
pub fn query_user_stake(deps: Deps, staker: String) -> StdResult<StakerResponse> {
    let config = CONFIG.load(deps.storage)?;    
    let valid_addr = deps.api.addr_validate(&staker)?;

    if config.vesting_contract.is_some() && valid_addr == config.clone().vesting_contract.unwrap() {
        let total = get_total_vesting(deps.querier, config.vesting_contract.unwrap().to_string())?;

        return Ok(StakerResponse {
            staker: valid_addr.to_string(),
            total_staked: total,
            deposit_list: vec![],
        })
    }

    let staker_deposits: Vec<StakeDeposit> = STAKED.load(deps.storage, valid_addr.clone())?;

    let total_staker_deposits: Uint128 = staker_deposits.clone()
        .into_iter()
        .map(|deposit| deposit.amount)
        .collect::<Vec<Uint128>>()
        .into_iter()
        .sum();

    Ok(StakerResponse {
        staker: valid_addr.to_string(),
        total_staked: total_staker_deposits,
        deposit_list: staker_deposits,
    })
}

/// Returns claimable assets for a given user
pub fn query_user_rewards(deps: Deps, env: Env, user: String) -> StdResult<RewardsResponse> {
    //Load state
    let config = CONFIG.load(deps.storage)?;
    let incentive_schedule = INCENTIVE_SCHEDULING.load(deps.storage)?;
    let fee_events = FEE_EVENTS.load(deps.storage)?;
    //Validate address
    let valid_addr = deps.api.addr_validate(&user)?;
    //Load user state
    let user_deposits: Vec<StakeDeposit> = match STAKED.load(deps.storage, valid_addr.clone()){
        Ok(deposits) => { deposits }
        Err(_) => vec![], //Not a staker
    };
    let DelegationInfo { delegated, delegated_to, commission } = match DELEGATIONS.load(deps.storage, valid_addr.clone()){
        Ok(delegation) => delegation,
        Err(_) => DelegationInfo {
            delegated: vec![],
            delegated_to: vec![],
            commission: Decimal::zero(),
        }
    };

    //Get claimables for each deposit
    if user_deposits != vec![] {    
        let total_rewarding_stake: Uint128 = user_deposits.clone()
            .into_iter()
            .map(|deposit| deposit.amount)
            .sum();

        let mut claimables = vec![];
        let mut accrued_interest = Uint128::zero();
        for deposit in user_deposits {
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
                commission,
            )?;
            claimables.extend(claims);
            accrued_interest += incentives;
        }

        //Filter out empty claimables
        claimables = claimables
            .into_iter()
            .filter(|claimable| claimable.amount != Uint128::zero())
            .collect::<Vec<Asset>>();

        Ok(RewardsResponse {
            claimables,
            accrued_interest,
        })
    } else if config.vesting_contract.is_some() && user == config.clone().vesting_contract.unwrap().to_string() {
        //Load total vesting
        let total = STAKING_TOTALS.load(deps.storage)?
            .vesting_contract;
        //Transform total with vesting rev multiplier
        let total = decimal_multiplication(
            Decimal::from_ratio(total, Uint128::one()),
            config.vesting_rev_multiplier)?
        .to_uint_floor();

        let mut claimables = vec![];

        //Create deposit
        let deposit = StakeDeposit {
            staker: Addr::unchecked(config.clone().vesting_contract.unwrap().to_string()),
            amount: total,
            stake_time: VESTING_STAKE_TIME.load(deps.storage)?,
            unstake_start_time: None,
        };

        let (claims, _) = get_deposit_claimables(
            deps.storage, 
            config.clone(), 
            incentive_schedule.clone(), 
            env.clone(), 
            fee_events.clone(), 
            deposit,
            vec![],
            vec![],
            total,
            Decimal::zero(),
        )?;
        claimables.extend(claims);
        
        //Filter out empty claimables
        claimables = claimables
            .into_iter()
            .filter(|claimable| claimable.amount != Uint128::zero())
            .collect::<Vec<Asset>>();

        Ok(RewardsResponse {
            claimables,
            accrued_interest: Uint128::zero(),
        })
    } else if let Ok(claims) = DELEGATE_CLAIMS.load(deps.storage, valid_addr.clone()){
        Ok(RewardsResponse {
            claimables: claims.0.clone().into_iter().map(|coin| Asset { amount: coin.amount, info: membrane::types::AssetInfo::NativeToken { denom: coin.denom } }).collect::<Vec<Asset>>(),
            accrued_interest: claims.1,
        })
    } else {
        Ok(RewardsResponse {
            claimables: vec![],
            accrued_interest: Uint128::zero(),
        })
    }
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
    
    let _iter: Vec<_> = STAKED
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .map(|item| {
            let stakers_in_loop = item.unwrap_or_else(|_| (Addr::unchecked("null"), vec![])).1;
            
            let stakers_in_loop = stakers_in_loop.clone()
                .into_iter()
                .filter(|deposit| deposit.stake_time > start_after && deposit.stake_time < end_before)
                .collect::<Vec<StakeDeposit>>();

            stakers.extend(stakers_in_loop);
        }).collect();

    //Filter out unstakers
    if !unstaking {
        stakers = stakers
            .clone()
            .into_iter()
            .filter(|deposit| deposit.unstake_start_time.is_none())
            .collect::<Vec<StakeDeposit>>();
    }

    //Take limit
    stakers = stakers
        .into_iter()
        .take(limit as usize)
        .collect::<Vec<StakeDeposit>>();

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
    env: Env,
    limit: Option<u32>,
    start_after: Option<String>,
    end_before: Option<u64>,
    user: Option<String>,
) -> StdResult<Vec<DelegationResponse>> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    let start = start_after.map(|user| Bound::ExclusiveRaw(user.into_bytes()));
    let end_before = end_before.unwrap_or_else(|| env.block.time.seconds() + 1u64);

    if let Some(user) = user {
        let user = deps.api.addr_validate(&user)?;
        let mut delegation = match DELEGATIONS.load(deps.storage, user.clone()){
            Ok(delegation) => delegation,
            Err(_) => return Err(StdError::GenericErr { msg: "No delegation info found for user".to_string() }),
        };
        
        //Filter out delegations that don't end before the end_before time
        delegation.delegated = delegation.delegated
            .clone()
            .into_iter()
            .filter(|delegation| delegation.time_of_delegation < end_before)
            .collect::<Vec<Delegation>>();
        delegation.delegated_to = delegation.delegated_to
            .clone()
            .into_iter()
            .filter(|delegation| delegation.time_of_delegation < end_before)
            .collect::<Vec<Delegation>>();

        return Ok(vec![DelegationResponse {
            user,
            delegation_info: delegation,
        }])
    }

    DELEGATIONS
        .range(deps.storage, start, None, cosmwasm_std::Order::Ascending)
        .take(limit as usize)
        .map(|item| {
            let (user, mut delegation) = item?;

            //Filter out delegations that don't end before the end_before time
            delegation.delegated = delegation.delegated
                .clone()
                .into_iter()
                .filter(|delegation| delegation.time_of_delegation < end_before)
                .collect::<Vec<Delegation>>();
            delegation.delegated_to = delegation.delegated_to
                .clone()
                .into_iter()
                .filter(|delegation| delegation.time_of_delegation < end_before)
                .collect::<Vec<Delegation>>();

            Ok(DelegationResponse {
                user,
                delegation_info: delegation,
            })
        })
        .collect()
}