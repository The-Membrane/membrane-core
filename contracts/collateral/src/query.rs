#![allow(non_snake_case)]
use cosmwasm_std::{
    to_json_binary, Binary, Decimal, Deps, DepsMut, Env, MessageInfo, Response,
    StdError, StdResult, Uint128,
};

use membrane::collateral::{
    CollateralAssessmentResponse, CollateralPositionResponse, CollateralPositionsResponse, Config,
    HistoricalLTVResponse, LTVShiftInfoResponse, PositionValueResponse,
};
use membrane::oracle::{PriceResponse, QueryMsg as OracleQueryMsg};
use membrane::types::{cAsset, AssetInfo, UserInfo};

use crate::circuit_breaker::check_and_update_circuit_breaker;
use crate::error::ContractError;
use crate::state::{
    AssetCircuitBreaker, CollateralVolatility, PriceTimestamp, StoredPrice,
    ASSET_CIRCUIT_BREAKERS, BASKET, CONFIG, HISTORICAL_ORACLE_PRICES,
    LTV_HISTORY, LTV_UPDATE_TRACKERS,
    POSITIONS, STORED_PRICES, VOLATILITY,
};

// ============================================================================
// Execute: RefreshAndAssess
// ============================================================================

/// Refresh oracle prices, update volatility/circuit breaker state, and return
/// a collateral assessment via response attributes.
///
/// Only callable by Debt or Liquidation contract.
///
/// CDP parity: replaces the mutable parts of cdp/query.rs::get_cAsset_ratios()
/// and cdp/query.rs::get_avg_LTV()
pub fn refresh_and_assess(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_owner: String,
    position_id: Uint128,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only Debt or Liquidation contract can call this
    if Some(info.sender.clone()) != config.debt_contract
        && Some(info.sender.clone()) != config.liquidation_contract
    {
        return Err(ContractError::Unauthorized {});
    }

    let owner = deps.api.addr_validate(&position_owner)?;
    let (_index, position) =
        crate::state::get_target_position(deps.storage, owner.clone(), position_id)?;

    let basket = BASKET.load(deps.storage)?;

    // Step 1: Query oracle for fresh prices and update STORED_PRICES + VOLATILITY
    let (prices, ratios, any_frozen, frozen_assets) = get_casset_ratios_and_prices(
        deps.storage,
        deps.querier,
        &env,
        &config,
        &position.collateral_assets,
    )?;

    // Step 2: Compute weighted average LTVs using basket's authoritative collateral_types
    let (avg_max_ltv, avg_max_borrow_ltv, avg_max_threshold_to_delay) =
        compute_avg_ltvs(&position.collateral_assets, &ratios, &basket.collateral_types)?;

    // Build response
    let assessment = CollateralAssessmentResponse {
        collateral_assets: position.collateral_assets.clone(),
        ratios: ratios.clone(),
        prices: prices.clone(),
        avg_max_ltv,
        avg_max_borrow_ltv,
        avg_max_threshold_to_delay,
        any_frozen,
        frozen_assets: frozen_assets.clone(),
    };

    // Return assessment as serialized attribute for reply parsing
    let assessment_json = to_json_binary(&assessment)
        .map_err(|e| ContractError::Std(e))?
        .to_string();

    Ok(Response::new()
        .add_attribute("method", "refresh_and_assess")
        .add_attribute("position_id", position_id.to_string())
        .add_attribute("avg_max_ltv", avg_max_ltv.to_string())
        .add_attribute("avg_max_borrow_ltv", avg_max_borrow_ltv.to_string())
        .add_attribute("any_frozen", any_frozen.to_string())
        .add_attribute("assessment", assessment_json))
}

