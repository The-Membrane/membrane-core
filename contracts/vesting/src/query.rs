use cosmwasm_std::{StdError, Deps, Env, StdResult, Uint128, Order};
use membrane::vesting::{UnlockedResponse, AllocationResponse, RecipientResponse, RecipientsResponse, VestingSchedulesResponse, VestingScheduleInfo, VestingStatsResponse};
use std::collections::HashSet;

use crate::{contract::get_unlocked_amount, state::{RECIPIENTS, VESTING_SCHEDULES, OLD_MBRN_RECEIVED}};
use crate::contract::calculate_vested_unlocked;

/// Returns the allocation of a recipient
pub fn query_allocation(deps: Deps, recipient: String) -> StdResult<AllocationResponse> {
    let recipient = match RECIPIENTS
        .load(deps.storage)?
        .into_iter()
        .find(|stored_recipient| stored_recipient.recipient == recipient)
    {
        Some(recipient) => recipient,
        None => {
            return Err(StdError::GenericErr {
                msg: String::from("Invalid recipient"),
            })
        }
    };

    if recipient.allocation.is_some() {
        let allocation = recipient.allocation.unwrap();
        Ok(AllocationResponse {
            amount: allocation.amount,
            amount_withdrawn: allocation.amount_withdrawn,
            start_time_of_allocation: allocation.start_time_of_allocation,
            vesting_period: allocation.vesting_period,
        })
    } else {
        Err(StdError::GenericErr {
            msg: String::from("Recipient has no allocation"),
        })
    }
}

///Returns the amount of tokens that can be unlocked by a recipient
pub fn query_unlocked(deps: Deps, env: Env, recipient: String) -> StdResult<UnlockedResponse> {
    let recipient = match RECIPIENTS
        .load(deps.storage)?
        .into_iter()
        .find(|stored_recipient| stored_recipient.recipient == recipient)
    {
        Some(recipient) => recipient,
        None => {
            return Err(StdError::GenericErr {
                msg: String::from("Invalid recipient"),
            })
        }
    };

    if recipient.allocation.is_some() {
        let unlocked_amount = get_unlocked_amount(recipient.allocation, env.block.time.seconds())?.0;
        Ok(UnlockedResponse { unlocked_amount })
    } else {
        Err(StdError::GenericErr {
            msg: String::from("Recipient has no allocation"),
        })
    }
}

/// Returns the list of recipients
pub fn query_recipients(deps: Deps) -> StdResult<RecipientsResponse> {
    let recipients = RECIPIENTS.load(deps.storage)?;

    let mut resp_list = vec![];
    for recipient in recipients {
        resp_list.push(RecipientResponse {
            recipient: recipient.recipient.to_string(),
            allocation: recipient.allocation,
            claimables: recipient.claimables,
        })
    }

    Ok(RecipientsResponse {
        recipients: resp_list,
    })
}

/// Returns the details of a recipient
pub fn query_recipient(deps: Deps, recipient: String) -> StdResult<RecipientResponse> {
    let recipients = RECIPIENTS.load(deps.storage)?;

    match recipients
        .into_iter()
        .find(|stored_recipient| stored_recipient.recipient == recipient)
    {
        Some(stored_recipient) => Ok(RecipientResponse {
            recipient: stored_recipient.recipient.to_string(),
            allocation: stored_recipient.allocation,
            claimables: stored_recipient.claimables,
        }),
        None => {
            Err(StdError::GenericErr {
                msg: String::from("Invalid recipient"),
            })
        }
    }
}

/// Query all vesting schedules for a user
pub fn query_vesting_schedules(
    deps: Deps,
    env: Env,
    user: String,
) -> StdResult<VestingSchedulesResponse> {
    let current_time = env.block.time.seconds();

    let schedules: Vec<VestingScheduleInfo> = VESTING_SCHEDULES
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|item| {
            let ((schedule_user, week_id), schedule) = item.ok()?;
            if schedule_user == user {
                // Calculate current unlocked amount
                let (unlocked_amount, _) = calculate_vested_unlocked(&schedule, current_time).ok()?;

                Some(VestingScheduleInfo {
                    week_id,
                    mbrn_to_mint: schedule.mbrn_to_mint,
                    amount_withdrawn: schedule.amount_withdrawn,
                    start_time: schedule.start_time,
                    vesting_period: schedule.vesting_period,
                    transmutation_count: schedule.transmutation_count,
                    unlocked_amount,
                })
            } else {
                None
            }
        })
        .collect();

    Ok(VestingSchedulesResponse { schedules })
}

/// Query specific vesting schedule
pub fn query_vesting_schedule(
    deps: Deps,
    env: Env,
    user: String,
    week_id: u64,
) -> StdResult<VestingScheduleInfo> {
    let schedule = VESTING_SCHEDULES
        .may_load(deps.storage, (user.clone(), week_id))?
        .ok_or_else(|| StdError::GenericErr {
            msg: format!("No vesting schedule found for user {} in week {}", user, week_id),
        })?;

    let current_time = env.block.time.seconds();
    let (unlocked_amount, _) = calculate_vested_unlocked(&schedule, current_time)?;

    Ok(VestingScheduleInfo {
        week_id,
        mbrn_to_mint: schedule.mbrn_to_mint,
        amount_withdrawn: schedule.amount_withdrawn,
        start_time: schedule.start_time,
        vesting_period: schedule.vesting_period,
        transmutation_count: schedule.transmutation_count,
        unlocked_amount,
    })
}

/// Query total unlocked across all schedules for a user
pub fn query_total_vested_unlocked(
    deps: Deps,
    env: Env,
    user: String,
) -> StdResult<UnlockedResponse> {
    let current_time = env.block.time.seconds();

    let total_unlocked: Uint128 = VESTING_SCHEDULES
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|item| {
            let ((schedule_user, _week_id), schedule) = item.ok()?;
            if schedule_user == user {
                let (unlocked_amount, _) = calculate_vested_unlocked(&schedule, current_time).ok()?;
                Some(unlocked_amount)
            } else {
                None
            }
        })
        .sum();

    Ok(UnlockedResponse { unlocked_amount: total_unlocked })
}

/// Query global vesting stats
pub fn query_vesting_stats(deps: Deps) -> StdResult<VestingStatsResponse> {
    let total_old_mbrn_received = OLD_MBRN_RECEIVED.load(deps.storage)?;

    let all_schedules: Vec<((String, u64), _)> = VESTING_SCHEDULES
        .range(deps.storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<_>>>()?;

    let total_schedules = all_schedules.len() as u64;

    // Count unique users
    let mut unique_users = HashSet::new();
    for ((user, _), _) in all_schedules {
        unique_users.insert(user);
    }
    let total_users = unique_users.len() as u64;

    Ok(VestingStatsResponse {
        total_old_mbrn_received,
        total_schedules,
        total_users,
    })
}
