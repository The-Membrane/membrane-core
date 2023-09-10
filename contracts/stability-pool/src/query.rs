use cosmwasm_std::{Deps, Env, StdResult, Uint128, Decimal, StdError};
use membrane::types::{AssetPool, Deposit};
use membrane::stability_pool::{LiquidatibleResponse, ClaimsResponse, DepositPositionResponse};
use membrane::helpers::accumulate_interest;

use crate::state::{CONFIG, ASSET, USERS};

/// Return AssetPool with customizers for the deposit list
pub fn query_asset_pool(
    deps: Deps,
    user: Option<String>,
    deposit_limit: Option<u32>,
    start_after: Option<u32>,
) -> StdResult<AssetPool>{    
    let mut asset_pool = ASSET.load(deps.storage)?;

    //Optional User deposits
    if let Some(user) = user {
        let user = deps.api.addr_validate(&user)?;
        asset_pool.deposits = asset_pool.clone().deposits
            .into_iter()
            .filter(|deposit| deposit.user.to_string() == user)
            .collect::<Vec<Deposit>>();
    };

    //Optional start_after deposits
    let start_after: u32  = if let Some(start) = start_after {
        start
    } else { 0 };
    
    //Optional deposits limit
    if let Some(limit) = deposit_limit {
        if start_after + limit > asset_pool.deposits.len() as u32 {
            return Err(StdError::GenericErr { msg: format!("Invalid limit, deposit length: {}", asset_pool.deposits.len()) });
        }
        asset_pool.deposits = asset_pool.deposits[start_after as usize..(start_after+limit) as usize].to_vec();
    } else {
        asset_pool.deposits = asset_pool.deposits[start_after as usize..].to_vec();
    }
    
    Ok(asset_pool)    
}

/// Return a user's frontmost deposit and the amount of capital ahead of it
pub fn query_capital_ahead_of_deposits(
    deps: Deps,
    user: String,
)-> StdResult<Vec<DepositPositionResponse>>{

    let asset_pool: AssetPool = ASSET.load(deps.storage)?;
    let user = deps.api.addr_validate(&user)?;

    let mut capital_ahead = Decimal::zero();
    let mut resp: Vec<DepositPositionResponse> = vec![];
    
    for deposit in asset_pool.deposits{
        //Push new response if we've reached a user deposit
        if deposit.user == user {
            resp.push(
                DepositPositionResponse { 
                    deposit: deposit.clone(), 
                    capital_ahead, 
                }
            );

            //Increase capital_ahead
            capital_ahead += deposit.amount;
        } else { 
            //Add to capital ahead of the next user deposit
            capital_ahead += deposit.amount;
        }
    }

    Ok( resp )
}

/// Return user's available incentives
pub fn query_user_incentives(
    deps: Deps, 
    env: Env,
    user: String,
) -> StdResult<Uint128>{
    let resp: Vec<Deposit> = query_deposits(deps, user)?;
    let rate = CONFIG.load(deps.storage)?.incentive_rate;

    let mut total_incentives = Uint128::zero();
    for deposit in resp {
        match deposit.unstake_time{
            Some(unstake_time) => {
                let time_elapsed = unstake_time - deposit.last_accrued;
                let stake = deposit.amount * Uint128::one();

                total_incentives += accumulate_interest(stake, rate, time_elapsed)?;
            },
            None => {
                let time_elapsed = env.block.time.seconds() - deposit.last_accrued;
                let stake = deposit.amount * Uint128::one();

                total_incentives += accumulate_interest(stake, rate, time_elapsed)?;
            },
        }        
    }

    Ok(total_incentives)
}

/// Return leftover amount from a hypothetical liquidation amount
pub fn query_liquidatible(deps: Deps, amount: Decimal) -> StdResult<LiquidatibleResponse> {
    
    let asset_pool = ASSET.load(deps.storage)?;
    let asset_amount_uint128 = amount * Uint128::new(1u128);
    let liquidatible_amount = asset_pool.credit_asset.amount;

    if liquidatible_amount > asset_amount_uint128 {
        Ok(LiquidatibleResponse {
            leftover: Decimal::percent(0),
        })
    } else {
        let leftover = asset_amount_uint128 - asset_pool.credit_asset.amount;
        Ok(LiquidatibleResponse {
            leftover: Decimal::from_ratio(leftover, Uint128::new(1u128)),
        })
    }
    
}

/// Return user's deposits 
pub fn query_deposits(
    deps: Deps,
    user: String,
) -> StdResult<Vec<Deposit>> {
    let valid_user = deps.api.addr_validate(&user)?;
    let asset_pool = ASSET.load(deps.storage)?;

    Ok(asset_pool
        .deposits
        .into_iter()
        .filter(|deposit| deposit.user == valid_user)
        .collect::<Vec<Deposit>>())
}

/// Return user's claimable assets
pub fn query_user_claims(deps: Deps, user: String) -> StdResult<ClaimsResponse> {
    let valid_user = deps.api.addr_validate(&user)?;

    match USERS.load(deps.storage, valid_user) {
        Ok(user) => {
            Ok(ClaimsResponse {
                claims: user.claimable_assets.to_vec(),
            })
        }
        Err(_) => {
            Err(StdError::GenericErr {
                msg: "User has no claimable assets".to_string(),
            })
        }
    }
}
