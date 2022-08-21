//Token factory fork
//https://github.com/osmosis-labs/bindings/blob/main/contracts/tokenfactory

use std::error::Error;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult, Uint128, QueryRequest, CosmosMsg, StdError, Coin,
};
use cw2::set_contract_version;

use crate::error::TokenFactoryError;
use membrane::osmosis_proxy::{ExecuteMsg, GetDenomResponse, InstantiateMsg, QueryMsg};
use crate::state::{State, STATE};
use osmo_bindings::{ OsmosisMsg, OsmosisQuerier, OsmosisQuery, PoolStateResponse, ArithmeticTwapToNowResponse };

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:osmosis-proxy";
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
        ExecuteMsg::CreateDenom { subdenom, basket_id } => create_denom(subdenom, basket_id),
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

// fn exit_pool(
//     sender: String,
//     pool_id: u64,
//     share_in_amount: Uint128,
//     token_out_mins: Vec<Coin>,
// ) -> Result<Response<>, TokenFactoryError>{
//     let mut token_mins: Vec<osmosis_std::types::cosmos::base::v1beta1::Coin> = vec![];
//     if token_out_mins != vec![]{
//         for token in token_out_mins {
//             token_mins.push( osmosis_std::types::cosmos::base::v1beta1::Coin {
//                 denom: token.denom,
//                 amount: token.amount.to_string(),
//             } );
//         }
//     }    

//     let msg: CosmosMsg = MsgExitPool {
//         sender,
//         pool_id,
//         share_in_amount: share_in_amount.to_string(),
//         token_out_mins: token_mins,
//     }.into();

//     Ok( Response::new()
//             .add_message(msg)
//             .add_attribute("method", "exit_pool") )

// }

pub fn create_denom(subdenom: String, basket_id: String) -> Result<Response<OsmosisMsg>, TokenFactoryError> {
    if subdenom.eq("") {
        return Err(TokenFactoryError::InvalidSubdenom { subdenom });
    }

    let create_denom_msg = OsmosisMsg::CreateDenom { subdenom: subdenom.clone() };

    let res = Response::new()
        .add_attribute("method", "create_denom")
        .add_attribute("sub_denom", subdenom)
        .add_attribute("basket_id", basket_id)
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
        QueryMsg::PoolState { id } => {
            to_binary(&get_pool_state(deps, id)?)
        },
        QueryMsg::ArithmeticTwapToNow { 
            id, 
            quote_asset_denom, 
            base_asset_denom, 
            start_time 
        } => {
            to_binary(&get_arithmetic_twap_to_now(deps, id, quote_asset_denom, base_asset_denom, start_time)? ) 
        },
    }
}

fn get_arithmetic_twap_to_now(
    deps: Deps<OsmosisQuery>, 
    id: u64,
    quote_asset_denom: String,
    base_asset_denom: String,
    start_time: i64,
) -> StdResult<ArithmeticTwapToNowResponse> {

    let msg = OsmosisQuery::arithmetic_twap_to_now( id, quote_asset_denom, base_asset_denom, start_time );
    let request: QueryRequest<OsmosisQuery> = OsmosisQuery::into(msg);

    let response: ArithmeticTwapToNowResponse = deps.querier.query( &request )?;

    Ok( response )
}

fn get_pool_state(deps: Deps<OsmosisQuery>, id: u64) -> StdResult<PoolStateResponse> {
    
    let msg = OsmosisQuery::PoolState { id };
    let request: QueryRequest<OsmosisQuery> = OsmosisQuery::into(msg);

    let response: PoolStateResponse = deps.querier.query( &request )?;

    Ok( response )
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