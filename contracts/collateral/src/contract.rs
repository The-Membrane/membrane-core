use cosmwasm_std::{
    entry_point, to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult,
};

use membrane::collateral::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg, Config, BasketCollateral};

use crate::error::ContractError;
use crate::state::{CONFIG, BASKET, FROZEN};

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
        oracle_contract: msg.oracle_contract
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        chain_proxy: msg.chain_proxy
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        ltv_disco: deps.api.addr_validate(&msg.ltv_disco)?,
        debt_contract: msg.debt_contract
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        liquidation_contract: msg.liquidation_contract
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        collateral_twap_timeframe: msg.collateral_twap_timeframe,
        oracle_time_limit: msg.oracle_time_limit,
        ltv_upward_kp: msg.ltv_upward_kp.unwrap_or(cosmwasm_std::Decimal::percent(5)),
        ltv_downward_period: msg.ltv_downward_period.unwrap_or(604_800), // 7 days
        ltv_downward_decay_rate: msg.ltv_downward_decay_rate.unwrap_or(cosmwasm_std::Decimal::percent(40)),
        ltv_borrow_distance: msg.ltv_borrow_distance.unwrap_or(cosmwasm_std::Decimal::percent(10)),
    };
    CONFIG.save(deps.storage, &config)?;

    let basket = BasketCollateral {
        basket_id: msg.create_basket.basket_id,
        current_position_id: cosmwasm_std::Uint128::zero(),
        collateral_types: msg.create_basket.collateral_types,
        collateral_supply_caps: msg.create_basket.collateral_supply_caps,
        multi_asset_supply_caps: msg.create_basket.multi_asset_supply_caps,
    };
    BASKET.save(deps.storage, &basket)?;

    FROZEN.save(deps.storage, &false)?;

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
        ExecuteMsg::Deposit { position_id, position_owner, affiliate_address } => {
            crate::positions::deposit(deps, env, info, position_id, position_owner, affiliate_address)
        },
        ExecuteMsg::Withdraw { position_id, assets, send_to } => {
            crate::positions::withdraw(deps, env, info, position_id, assets, send_to)
        },
        ExecuteMsg::EditBasket(edit) => execute_edit_basket(deps, info, edit),
        ExecuteMsg::EditcAsset { asset, max_borrow_LTV, max_LTV, max_threshold_to_delay } => {
            execute_edit_casset(deps, info, asset, max_borrow_LTV, max_LTV, max_threshold_to_delay)
        },
        ExecuteMsg::UpdateBasketLTVs {} => crate::ltv_updater::update_basket_ltvs(deps, env),
        ExecuteMsg::CollateralRateAssurance { collateral_denoms } => {
            execute_collateral_rate_assurance(deps, env, info, collateral_denoms)
        },
        ExecuteMsg::SetFrozen { frozen } => execute_set_frozen(deps, info, frozen),
        ExecuteMsg::RefreshAndAssess { position_owner, position_id } => {
            crate::query::refresh_and_assess(deps, env, info, position_owner, position_id)
        },
        ExecuteMsg::AuthorizedWithdraw { position_id, position_owner, assets, send_to } => {
            crate::positions::authorized_withdraw(deps, env, info, position_id, position_owner, assets, send_to)
        },
        ExecuteMsg::AuthorizedSellCollateral { position_id, position_owner, max_spread, send_proceeds_to, sell_amounts } => {
            crate::positions::authorized_sell_collateral(deps, env, info, position_id, position_owner, max_spread, send_proceeds_to, sell_amounts)
        },
        ExecuteMsg::ActivateSupplyCaps { position_id, position_owner } => {
            crate::risk_engine::activate_supply_caps(deps, env, info, position_id, position_owner)
        },
        ExecuteMsg::DeactivateSupplyCaps { position_id, position_owner } => {
            crate::risk_engine::deactivate_supply_caps(deps, env, info, position_id, position_owner)
        },
        ExecuteMsg::LiquidationClaimUpdate { position_id, position_owner, collateral_reductions } => {
            crate::positions::liquidation_claim_update(deps, env, info, position_id, position_owner, collateral_reductions)
        },
        ExecuteMsg::AuthorizedTransfer { position_id, position_owner, assets, recipient } => {
            crate::positions::authorized_transfer(deps, env, info, position_id, position_owner, assets, recipient)
        },
        ExecuteMsg::Callback(callback_msg) => {
            // Only callable by self
            if info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }
            match callback_msg {
                membrane::collateral::CallbackMsg::WithdrawCallback { position_id, position_owner } => {
                    crate::positions::withdraw_callback(deps, env, position_id, position_owner)
                },
            }
        },
    }
}

