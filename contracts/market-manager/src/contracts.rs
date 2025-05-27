use std::str::FromStr;

use cosmwasm_std::{
    attr, entry_point, to_json_binary, Binary, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, Order, QueryRequest, Reply, Response, StdError, StdResult, SubMsg, Uint128, WasmMsg, WasmQuery
};
use cw2::set_contract_version;

use cw_storage_plus::Bound;
use membrane::market_manager::{Config, ExecuteMsg, InstantiateMsg, ManagerEdit, MarketData, MarketInstantiation, MarketItem, MigrateMsg, PendingMarket, QueryMsg};
use membrane::managed_market::{InstantiateMsg as ManagedMarketInstantiateMsg, Config as ManagedMarketConfig, MarketParams, QueryMsg as ManagedMarketQueryMsg};


use crate::error::ContractError;
use crate::state::{CONFIG, MANAGED_MARKETS, OWNERSHIP_TRANSFER, PENDING_MARKET};

// Contract name and version used for migration.
const CONTRACT_NAME: &str = "market_manager";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Constants
const MAX_LIMIT: u64 = 31u64;
const INSTANTIATE_REPLY_ID: u64 = 1;

//Todo
// - Remove the initial test market instead of adding the social links to it

//Config
// - manager whitelist
// - current managed market CODE_ID

// State
// - Map of <Manager, Vec<Market Address>>

//This contract will:
// - Allow the managers to instantiate new markets
// - Allow this contract's owner to update the list of managers
// - Allow this contract's owner to update the current managed market code id

// Set config
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let config: Config = Config {
        owner: deps.api.addr_validate(&msg.owner)?,
        managed_market_code_id: msg.managed_market_code_id,
        manager_whitelist: msg.manager_whitelist.into_iter()
            .map(|addr| deps.api.addr_validate(&addr))
            .collect::<Result<Vec<_>, _>>()?,
        osmosis_proxy_contract: deps.api.addr_validate(&msg.osmosis_proxy_contract)?,
    };

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()    
        .add_attribute("method", "instantiate")
        .add_attribute("config", format!("{:?}", config))
        .add_attribute("contract_address", env.contract.address)
    )
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::UpdateConfig {
            owner,
            managed_market_code_id,
            edit_managers,
        } => update_config(deps, info, owner, managed_market_code_id, edit_managers),
        ExecuteMsg::InstantiateMarket { params } => {
            instantiate_market(deps, _env, info, params)
        },
        ExecuteMsg::MigrateMarket { market_address } => {
            migrate_market(deps, info, market_address)
        },
        ExecuteMsg::UpdateMarketItem { market_address, socials } => {
            update_market_item(deps, info, market_address, socials)
        },
    }
}

//Update market item of existing market
// - This will be done by the manager of the market
fn update_market_item(
    deps: DepsMut,
    info: MessageInfo,
    market_address: String,
    // Update the socials of the market
    socials: Option<Vec<String>>,
) -> Result<Response, ContractError> {

    //Load the manager's (sender's) markets
    let mut manager_markets = MANAGED_MARKETS.load(deps.storage, info.sender.clone().to_string())?;

    //Check if the market is managed by the sender
    let (index, _) = manager_markets.clone()
        .into_iter()
        .enumerate()
        .find(|(_, m)| m.address == market_address)
        .ok_or(ContractError::Unauthorized {})?;
    
    //Update the market item
    if let Some(socials) = socials.clone() {
        manager_markets[index].socials = socials;
    }
    //Save the updated market item
    MANAGED_MARKETS.save(deps.storage, info.sender.clone().to_string(), &manager_markets)?;

    Ok(Response::new()
        .add_attribute("method", "update_market_item")
        .add_attribute("market_address", market_address)
        .add_attribute("manager", info.sender.to_string())
        .add_attribute("socials", format!("{:?}", socials))
    )

}

// Migrate an existing market
// - This will be done by the manager of the market
fn migrate_market(
    deps: DepsMut,
    info: MessageInfo,
    market_address: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    //Load the manager's (sender's) markets
    let manager_markets = MANAGED_MARKETS.load(deps.storage, info.sender.clone().to_string())?;

    //Check if the market is managed by the sender
    let _ = manager_markets
        .iter()
        .find(|m| m.address == market_address)
        .ok_or(ContractError::Unauthorized {})?;
    
    //Create migration message
    let msg = CosmosMsg::Wasm(WasmMsg::Migrate {
        contract_addr: market_address.clone(),
        new_code_id: config.managed_market_code_id,
        msg: to_json_binary(&MigrateMsg {})?,
    });

    Ok(Response::new()
        .add_message(msg)
        .add_attribute("method", "migrate_market")
        .add_attribute("market_address", market_address)
        .add_attribute("manager", info.sender.to_string())
    )
}

