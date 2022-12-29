use std::env;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, coin, to_binary, Addr, Api, BankMsg, Binary, CosmosMsg, Decimal, Deps,
    DepsMut, Env, MessageInfo, Response, StdError, StdResult, Storage, Uint128, WasmMsg, QueryRequest, WasmQuery, QuerierWrapper,
};
use cw2::set_contract_version;
use cw20::Cw20ExecuteMsg;

use membrane::apollo_router::{Cw20HookMsg as RouterCw20HookMsg, ExecuteMsg as RouterExecuteMsg, SwapToAssetsInput};
use membrane::helpers::{assert_sent_native_token_balance, validate_position_owner, asset_to_coin, withdrawal_msg};
use membrane::osmosis_proxy::ExecuteMsg as OsmoExecuteMsg;
use membrane::governance::{QueryMsg as Gov_QueryMsg, ProposalListResponse, ProposalStatus};
use membrane::discount_vault::{Config, ExecuteMsg, InstantiateMsg, QueryMsg};
use membrane::vesting::{QueryMsg as Vesting_QueryMsg, RecipientsResponse};
use membrane::types::{Asset, AssetInfo, FeeEvent, LiqAsset, StakeDeposit};
use membrane::math::decimal_division;

use crate::error::ContractError;
use crate::state::{CONFIG, USERS};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:discount_vault";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Constants
const SECONDS_PER_YEAR: u64 = 31_536_000u64;
const SECONDS_PER_DAY: u64 = 86_400u64;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let mut config: Config;

    if msg.owner.is_some() {
        config = Config {
            owner: deps.api.addr_validate(&msg.owner.unwrap())?,
            positions_contract: deps.api.addr_validate(&msg.positions_contract)?,
            accepted_lps: msg.accepted_lps,
        };
    } else {
        config = Config {
            owner: info.sender,
            positions_contract: deps.api.addr_validate(&msg.positions_contract)?,
            accepted_lps: msg.accepted_lps,
        };
    }

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
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
    }
}

