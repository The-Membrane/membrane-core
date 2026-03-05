use std::env;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdError, StdResult,
};
use cw2::set_contract_version;
use membrane::ltv_disco::{Config, ExecuteMsg, InstantiateMsg, QueryMsg, MigrateMsg};

use crate::error::ContractError;
use crate::execute::{
    create_queue, update_queue, submit_deposit,
    request_unstake, complete_unstake, cancel_unstake,
    move_deposit, update_deposit,
    add_bad_debt, add_revenue, claim_revenue_for_user, update_config,
    execute_rate_assurance, execute_set_affiliate,
    set_manager_fee, clean_manager_fee, add_deposit_token_revenue,
    send_mbrn_for_sale,
};
use crate::query::{
    query_config, query_asset_queue, query_backing_deposit,
    query_backing_deposits_by_user, query_can_handle_bad_debt,
    query_cumulative_revenue, query_pending_claims, query_user_lifetime_revenue,
    query_revenue_events, query_assets, query_daily_tvl, query_user_total_deposits,
    query_all_user_deposits, query_vault_token_conversion, query_deposit_token_conversion,
    query_managed_deposit_keys, query_total_insurance, query_manager_fee, query_daily_deposits,
    query_unstake_requests, query_slot_weights, query_average_ltvs,
};
use crate::reply::handle_compound_swap_reply;
use crate::state::CONFIG;

