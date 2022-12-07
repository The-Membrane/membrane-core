use cosmwasm_std::{Deps, Env, StdResult, Uint128, Decimal, StdError, WasmQuery, QueryRequest, to_binary};
use membrane::types::{AssetPool, Deposit};
use membrane::stability_pool::{LiquidatibleResponse, ClaimsResponse, DepositPositionResponse};
use membrane::osmosis_proxy::TokenInfoResponse;
use membrane::math::{decimal_division, decimal_multiplication};
use membrane::osmosis_proxy::QueryMsg as OsmoQueryMsg;
use membrane::helpers::accumulate_interest;

use crate::state::{CONFIG, ASSET, USERS};

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
    let rate = query_rate(deps)?;

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

pub fn query_rate( deps: Deps ) -> StdResult<Decimal>{
    let config = CONFIG.load(deps.storage)?;
    let asset_pool: AssetPool = ASSET.load(deps.storage)?;

    let asset_current_supply = deps.querier
    .query::<TokenInfoResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.osmosis_proxy.to_string(),
        msg: to_binary(&OsmoQueryMsg::GetTokenInfo {
            denom: asset_pool.credit_asset.info.to_string(),
        })?,
    }))?
    .current_supply;

    //Set Rate
    //The 2 slope model is based on total credit supply AFTER liquidations.
    //So the users who are distributed liq_funds will get rates based off the AssetPool's total AFTER their funds were used.
    let mut rate = config.incentive_rate;
    if !config
        .desired_ratio_of_total_credit_supply
        .is_zero()
    {
        let asset_util_ratio = decimal_division(
            Decimal::from_ratio(asset_pool.credit_asset.amount, Uint128::new(1u128)),
            Decimal::from_ratio(asset_current_supply, Uint128::new(1u128)),
        );
        let mut proportion_of_desired_util = decimal_division(
            asset_util_ratio,
            config.desired_ratio_of_total_credit_supply,
        );

        if proportion_of_desired_util.is_zero() {
            proportion_of_desired_util = Decimal::one();
        }

        let rate_multiplier = decimal_division(Decimal::one(), proportion_of_desired_util);

        rate = decimal_multiplication(config.incentive_rate, rate_multiplier);
    }

    Ok(rate)
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
