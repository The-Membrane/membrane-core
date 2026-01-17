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
    refresh_lock,
    create_queue, update_queue, submit_deposit, withdraw_deposit, 
    add_bad_debt, add_revenue, claim_revenue_for_user, disperse_revenue, update_config,
    execute_rate_assurance, lock_deposit, move_deposit, update_manager, toggle_withdrawals, execute_set_affiliate,
    set_manager_fee, clean_manager_fee, add_deposit_token_revenue
};
use crate::query::{
    query_config, query_ltv_queue, query_backing_deposit,
    query_backing_deposits_by_user, query_average_ltvs, query_can_handle_bad_debt, 
    query_cumulative_revenue, query_pending_claims, query_user_lifetime_revenue, query_revenue_events, query_assets, query_daily_tvl, query_daily_ltv, query_user_total_deposits, query_locked_deposits,
    query_vault_token_conversion, query_deposit_token_conversion, query_managed_deposit_keys, query_total_insurance, query_manager_fee
};
use crate::reply::{handle_liquidation_swap_reply, handle_compound_swap_reply};
use crate::state::{CONFIG};

//Reply IDs
// pub const TRANSMUTER_REPLY_ID: u64 = 1u64;
pub const LIQUIDATION_SWAP_REPLY_ID: u64 = 2u64;
pub const COMPOUND_SWAP_REPLY_ID: u64 = 3u64;

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

    let lock_duration_ceiling = msg.lock_duration_ceiling.unwrap_or(365u64); // Default 365 days
    let affiliate_fee = msg.affiliate_fee.unwrap_or(cosmwasm_std::Decimal::percent(1)); // Default 1%
    let max_management_fee = msg.max_management_fee.unwrap_or(cosmwasm_std::Decimal::percent(5)); // Default 0%
    let ltv_delta_minimum = msg.ltv_delta_minimum.unwrap_or(cosmwasm_std::Decimal::percent(1)); // Default 1%
    
    // Validate max_management_fee
    if max_management_fee > cosmwasm_std::Decimal::one() {
        return Err(ContractError::CustomError {
            val: "max_management_fee must be less than or equal to 1".to_string(),
        });
    }

    let emissions_voting_contract = msg.emissions_voting_contract
        .map(|addr| deps.api.addr_validate(&addr))
        .transpose()?;

    let points_system_contract = msg.points_system_contract
        .map(|addr| deps.api.addr_validate(&addr))
        .transpose()?;

    let revenue_distributor = msg.revenue_distributor
        .map(|addr| deps.api.addr_validate(&addr))
        .transpose()?;

    let auction_contract = msg.auction_contract
        .map(|addr| deps.api.addr_validate(&addr))
        .transpose()?;

    if msg.owner.is_some() {
        config = Config {
            owner: deps.api.addr_validate(&msg.owner.unwrap())?,
            cdp_contract,
            deposit_denom: msg.deposit_denom,
            cdt_denom: msg.cdt_denom,
            minimum_deposit: msg.minimum_deposit,
            max_ltv: msg.max_ltv,
            percent_to_disperse: msg.percent_to_disperse,
            dispersal_window: msg.dispersal_window,
            activation_window: msg.activation_window,
            oracle_contract,
            chain_proxy_contract,
            emissions_voting_contract,
            lock_duration_ceiling,
            affiliate_fee,
            max_management_fee,
            ltv_delta_minimum,
            points_system_contract,
            revenue_distributor,
            auction_contract,
            mbrn_denom: msg.mbrn_denom,
        };
    } else {
        config = Config {
            owner: info.sender,
            cdp_contract,
            deposit_denom: msg.deposit_denom,
            cdt_denom: msg.cdt_denom,
            minimum_deposit: msg.minimum_deposit,
            max_ltv: msg.max_ltv,
            percent_to_disperse: msg.percent_to_disperse,
            dispersal_window: msg.dispersal_window,
            activation_window: msg.activation_window,
            oracle_contract,
            chain_proxy_contract,
            emissions_voting_contract,
            lock_duration_ceiling,
            affiliate_fee,
            max_management_fee,
            ltv_delta_minimum,
            points_system_contract,
            revenue_distributor,
            auction_contract,
            mbrn_denom: msg.mbrn_denom,
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
        ExecuteMsg::UpdateQueue { asset, max_ltv, percent_to_disperse } => {
            update_queue(deps, env, info, asset, max_ltv, percent_to_disperse)
        }
        ExecuteMsg::SubmitDeposit { deposit_input, deposit_owner, locked, deposit_id, manager, affiliate_address } => {
            submit_deposit(deps, env, info, deposit_input, deposit_owner, locked, deposit_id, manager, affiliate_address)
        }
        ExecuteMsg::WithdrawDeposit { asset, ltv, max_borrow_ltv, deposit_id, amount, epoch_start_time } => {
            withdraw_deposit(deps, env, info, asset, ltv, max_borrow_ltv, deposit_id, amount, epoch_start_time)
        }
        ExecuteMsg::Lock { asset, ltv, max_borrow_ltv, deposit_id, locked, amount, epoch_start_time } => {
            lock_deposit(deps, env, info, asset, ltv, max_borrow_ltv, deposit_id, locked, amount, epoch_start_time)
        }
        ExecuteMsg::RefreshLock { user, asset, ltv, max_borrow_ltv, deposit_id, epoch_start_time } => {
            refresh_lock(deps, env, info, user, asset, ltv, max_borrow_ltv, deposit_id, epoch_start_time)
        }
        ExecuteMsg::MoveDeposit { asset, ltv, max_borrow_ltv, deposit_id, destination, amount, user, epoch_start_time } => {
            move_deposit(deps, env, info, asset, ltv, max_borrow_ltv, deposit_id, destination, amount, user, epoch_start_time)
        }
        ExecuteMsg::UpdateManager { asset, ltv, max_borrow_ltv, deposit_id, manager, epoch_start_time } => {
            update_manager(deps, env, info, asset, ltv, max_borrow_ltv, deposit_id, manager, epoch_start_time)
        }
        ExecuteMsg::ToggleWithdrawals { user, asset, ltv, max_borrow_ltv, deposit_id, enabled, epoch_start_time } => {
            toggle_withdrawals(deps, info, user, asset, ltv, max_borrow_ltv, deposit_id, enabled, epoch_start_time)
        }
        ExecuteMsg::AddBadDebt { asset, amount } => {
            add_bad_debt(deps, env, info, asset, amount)
        }
        ExecuteMsg::AddRevenue { asset } => {
            add_revenue(deps, env, info, asset)
        }
        ExecuteMsg::ClaimRevenueForUser { user, asset, max_ltv: _, max_borrow_ltv: _, limit, compound_action } => {
            claim_revenue_for_user(deps, env, info, user, asset, limit, compound_action)
        }
        ExecuteMsg::DisperseRevenue { asset } => {
            disperse_revenue(deps, env, info, asset)
        }
        // ExecuteMsg::RetryFailedBadDebt { asset } => {
        //     retry_failed_bad_debt(deps, env, info, asset)
        // }
        ExecuteMsg::UpdateConfig { owner, cdp_contract, deposit_denom, cdt_denom, minimum_deposit, percent_to_disperse, dispersal_window, activation_window, oracle_contract, chain_proxy_contract, emissions_voting_contract, lock_duration_ceiling, affiliate_fee, max_management_fee, ltv_delta_minimum, points_system_contract, revenue_distributor, auction_contract, mbrn_denom } => {
            update_config(
                deps, 
                info, 
                owner, 
                cdp_contract, 
                deposit_denom, 
                cdt_denom, 
                minimum_deposit, 
                percent_to_disperse, 
                dispersal_window, 
                activation_window, 
                oracle_contract, 
                chain_proxy_contract, 
                emissions_voting_contract,
                lock_duration_ceiling,
                affiliate_fee,
                max_management_fee,
                ltv_delta_minimum,
                points_system_contract,
                revenue_distributor,
                auction_contract,
                mbrn_denom
            )
        }
        ExecuteMsg::SetAffiliate { user, affiliate_address, label } => {
            execute_set_affiliate(deps, env, info, user, affiliate_address, label)
        }
        ExecuteMsg::SetManagerFee { fee } => {
            set_manager_fee(deps, info, fee)
        }
        ExecuteMsg::CleanManagerFee { manager } => {
            clean_manager_fee(deps, info, manager)
        }
        ExecuteMsg::AddDepositTokenRevenue { per_asset_distribution } => {
            add_deposit_token_revenue(deps, env, info, per_asset_distribution)
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
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        QueryMsg::GetLTVQueue { asset } => to_json_binary(&query_ltv_queue(deps, env, asset)?),
        QueryMsg::GetBackingDeposit { user, asset, ltv, max_borrow_ltv, deposit_id } => {
            to_json_binary(&query_backing_deposit(deps, user, asset, ltv, max_borrow_ltv, deposit_id)?)
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
        QueryMsg::GetDailyLTV { asset } => {
            to_json_binary(&query_daily_ltv(deps, asset)?)
        }
        QueryMsg::UserTotalDeposits { user } => {
            to_json_binary(&query_user_total_deposits(deps, user)?)
        }
        QueryMsg::GetManagedDepositKeys { manager, limit, start_after } => {
            to_json_binary(&query_managed_deposit_keys(deps, manager, limit, start_after)?)
        }
        QueryMsg::GetLockedDeposits { user } => {
            to_json_binary(&query_locked_deposits(deps, user)?)
        }
        QueryMsg::VaultTokenConversion { asset, ltv, max_borrow_ltv, vault_tokens } => {
            to_json_binary(&query_vault_token_conversion(deps, asset, ltv, max_borrow_ltv, vault_tokens)?)
        }
        QueryMsg::DepositTokenConversion { asset, ltv, max_borrow_ltv, deposit_tokens } => {
            to_json_binary(&query_deposit_token_conversion(deps, asset, ltv, max_borrow_ltv, deposit_tokens)?)
        }
        QueryMsg::GetAffiliates { user } => {
            to_json_binary(&crate::state::AFFILIATES.load(deps.storage, user).unwrap_or_else(|_| vec![]))
        }
        QueryMsg::GetTotalInsurance {} => {
            to_json_binary(&query_total_insurance(deps, env)?)
        }
        QueryMsg::GetManagerFee { manager } => {
            to_json_binary(&query_manager_fee(deps, manager)?)
        }
    }
}

/// Handle replies from sub-messages
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.id {
        // TRANSMUTER_REPLY_ID => handle_transmuter_withdraw_reply(deps, env, msg),
        LIQUIDATION_SWAP_REPLY_ID => handle_liquidation_swap_reply(deps, env, msg),
        COMPOUND_SWAP_REPLY_ID => handle_compound_swap_reply(deps, env, msg).map_err(|e| StdError::generic_err(e.to_string())),
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
