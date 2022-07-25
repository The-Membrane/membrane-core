//Token factory fork
//https://github.com/osmosis-labs/bindings/blob/main/contracts/tokenfactory

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult, Uint128,
};
use cw2::set_contract_version;

use crate::error::TokenFactoryError;
use membrane::osmosis_proxy::{ExecuteMsg, GetDenomResponse, InstantiateMsg, QueryMsg};
use crate::state::{State, STATE};
use osmo_bindings::{ OsmosisMsg, OsmosisQuerier, OsmosisQuery };

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:tokenfactory-demo";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut<OsmosisQuery>,
    _env: Env,
    info: MessageInfo,
    _msg: InstantiateMsg,
) -> Result<Response, TokenFactoryError> {
    let state = State {
        owner: info.sender.clone(),
    };
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    STATE.save(deps.storage, &state)?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("owner", info.sender))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut<OsmosisQuery>,
    _env: Env,
    _info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response<OsmosisMsg>, TokenFactoryError> {
    match msg {
        ExecuteMsg::CreateDenom { subdenom } => create_denom(subdenom),
        ExecuteMsg::ChangeAdmin {
            denom,
            new_admin_address,
        } => change_admin(deps, denom, new_admin_address),
        ExecuteMsg::MintTokens {
            denom,
            amount,
            mint_to_address,
        } => mint_tokens(deps, denom, amount, mint_to_address),
        ExecuteMsg::BurnTokens {
            denom,
            amount,
            burn_from_address,
        } => burn_tokens(deps, denom, amount, burn_from_address),
    }
}

pub fn create_denom(subdenom: String) -> Result<Response<OsmosisMsg>, TokenFactoryError> {
    if subdenom.eq("") {
        return Err(TokenFactoryError::InvalidSubdenom { subdenom });
    }

    let create_denom_msg = OsmosisMsg::CreateDenom { subdenom };

    let res = Response::new()
        .add_attribute("method", "create_denom")
        .add_message(create_denom_msg);

    Ok(res)
}

pub fn change_admin(
    deps: DepsMut<OsmosisQuery>,
    denom: String,
    new_admin_address: String,
) -> Result<Response<OsmosisMsg>, TokenFactoryError> {
    deps.api.addr_validate(&new_admin_address)?;

    validate_denom(deps, denom.clone())?;

    let change_admin_msg = OsmosisMsg::ChangeAdmin {
        denom,
        new_admin_address,
    };

    let res = Response::new()
        .add_attribute("method", "change_admin")
        .add_message(change_admin_msg);

    Ok(res)
}

pub fn mint_tokens(
    deps: DepsMut<OsmosisQuery>,
    denom: String,
    amount: Uint128,
    mint_to_address: String,
) -> Result<Response<OsmosisMsg>, TokenFactoryError> {
    deps.api.addr_validate(&mint_to_address)?;

    if amount.eq(&Uint128::new(0_u128)) {
        return Result::Err(TokenFactoryError::ZeroAmount {});
    }

    validate_denom(deps, denom.clone())?;

    let mint_tokens_msg = OsmosisMsg::mint_contract_tokens(denom, amount, mint_to_address);

    let res = Response::new()
        .add_attribute("method", "mint_tokens")
        .add_message(mint_tokens_msg);

    Ok(res)
}

pub fn burn_tokens(
    deps: DepsMut<OsmosisQuery>,
    denom: String,
    amount: Uint128,
    burn_from_address: String,
) -> Result<Response<OsmosisMsg>, TokenFactoryError> {
    if !burn_from_address.is_empty() {
        return Result::Err(TokenFactoryError::BurnFromAddressNotSupported {
            address: burn_from_address,
        });
    }

    if amount.eq(&Uint128::new(0_u128)) {
        return Result::Err(TokenFactoryError::ZeroAmount {});
    }

    validate_denom(deps, denom.clone())?;

    let burn_token_msg = OsmosisMsg::burn_contract_tokens(denom, amount, burn_from_address);

    let res = Response::new()
        .add_attribute("method", "burn_tokens")
        .add_message(burn_token_msg);

    Ok(res)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps<OsmosisQuery>, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetDenom {
            creator_address,
            subdenom,
        } => to_binary(&get_denom(deps, creator_address, subdenom)),
        QueryMsg::SpotPrice { asset } => todo!(),
    }
}

fn get_denom(deps: Deps<OsmosisQuery>, creator_addr: String, subdenom: String) -> GetDenomResponse {
    let querier = OsmosisQuerier::new(&deps.querier);
    let response = querier.full_denom(creator_addr, subdenom).unwrap();

    GetDenomResponse {
        denom: response.denom,
    }
}

fn validate_denom(deps: DepsMut<OsmosisQuery>, denom: String) -> Result<(), TokenFactoryError> {
    let denom_to_split = denom.clone();
    let tokenfactory_denom_parts: Vec<&str> = denom_to_split.split('/').collect();

    if tokenfactory_denom_parts.len() != 3 {
        return Result::Err(TokenFactoryError::InvalidDenom {
            denom,
            message: std::format!(
                "denom must have 3 parts separated by /, had {}",
                tokenfactory_denom_parts.len()
            ),
        });
    }

    let prefix = tokenfactory_denom_parts[0];
    let creator_address = tokenfactory_denom_parts[1];
    let subdenom = tokenfactory_denom_parts[2];

    if !prefix.eq_ignore_ascii_case("factory") {
        return Result::Err(TokenFactoryError::InvalidDenom {
            denom,
            message: std::format!("prefix must be 'factory', was {}", prefix),
        });
    }

    // Validate denom by attempting to query for full denom
    let response = OsmosisQuerier::new(&deps.querier)
        .full_denom(String::from(creator_address), String::from(subdenom));
    if response.is_err() {
        return Result::Err(TokenFactoryError::InvalidDenom {
            denom,
            message: response.err().unwrap().to_string(),
        });
    }

    Result::Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{
        mock_env, mock_info, MockApi, MockQuerier, MockStorage, MOCK_CONTRACT_ADDR,
    };
    use cosmwasm_std::{
        coins, from_binary, Attribute, ContractResult, CosmosMsg, OwnedDeps, Querier, StdError,
        SystemError, SystemResult,
    };
}