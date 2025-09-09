// base_nft_queries.rs

use cosmwasm_std::{Deps, Env, StdResult, Binary, to_json_binary};
use cw721_base::{Cw721Contract, QueryMsg as Cw721QueryMsg};
use crate::contract::CarCw721;

// Re-export all the base query functions for easy access
pub fn query_owner_of(
    deps: Deps,
    env: Env,
    token_id: String,
    include_expired: Option<bool>,
) -> StdResult<Binary> {
    let contract: CarCw721 = Cw721Contract::default();
    let query_msg = Cw721QueryMsg::OwnerOf {
        token_id,
        include_expired,
    };
    contract.query(deps, env, query_msg)
}

pub fn query_approval(
    deps: Deps,
    env: Env,
    token_id: String,
    spender: String,
    include_expired: Option<bool>,
) -> StdResult<Binary> {
    let contract: CarCw721 = Cw721Contract::default();
    let query_msg = Cw721QueryMsg::Approval {
        token_id,
        spender,
        include_expired,
    };
    contract.query(deps, env, query_msg)
}

pub fn query_approvals(
    deps: Deps,
    env: Env,
    token_id: String,
    include_expired: Option<bool>,
) -> StdResult<Binary> {
    let contract: CarCw721 = Cw721Contract::default();
    let query_msg = Cw721QueryMsg::Approvals {
        token_id,
        include_expired,
    };
    contract.query(deps, env, query_msg)
}

pub fn query_all_operators(
    deps: Deps,
    env: Env,
    owner: String,
    include_expired: Option<bool>,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<Binary> {
    let contract: CarCw721 = Cw721Contract::default();
    let query_msg = Cw721QueryMsg::AllOperators {
        owner,
        include_expired,
        start_after,
        limit,
    };
    contract.query(deps, env, query_msg)
}

pub fn query_num_tokens(deps: Deps, env: Env) -> StdResult<Binary> {
    let contract: CarCw721 = Cw721Contract::default();
    let query_msg = Cw721QueryMsg::NumTokens {};
    contract.query(deps, env, query_msg)
}

pub fn query_contract_info(deps: Deps, env: Env) -> StdResult<Binary> {
    let contract: CarCw721 = Cw721Contract::default();
    let query_msg = Cw721QueryMsg::ContractInfo {};
    contract.query(deps, env, query_msg)
}

pub fn query_nft_info(
    deps: Deps,
    env: Env,
    token_id: String,
) -> StdResult<Binary> {
    let contract: CarCw721 = Cw721Contract::default();
    let query_msg = Cw721QueryMsg::NftInfo { token_id };
    contract.query(deps, env, query_msg)
}

pub fn query_all_nft_info(
    deps: Deps,
    env: Env,
    token_id: String,
    include_expired: Option<bool>,
) -> StdResult<Binary> {
    let contract: CarCw721 = Cw721Contract::default();
    let query_msg = Cw721QueryMsg::AllNftInfo {
        token_id,
        include_expired,
    };
    contract.query(deps, env, query_msg)
}

pub fn query_tokens(
    deps: Deps,
    env: Env,
    owner: String,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<Binary> {
    let contract: CarCw721 = Cw721Contract::default();
    let query_msg = Cw721QueryMsg::Tokens {
        owner,
        start_after,
        limit,
    };
    contract.query(deps, env, query_msg)
}

pub fn query_all_tokens(
    deps: Deps,
    env: Env,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<Binary> {
    let contract: CarCw721 = Cw721Contract::default();
    let query_msg = Cw721QueryMsg::AllTokens {
        start_after,
        limit,
    };
    contract.query(deps, env, query_msg)
}

pub fn query_minter(deps: Deps, env: Env) -> StdResult<Binary> {
    let contract: CarCw721 = Cw721Contract::default();
    let query_msg = Cw721QueryMsg::Minter {};
    contract.query(deps, env, query_msg)
}