/// Query oracle for fresh prices, update STORED_PRICES and VOLATILITY state,
/// and check circuit breakers for each asset.
///
/// Returns (prices, value_ratios, any_frozen, frozen_denoms).
///
/// CDP parity: maps to cdp/query.rs::get_cAsset_ratios() (the mutable state write part)
fn get_casset_ratios_and_prices(
    storage: &mut dyn cosmwasm_std::Storage,
    querier: cosmwasm_std::QuerierWrapper,
    env: &Env,
    config: &Config,
    collateral_assets: &[cAsset],
) -> Result<(Vec<PriceResponse>, Vec<Decimal>, bool, Vec<String>), ContractError> {
    let oracle_addr = config
        .oracle_contract
        .clone()
        .ok_or(ContractError::CustomError {
            val: "No oracle contract configured".to_string(),
        })?;

    // Collect asset infos for batch price query
    let asset_infos: Vec<AssetInfo> = collateral_assets
        .iter()
        .map(|ca| ca.asset.info.clone())
        .collect();

    // Query oracle for prices
    let price_resps: Vec<PriceResponse> = querier
        .query(&cosmwasm_std::QueryRequest::Wasm(
            cosmwasm_std::WasmQuery::Smart {
                contract_addr: oracle_addr.to_string(),
                msg: to_json_binary(&OracleQueryMsg::Prices {
                    asset_infos: asset_infos.clone(),
                    twap_timeframe: config.collateral_twap_timeframe,
                    oracle_time_limit: config.oracle_time_limit,
                })?,
            },
        ))
        .map_err(StdError::from)?;

    // Compute values and ratios
    let mut total_value = Decimal::zero();
    let mut values: Vec<Decimal> = Vec::new();
    let mut any_frozen = false;
    let mut frozen_assets: Vec<String> = Vec::new();

    for (i, ca) in collateral_assets.iter().enumerate() {
        let price = price_resps
            .get(i)
            .ok_or(ContractError::CustomError {
                val: "Price response length mismatch".to_string(),
            })?;

        // Save fresh price to STORED_PRICES
        let denom = ca.asset.info.to_string();
        STORED_PRICES.save(
            storage,
            denom.clone(),
            &StoredPrice {
                price: price.clone(),
                last_time_updated: env.block.time.seconds(),
            },
        )?;

        // Update VOLATILITY for this asset
        update_volatility(storage, env, &denom, price.price)?;

        // Update historical oracle prices
        update_historical_prices(storage, env, &denom, price.price)?;

        // Check circuit breaker
        let is_frozen = check_and_update_circuit_breaker(
            storage,
            env,
            denom.clone(),
            price.price,
            Decimal::percent(10),
        )
        .map_err(ContractError::from)?;

        if is_frozen {
            any_frozen = true;
            frozen_assets.push(denom);
        }

        // Calculate asset value = amount * price
        let amount_dec = Decimal::from_ratio(ca.asset.amount, 1u128);
        let value = amount_dec * price.price;
        values.push(value);
        total_value += value;
    }

    // Calculate ratios
    let ratios: Vec<Decimal> = if total_value.is_zero() {
        values.iter().map(|_| Decimal::zero()).collect()
    } else {
        values.iter().map(|v| *v / total_value).collect()
    };

    Ok((price_resps, ratios, any_frozen, frozen_assets))
}

