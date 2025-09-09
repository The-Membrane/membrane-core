use cosmwasm_std::{DepsMut, Env, MessageInfo, Response, Binary};
use cw721_base::{Cw721Contract, ExecuteMsg as Cw721ExecuteMsg, MintMsg};
use cw721::Expiration;
use crate::error::CarError;
use crate::contract::CarCw721;
use membrane::types::CarMetadata;

pub fn execute_transfer_nft(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    recipient: String,
    token_id: String,
) -> Result<Response, CarError> {
    let contract: CarCw721 = Cw721Contract::default();
    contract
        .execute(deps, env, info, Cw721ExecuteMsg::TransferNft { recipient, token_id })
        .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))
        .map_err(CarError::from)
}

pub fn execute_send_nft(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    contract_addr: String,
    token_id: String,
    msg: Binary,
) -> Result<Response, CarError> {
    let contract: CarCw721 = Cw721Contract::default();
    contract
        .execute(deps, env, info, Cw721ExecuteMsg::SendNft { contract: contract_addr, token_id, msg })
        .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))
        .map_err(CarError::from)
}

pub fn execute_approve(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    spender: String,
    token_id: String,
    expires: Option<Expiration>,
) -> Result<Response, CarError> {
    let contract: CarCw721 = Cw721Contract::default();
    contract
        .execute(deps, env, info, Cw721ExecuteMsg::Approve { spender, token_id, expires })
        .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))
        .map_err(CarError::from)
}

pub fn execute_revoke(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    spender: String,
    token_id: String,
) -> Result<Response, CarError> {
    let contract: CarCw721 = Cw721Contract::default();
    contract
        .execute(deps, env, info, Cw721ExecuteMsg::Revoke { spender, token_id })
        .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))
        .map_err(CarError::from)
}   

pub fn execute_approve_all(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    operator: String,
    expires: Option<Expiration>,
) -> Result<Response, CarError> {
    let contract: CarCw721 = Cw721Contract::default();
    contract
        .execute(deps, env, info, Cw721ExecuteMsg::ApproveAll { operator, expires })
        .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))
        .map_err(CarError::from)
}

pub fn execute_revoke_all(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    operator: String,
) -> Result<Response, CarError> {
    let contract: CarCw721 = Cw721Contract::default();
    contract
        .execute(deps, env, info, Cw721ExecuteMsg::RevokeAll { operator })
        .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))
        .map_err(CarError::from)
}

pub fn execute_mint(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    mint: MintMsg<Option<CarMetadata>>,
) -> Result<Response, CarError> {
    let contract: CarCw721 = Cw721Contract::default();
    contract
        .execute(deps, env, info, Cw721ExecuteMsg::Mint(mint))
        .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))
        .map_err(CarError::from)
}

pub fn execute_burn(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    token_id: String,
) -> Result<Response, CarError> {
    let contract: CarCw721 = Cw721Contract::default();
    contract
        .execute(deps, env, info, Cw721ExecuteMsg::Burn { token_id })
        .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))
        .map_err(CarError::from)
}

pub fn execute_extension(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: cosmwasm_std::Empty,
) -> Result<Response, CarError> {
    let contract: CarCw721 = Cw721Contract::default();
    contract
        .execute(deps, env, info, Cw721ExecuteMsg::Extension { msg })
        .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))
        .map_err(CarError::from)
}   