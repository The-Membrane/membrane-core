use cosmwasm_std::{Deps, Env, StdResult, Uint128, Decimal, StdError, WasmQuery, QueryRequest, to_binary};
use membrane::types::{AssetPool, Deposit};
use membrane::stability_pool::{LiquidatibleResponse, ClaimsResponse, DepositPositionResponse};
use membrane::osmosis_proxy::TokenInfoResponse;
use membrane::math::{decimal_division, decimal_multiplication};
use membrane::osmosis_proxy::QueryMsg as OsmoQueryMsg;
use membrane::helpers::accumulate_interest;

use crate::state::{CONFIG, ASSET, USERS};

pub fn query_asset_pool(
    deps: Deps,
    user: Option<String>,
    deposit_limit: Option<u32>,
) -> StdResult<AssetPool>{    
    let mut asset_pool = ASSET.load(deps.storage)?;
    
    if let Some(limit) = deposit_limit {
        asset_pool.deposits = asset_pool.deposits[0..limit as usize].to_vec();
    } else if let Some(user) = user {
        asset_pool.deposits = asset_pool.clone().deposits
            .into_iter()
            .filter(|deposit| deposit.user.to_string() == user)
            .collect::<Vec<Deposit>>();
    }
    
    Ok(asset_pool)    
}

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

pub fn query_user_claims(deps: Deps, user: String) -> StdResult<ClaimsResponse> {
    let valid_user = deps.api.addr_validate(&user)?;

    match USERS.load(deps.storage, valid_user) {
        Ok(user) => {
            Ok(ClaimsResponse {
                claims: user.claimable_assets,
            })
        }
        Err(_) => {
            Err(StdError::GenericErr {
                msg: "User has no claimable assets".to_string(),
            })
        }
    }
}
