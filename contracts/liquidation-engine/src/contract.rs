use cosmwasm_std::{
    entry_point, to_json_binary, Binary, Decimal, Deps, DepsMut, Env, MessageInfo, Response,
    StdResult,
};

use membrane::liquidation_engine::{
    CallbackMsg, Config, ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg, UpdateConfig,
};

use crate::error::ContractError;
use crate::state::CONFIG;

// ============================================================================
// Instantiate
// ============================================================================

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let owner = if let Some(owner) = msg.owner {
        deps.api.addr_validate(&owner)?
    } else {
        info.sender.clone()
    };

    let config = Config {
        owner,
        collateral_contract: deps.api.addr_validate(&msg.collateral_contract)?,
        debt_contract: deps.api.addr_validate(&msg.debt_contract)?,
        liq_queue: msg
            .liq_queue
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        chain_proxy: msg
            .chain_proxy
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        debt_auction: msg
            .debt_auction
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        staking_contract: msg
            .staking_contract
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        liq_fee: msg.liq_fee,
        credit_denom: msg.credit_denom,
        liquidation_delay: msg.liquidation_delay.unwrap_or(28800),
    };
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("owner", config.owner.to_string()))
}

// ============================================================================
// Execute
// ============================================================================

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::UpdateConfig(update) => execute_update_config(deps, info, update),
        ExecuteMsg::Liquidate {
            position_id,
            position_owner,
        } => crate::liquidations::liquidate(deps, env, info, position_id, position_owner),
        ExecuteMsg::Callback(callback_msg) => {
            // Only callable by self
            if info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }
            match callback_msg {
                CallbackMsg::BadDebtCheck {
                    position_id,
                    position_owner,
                } => crate::liquidations::bad_debt_check(deps, env, position_id, position_owner),
            }
        }
    }
}

fn execute_update_config(
    deps: DepsMut,
    info: MessageInfo,
    update: UpdateConfig,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    if let Some(owner) = update.owner {
        config.owner = deps.api.addr_validate(&owner)?;
    }
    if let Some(collateral_contract) = update.collateral_contract {
        config.collateral_contract = deps.api.addr_validate(&collateral_contract)?;
    }
    if let Some(debt_contract) = update.debt_contract {
        config.debt_contract = deps.api.addr_validate(&debt_contract)?;
    }
    if let Some(liq_queue) = update.liq_queue {
        config.liq_queue = Some(deps.api.addr_validate(&liq_queue)?);
    }
    if let Some(chain_proxy) = update.chain_proxy {
        config.chain_proxy = Some(deps.api.addr_validate(&chain_proxy)?);
    }
    if let Some(debt_auction) = update.debt_auction {
        config.debt_auction = Some(deps.api.addr_validate(&debt_auction)?);
    }
    if let Some(staking_contract) = update.staking_contract {
        config.staking_contract = Some(deps.api.addr_validate(&staking_contract)?);
    }
    if let Some(liq_fee) = update.liq_fee {
        config.liq_fee = liq_fee;
    }
    if let Some(credit_denom) = update.credit_denom {
        config.credit_denom = Some(credit_denom);
    }
    if let Some(liquidation_delay) = update.liquidation_delay {
        config.liquidation_delay = liquidation_delay;
    }
    CONFIG.save(deps.storage, &config)?;
    Ok(Response::new().add_attribute("method", "update_config"))
}

// ============================================================================
// Query
// ============================================================================

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::SimulateLiquidation {
            position_id,
            position_owner,
        } => crate::query::query_simulate_liquidation(deps, position_id, position_owner),
        QueryMsg::GetLiquidationStats { start_after, limit } => {
            crate::query::query_liquidation_stats(deps, start_after, limit)
        }
        QueryMsg::GetLiquidationTimer {
            position_id,
            position_owner,
        } => crate::query::query_liquidation_timer(deps, position_id, position_owner),
        QueryMsg::GetLastLiquidationTimestampForAsset { asset } => {
            crate::query::query_last_liquidation_timestamp_for_asset(deps, asset)
        }
    }
}

// ============================================================================
// Reply
// ============================================================================

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(
    deps: DepsMut,
    env: Env,
    msg: cosmwasm_std::Reply,
) -> Result<Response, ContractError> {
    crate::reply::handle_reply(deps, env, msg)
}

// ============================================================================
// Migrate
// ============================================================================

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    Ok(Response::default())
}