/// Update volatility tracking for an asset.
///
/// CDP parity: maps to the volatility tracking in cdp/query.rs::get_cAsset_ratios()
fn update_volatility(
    storage: &mut dyn cosmwasm_std::Storage,
    env: &Env,
    asset_denom: &str,
    current_price: Decimal,
) -> Result<(), ContractError> {
    let stored = STORED_PRICES.may_load(storage, asset_denom.to_string())?;

    if let Some(prev_stored) = stored {
        let time_elapsed = env.block.time.seconds().saturating_sub(prev_stored.last_time_updated);
        if time_elapsed == 0 {
            return Ok(());
        }

        let prev_price = prev_stored.price.price;
        if prev_price.is_zero() {
            return Ok(());
        }

        // Calculate price change ratio
        let (higher, lower) = if current_price > prev_price {
            (current_price, prev_price)
        } else {
            (prev_price, current_price)
        };
        let diff = higher
            .checked_sub(lower)
            .unwrap_or(Decimal::zero());
        let price_change = diff
            .checked_div(prev_price)
            .unwrap_or(Decimal::zero());

        // Speed = price_change / time_elapsed (for volatility index)
        let speed = Decimal::from_ratio(
            price_change.atomics(),
            Decimal::from_ratio(time_elapsed as u128, 1u128).atomics(),
        );

        let mut vol = VOLATILITY
            .may_load(storage, asset_denom.to_string())?
            .unwrap_or(CollateralVolatility {
                index: Decimal::zero(),
                volatility_list: vec![],
                raw_volatility_list: vec![],
            });

        // Add to volatility lists (keep last 10 entries)
        vol.volatility_list.push(speed);
        if vol.volatility_list.len() > 10 {
            vol.volatility_list.remove(0);
        }
        vol.raw_volatility_list.push(price_change);
        if vol.raw_volatility_list.len() > 10 {
            vol.raw_volatility_list.remove(0);
        }

        // Update index as average of speed list
        let sum: Decimal = vol.volatility_list.iter().fold(Decimal::zero(), |acc, v| acc + *v);
        if !vol.volatility_list.is_empty() {
            vol.index = sum / Decimal::from_ratio(vol.volatility_list.len() as u128, 1u128);
        }

        VOLATILITY.save(storage, asset_denom.to_string(), &vol)?;
    }

    Ok(())
}

/// Update historical oracle prices for an asset.
fn update_historical_prices(
    storage: &mut dyn cosmwasm_std::Storage,
    env: &Env,
    asset_denom: &str,
    current_price: Decimal,
) -> Result<(), ContractError> {
    use crate::state::MAX_ORACLE_ENTRIES;

    let mut history = HISTORICAL_ORACLE_PRICES
        .may_load(storage, asset_denom.to_string())?
        .unwrap_or_default();

    history.push(PriceTimestamp {
        price: current_price.to_string(),
        timestamp: env.block.time.seconds(),
    });

    // Trim to max entries
    if history.len() > MAX_ORACLE_ENTRIES {
        let excess = history.len() - MAX_ORACLE_ENTRIES;
        history.drain(..excess);
    }

    HISTORICAL_ORACLE_PRICES.save(storage, asset_denom.to_string(), &history)?;

    Ok(())
}

/// Compute weighted average LTVs and threshold from position ratios and basket's authoritative collateral_types.
///
/// CDP parity: maps to cdp/query.rs::get_avg_LTV()
/// Returns (avg_max_ltv, avg_max_borrow_ltv, avg_max_threshold_to_delay).
fn compute_avg_ltvs(
    position_assets: &[cAsset],
    ratios: &[Decimal],
    collateral_types: &[cAsset],
) -> Result<(Decimal, Decimal, Decimal), ContractError> {
    let mut avg_max_ltv = Decimal::zero();
    let mut avg_max_borrow_ltv = Decimal::zero();
    let mut avg_max_threshold_to_delay = Decimal::zero();

    for (i, pa) in position_assets.iter().enumerate() {
        let ratio = ratios.get(i).copied().unwrap_or(Decimal::zero());

        // Find authoritative LTV from basket's collateral_types
        let ct = collateral_types
            .iter()
            .find(|ct| ct.asset.info.equal(&pa.asset.info));

        if let Some(ct) = ct {
            avg_max_ltv += ratio * ct.max_LTV;
            avg_max_borrow_ltv += ratio * ct.max_borrow_LTV;
            avg_max_threshold_to_delay += ratio * ct.max_threshold_to_delay;
        }
    }

    Ok((avg_max_ltv, avg_max_borrow_ltv, avg_max_threshold_to_delay))
}

// ============================================================================
// Read-only Queries
// ============================================================================

