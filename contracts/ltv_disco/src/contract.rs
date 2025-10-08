use std::env;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdError, StdResult
};
use cw2::set_contract_version;
use membrane::ltv_disco::{Config, ExecuteMsg, InstantiateMsg, QueryMsg, MigrateMsg};

use crate::error::ContractError;
use crate::execute::{
    create_queue, update_queue, submit_deposit, withdraw_deposit, 
    add_bad_debt, add_revenue, activate_dispersal, disperse_revenue, update_config, retry_failed_bad_debt
};
use crate::query::{
    query_config, query_ltv_queue, query_backing_deposit, 
    query_backing_deposits_by_user, query_average_ltvs, query_can_handle_bad_debt
};
use crate::reply::handle_transmuter_withdraw_reply;
use crate::state::{CONFIG};

//Reply IDs
pub const TRANSMUTER_REPLY_ID: u64 = 1u64;

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

    if msg.owner.is_some() {
        config = Config {
            owner: deps.api.addr_validate(&msg.owner.unwrap())?,
            cdp_contract,
            deposit_denom: msg.deposit_denom,
            minimum_deposit: msg.minimum_deposit,
            waiting_period: msg.waiting_period,
            max_ltv: msg.max_ltv,
            percent_to_disperse: msg.percent_to_disperse,
            dispersal_window: msg.dispersal_window,
            activation_window: msg.activation_window
        };
    } else {
        config = Config {
            owner: info.sender,
            cdp_contract,
            deposit_denom: msg.deposit_denom,
            minimum_deposit: msg.minimum_deposit,
            waiting_period: msg.waiting_period,
            max_ltv: msg.max_ltv,
            percent_to_disperse: msg.percent_to_disperse,
            dispersal_window: msg.dispersal_window,
            activation_window: msg.activation_window
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
        ExecuteMsg::WithdrawDeposit { deposit_id, asset, amount } => {
            withdraw_deposit(deps, env, info, deposit_id, asset, amount)
        }
        ExecuteMsg::AddBadDebt { asset, amount } => {
            add_bad_debt(deps, env, info, asset, amount)
        }
        ExecuteMsg::AddRevenue { asset } => {
            add_revenue(deps, env, info, asset)
        }
        ExecuteMsg::ActivateDispersal { asset, dispersal_window } => {
            activate_dispersal(deps, env, info, asset, dispersal_window)
        }
        ExecuteMsg::DisperseRevenue { asset } => {
            disperse_revenue(deps, env, info, asset)
        }
        ExecuteMsg::RetryFailedBadDebt { asset } => {
            retry_failed_bad_debt(deps, env, info, asset)
        }
        ExecuteMsg::UpdateConfig { owner, cdp_contract, deposit_denom, minimum_deposit, waiting_period, percent_to_disperse, dispersal_window, activation_window} => {
            update_config(deps, info, owner, cdp_contract, deposit_denom, minimum_deposit, waiting_period, percent_to_disperse, dispersal_window, activation_window)
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        QueryMsg::GetLTVQueue { asset } => to_json_binary(&query_ltv_queue(deps, asset)?),
        QueryMsg::GetBackingDeposit { deposit_id, asset } => {
            to_json_binary(&query_backing_deposit(deps, deposit_id, asset)?)
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
    }
}

/// Handle replies from sub-messages
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.id {
        TRANSMUTER_REPLY_ID => handle_transmuter_withdraw_reply(deps, env, msg),
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