// Instantiate a new market
fn instantiate_market(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    params: MarketInstantiation,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    //Check if sender is a manager
    if !config.manager_whitelist.contains(&info.sender) {
        return Err(ContractError::Unauthorized {});
    }

    //Instantiate new market
    let msg = CosmosMsg::Wasm(WasmMsg::Instantiate {
        admin: Some(env.contract.address.to_string()),
        code_id: config.managed_market_code_id,
        msg: to_json_binary(&ManagedMarketInstantiateMsg {
            owner: info.sender.to_string(),
            osmosis_proxy_contract: config.osmosis_proxy_contract.to_string(),
            whitelisted_debt_suppliers: params.clone().whitelisted_debt_suppliers,
            max_slippage: params.clone().max_slippage,
            collateral_params: params.clone().collateral_params,
            rate_params: params.clone().rate_params,
            pool_for_oracle_and_liquidations: params.clone().pool_for_oracle_and_liquidations,
            borrow_fee: params.clone().borrow_fee,
            whitelisted_collateral_suppliers: params.clone().whitelisted_collateral_suppliers,
            pause_option: params.clone().pause_option,
            debt_supply_cap: params.clone().debt_supply_cap,
            borrow_cap: params.clone().borrow_cap,
            per_user_debt_cap: params.clone().per_user_debt_cap,
            debt_minimum: params.clone().debt_minimum,
            
            
        })?,
        funds: vec![],
        label: format!("Managed Market by {}", info.sender.to_string()),
    });

    //Convert to submsg.
    // This is a reply on success message, so we can get the contract address
    let msg = SubMsg::reply_on_success(msg, INSTANTIATE_REPLY_ID);

    //Save pending market info 
    PENDING_MARKET.save(deps.storage, &PendingMarket {
        name: params.name.clone(),
        socials: params.socials.clone(),
        manager: info.sender.to_string(),
    })?;

    Ok(Response::new()
        .add_submessage(msg)
        .add_attribute("method", "instantiate_market")
        .add_attribute("name", params.clone().name)
        .add_attribute("manager",  info.sender.to_string())
        .add_attribute("market_params", format!("{:?}", params))
    )
}


// Update contract configuration
fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    managed_market_code_id: Option<u64>,
    edit_managers: Option<ManagerEdit>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    let mut attrs = vec![attr("method", "update_config")];

    //Assert Authority
    if info.sender != config.owner {
        //Check if ownership transfer is in progress & transfer if so
        if info.sender == OWNERSHIP_TRANSFER.load(deps.storage)? {
            config.owner = info.sender;
        } else {
            return Err(ContractError::Unauthorized {});
        }
    }

    //Update config
    if let Some(code_id) = managed_market_code_id {
        config.managed_market_code_id = code_id;
        attrs.push(attr("managed_market_code_id", code_id.to_string()));
    }
    //Update managers
    if let Some(edit) = edit_managers {
        //Add managers
        if let Some(add) = edit.add.clone() {
            for addr in add {
                let valid_addr = deps.api.addr_validate(&addr)?;
                config.manager_whitelist.push(valid_addr);
            }
            attrs.push(attr("added_managers", format!("{:?}", edit.add)));
        }
        //Remove managers   
        if let Some(remove) = edit.remove.clone() {
            for addr in remove {
                let valid_addr = deps.api.addr_validate(&addr)?;
                config.manager_whitelist.retain(|x| x != &valid_addr);
            }
            attrs.push(attr("removed_managers", format!("{:?}", edit.remove)));
        }
    }

    //Save optionals
    if let Some(addr) = owner {
        let valid_addr = deps.api.addr_validate(&addr)?;

        //Set owner transfer state
        OWNERSHIP_TRANSFER.save(deps.storage, &valid_addr)?;
        attrs.push(attr("owner_transfer", valid_addr));   
    }
    
    //Save Config
    CONFIG.save(deps.storage, &config)?;
    attrs.push(attr("updated_config", format!("{:?}", config)));

    Ok(Response::new()
        .add_attributes(attrs))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::MarketsManaged { manager } => {
            let markets = MANAGED_MARKETS
                .may_load(deps.storage, manager)?
                .unwrap_or_default();
            to_json_binary(&markets)
        },
        QueryMsg::Managers { start_after, limit } => {
            let managers = query_managers(deps, _env, start_after, limit)?;
            to_json_binary(&managers)
        },
        QueryMsg::MarketParams { manager, start_after, limit } => {
            let market_params = query_market_params(deps, _env, manager, start_after, limit)?;
            to_json_binary(&market_params)
        },

    }
}