/// Query collateral positions with pagination.
///
/// CDP parity: maps to cdp/query.rs::query_basket_positions() (collateral portion)
pub fn query_collateral_positions(
    deps: Deps,
    _start_after: Option<String>,
    _limit: Option<u32>,
    user_info: Option<UserInfo>,
    user: Option<String>,
) -> StdResult<Binary> {
    // Single position lookup
    if let Some(ui) = user_info {
        let owner = deps.api.addr_validate(&ui.position_owner)?;
        let positions = POSITIONS
            .may_load(deps.storage, owner.clone())?
            .unwrap_or_default();

        let target = positions.into_iter().find(|p| p.position_id == ui.position_id);

        let response_positions = match target {
            Some(pos) => vec![CollateralPositionResponse {
                position_id: pos.position_id,
                collateral_assets: pos.collateral_assets.clone(),
                cAsset_ratios: vec![], // Would need oracle query for ratios
            }],
            None => vec![],
        };

        return to_json_binary(&CollateralPositionsResponse {
            user: ui.position_owner,
            positions: response_positions,
        });
    }

    // User positions lookup
    if let Some(user_str) = user {
        let owner = deps.api.addr_validate(&user_str)?;
        let positions = POSITIONS
            .may_load(deps.storage, owner)?
            .unwrap_or_default();

        let response_positions: Vec<CollateralPositionResponse> = positions
            .into_iter()
            .map(|pos| CollateralPositionResponse {
                position_id: pos.position_id,
                collateral_assets: pos.collateral_assets,
                cAsset_ratios: vec![],
            })
            .collect();

        return to_json_binary(&CollateralPositionsResponse {
            user: user_str,
            positions: response_positions,
        });
    }

    // TODO: Paginated all-positions query
    to_json_binary(&Vec::<CollateralPositionsResponse>::new())
}

/// Query historical oracle prices for an asset.
///
/// CDP parity: maps to cdp/query.rs::query_historical_oracle_prices()
pub fn query_historical_oracle_prices(deps: Deps, asset: String) -> StdResult<Binary> {
    let history = HISTORICAL_ORACLE_PRICES
        .may_load(deps.storage, asset)?
        .unwrap_or_default();
    to_json_binary(&history)
}

/// Check if assets are in volatile windows.
///
/// CDP parity: maps to cdp/query.rs::query_volatility_window()
pub fn query_volatility_window(deps: Deps, assets: Vec<String>) -> StdResult<Binary> {
    let mut results: Vec<(String, bool)> = Vec::new();

    for asset in assets {
        let vol = VOLATILITY.may_load(deps.storage, asset.clone())?;
        let is_volatile = match vol {
            Some(v) => v.index > Decimal::percent(1), // 1% threshold
            None => false,
        };
        results.push((asset, is_volatile));
    }

    to_json_binary(&results)
}

/// Simulate liquidation market sales for slippage estimation.
///
/// CDP parity: maps to cdp/query.rs::query_simulate_liquidation()
pub fn query_simulate_liquidation(
    _deps: Deps,
    _collateral_to_sell: Vec<cosmwasm_std::Coin>,
    _target_denom: String,
) -> StdResult<Binary> {
    // TODO: Port full simulation logic from CDP
    to_json_binary(&"simulation_not_yet_implemented")
}

/// Get circuit breaker status for specified assets.
pub fn query_circuit_breaker_status(deps: Deps, assets: Vec<String>) -> StdResult<Binary> {
    let mut results: Vec<(String, Option<AssetCircuitBreaker>)> = Vec::new();

    for asset in assets {
        let breaker = ASSET_CIRCUIT_BREAKERS.may_load(deps.storage, asset.clone())?;
        results.push((asset, breaker));
    }

    to_json_binary(&results)
}

