use std::str::FromStr;

use cosmwasm_std::{
    attr, entry_point, to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Order,
    QueryRequest, Response, StdResult, Uint128, WasmQuery,
};
use cw2::set_contract_version;

use membrane::liquidity_check::{Config, ExecuteMsg, InstantiateMsg, QueryMsg};
use membrane::osmosis_proxy::QueryMsg as OsmoQueryMsg;
use membrane::types::{AssetInfo, LiquidityInfo, PoolStateResponse};

use cw_storage_plus::Bound;

use crate::error::ContractError;
use crate::state::{ASSETS, CONFIG};

// Contract name and version used for migration.
const CONTRACT_NAME: &str = "liquidity_check";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Constants
const MAX_LIMIT: u64 = 31u64;

pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let config: Config;
    if let Some(owner) = msg.owner {
        config = Config {
            owner: deps.api.addr_validate(&owner)?,
            osmosis_proxy: deps.api.addr_validate(&msg.osmosis_proxy)?,
            positions_contract: deps.api.addr_validate(&msg.positions_contract)?,
        };
    } else {
        config = Config {
            owner: info.sender,
            osmosis_proxy: deps.api.addr_validate(&msg.osmosis_proxy)?,
            positions_contract: deps.api.addr_validate(&msg.positions_contract)?,
        };
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::AddAsset { asset } => add_asset(deps, info, asset),
        ExecuteMsg::EditAsset { asset } => edit_asset(deps, info, asset),
        ExecuteMsg::RemoveAsset { asset } => remove_asset(deps, info, asset),
        ExecuteMsg::UpdateConfig {
            owner,
            osmosis_proxy,
            positions_contract,
        } => update_config(deps, info, owner, osmosis_proxy, positions_contract),
    }
}

fn add_asset(
    deps: DepsMut,
    info: MessageInfo,
    asset: LiquidityInfo,
) -> Result<Response, ContractError> {

    let config = CONFIG.load(deps.storage)?;

    //Assert Authority
    if info.sender != config.owner && info.sender != config.positions_contract {
        return Err(ContractError::Unauthorized {});
    }

    let mut attrs = vec![attr("method", "add_asset")];

    //No duplicates
    if let Err(_err) = ASSETS.load(deps.storage, asset.asset.to_string()) {
        ASSETS.save(deps.storage, asset.asset.to_string(), &asset)?;

        attrs.push(attr("added_asset", asset.asset.to_string()));
        attrs.push(attr("pool_ids", format!("{:?}", asset.pool_ids)));
    } else {
        return Err(ContractError::CustomError {
            val: String::from("Duplicate assets"),
        });
    }

    Ok(Response::new().add_attributes(attrs))
}

fn edit_asset(
    deps: DepsMut,
    info: MessageInfo,
    asset: LiquidityInfo,
) -> Result<Response, ContractError> {

    let config = CONFIG.load(deps.storage)?;

    //Assert Authority
    if info.sender != config.owner && info.sender != config.positions_contract {
        return Err(ContractError::Unauthorized {});
    }

    let mut attrs = vec![attr("method", "edit_asset")];

    //Add onto object
    ASSETS.update(
        deps.storage,
        asset.asset.to_string(),
        |stored_asset| -> Result<LiquidityInfo, ContractError> {
            //Can easily add new fields if multiple DEXs are desired
            if let Some(mut stored_asset) = stored_asset {
                stored_asset.pool_ids.extend(asset.clone().pool_ids);

                attrs.push(attr(
                    "added_pool_ids",
                    format!("{:?}", asset.clone().pool_ids),
                ));

                Ok(stored_asset)
            } else {
                Ok(asset)
            }
        },
    )?;

    Ok(Response::new().add_attributes(attrs))
}

fn remove_asset(
    deps: DepsMut,
    info: MessageInfo,
    asset: AssetInfo,
) -> Result<Response, ContractError> {

    let config = CONFIG.load(deps.storage)?;

    //Assert Authority
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let attrs = vec![
        attr("method", "remove_asset"),
        attr("removed_asset", asset.to_string()),
    ];

    //Remove asset info
    ASSETS.remove(deps.storage, asset.to_string());

    Ok(Response::new().add_attributes(attrs))
}

fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    osmosis_proxy: Option<String>,
    positions_contract: Option<String>,
) -> Result<Response, ContractError> {

    let mut config = CONFIG.load(deps.storage)?;

    //Assert authority
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    //Save optionals
    if let Some(addr) = owner {
        config.owner = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = osmosis_proxy {
        config.osmosis_proxy = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = positions_contract {
        config.positions_contract = deps.api.addr_validate(&addr)?;
    }

    //Save Config
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::Assets {
            asset_info,
            limit,
            start_after,
        } => to_binary(&get_assets(deps, asset_info, limit, start_after)?),
        QueryMsg::Liquidity { asset } => to_binary(&get_liquidity(deps, asset)?),
    }
}

fn get_assets(
    deps: Deps,
    asset: Option<AssetInfo>,
    limit: Option<u64>,
    start_after: Option<AssetInfo>,
) -> StdResult<Vec<LiquidityInfo>> {

    let limit = limit.unwrap_or(MAX_LIMIT) as usize;

    let start = start_after.map(|start| Bound::exclusive(start.to_string()));

    if let Some(asset) = asset {
        Ok(vec![ASSETS.load(deps.storage, asset.to_string())?])
    } else {
        ASSETS
            .range(deps.storage, start, None, Order::Ascending)
            .take(limit)
            .map(|item| {
                let (_asset, info) = item.unwrap();

                Ok(info)
            })
            .collect::<StdResult<Vec<LiquidityInfo>>>()
    }
}

//This only works for native tokens on Osmosis, which is fine for now
fn get_liquidity(deps: Deps, asset: AssetInfo) -> StdResult<Uint128> {
    
    let config = CONFIG.load(deps.storage)?;

    let denom = asset.to_string();

    let asset = ASSETS.load(deps.storage, denom.clone())?;

    let mut total_pooled = Uint128::zero();

    for id in asset.pool_ids {
        let res: PoolStateResponse = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: config.clone().osmosis_proxy.to_string(),
            msg: to_binary(&OsmoQueryMsg::PoolState { id })?,
        }))?;

        let pooled_amount = if let Some(pooled_asset) = res
            .assets
            .into_iter()
            .find(|coin| coin.denom == denom){
                Uint128::from_str(&pooled_asset.amount).unwrap()
            } else {
                return Err(cosmwasm_std::StdError::GenericErr { msg: format!("This LP doesn't contain {}", denom) })
            };

        total_pooled += pooled_amount;
    }

    Ok(total_pooled)
}