fn execute_update_config(
    deps: DepsMut,
    info: MessageInfo,
    update: membrane::collateral::UpdateConfig,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    if let Some(owner) = update.owner {
        config.owner = deps.api.addr_validate(&owner)?;
    }
    if let Some(oracle_contract) = update.oracle_contract {
        config.oracle_contract = Some(deps.api.addr_validate(&oracle_contract)?);
    }
    if let Some(chain_proxy) = update.chain_proxy {
        config.chain_proxy = Some(deps.api.addr_validate(&chain_proxy)?);
    }
    if let Some(ltv_disco) = update.ltv_disco {
        config.ltv_disco = deps.api.addr_validate(&ltv_disco)?;
    }
    if let Some(debt_contract) = update.debt_contract {
        config.debt_contract = Some(deps.api.addr_validate(&debt_contract)?);
    }
    if let Some(liquidation_contract) = update.liquidation_contract {
        config.liquidation_contract = Some(deps.api.addr_validate(&liquidation_contract)?);
    }
    if let Some(collateral_twap_timeframe) = update.collateral_twap_timeframe {
        config.collateral_twap_timeframe = collateral_twap_timeframe;
    }
    if let Some(oracle_time_limit) = update.oracle_time_limit {
        config.oracle_time_limit = oracle_time_limit;
    }
    if let Some(ltv_upward_kp) = update.ltv_upward_kp {
        config.ltv_upward_kp = ltv_upward_kp;
    }
    if let Some(ltv_downward_period) = update.ltv_downward_period {
        config.ltv_downward_period = ltv_downward_period;
    }
    if let Some(ltv_downward_decay_rate) = update.ltv_downward_decay_rate {
        config.ltv_downward_decay_rate = ltv_downward_decay_rate;
    }
    if let Some(ltv_borrow_distance) = update.ltv_borrow_distance {
        config.ltv_borrow_distance = ltv_borrow_distance;
    }
    CONFIG.save(deps.storage, &config)?;
    Ok(Response::new().add_attribute("method", "update_config"))
}

fn execute_edit_basket(
    deps: DepsMut,
    info: MessageInfo,
    edit: membrane::collateral::EditBasket,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let mut basket = BASKET.load(deps.storage)?;

    if let Some(new_casset) = edit.added_cAsset {
        basket.collateral_types.push(new_casset);
    }
    if let Some(supply_caps) = edit.collateral_supply_caps {
        for new_cap in supply_caps {
            if let Some(index) = basket.collateral_supply_caps
                .iter()
                .position(|cap| cap.asset_info.equal(&new_cap.asset_info))
            {
                basket.collateral_supply_caps[index].supply_cap_ratio = new_cap.supply_cap_ratio;
                basket.collateral_supply_caps[index].stability_pool_ratio_for_debt_cap = new_cap.stability_pool_ratio_for_debt_cap;
            }
        }
    }
    if let Some(multi_caps) = edit.multi_asset_supply_caps {
        for new_cap in multi_caps {
            if let Some(index) = basket.multi_asset_supply_caps
                .iter()
                .position(|cap| membrane::types::equal(&cap.assets, &new_cap.assets))
            {
                basket.multi_asset_supply_caps[index].supply_cap_ratio = new_cap.supply_cap_ratio;
            } else {
                basket.multi_asset_supply_caps.push(new_cap);
            }
        }
    }

    BASKET.save(deps.storage, &basket)?;
    Ok(Response::new().add_attribute("method", "edit_basket"))
}

fn execute_edit_casset(
    deps: DepsMut,
    info: MessageInfo,
    asset: membrane::types::AssetInfo,
    max_borrow_ltv: Option<cosmwasm_std::Decimal>,
    max_ltv: Option<cosmwasm_std::Decimal>,
    max_threshold_to_delay: Option<cosmwasm_std::Decimal>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let mut basket = BASKET.load(deps.storage)?;

    for casset in basket.collateral_types.iter_mut() {
        if casset.asset.info.equal(&asset) {
            if let Some(max_borrow) = max_borrow_ltv {
                casset.max_borrow_LTV = max_borrow;
            }
            if let Some(max) = max_ltv {
                casset.max_LTV = max;
                // Update manual baseline LTV (drift target when Disco returns 0)
                crate::state::MANUAL_MAX_LTVS.save(deps.storage, casset.asset.info.to_string(), &max)?;
            }
            if let Some(threshold) = max_threshold_to_delay {
                casset.max_threshold_to_delay = threshold;
            }
            break;
        }
    }

    BASKET.save(deps.storage, &basket)?;
    Ok(Response::new().add_attribute("method", "edit_casset"))
}

