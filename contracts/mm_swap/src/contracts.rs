#![allow(non_snake_case)]
use std::collections::{HashSet, VecDeque};
use cosmwasm_std::{
    attr, entry_point, to_binary, to_json_binary, Addr, BankMsg, Binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdError, StdResult, SubMsg, WasmMsg
};

use cw2::set_contract_version;

use osmosis_std::types::osmosis::poolmanager::v1beta1::{MsgSwapExactAmountIn, SwapAmountInRoute};
use osmosis_std::types::osmosis::gamm::v1beta1::MsgExitPool;
use membrane::mm_oracle::QueryMsg as OracleQueryMsg;
use membrane::oracle::PriceResponse;
use membrane::math::decimal_multiplication;
use membrane::mm_swap::{Config, ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg};
use membrane::types::{AssetInfo, OsmosisRouteInfo};
use membrane::mars_vault_token::ExecuteMsg as MarsVaultExecuteMsg;

use crate::error::ContractError;
use crate::state::{SwapInfo, CONFIG, SWAP_INFO, SWAP_ROUTES};

const SWAP_REPLY_ID: u64 = 2u64;
const USE_BALANCE_SWAP_REPLY_ID: u64 = 3u64;

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
        ExecuteMsg::Swap { caller, token_in: _, token_out, max_slippage } => {
            execute_swap(deps, env, info.funds.clone(), info.sender.clone(), caller, token_out, max_slippage)
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

// fn assert_owner(storage: &dyn cosmwasm_std::Storage, sender: &cosmwasm_std::Addr) -> Result<(), ContractError> {
//     let config = CONFIG.load(storage)?;
//     if &config.owner != sender {
//         return Err(ContractError::Unauthorized {});
//     }
//     Ok(())
// }

fn execute_swap(
    deps: DepsMut,
    env: Env,
    funds: Vec<Coin>,
    swapper: Addr,
    caller: String,
    token_out: String,
    max_slippage: Decimal,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let swap_routes = SWAP_ROUTES.load(deps.storage, caller.clone())?;
    let mut msgs = vec![];
    let mut used_special = false;

    //If no funds sent, error
    if funds.is_empty() {
        return Err(ContractError::ZeroAmount {});
    }

    //create swap msgs for each asset sent
    for coin in funds.into_iter() {
        //Get routes
        let routes: Vec<SwapAmountInRoute> = get_swap_route(swap_routes.clone(), coin.denom.clone(), token_out.clone())?;
        
        //If coin's denom is a VT or a GAMM, do special exit
        if coin.denom.contains("gamm/"){
            //Toggle used_special
            used_special = true;
            //Withdraw from GAMM pool
            let withdraw_msg: CosmosMsg = MsgExitPool {
                sender: env.contract.address.to_string(),
                pool_id: routes[0].pool_id,
                share_in_amount: coin.amount.to_string(),
                token_out_mins: vec![],
            }.into();
            //Add as Submsg
            msgs.push(SubMsg::reply_on_success(withdraw_msg, USE_BALANCE_SWAP_REPLY_ID));
        } 
        //ID 0 means its a VT
        else if routes[0].pool_id == 0 {
            //Toggle used_special
            used_special = true;
            //Get the VT address from the token denom
            let vt_address = coin.denom.split('/').collect::<Vec<&str>>()[1];
            //Exit VT
            let exit_msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: vt_address.to_string(),
                msg: to_json_binary(&MarsVaultExecuteMsg::ExitVault {  })?,
                funds: vec![coin.clone()],
            });
            //Add as Submsg
            msgs.push(SubMsg::reply_on_success(exit_msg, USE_BALANCE_SWAP_REPLY_ID));
        } 
        //Act Normal
        else {        

            //Get token_in & token_out prices
            let token_prices: Vec<PriceResponse> = deps.querier.query_wasm_smart(
                config.oracle_address.clone().unwrap().to_string(), 
                &OracleQueryMsg::Prices { 
                    caller: caller.clone(),
                asset_infos: vec![
                    coin.denom.clone(),
                    token_out.clone(),
                    ],
                twap_timeframe: 0u64,
                oracle_time_limit: 0u64,
            })?;
            let token_in_price = token_prices[0].clone();
            let token_out_price = token_prices[1].clone();

            //Calculate min amount out
            let token_in_value = token_in_price.get_value(coin.amount)?;
            let token_out_min_value = decimal_multiplication(token_in_value, Decimal::one() - max_slippage)?;
            let token_out_min_amount = token_out_price.get_amount(token_out_min_value)?;

            //Create Msg
            let msg: CosmosMsg = MsgSwapExactAmountIn {
                sender: env.contract.address.to_string(),
                routes,
                token_in: Some(osmosis_std::types::cosmos::base::v1beta1::Coin {
                    amount: coin.amount.to_string(),
                    denom: coin.denom,
                }),
                token_out_min_amount: token_out_min_amount.to_string(),
                
            }.into();
            //Add Msgs
            msgs.push(SubMsg::new(msg));
        }
    }

    //Set Swap Info
    SWAP_INFO.save(deps.storage, &SwapInfo {
        swapper,
        caller,
        token_out: token_out.clone(),
        max_slippage,
    })?;
    
    //If we are using a special exit & its the only msg, we don't want to change the reply ID
    if !(msgs.len() == 1 && used_special) {
        //Remove last msg from msgs
        let last_msg = match msgs.pop(){
            Some(msg) => msg,
            None => return Err(ContractError::CustomError { val: String::from("No messages to swap") })
        };

        //Set the last msg of the list to be a submessage with a swap reply
        msgs.push(SubMsg::reply_on_success(last_msg.msg, SWAP_REPLY_ID));
    }


    Ok(Response::new()
    .add_attribute("token_out", token_out)
    .add_attribute("max_slippage", max_slippage.to_string())
    .add_submessages(msgs))
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
    mut caller: String,
    denom: String,
    route_info: OsmosisRouteInfo,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    //Anyone can add_routes
    //Only the owner can use caller's that are not themselves
    if info.sender != config.owner {
        caller = info.sender.to_string();
    }

    // Save OsmosisRouteInfo for reference
    // ROUTES.save(deps.storage, (caller.clone(), denom.clone()), &route_info)?;

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
    mut caller: String,
    denom: String,
    route_info: Option<OsmosisRouteInfo>,
    remove: bool,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    //Anyone can edit_routes
    //Only the owner can use caller's that are not themselves
    if info.sender != config.owner {
        caller = info.sender.to_string();
    }

    // Update ROUTES and SWAP_ROUTES accordingly
    if remove {
        // ROUTES.remove(deps.storage, (caller.clone(), denom.clone()));

        // Remove from swap routes
        if let Some(mut swap_routes) = SWAP_ROUTES.may_load(deps.storage, caller.clone())? {
            swap_routes.retain(|r| !(r.token_in == denom || r.route_out.token_out_denom == denom));
            SWAP_ROUTES.save(deps.storage, caller.clone(), &swap_routes)?;
        }
    } else if let Some(route) = route_info {
        // Replace stored OsmosisRouteInfo
        // ROUTES.save(deps.storage, (caller.clone(), denom.clone()), &route)?;

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
            to_binary(&SWAP_ROUTES.load(deps.storage, caller.clone())?)
        }
    }
}


