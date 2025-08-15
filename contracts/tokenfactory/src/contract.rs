//! Tokenfactory CosmWasm contract implementation

#![allow(unused_imports)]
#![allow(non_snake_case)]

use cosmwasm_std::{
    attr, entry_point, to_binary, Binary, CosmosMsg, Deps, DepsMut, Env, MessageInfo,
    Response, StdResult, SubMsg,
};

use cw2::set_contract_version;

use osmosis_std::types::{
    cosmos::base::v1beta1::Coin as OsmosisCoin,
    osmosis::tokenfactory::v1beta1 as TokenFactory,
};

use membrane::tokenfactory::{Config, ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg};

use crate::error::ContractError;
use crate::state::{CONFIG, DENOMS};

// Contract name/version info for migration
const CONTRACT_NAME: &str = "tokenfactory";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Instantiate contract
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let owner = match msg.owner {
        Some(owner_str) => deps.api.addr_validate(&owner_str)?,
        None => info.sender.clone(),
    };

    let config = Config { owner: owner.clone() };
    CONFIG.save(deps.storage, &config)?;
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::new().add_attributes(vec![
        attr("action", "instantiate"),
        attr("owner", owner.to_string()),
    ]))
}

/// Execute entry
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::UpdateConfig { owner } => update_config(deps, info, owner),
        ExecuteMsg::CreateDenom { subdenom } => create_denom(deps, env, info, subdenom),
        ExecuteMsg::MintTokens { amount, mint_to_address } => {
            mint_tokens(deps, env, info, amount, mint_to_address)
        }
        ExecuteMsg::BurnTokens {} => burn_tokens(deps, env, info),
    }
}

fn assert_owner(storage: &dyn cosmwasm_std::Storage, sender: &cosmwasm_std::Addr) -> Result<(), ContractError> {
    let cfg = CONFIG.load(storage)?;
    if &cfg.owner != sender {
        return Err(ContractError::Unauthorized {});
    }
    Ok(())
}

// Update owner
fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    new_owner: Option<String>,
) -> Result<Response, ContractError> {
    // Validate new owner before mutable borrow of storage
    let validated_owner = if let Some(owner_str) = new_owner {
        Some(deps.api.addr_validate(&owner_str)?)
    } else {
        None
    };

    CONFIG.update(deps.storage, |mut cfg| -> Result<Config, ContractError> {
        if info.sender != cfg.owner {
            return Err(ContractError::Unauthorized {});
        }
        if let Some(owner_addr) = validated_owner {
            cfg.owner = owner_addr;
        }
        Ok(cfg)
    })?;

    Ok(Response::new().add_attribute("action", "update_config"))
}

/// Create denom via Osmosis tokenfactory
fn create_denom(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    subdenom: String,
) -> Result<Response, ContractError> {
    // Only owner can create denoms
    assert_owner(deps.storage, &info.sender)?;

    if subdenom.is_empty() {
        return Err(ContractError::InvalidSubdenom { subdenom });
    }

    // Build TokenFactory message
    let msg = TokenFactory::MsgCreateDenom {
        sender: env.contract.address.to_string(),
        subdenom: subdenom.clone(),
    };

    // Compute resulting denom and store for bookkeeping
    let denom = format!(
        "factory/{}/{}",
        env.contract.address.to_string(),
        subdenom
    );
    DENOMS.save(deps.storage, denom.clone(), &true)?;

    let res = Response::new()
        .add_attribute("action", "create_denom")
        .add_attribute("subdenom", subdenom)
        .add_attribute("denom", denom.clone())
        .add_submessage(SubMsg::new(msg));

    Ok(res)
}

/// Mint tokens to an address
fn mint_tokens(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    amount: Option<OsmosisCoin>,
    mint_to_address: String,
) -> Result<Response, ContractError> {
    assert_owner(deps.storage, &info.sender)?;

    let amount = amount.ok_or(ContractError::ZeroAmount {})?;

    if amount.amount == "0" {
        return Err(ContractError::ZeroAmount {});
    }

    deps.api.addr_validate(&mint_to_address)?;

    // Build mint msg
    let mint_msg: CosmosMsg = TokenFactory::MsgMint {
        sender: env.contract.address.to_string(),
        amount: Some(amount.clone()),
        mint_to_address: mint_to_address.clone(),
    }
    .into();

    Ok(Response::new()
        .add_attribute("action", "mint_tokens")
        .add_attribute("denom", amount.denom)
        .add_attribute("amount", amount.amount)
        .add_attribute("to", mint_to_address)
        .add_message(mint_msg))
}

/// Burn tokens supplied in the transaction funds from the caller
fn burn_tokens(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    // Anyone can burn their own funds
    let coin = info
        .funds
        .get(0)
        .cloned()
        .ok_or(ContractError::ZeroAmount {})?;

    if coin.amount.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    // Build burn msg
    let burn_msg: CosmosMsg = TokenFactory::MsgBurn {
        sender: env.contract.address.to_string(),
        amount: Some(OsmosisCoin {
            denom: coin.denom.clone(),
            amount: coin.amount.to_string(),
        }),
        burn_from_address: info.sender.to_string(),
    }
    .into();

    Ok(Response::new()
        .add_attribute("action", "burn_tokens")
        .add_attribute("denom", coin.denom)
        .add_attribute("amount", coin.amount)
        .add_message(burn_msg))
}

/// Query entry
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::Denoms { limit, start_after } => {
            let limit = limit.unwrap_or(50) as usize;
            let denoms: StdResult<Vec<String>> = DENOMS
                .keys(deps.storage, None, None, cosmwasm_std::Order::Ascending)
                .map(|k| k.map(|d| d))
                .collect();
            let mut denoms = denoms?;
            // Apply start_after if provided
            if let Some(start) = start_after {
                if let Some(pos) = denoms.iter().position(|d| d == &start) {
                    denoms = denoms.into_iter().skip(pos + 1).collect();
                }
            }
            denoms.truncate(limit);
            to_binary(&denoms)
        }
    }
}

/// Migrate contract
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    Ok(Response::new().add_attribute("action", "migrate"))
} 