/// Collateral rate assurance verification callback.
/// Checks that collateral-per-state-unit ratios haven't changed beyond tolerance
/// between the pre-operation snapshot and the current post-operation state.
///
/// CDP parity: maps to cdp/contract.rs::collateral_rate_assurance()
fn execute_collateral_rate_assurance(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    collateral_denoms: Option<Vec<String>>,
) -> Result<Response, ContractError> {
    // Only callable by self (callback pattern)
    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized {});
    }

    let basket = BASKET.load(deps.storage)?;
    let mut attrs = vec![cosmwasm_std::Attribute::new("method", "collateral_rate_assurance")];
    let mut errors = Vec::new();

    // Determine which denoms to check
    let denoms_to_check = if let Some(denoms) = collateral_denoms {
        denoms
    } else {
        basket.collateral_types.iter()
            .map(|c| c.asset.info.to_string())
            .collect()
    };

    for denom in denoms_to_check {
        if let Ok(assurance) = crate::state::COLLATERAL_RATE_ASSURANCE.load(deps.storage, denom.clone()) {
            // Get current contract balance for this denom
            let current_balance = deps.querier.query_balance(
                env.contract.address.to_string(),
                denom.clone(),
            )?.amount;

            // Get current supply from basket supply caps
            let current_supply = basket.collateral_supply_caps.iter()
                .find(|cap| cap.asset_info.to_string() == denom)
                .map(|cap| cap.current_supply)
                .unwrap_or(cosmwasm_std::Uint128::zero());

            // Calculate current collateral-per-state-unit rate
            let current_rate = if current_supply.is_zero() {
                cosmwasm_std::Uint128::zero()
            } else {
                current_balance.multiply_ratio(
                    cosmwasm_std::Uint128::new(1_000_000),
                    current_supply,
                )
            };

            // Compare pre vs post — tolerance of 1 unit (rounding)
            let difference = if current_rate > assurance.pre_collateral_per_one {
                current_rate - assurance.pre_collateral_per_one
            } else {
                assurance.pre_collateral_per_one - current_rate
            };

            if difference > cosmwasm_std::Uint128::one() {
                errors.push(format!(
                    "Collateral rate assurance failed for {}: pre: {} --- post: {}",
                    denom, assurance.pre_collateral_per_one, current_rate
                ));
            }

            attrs.push(cosmwasm_std::Attribute::new(
                format!("{}_pre_rate", denom),
                assurance.pre_collateral_per_one.to_string(),
            ));
            attrs.push(cosmwasm_std::Attribute::new(
                format!("{}_post_rate", denom),
                current_rate.to_string(),
            ));

            // Clean up the snapshot
            crate::state::COLLATERAL_RATE_ASSURANCE.remove(deps.storage, denom);
        }
    }

    if !errors.is_empty() {
        return Err(ContractError::CustomError { val: errors.join("; ") });
    }

    Ok(Response::new().add_attributes(attrs))
}

fn execute_set_frozen(
    deps: DepsMut,
    info: MessageInfo,
    frozen: bool,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    FROZEN.save(deps.storage, &frozen)?;

    // TODO: Send SubMsg to Debt contract to sync frozen state

    Ok(Response::new()
        .add_attribute("method", "set_frozen")
        .add_attribute("frozen", frozen.to_string()))
}

// ============================================================================
// Query
// ============================================================================

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(
    deps: Deps,
    _env: Env,
    msg: QueryMsg,
) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::GetCollateralPositions { start_after, limit, user_info, user } => {
            crate::query::query_collateral_positions(deps, start_after, limit, user_info, user)
        },
        QueryMsg::GetBasket {} => to_json_binary(&BASKET.load(deps.storage)?),
        QueryMsg::GetHistoricalOraclePrices { asset } => {
            crate::query::query_historical_oracle_prices(deps, asset)
        },
        QueryMsg::CheckVolatilityWindow { assets } => {
            crate::query::query_volatility_window(deps, assets)
        },
        QueryMsg::SimulateLiquidation { collateral_to_sell, target_denom } => {
            crate::query::query_simulate_liquidation(deps, collateral_to_sell, target_denom)
        },
        QueryMsg::GetCircuitBreakerStatus { assets } => {
            crate::query::query_circuit_breaker_status(deps, assets)
        },
        QueryMsg::GetFrozen {} => to_json_binary(&FROZEN.load(deps.storage)?),
        QueryMsg::GetPositionValue { position_owner, position_id } => {
            crate::query::query_position_value(deps, position_owner, position_id)
        },
        QueryMsg::GetHistoricalLTVs { asset_denom, start_time, end_time, limit } => {
            crate::query::query_historical_ltv(deps, asset_denom, start_time, end_time, limit)
        },
        QueryMsg::GetLTVShiftInfo { asset_denom } => {
            crate::query::query_ltv_shift_info(deps, _env, asset_denom)
        },
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
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    Ok(Response::default())
}
