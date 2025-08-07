#![allow(non_snake_case)]
use std::collections::{HashSet, VecDeque};
use cosmwasm_std::{
    attr, entry_point, to_binary, Binary, Decimal, Deps, DepsMut, Env, MessageInfo, Response, StdResult, SubMsg,
};

use cw2::set_contract_version;

use osmosis_std::types::osmosis::poolmanager::v1beta1::{MsgSwapExactAmountIn, SwapAmountInRoute};
use membrane::mm_oracle::QueryMsg as OracleQueryMsg;
use membrane::oracle::PriceResponse;
use membrane::math::decimal_multiplication;
use membrane::mm_swap::{Config, ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg};
use membrane::types::OsmosisRouteInfo;

use crate::error::ContractError;
use crate::state::{CONFIG, ROUTES, SWAP_ROUTES};

// Contract name/version info for migration
const CONTRACT_NAME: &str = "mm_swap";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    // determine owner
    let owner = match msg.owner {
        Some(owner_str) => deps.api.addr_validate(&owner_str)?,
        None => info.sender.clone(),
    };

    let config = Config { owner: owner.clone(), oracle_address: msg.oracle_address.map(|a| deps.api.addr_validate(&a)).transpose()? };

    CONFIG.save(deps.storage, &config)?;
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::new().add_attributes(vec![
        attr("action", "instantiate"),
        attr("owner", owner),
    ]))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Swap { caller, token_in, token_out, max_slippage } => {
            execute_swap(deps, env, info, caller, token_in, token_out, max_slippage)
        }
        ExecuteMsg::UpdateConfig { owner, oracle_address } => update_config(deps, info, owner, oracle_address),
        ExecuteMsg::AddRoute { caller, denom, route_info } => {
            add_route(deps, info, caller, denom, route_info)
        }
        ExecuteMsg::EditRoute { caller, denom, route_info, remove } => {
            edit_route(deps, info, caller, denom, route_info, remove)
        }
    }
}

fn assert_owner(storage: &dyn cosmwasm_std::Storage, sender: &cosmwasm_std::Addr) -> Result<(), ContractError> {
    let config = CONFIG.load(storage)?;
    if &config.owner != sender {
        return Err(ContractError::Unauthorized {});
    }
    Ok(())
}

fn execute_swap(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    caller: String,
    token_in: String,
    token_out: String,
    max_slippage: Decimal,
) -> Result<Response, ContractError> {
    // Load caller-specific swap routes
    let swap_routes_vec = SWAP_ROUTES
        .may_load(deps.storage, caller.clone())?
        .ok_or_else(|| ContractError::CustomError { val: "No routes saved for caller".to_string() })?;

    // Compute path with BFS similar to osmosis-proxy implementation
    let routes: Vec<SwapAmountInRoute> = get_swap_route(swap_routes_vec, token_in.clone(), token_out.clone())?;

    // Determine amount of token_in sent
    let sent_coin = info
        .funds
        .iter()
        .find(|c| c.denom == token_in)
        .cloned()
        .ok_or_else(|| ContractError::CustomError { val: "No matching funds for token_in".to_string() })?;

    // Load config to get oracle address
    let config = CONFIG.load(deps.storage)?;
    let oracle_addr = config.oracle_address.ok_or(ContractError::CustomError { val: "Oracle address unset".to_string() })?;
    // Query prices
    let prices: Vec<PriceResponse> = deps.querier.query_wasm_smart(
        oracle_addr.to_string(),
        &OracleQueryMsg::Prices { caller: caller.clone(), asset_infos: vec![token_in.clone(), token_out.clone()], twap_timeframe: 0, oracle_time_limit: 0 },
    )?;
    let token_in_price = prices[0].clone();
    let token_out_price = prices[1].clone();

    let token_in_value = token_in_price.get_value(sent_coin.amount)?;
    let min_value = decimal_multiplication(token_in_value, Decimal::one() - max_slippage)?;
    let token_out_min_amount = token_out_price.get_amount(min_value)?;

    // Build swap message
    let swap_msg = MsgSwapExactAmountIn {
        sender: env.contract.address.to_string(),
        routes: routes.clone(),
        token_in: Some(osmosis_std::types::cosmos::base::v1beta1::Coin { denom: sent_coin.denom.clone(), amount: sent_coin.amount.to_string(), }),
        token_out_min_amount: token_out_min_amount.to_string(),
    };
    let cosmos_msg: cosmwasm_std::CosmosMsg = swap_msg.clone().into();
    let submsg = SubMsg::new(cosmos_msg);

    Ok(Response::new()
        .add_attribute("action", "swap")
        .add_attribute("caller", caller)
        .add_attribute("token_in", token_in)
        .add_attribute("token_out", token_out)
        .add_attribute("max_slippage", max_slippage.to_string())
        .add_submessage(submsg))
}