/// Get position collateral value and weighted average LTV info from stored prices.
/// Used by the Debt contract's fulfill_intents to compute deployment venue LTV allocations
/// without needing a SubMsg (read-only query from stored state).
///
/// CDP parity: equivalent to the value/LTV portion of cdp/query.rs::insolvency_check()
pub fn query_position_value(
    deps: Deps,
    position_owner: String,
    position_id: Uint128,
) -> StdResult<Binary> {
    let owner = deps.api.addr_validate(&position_owner)?;
    let basket = BASKET.load(deps.storage)?;

    let (_index, position) =
        crate::state::get_target_position(deps.storage, owner, position_id)
            .map_err(|e| StdError::generic_err(format!("Position not found: {}", e)))?;

    // Compute total value from STORED_PRICES
    let mut total_value = Decimal::zero();
    let mut asset_values: Vec<Decimal> = Vec::new();

    for ca in &position.collateral_assets {
        let stored = STORED_PRICES
            .may_load(deps.storage, ca.asset.info.to_string())?;

        let value = match stored {
            Some(sp) => {
                let amount_dec = Decimal::from_ratio(ca.asset.amount, 1u128);
                amount_dec * sp.price.price
            }
            None => Decimal::zero(),
        };
        asset_values.push(value);
        total_value += value;
    }

    // Compute weighted average LTVs
    let mut avg_max_borrow_ltv = Decimal::zero();
    let mut avg_max_ltv = Decimal::zero();
    let mut avg_max_threshold_to_delay = Decimal::zero();

    if !total_value.is_zero() {
        for (i, ca) in position.collateral_assets.iter().enumerate() {
            let ratio = asset_values[i] / total_value;

            if let Some(ct) = basket
                .collateral_types
                .iter()
                .find(|ct| ct.asset.info.equal(&ca.asset.info))
            {
                avg_max_ltv += ratio * ct.max_LTV;
                avg_max_borrow_ltv += ratio * ct.max_borrow_LTV;
                avg_max_threshold_to_delay += ratio * ct.max_threshold_to_delay;
            }
        }
    }

    to_json_binary(&PositionValueResponse {
        total_value,
        avg_max_borrow_ltv,
        avg_max_ltv,
        avg_max_threshold_to_delay,
    })
}

// ============================================================================
// LTV Update Queries
// ============================================================================

/// Query historical LTV snapshots for an asset, filtered by time range.
pub fn query_historical_ltv(
    deps: Deps,
    asset_denom: String,
    start_time: Option<u64>,
    end_time: Option<u64>,
    limit: Option<u32>,
) -> StdResult<Binary> {
    let limit = limit.unwrap_or(100).min(500) as usize;
    let start = start_time.unwrap_or(0);
    let end = end_time.unwrap_or(u64::MAX);

    let all_snapshots = LTV_HISTORY
        .may_load(deps.storage, asset_denom.clone())?
        .unwrap_or_default();

    let snapshots = all_snapshots
        .into_iter()
        .filter(|s| s.timestamp >= start && s.timestamp <= end)
        .take(limit)
        .collect();

    to_json_binary(&HistoricalLTVResponse {
        asset_denom,
        snapshots,
    })
}

/// Query pending LTV shift info for an asset.
/// Returns timing information about any staged downward LTV adjustment.
pub fn query_ltv_shift_info(
    deps: Deps,
    env: Env,
    asset_denom: String,
) -> StdResult<Binary> {
    let tracker = LTV_UPDATE_TRACKERS
        .may_load(deps.storage, asset_denom.clone())?
        .ok_or_else(|| StdError::generic_err(format!(
            "No LTV tracker found for asset {}", asset_denom
        )))?;

    let current_time = env.block.time.seconds();

    let (next_shift_time, time_until_shift) = if let Some(staged_ts) = tracker.staged_timestamp {
        let config = CONFIG.load(deps.storage)?;
        let shift_time = staged_ts + config.ltv_downward_period;
        let time_until = if shift_time > current_time {
            shift_time - current_time
        } else {
            0
        };
        (shift_time, time_until)
    } else {
        (0, 0)
    };

    let disco_blocked = crate::state::DISCO_BLOCKED
        .may_load(deps.storage, asset_denom.clone())?
        .unwrap_or(false);

    to_json_binary(&LTVShiftInfoResponse {
        next_shift_time,
        time_until_shift,
        staged_max_ltv: tracker.staged_max_ltv,
        disco_blocked,
    })
}
