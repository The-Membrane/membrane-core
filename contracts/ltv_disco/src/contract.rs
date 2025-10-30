use std::env;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdError, StdResult
};
use cw2::set_contract_version;
use membrane::ltv_disco::{Config, ExecuteMsg, InstantiateMsg, QueryMsg, MigrateMsg};

use crate::error::ContractError;
use crate::execute::{
    create_queue, update_queue, submit_deposit, withdraw_deposit, 
    add_bad_debt, add_revenue, claim_revenue_for_user, disperse_revenue, update_config,
    execute_rate_assurance
};
use crate::query::{
    query_config, query_ltv_queue, query_backing_deposit,
    query_backing_deposits_by_user, query_average_ltvs, query_can_handle_bad_debt, 
    query_cumulative_revenue, query_pending_claims, query_user_lifetime_revenue, query_revenue_events, query_assets, query_daily_tvl, query_user_total_deposits
};
use crate::reply::{handle_liquidation_swap_reply};
use crate::state::{CONFIG};

//Reply IDs
// pub const TRANSMUTER_REPLY_ID: u64 = 1u64;
pub const LIQUIDATION_SWAP_REPLY_ID: u64 = 2u64;

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:ltv_disco";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let config: Config;

    let cdp_contract = deps.api.addr_validate(&msg.cdp_contract)?;
    let oracle_contract = deps.api.addr_validate(&msg.oracle_contract)?;
    let chain_proxy_contract = deps.api.addr_validate(&msg.chain_proxy_contract)?;

    if msg.owner.is_some() {
        config = Config {
            owner: deps.api.addr_validate(&msg.owner.unwrap())?,
            cdp_contract,
            deposit_denom: msg.deposit_denom,
            cdt_denom: msg.cdt_denom,
            minimum_deposit: msg.minimum_deposit,
            waiting_period: msg.waiting_period,
            max_ltv: msg.max_ltv,
            percent_to_disperse: msg.percent_to_disperse,
            dispersal_window: msg.dispersal_window,
            activation_window: msg.activation_window,
            oracle_contract,
            chain_proxy_contract,
        };
    } else {
        config = Config {
            owner: info.sender,
            cdp_contract,
            deposit_denom: msg.deposit_denom,
            cdt_denom: msg.cdt_denom,
            minimum_deposit: msg.minimum_deposit,
            waiting_period: msg.waiting_period,
            max_ltv: msg.max_ltv,
            percent_to_disperse: msg.percent_to_disperse,
            dispersal_window: msg.dispersal_window,
            activation_window: msg.activation_window,
            oracle_contract,
            chain_proxy_contract,
        };
    }

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("config", format!("{:?}", config))
        .add_attribute("contract_address", env.contract.address))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
        match msg {
            ExecuteMsg::CreateQueue { asset } => {
                create_queue(deps, env, info, asset)
            }
        ExecuteMsg::UpdateQueue { asset, max_ltv } => {
            update_queue(deps, env, info, asset, max_ltv)
        }
        ExecuteMsg::SubmitDeposit { deposit_input, deposit_owner } => {
            submit_deposit(deps, env, info, deposit_input, deposit_owner)
        }
        ExecuteMsg::WithdrawDeposit { asset, ltv, max_borrow_ltv, amount } => {
            withdraw_deposit(deps, env, info, asset, ltv, max_borrow_ltv, amount)
        }
        ExecuteMsg::AddBadDebt { asset, amount } => {
            add_bad_debt(deps, env, info, asset, amount)
        }
        ExecuteMsg::AddRevenue { asset } => {
            add_revenue(deps, env, info, asset)
        }
        ExecuteMsg::ClaimRevenueForUser { user, asset, max_ltv: _, max_borrow_ltv: _, limit } => {
            claim_revenue_for_user(deps, env, info, user, asset, limit)
        }
        ExecuteMsg::DisperseRevenue { asset } => {
            disperse_revenue(deps, env, info, asset)
        }
        // ExecuteMsg::RetryFailedBadDebt { asset } => {
        //     retry_failed_bad_debt(deps, env, info, asset)
        // }
        ExecuteMsg::UpdateConfig { owner, cdp_contract, deposit_denom, cdt_denom, minimum_deposit, waiting_period, percent_to_disperse, dispersal_window, activation_window, oracle_contract, chain_proxy_contract } => {
            update_config(deps, info, owner, cdp_contract, deposit_denom, cdt_denom, minimum_deposit, waiting_period, percent_to_disperse, dispersal_window, activation_window, oracle_contract, chain_proxy_contract)
        }
        // ExecuteMsg::PostDepositTrackerEntry { asset, max_ltv, max_borrow_ltv } => {
        //     post_deposit_tracker_entry(deps, env, info, asset, max_ltv, max_borrow_ltv)
        // }
        ExecuteMsg::RateAssurance { asset, max_ltv, max_borrow_ltv } => {
            execute_rate_assurance(deps, env, info, asset, max_ltv, max_borrow_ltv)
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        QueryMsg::GetLTVQueue { asset } => to_json_binary(&query_ltv_queue(deps, asset)?),
        QueryMsg::GetBackingDeposit { user, asset, ltv, max_borrow_ltv } => {
            to_json_binary(&query_backing_deposit(deps, user, asset, ltv, max_borrow_ltv)?)
        }
        QueryMsg::GetBackingDepositsByUser { user, asset, limit, start_after } => {
            to_json_binary(&query_backing_deposits_by_user(deps, user, asset, limit, start_after)?)
        }
        QueryMsg::GetAverageLTVs { assets } => {
            to_json_binary(&query_average_ltvs(deps, assets)?)
        }
        QueryMsg::CanHandleBadDebt { asset, amount } => {
            to_json_binary(&query_can_handle_bad_debt(deps, asset, amount)?)
        }
        QueryMsg::GetCumulativeRevenue { asset, max_ltv, max_borrow_ltv } => {
            to_json_binary(&query_cumulative_revenue(deps, asset, max_ltv, max_borrow_ltv)?)
        }
        QueryMsg::PendingClaims { user, asset } => {
            to_json_binary(&query_pending_claims(deps, user, asset)?)
        }
        QueryMsg::GetUserLifetimeRevenue { user, asset } => {
            to_json_binary(&query_user_lifetime_revenue(deps, user, asset)?)
        }
        QueryMsg::GetRevenueEvents { asset, max_ltv, max_borrow_ltv } => {
            to_json_binary(&query_revenue_events(deps, asset, max_ltv, max_borrow_ltv)?)
        }
        QueryMsg::GetAssets {} => {
            to_json_binary(&query_assets(deps)?)
        }
        QueryMsg::GetDailyTVL {} => {
            to_json_binary(&query_daily_tvl(deps)?)
        }
        QueryMsg::UserTotalDeposits { user } => {
            to_json_binary(&query_user_total_deposits(deps, user)?)
        }
    }
}

/// Handle replies from sub-messages
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.id {
        // TRANSMUTER_REPLY_ID => handle_transmuter_withdraw_reply(deps, env, msg),
        LIQUIDATION_SWAP_REPLY_ID => handle_liquidation_swap_reply(deps, env, msg),
        id => Err(StdError::generic_err(format!("invalid reply id: {}", id))),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(
    deps: DepsMut,
    _env: Env,
    _msg: MigrateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    Ok(Response::new().add_attribute("method", "migrate"))
}