/// Breadth-first search to find swap path similar to osmosis-proxy
fn get_swap_route(swap_routes: Vec<membrane::types::SwapRoute>, token_in: String, token_out: String) -> Result<Vec<SwapAmountInRoute>, ContractError> {
    let mut visited: std::collections::HashSet<String> = HashSet::new();
    let mut queue: VecDeque<(String, Vec<SwapAmountInRoute>)> = VecDeque::new();
    queue.push_back((token_in.clone(), vec![]));
    visited.insert(token_in.clone());

    while let Some((current_token, current_path)) = queue.pop_front() {
        for route in swap_routes.iter().filter(|r| r.token_in == current_token) {
            let mut path = current_path.clone();
            path.push(route.route_out.clone());
            let next_token = route.route_out.token_out_denom.clone();
            if next_token == token_out {
                return Ok(path);
            }
            if !visited.contains(&next_token) {
                visited.insert(next_token.clone());
                queue.push_back((next_token, path));
            }
        }
    }

    Err(ContractError::CustomError { val: format!("No route from {} to {}", token_in, token_out) })
}

fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    new_owner: Option<String>,
    oracle_address: Option<String>,
) -> Result<Response, ContractError> {
    let validated_owner = new_owner.map(|o| deps.api.addr_validate(&o)).transpose()?;
    let validated_oracle = oracle_address.map(|o| deps.api.addr_validate(&o)).transpose()?;

    CONFIG.update(deps.storage, |mut cfg| -> Result<Config, ContractError> {
        if info.sender != cfg.owner {
            return Err(ContractError::Unauthorized {});
        }
        if let Some(owner_addr) = validated_owner {
            cfg.owner = owner_addr;
        }
        if let Some(oracle_addr) = validated_oracle {
            cfg.oracle_address = Some(oracle_addr);
        }
        Ok(cfg)
    })?;

    Ok(Response::new().add_attribute("action", "update_config"))
}

fn add_route(
    deps: DepsMut,
    info: MessageInfo,
    caller: String,
    denom: String,
    route_info: OsmosisRouteInfo,
) -> Result<Response, ContractError> {
    assert_owner(deps.storage, &info.sender)?;

    // Save OsmosisRouteInfo for reference
    ROUTES.save(deps.storage, (caller.clone(), denom.clone()), &route_info)?;

    // Convert OsmosisRouteInfo pools to SwapRoute entries (forward & reverse)
    let mut swap_routes = SWAP_ROUTES.may_load(deps.storage, caller.clone())?.unwrap_or_default();

    for pool in route_info.pools_for_osmo_twap.iter() {
        // forward edge denom -> quote
        let forward = SwapAmountInRoute {
            pool_id: pool.pool_id,
            token_out_denom: pool.quote_asset_denom.clone(),
        };
        let forward_route = membrane::types::SwapRoute {
            token_in: denom.clone(),
            route_out: forward.clone(),
        };
        if !swap_routes.iter().any(|r| r.token_in == forward_route.token_in && r.route_out == forward_route.route_out) {
            swap_routes.push(forward_route);
        }

        // reverse edge quote -> denom
        let reverse = SwapAmountInRoute {
            pool_id: pool.pool_id,
            token_out_denom: denom.clone(),
        };
        let reverse_route = membrane::types::SwapRoute {
            token_in: pool.quote_asset_denom.clone(),
            route_out: reverse.clone(),
        };
        if !swap_routes.iter().any(|r| r.token_in == reverse_route.token_in && r.route_out == reverse_route.route_out) {
            swap_routes.push(reverse_route);
        }
    }

    SWAP_ROUTES.save(deps.storage, caller.clone(), &swap_routes)?;

    Ok(Response::new().add_attributes(vec![
        attr("action", "add_route"),
        attr("caller", caller),
        attr("denom", denom),
    ]))
}