//Query Markets managed by a manager
//Return the list of managers
fn query_market_params(
    deps: Deps,
    _env: Env,
    // Manager
    manager: String,
    //Market contract
    start_after: Option<String>,
    //Market limiter
    limit: Option<u32>,
) -> StdResult<Vec<MarketData>> {
    let limit = limit.unwrap_or(MAX_LIMIT as u32);
    
    //Get the markets from the MANAGED_MARKETS map
    let markets = MANAGED_MARKETS
        .may_load(deps.storage, manager)?
        .unwrap_or_default();
    //Limit the markets by finding the index of the start_after market
    let start_index = if let Some(start_after) = start_after {
        markets.iter().position(|m| m.address == start_after).unwrap_or(0)
    } else {
        0
    };
    //Limit the markets
    let markets = markets
        .into_iter()
        .skip(start_index)
        .take(limit as usize)
        .collect::<Vec<_>>();

    //For each market
    // - Query the Market's config
    // - Query the Market's collateral denoms
    // - Query the Market's market params per collateral 
    // - Create an instance of MarketData for each queried per collateral params
    // - Return the list of MarketData
    let mut market_data = vec![];
    for market in markets {

        let market_config: ManagedMarketConfig = deps.querier.query::<ManagedMarketConfig>(
            &QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: market.address.clone(),
                msg: to_json_binary(&ManagedMarketQueryMsg::Config {})?,
            }),
        )?;

        // let collateral_denoms: Vec<String> = deps.querier.query::<Vec<String>>(
        //     &QueryRequest::Wasm(WasmQuery::Smart {
        //         contract_addr: market.address.clone(),
        //         msg: to_json_binary(&ManagedMarketQueryMsg::GetCollateralAssets { 
        //             start_after: None, 
        //             limit: None
        //         })?,
        //     }),
        // )?;

        let market_params: Vec<MarketParams> = deps.querier.query::<Vec<MarketParams>>(
            &QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: market.address.clone(),
                msg: to_json_binary(&ManagedMarketQueryMsg::MarketParams {
                    collateral_denom: None,
                    start_after: None,
                    limit: None,
                })?,
            }),
        )?;

        //Create MarketData
        for params in market_params {
            let data = MarketData {
                address: market.address.clone(),
                name: market.name.clone(),
                socials: market.socials.clone(),
                config: market_config.clone(),
                params: params,
            };
            market_data.push(data);
        }        
    }


    //Return
    Ok(market_data)
}

//Query managers
//Return the list of managers
fn query_managers(
    deps: Deps,
    _env: Env,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<Vec<String>> {
    let limit = limit.unwrap_or(MAX_LIMIT as u32);
    let start = if let Some(start) = start_after {
        Some(Bound::exclusive(start))
    } else {
        None
    };

    //Get the managers from the Keys of the MANAGED_MARKETS map
    let managers = MANAGED_MARKETS
        .keys(deps.storage, start, None, Order::Ascending)
        .take(limit as usize)
        .collect::<StdResult<Vec<String>>>()?;

    //Return
    Ok(managers)
}


#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.id {
        INSTANTIATE_REPLY_ID => handle_instantiate_reply(deps, env, msg),
        id => Err(StdError::generic_err(format!("invalid reply id: {}", id))),
    }
}

/// Save contract address for newly instantiated market
pub fn handle_instantiate_reply(deps: DepsMut, _env: Env, msg: Reply)-> StdResult<Response>{
    match msg.result.into_result() {
        Ok(result) => {
            let config = CONFIG.load(deps.storage)?;
            
            //Get contract address
            let instantiate_event = result
                .events
                .iter()
                .find(|e| {
                    e.attributes
                        .iter()
                        .any(|attr| attr.key == "_contract_address")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "_contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save new address under the manager's state
            let pending_market = PENDING_MARKET.load(deps.storage)?;

            let mut manager_markets = MANAGED_MARKETS
                .may_load(deps.storage, pending_market.manager.clone())?
                .unwrap_or_default();

            //Add new market to manager's state
            manager_markets.push(MarketItem {
                name: pending_market.name.clone(),
                socials: pending_market.socials.clone(),
                address: valid_address.to_string(),
            });
            MANAGED_MARKETS.save(
                deps.storage,
                pending_market.manager.clone(),
                &manager_markets,
            )?;
            //Remove pending market
            PENDING_MARKET.remove(deps.storage);
            //Add attributes
            let mut attrs = vec![
                attr("method", "handle_instantiate_reply"),
                attr("manager", pending_market.manager),
                attr("market_name", pending_market.name),
                attr("market_address", valid_address.to_string()),
            ];
            attrs.push(attr("managed_markets", format!("{:?}", manager_markets)));
                       
            Ok(Response::new()
                .add_attributes(attrs)

            )
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}



#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {

    //Remove market for existing manager
    MANAGED_MARKETS.remove(deps.storage, "osmo1hfv5gzmpjpgc2ml0qf87j9lrwu9dayq24m33r0".to_string());
    
    //Return response
    Ok(Response::default())
}