// Reply IDs
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
    let cdp_contract = deps.api.addr_validate(&msg.cdp_contract)?;
    let oracle_contract = deps.api.addr_validate(&msg.oracle_contract)?;
    let chain_proxy_contract = deps.api.addr_validate(&msg.chain_proxy_contract)?;

    let unstaking_period = msg.unstaking_period.unwrap_or(172800u64); // Default 2 days
    let affiliate_fee = msg.affiliate_fee.unwrap_or(cosmwasm_std::Decimal::percent(1));
    let max_management_fee = msg.max_management_fee.unwrap_or(cosmwasm_std::Decimal::percent(5));

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

    let owner = if let Some(owner_str) = msg.owner {
        deps.api.addr_validate(&owner_str)?
    } else {
        info.sender
    };

    let config = Config {
        owner,
        cdp_contract,
        deposit_denom: msg.deposit_denom,
        cdt_denom: msg.cdt_denom,
        minimum_deposit: msg.minimum_deposit,
        unstaking_period,
        oracle_contract,
        chain_proxy_contract,
        emissions_voting_contract,
        affiliate_fee,
        max_management_fee,
        points_system_contract,
        revenue_distributor,
        auction_contract,
        mbrn_denom: msg.mbrn_denom,
    };

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
        ExecuteMsg::CreateQueue { asset, min_ltv, max_ltv } => {
            create_queue(deps, env, info, asset, min_ltv, max_ltv)
        }
        ExecuteMsg::UpdateQueue { asset, min_ltv, max_ltv } => {
            update_queue(deps, env, info, asset, min_ltv, max_ltv)
        }
        ExecuteMsg::SubmitDeposit { deposit_input, deposit_owner, deposit_id, manager, affiliate_address, revenue_destination } => {
            submit_deposit(deps, env, info, deposit_input, deposit_owner, deposit_id, manager, affiliate_address, revenue_destination)
        }
        ExecuteMsg::RequestUnstake { asset, slot, deposit_id, amount } => {
            request_unstake(deps, env, info, asset, slot, deposit_id, amount)
        }
        ExecuteMsg::CompleteUnstake { asset, slot, deposit_id } => {
            complete_unstake(deps, env, info, asset, slot, deposit_id)
        }
        ExecuteMsg::CancelUnstake { asset, slot, deposit_id } => {
            cancel_unstake(deps, env, info, asset, slot, deposit_id)
        }
        ExecuteMsg::MoveDeposit { asset, slot, deposit_id, destination, amount, user } => {
            move_deposit(deps, env, info, asset, slot, deposit_id, destination, amount, user)
        }
        ExecuteMsg::UpdateDeposit { asset, slot, deposit_id, deposit_owner, manager, revenue_destination } => {
            update_deposit(deps, env, info, asset, slot, deposit_id, deposit_owner, manager, revenue_destination)
        }
        ExecuteMsg::AddBadDebt { asset, amount } => {
            add_bad_debt(deps, env, info, asset, amount)
        }
        ExecuteMsg::AddRevenue { asset } => {
            add_revenue(deps, env, info, asset)
        }
        ExecuteMsg::ClaimRevenueForUser { user, asset, limit, compound_action } => {
            claim_revenue_for_user(deps, env, info, user, asset, limit, compound_action)
        }
        ExecuteMsg::UpdateConfig { owner, cdp_contract, deposit_denom, cdt_denom, minimum_deposit, unstaking_period, oracle_contract, chain_proxy_contract, emissions_voting_contract, affiliate_fee, max_management_fee, points_system_contract, revenue_distributor, auction_contract, mbrn_denom } => {
            update_config(
                deps,
                info,
                owner,
                cdp_contract,
                deposit_denom,
                cdt_denom,
                minimum_deposit,
                unstaking_period,
                oracle_contract,
                chain_proxy_contract,
                emissions_voting_contract,
                affiliate_fee,
                max_management_fee,
                points_system_contract,
                revenue_distributor,
                auction_contract,
                mbrn_denom,
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
        ExecuteMsg::SendMBRNForSale { amount, recipient } => {
            send_mbrn_for_sale(deps, env, info, amount, recipient)
        }
        ExecuteMsg::RateAssurance { asset, slot } => {
            execute_rate_assurance(deps, env, info, asset, slot)
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        QueryMsg::GetAssetQueue { assets, limit, start_after } => {
            to_json_binary(&query_asset_queue(deps, assets, limit, start_after)?)
        }
        QueryMsg::GetBackingDeposit { user, asset, slot, deposit_id } => {
            to_json_binary(&query_backing_deposit(deps, user, asset, slot, deposit_id)?)
        }
        QueryMsg::GetBackingDepositsByUser { user, asset, limit, start_after } => {
            to_json_binary(&query_backing_deposits_by_user(deps, user, asset, limit, start_after)?)
        }
        QueryMsg::CanHandleBadDebt { asset, amount } => {
            to_json_binary(&query_can_handle_bad_debt(deps, asset, amount)?)
        }
        QueryMsg::GetCumulativeRevenue { asset, slot } => {
            to_json_binary(&query_cumulative_revenue(deps, asset, slot)?)
        }
        QueryMsg::PendingClaims { user, asset } => {
            to_json_binary(&query_pending_claims(deps, user, asset)?)
        }
        QueryMsg::GetUserLifetimeRevenue { user, asset } => {
            to_json_binary(&query_user_lifetime_revenue(deps, user, asset)?)
        }
        QueryMsg::GetRevenueEvents { asset, slot } => {
            to_json_binary(&query_revenue_events(deps, asset, slot)?)
        }
        QueryMsg::GetAssets {} => {
            to_json_binary(&query_assets(deps)?)
        }
        QueryMsg::GetDailyTVL {} => {
            to_json_binary(&query_daily_tvl(deps)?)
        }
        QueryMsg::GetDailyDeposits { asset } => {
            to_json_binary(&query_daily_deposits(deps, asset)?)
        }
        QueryMsg::UserTotalDeposits { user } => {
            to_json_binary(&query_user_total_deposits(deps, user)?)
        }
        QueryMsg::GetManagedDepositKeys { manager, limit, start_after } => {
            to_json_binary(&query_managed_deposit_keys(deps, manager, limit, start_after)?)
        }
        QueryMsg::GetAllUserDeposits { user } => {
            to_json_binary(&query_all_user_deposits(deps, user)?)
        }
        QueryMsg::VaultTokenConversion { asset, slot, vault_tokens } => {
            to_json_binary(&query_vault_token_conversion(deps, asset, slot, vault_tokens)?)
        }
        QueryMsg::DepositTokenConversion { asset, slot, deposit_tokens } => {
            to_json_binary(&query_deposit_token_conversion(deps, asset, slot, deposit_tokens)?)
        }
        QueryMsg::GetAffiliates { user } => {
            to_json_binary(&crate::state::AFFILIATES.load(deps.storage, user).unwrap_or_default())
        }
        QueryMsg::GetTotalInsurance {} => {
            to_json_binary(&query_total_insurance(deps, env)?)
        }
        QueryMsg::GetManagerFee { manager } => {
            to_json_binary(&query_manager_fee(deps, manager)?)
        }
        QueryMsg::GetUnstakeRequests { user, asset } => {
            to_json_binary(&query_unstake_requests(deps, user, asset)?)
        }
        QueryMsg::GetSlotWeights { asset } => {
            to_json_binary(&query_slot_weights(deps, asset)?)
        }
        QueryMsg::GetAverageLTVs { assets } => {
            to_json_binary(&query_average_ltvs(deps, assets)?)
        }
    }
}

/// Handle replies from sub-messages
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.id {
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
