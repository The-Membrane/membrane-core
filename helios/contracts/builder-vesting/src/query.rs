use cosmwasm_std::{StdError, Deps, Env, StdResult};
use membrane::builder_vesting::{UnlockedResponse, AllocationResponse, RecipientResponse};

use crate::{contract::get_unlocked_amount, state::RECIPIENTS};


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
        let unlocked_amount = get_unlocked_amount(recipient.allocation, env.block.time.seconds()).0;
        Ok(UnlockedResponse { unlocked_amount })
    } else {
        Err(StdError::GenericErr {
            msg: String::from("Recipient has no allocation"),
        })
    }
}

pub fn query_recipients(deps: Deps) -> StdResult<Vec<RecipientResponse>> {
    let recipients = RECIPIENTS.load(deps.storage)?;

    let mut resp_list = vec![];

    for recipient in recipients {
        resp_list.push(RecipientResponse {
            recipient: recipient.recipient.to_string(),
            allocation: recipient.allocation,
            claimables: recipient.claimables,
        })
    }

    Ok(resp_list)
}

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
