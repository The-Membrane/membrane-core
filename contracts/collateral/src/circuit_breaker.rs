use std::str::FromStr;

use cosmwasm_std::{
    to_json_binary, Decimal, Env, QuerierWrapper, QueryRequest, StdError, StdResult, Storage,
    WasmQuery,
};

use membrane::collateral::Config;
use membrane::oracle::{PriceResponse, QueryMsg as OracleQueryMsg};
use membrane::types::{cAsset, AssetInfo};

use crate::error::ContractError;
use crate::state::{
    AssetCircuitBreaker, ASSET_CIRCUIT_BREAKERS, HISTORICAL_ORACLE_PRICES, PriceTimestamp,
};

/// Calculate a reference price for an asset using historical oracle data.
/// Falls back to the current price if not enough history is available or
/// if parsing errors occur.
///
/// CDP parity: maps to the reference price calculation in cdp/circuit_breaker.rs
fn calculate_reference_price(
    storage: &dyn Storage,
    asset_denom: &str,
    current_price: Decimal,
) -> StdResult<Decimal> {
    let historical: Vec<PriceTimestamp> = HISTORICAL_ORACLE_PRICES
        .may_load(storage, asset_denom.to_string())?
        .unwrap_or_else(|| vec![]);

    // Use average of last 5 valid prices if possible
    let recent_prices: Vec<Decimal> = historical
        .iter()
        .rev()
        .filter_map(|pt| Decimal::from_str(&pt.price).ok())
        .take(5)
        .collect();

    if recent_prices.is_empty() {
        // No usable history, fall back to current price
        return Ok(current_price);
    }

    // If we got fewer than 5, that's fine; average whatever we have
    let sum: Decimal = recent_prices
        .iter()
        .fold(Decimal::zero(), |acc, p| acc + *p);

    // Divide by count; safe because count > 0 due to is_empty() check
    let count = Decimal::from_ratio(recent_prices.len() as u128, 1u128);
    let avg = sum
        .checked_div(count)
        .map_err(|e| StdError::generic_err(e.to_string()))?;

    Ok(avg)
}

/// Check price deviation and auto-freeze/unfreeze an asset.
///
/// Returns:
/// - Ok(true) if the asset is currently frozen after this check
/// - Ok(false) if the asset is not frozen after this check
///
/// CDP parity: maps to cdp/circuit_breaker.rs::check_and_update_circuit_breaker()
pub fn check_and_update_circuit_breaker(
    storage: &mut dyn Storage,
    env: &Env,
    asset_denom: String,
    current_price: Decimal,
    price_deviation_threshold: Decimal,
) -> StdResult<bool> {
    // Load or initialize breaker
    let mut breaker: AssetCircuitBreaker = ASSET_CIRCUIT_BREAKERS
        .may_load(storage, asset_denom.clone())?
        .unwrap_or(AssetCircuitBreaker {
            frozen: false,
            frozen_at: 0,
            price_deviation_threshold,
            reference_price: None,
        });

    // Keep the threshold up to date if caller passes a different one
    breaker.price_deviation_threshold = price_deviation_threshold;

    // Determine reference price
    let reference_price = if breaker.frozen {
        // While frozen, keep the original baseline
        breaker.reference_price.unwrap_or_else(|| {
            // If somehow reference_price is None while frozen, recalculate
            calculate_reference_price(storage, &asset_denom, current_price)
                .unwrap_or(current_price)
        })
    } else {
        // While healthy, always recalculate from history
        let ref_price = calculate_reference_price(storage, &asset_denom, current_price)?;
        breaker.reference_price = Some(ref_price);
        ref_price
    };

    // Guard against zero reference price to avoid division by zero
    if reference_price.is_zero() {
        breaker.reference_price = Some(current_price);
        ASSET_CIRCUIT_BREAKERS.save(storage, asset_denom, &breaker)?;
        return Ok(false);
    }

    // Compute absolute percentage deviation: |current - ref| / ref
    let (higher, lower) = if current_price > reference_price {
        (current_price, reference_price)
    } else {
        (reference_price, current_price)
    };

    let diff = higher
        .checked_sub(lower)
        .map_err(|e| StdError::generic_err(e.to_string()))?;
    let price_change = diff
        .checked_div(reference_price)
        .map_err(|e| StdError::generic_err(e.to_string()))?;

    if breaker.frozen {
        // Auto-unfreeze if price is back within threshold
        if price_change <= breaker.price_deviation_threshold {
            breaker.frozen = false;
            breaker.frozen_at = 0;
            breaker.reference_price = Some(current_price);
            ASSET_CIRCUIT_BREAKERS.save(storage, asset_denom, &breaker)?;
            Ok(false)
        } else {
            // Still frozen
            breaker.reference_price = Some(reference_price);
            ASSET_CIRCUIT_BREAKERS.save(storage, asset_denom, &breaker)?;
            Ok(true)
        }
    } else {
        // Not frozen yet; freeze if deviation exceeds threshold
        if price_change > breaker.price_deviation_threshold {
            breaker.frozen = true;
            breaker.frozen_at = env.block.time.seconds();
            breaker.reference_price = Some(reference_price);
            ASSET_CIRCUIT_BREAKERS.save(storage, asset_denom, &breaker)?;
            Ok(true)
        } else {
            // Remain unfrozen; update reference to track normal movements
            breaker.reference_price = Some(reference_price);
            ASSET_CIRCUIT_BREAKERS.save(storage, asset_denom, &breaker)?;
            Ok(false)
        }
    }
}