fn edit_route(
    deps: DepsMut,
    info: MessageInfo,
    caller: String,
    denom: String,
    route_info: Option<OsmosisRouteInfo>,
    remove: bool,
) -> Result<Response, ContractError> {
    assert_owner(deps.storage, &info.sender)?;

    // Update ROUTES and SWAP_ROUTES accordingly
    if remove {
        ROUTES.remove(deps.storage, (caller.clone(), denom.clone()));

        // Remove from swap routes
        if let Some(mut swap_routes) = SWAP_ROUTES.may_load(deps.storage, caller.clone())? {
            swap_routes.retain(|r| !(r.token_in == denom || r.route_out.token_out_denom == denom));
            SWAP_ROUTES.save(deps.storage, caller.clone(), &swap_routes)?;
        }
    } else if let Some(route) = route_info {
        // Replace stored OsmosisRouteInfo
        ROUTES.save(deps.storage, (caller.clone(), denom.clone()), &route)?;

        // Regenerate swap routes for denom: remove related then add new as in add_route
        let mut swap_routes = SWAP_ROUTES.may_load(deps.storage, caller.clone())?.unwrap_or_default();
        // Remove existing entries
        swap_routes.retain(|r| !(r.token_in == denom || r.route_out.token_out_denom == denom));
        // Add new edges
        for pool in route.pools_for_osmo_twap.iter() {
            let forward = SwapAmountInRoute { pool_id: pool.pool_id, token_out_denom: pool.quote_asset_denom.clone() };
            let forward_route = membrane::types::SwapRoute { token_in: denom.clone(), route_out: forward.clone() };
            swap_routes.push(forward_route);

            let reverse = SwapAmountInRoute { pool_id: pool.pool_id, token_out_denom: denom.clone() };
            let reverse_route = membrane::types::SwapRoute { token_in: pool.quote_asset_denom.clone(), route_out: reverse.clone() };
            swap_routes.push(reverse_route);
        }
        SWAP_ROUTES.save(deps.storage, caller.clone(), &swap_routes)?;
    }

    Ok(Response::new().add_attributes(vec![
        attr("action", "edit_route"),
        attr("caller", caller),
        attr("denom", denom),
        attr("removed", remove.to_string()),
    ]))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::Routes { caller, asset_infos } => {
            // Collect routes
            let res: Vec<(String, OsmosisRouteInfo)> = if let Some(denoms) = asset_infos {
                denoms
                    .into_iter()
                    .filter_map(|d| {
                        ROUTES
                            .may_load(deps.storage, (caller.clone(), d.clone()))
                            .ok()
                            .flatten()
                            .map(|r| (d, r))
                    })
                    .collect()
            } else {
                ROUTES
                    .prefix(caller.clone())
                    .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
                    .map(|item| {
                        item.map(|(denom, route)| (denom, route))
                    })
                    .collect::<StdResult<Vec<_>>>()?
            };
            to_binary(&res)
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    Ok(Response::new().add_attribute("action", "migrate"))
}
