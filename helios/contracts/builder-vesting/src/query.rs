use cosmwasm_std::{StdError, Deps, Env, StdResult};
use membrane::builder_vesting::{UnlockedResponse, AllocationResponse, ReceiverResponse};

use crate::{contract::get_unlocked_amount, state::RECEIVERS};


pub fn query_allocation(deps: Deps, receiver: String) -> StdResult<AllocationResponse> {
    let receiver = match RECEIVERS
        .load(deps.storage)?
        .into_iter()
        .find(|stored_receiver| stored_receiver.receiver == receiver)
    {
        Some(receiver) => receiver,
        None => {
            return Err(StdError::GenericErr {
                msg: String::from("Invalid receiver"),
            })
        }
    };

    if receiver.allocation.is_some() {
        let allocation = receiver.allocation.unwrap();
        Ok(AllocationResponse {
            amount: allocation.amount.to_string(),
            amount_withdrawn: allocation.amount_withdrawn.to_string(),
            start_time_of_allocation: allocation.start_time_of_allocation.to_string(),
            vesting_period: allocation.vesting_period,
        })
    } else {
        Err(StdError::GenericErr {
            msg: String::from("Receiver has no allocation"),
        })
    }
}

pub fn query_unlocked(deps: Deps, env: Env, receiver: String) -> StdResult<UnlockedResponse> {
    let receiver = match RECEIVERS
        .load(deps.storage)?
        .into_iter()
        .find(|stored_receiver| stored_receiver.receiver == receiver)
    {
        Some(receiver) => receiver,
        None => {
            return Err(StdError::GenericErr {
                msg: String::from("Invalid receiver"),
            })
        }
    };

    if receiver.allocation.is_some() {
        let unlocked_amount = get_unlocked_amount(receiver.allocation, env.block.time.seconds()).0;
        Ok(UnlockedResponse { unlocked_amount })
    } else {
        Err(StdError::GenericErr {
            msg: String::from("Receiver has no allocation"),
        })
    }
}

pub fn query_receivers(deps: Deps) -> StdResult<Vec<ReceiverResponse>> {
    let receivers = RECEIVERS.load(deps.storage)?;

    let mut resp_list = vec![];

    for receiver in receivers {
        resp_list.push(ReceiverResponse {
            receiver: receiver.receiver.to_string(),
            allocation: receiver.allocation,
            claimables: receiver.claimables,
        })
    }

    Ok(resp_list)
}

pub fn query_receiver(deps: Deps, receiver: String) -> StdResult<ReceiverResponse> {
    let receivers = RECEIVERS.load(deps.storage)?;

    match receivers
        .into_iter()
        .find(|stored_receiver| stored_receiver.receiver == receiver)
    {
        Some(stored_receiver) => Ok(ReceiverResponse {
            receiver: stored_receiver.receiver.to_string(),
            allocation: stored_receiver.allocation,
            claimables: stored_receiver.claimables,
        }),
        None => {
            Err(StdError::GenericErr {
                msg: String::from("Invalid receiver"),
            })
        }
    }
}