/// Check if any of the provided assets are frozen before executing an operation.
/// This function auto-updates circuit breaker state based on current prices.
///
/// Blocks: withdraw, increase_debt, liquidate when an asset is frozen.
///
/// CDP parity: maps to cdp/circuit_breaker.rs::check_assets_not_frozen()
pub fn check_assets_not_frozen(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: &Env,
    config: &Config,
    assets: &[cAsset],
) -> Result<(), ContractError> {
    // If no oracle is configured, we can't perform price-based circuit breaking
    let oracle_addr = match config.oracle_contract.clone() {
        Some(addr) => addr,
        None => return Ok(()),
    };

    // Collect all native token asset_infos for batch querying
    let mut native_asset_infos: Vec<AssetInfo> = Vec::new();
    let mut asset_denoms: Vec<String> = Vec::new();

    for c_asset in assets {
        let asset_info = c_asset.asset.info.clone();
        let asset_denom = asset_info.to_string();

        match asset_info {
            AssetInfo::Token { .. } => {
                // Skip cw20 assets for circuit breaking
                continue;
            }
            AssetInfo::NativeToken { .. } => {
                native_asset_infos.push(asset_info);
                asset_denoms.push(asset_denom);
            }
        }
    }

    // If no native assets to check, return early
    if native_asset_infos.is_empty() {
        return Ok(());
    }

    // Query all prices at once using Prices (plural) for efficiency
    let price_resps: Vec<PriceResponse> = querier
        .query(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: oracle_addr.to_string(),
            msg: to_json_binary(&OracleQueryMsg::Prices {
                asset_infos: native_asset_infos.clone(),
                twap_timeframe: 0u64,
                oracle_time_limit: config.oracle_time_limit,
            })?,
        }))
        .map_err(StdError::from)?;

    // Check each asset's circuit breaker status
    for (i, asset_denom) in asset_denoms.iter().enumerate() {
        let current_price = price_resps
            .get(i)
            .ok_or_else(|| StdError::generic_err("Price response mismatch"))?
            .price;

        // Use a default 10% deviation threshold for now; can be made per-asset later.
        let is_frozen = check_and_update_circuit_breaker(
            storage,
            env,
            asset_denom.clone(),
            current_price,
            Decimal::percent(10),
        )
        .map_err(ContractError::from)?;

        if is_frozen {
            return Err(ContractError::AssetFrozen {
                denom: asset_denom.clone(),
            });
        }
    }

    Ok(())
}