#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.id {
        SWAP_REPLY_ID => handle_swap_reply(deps, env, msg),
        USE_BALANCE_SWAP_REPLY_ID => handle_swap_balances_reply(deps, env, msg),
        id => Err(StdError::generic_err(format!("invalid reply id: {}", id))),
    }
}


fn handle_swap_balances_reply(
    deps: DepsMut,
    env: Env,
    msg: Reply,
) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(_) => {
            //Get swapper
            let swap_info = SWAP_INFO.load(deps.storage)?;

            //Swap all assets in the contract
            let balances = deps.querier.query_all_balances(&env.contract.address)?;

            //Execute swap with new balances
            let res = match execute_swap(
                deps, 
                env, 
                balances.clone(),
                swap_info.swapper.clone(),
                swap_info.caller.clone(),
                swap_info.token_out.clone(),
                swap_info.max_slippage.clone(),
            ){
                Ok(res) => res,
                Err(err) => return Err(StdError::GenericErr { msg: err.to_string() }),
            };  

            return Ok(res
            .add_attribute("swap_info", format!("{:?}", swap_info))
            .add_attribute("tokens_received", format!("{:?}", balances)))
        } //We only reply on success
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }
}

fn handle_swap_reply(
    deps: DepsMut,
    env: Env,
    msg: Reply,
) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(_) => {
            //Get swapper
            let swapper = SWAP_INFO.load(deps.storage)?.swapper;

            //Send all assets in the contract to the swapper
            let balances = deps.querier.query_all_balances(&env.contract.address)?;

            let msg: CosmosMsg = CosmosMsg::Bank(BankMsg::Send {
                to_address: swapper.clone().to_string(),
                amount: balances.clone(),
            });

            //Remove swapper
            // SWAP_INFO.remove(deps.storage);
            //Don't remove incase we have 2 swap replies due to a special exit

            return Ok(Response::new()
            .add_attribute("swapper", swapper)
            .add_attribute("tokens_received", format!("{:?}", balances))
            .add_message(msg))
        } //We only reply on success
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    Ok(Response::new().add_attribute("action", "migrate"))
}
