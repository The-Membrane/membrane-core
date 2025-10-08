use cosmwasm_std::{Deps, StdResult, Uint128, Decimal, Addr};
use membrane::ltv_disco::{
    Config, LTVQueue, BackingDeposit, AverageLTVsResponse,
    LTVQueueResponse, BackingDepositResponse, BackingDepositsByUserResponse
};

use crate::state::{CONFIG, LTV_QUEUES};

const MAX_LIMIT: u32 = 32;

/// Query contract configuration
pub fn query_config(deps: Deps) -> StdResult<Config> {
    CONFIG.load(deps.storage)
}

/// Query LTV queue for an asset
pub fn query_ltv_queue(deps: Deps, asset: String) -> StdResult<LTVQueueResponse> {
    let queue = LTV_QUEUES.load(deps.storage, asset)?;
    Ok(LTVQueueResponse { queue })
}

/// Query backing deposit by ID
pub fn query_backing_deposit(deps: Deps, deposit_id: Uint128, asset: String) -> StdResult<BackingDepositResponse> {
    let queue = LTV_QUEUES.load(deps.storage, asset)?;
    
    let deposit = queue.slots
        .into_iter()
        .flat_map(|slot| slot.deposit_groups)
        .flat_map(|group| group.backing_deposits)
        .find(|deposit| deposit.id == deposit_id)
        .ok_or_else(|| cosmwasm_std::StdError::NotFound {
            kind: "BackingDeposit".to_string(),
        })?;

    Ok(BackingDepositResponse { deposit })
}

/// Query backing deposits by user
pub fn query_backing_deposits_by_user(
    deps: Deps,
    user: String,
    asset: String,
    limit: Option<u32>,
    start_after: Option<Uint128>,
) -> StdResult<BackingDepositsByUserResponse> {
    let queue = LTV_QUEUES.load(deps.storage, asset)?;
    let user_addr = deps.api.addr_validate(&user)?;
    
    let mut deposits = Vec::new();
    let limit = limit.unwrap_or(MAX_LIMIT) as usize;
    let start = start_after.unwrap_or_else(Uint128::zero);

    for slot in queue.slots {
        for group in slot.deposit_groups {
            deposits.extend(
                group.backing_deposits
                    .into_iter()
                    .filter(|deposit| deposit.id > start)
                    .filter(|deposit| deposit.user == user_addr)
                    .collect::<Vec<_>>(),
            );
        }
    }

    deposits.truncate(limit);
    Ok(BackingDepositsByUserResponse { deposits })
}

/// Query average LTVs for assets
pub fn query_average_ltvs(deps: Deps, assets: Vec<String>) -> StdResult<AverageLTVsResponse> {
    let mut total_weighted_ltv = Decimal::zero();
    let mut total_weighted_borrow_ltv = Decimal::zero();
    let mut total_weight = Decimal::zero();
    let mut total_borrow_weight = Decimal::zero();

    for asset in assets {
        if let Ok(queue) = LTV_QUEUES.load(deps.storage, asset) {
            for slot in queue.slots {
                if !slot.total_deposit_tokens.is_zero() {
                    // Use total_deposit_tokens as weight for LTV
                    let weight = Decimal::from_ratio(slot.total_deposit_tokens.u128(), 1u128);
                    total_weighted_ltv += slot.ltv * weight;
                    total_weight += weight;
                }

                for group in slot.deposit_groups {
                    for deposit in group.backing_deposits {
                        let borrow_weight = Decimal::from_ratio(deposit.vault_tokens.u128(), 1u128);
                        total_weighted_borrow_ltv += deposit.max_borrow_ltv * borrow_weight;
                        total_borrow_weight += borrow_weight;
                    }
                }
            }
        }
    }

    let average_max_ltv = if total_weight.is_zero() {
        Decimal::zero()
    } else {
        total_weighted_ltv / total_weight
    };

    let average_max_borrow_ltv = if total_borrow_weight.is_zero() {
        Decimal::zero()
    } else {
        total_weighted_borrow_ltv / total_borrow_weight
    };

    Ok(AverageLTVsResponse { 
        average_max_ltv,
        average_max_borrow_ltv 
    })
}

/// Query if the LTV Disco can handle bad debt for an asset
pub fn query_can_handle_bad_debt(deps: Deps, asset: String, amount: Uint128) -> StdResult<bool> {
    let queue: LTVQueue = match LTV_QUEUES.load(deps.storage, asset){
        Ok(queue) => queue,
        Err(_) => return Ok(false),
    };

    //Sum the total deposit tokens in the queue
    let total_deposit_tokens = queue.slots
        .into_iter()
        .flat_map(|slot| slot.deposit_groups)
        .map(|group| group.total_deposit_tokens)
        .sum::<Uint128>();

    //Check if the total deposit tokens is greater than the amount
    let can_handle_bad_debt = total_deposit_tokens.u128() >= amount.u128();

    Ok(can_handle_bad_debt)
}