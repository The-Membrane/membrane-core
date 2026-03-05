use std::str::FromStr;

use cosmwasm_std::{
    to_json_binary, Decimal, Env, QuerierWrapper, QueryRequest, StdError, StdResult, Storage,
    WasmQuery,
};

use membrane::cdp::Config;
use membrane::oracle::{PriceResponse, QueryMsg as OracleQueryMsg};
use membrane::types::{cAsset, AssetInfo};

use crate::error::ContractError;
use crate::state::{
    AssetCircuitBreaker, ASSET_CIRCUIT_BREAKERS, HISTORICAL_ORACLE_PRICES, PriceTimestamp,
};

/// Calculate a reference price for an asset using historical oracle data.
/// Falls back to the current price if not enough history is available or
/// if parsing errors occur.
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

        // Only native tokens are supported here; cw20s aren't supported by CDP anyway
        // but we check to be explicit.
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
            return Err(ContractError::CustomError {
                val: format!("Asset {} is frozen due to price deviation. Withdrawals, debt increases, and liquidations are temporarily disabled.", asset_denom),
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env, MockQuerier},
        Addr, Decimal, QuerierWrapper, Storage, SystemResult, ContractResult, WasmQuery,
    };
    use crate::state::{ASSET_CIRCUIT_BREAKERS, HISTORICAL_ORACLE_PRICES, PriceTimestamp};
    use membrane::cdp::Config;
    use membrane::types::{cAsset, IRMConfig, Asset, AssetInfo};
    use membrane::oracle::PriceResponse;

    fn setup_storage_with_history(
        storage: &mut dyn Storage,
        asset_denom: &str,
        prices: Vec<&str>,
    ) {
        let mut historical = vec![];
        for (i, price_str) in prices.iter().enumerate() {
            historical.push(PriceTimestamp {
                price: price_str.to_string(),
                timestamp: (1000 + i as u64) * 1000, // Simulate timestamps
            });
        }
        HISTORICAL_ORACLE_PRICES
            .save(storage, asset_denom.to_string(), &historical)
            .unwrap();
    }

    #[test]
    fn test_auto_freeze_on_price_spike() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let asset_denom = "debit".to_string();
        let threshold = Decimal::percent(10);

        // Setup historical prices: all at $1.00
        setup_storage_with_history(deps.as_mut().storage, &asset_denom, vec!["1.0", "1.0", "1.0", "1.0", "1.0"]);

        // First check with normal price - should not freeze
        let current_price = Decimal::from_str("1.05").unwrap(); // 5% increase, within threshold
        let is_frozen = check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset_denom.clone(),
            current_price,
            threshold,
        )
        .unwrap();
        assert!(!is_frozen, "Should not freeze on 5% deviation");

        // Now check with price spike - should freeze
        let current_price = Decimal::from_str("1.15").unwrap(); // 15% increase, exceeds threshold
        let is_frozen = check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset_denom.clone(),
            current_price,
            threshold,
        )
        .unwrap();
        assert!(is_frozen, "Should freeze on 15% deviation");

        // Verify frozen state
        let breaker = ASSET_CIRCUIT_BREAKERS
            .load(deps.as_ref().storage, asset_denom.clone())
            .unwrap();
        assert!(breaker.frozen, "Breaker should be frozen");
        assert!(breaker.frozen_at > 0, "frozen_at should be set");
    }

    #[test]
    fn test_auto_unfreeze_on_price_normalization() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let asset_denom = "debit".to_string();
        let threshold = Decimal::percent(10);

        // Setup historical prices
        setup_storage_with_history(deps.as_mut().storage, &asset_denom, vec!["1.0", "1.0", "1.0", "1.0", "1.0"]);

        // Freeze the asset first
        let spike_price = Decimal::from_str("1.15").unwrap(); // 15% spike
        check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset_denom.clone(),
            spike_price,
            threshold,
        )
        .unwrap();

        // Verify it's frozen
        let breaker = ASSET_CIRCUIT_BREAKERS
            .load(deps.as_ref().storage, asset_denom.clone())
            .unwrap();
        assert!(breaker.frozen, "Should be frozen after spike");

        // Now check with normalized price - should unfreeze
        let normal_price = Decimal::from_str("1.05").unwrap(); // 5% from reference, within threshold
        let is_frozen = check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset_denom.clone(),
            normal_price,
            threshold,
        )
        .unwrap();
        assert!(!is_frozen, "Should unfreeze when price normalizes");

        // Verify unfrozen state
        let breaker = ASSET_CIRCUIT_BREAKERS
            .load(deps.as_ref().storage, asset_denom.clone())
            .unwrap();
        assert!(!breaker.frozen, "Breaker should be unfrozen");
        assert_eq!(breaker.frozen_at, 0, "frozen_at should be reset");
    }

    #[test]
    fn test_reference_price_calculation_from_history() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let asset_denom = "debit".to_string();
        let threshold = Decimal::percent(10);

        // Setup historical prices: [1.0, 1.0, 1.0, 1.0, 1.0]
        // Average should be 1.0
        setup_storage_with_history(deps.as_mut().storage, &asset_denom, vec!["1.0", "1.0", "1.0", "1.0", "1.0"]);

        let current_price = Decimal::from_str("1.0").unwrap();
        let is_frozen = check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset_denom.clone(),
            current_price,
            threshold,
        )
        .unwrap();

        assert!(!is_frozen, "Should not freeze when price matches reference");

        // Check reference price was set
        let breaker = ASSET_CIRCUIT_BREAKERS
            .load(deps.as_ref().storage, asset_denom.clone())
            .unwrap();
        assert!(breaker.reference_price.is_some(), "Reference price should be set");
    }

    #[test]
    fn test_reference_price_with_fewer_than_5_prices() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let asset_denom = "debit".to_string();
        let threshold = Decimal::percent(10);

        // Setup only 3 historical prices
        setup_storage_with_history(deps.as_mut().storage, &asset_denom, vec!["1.0", "1.0", "1.0"]);

        let current_price = Decimal::from_str("1.0").unwrap();
        let is_frozen = check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset_denom.clone(),
            current_price,
            threshold,
        )
        .unwrap();

        assert!(!is_frozen, "Should work with fewer than 5 prices");
    }

    #[test]
    fn test_reference_price_no_history_falls_back_to_current() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let asset_denom = "debit".to_string();
        let threshold = Decimal::percent(10);

        // No historical prices
        let current_price = Decimal::from_str("1.0").unwrap();
        let is_frozen = check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset_denom.clone(),
            current_price,
            threshold,
        )
        .unwrap();

        assert!(!is_frozen, "Should not freeze when no history (uses current as reference)");
    }

    #[test]
    fn test_price_drop_triggers_freeze() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let asset_denom = "debit".to_string();
        let threshold = Decimal::percent(10);

        // Setup historical prices at $1.00
        setup_storage_with_history(deps.as_mut().storage, &asset_denom, vec!["1.0", "1.0", "1.0", "1.0", "1.0"]);

        // Price drops 15% - should freeze
        let drop_price = Decimal::from_str("0.85").unwrap(); // 15% drop
        let is_frozen = check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset_denom.clone(),
            drop_price,
            threshold,
        )
        .unwrap();

        assert!(is_frozen, "Should freeze on 15% price drop");
    }

    #[test]
    fn test_frozen_state_preserves_reference_price() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let asset_denom = "debit".to_string();
        let threshold = Decimal::percent(10);

        // Setup historical prices
        setup_storage_with_history(deps.as_mut().storage, &asset_denom, vec!["1.0", "1.0", "1.0", "1.0", "1.0"]);

        // Freeze with spike
        let spike_price = Decimal::from_str("1.15").unwrap();
        check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset_denom.clone(),
            spike_price,
            threshold,
        )
        .unwrap();

        // Get reference price when frozen
        let breaker_before = ASSET_CIRCUIT_BREAKERS
            .load(deps.as_ref().storage, asset_denom.clone())
            .unwrap();
        let frozen_reference = breaker_before.reference_price.unwrap();

        // Add new historical prices (these shouldn't change reference while frozen)
        setup_storage_with_history(deps.as_mut().storage, &asset_denom, vec!["1.1", "1.1", "1.1", "1.1", "1.1"]);

        // Check again while still frozen (price still high)
        let still_high_price = Decimal::from_str("1.14").unwrap(); // Still above threshold
        check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset_denom.clone(),
            still_high_price,
            threshold,
        )
        .unwrap();

        // Reference should remain the same while frozen
        let breaker_after = ASSET_CIRCUIT_BREAKERS
            .load(deps.as_ref().storage, asset_denom.clone())
            .unwrap();
        assert_eq!(
            breaker_after.reference_price.unwrap(),
            frozen_reference,
            "Reference price should not change while frozen"
        );
    }

    #[test]
    fn test_unfrozen_state_updates_reference_price() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let asset_denom = "debit".to_string();
        let threshold = Decimal::percent(10);

        // Setup initial historical prices
        setup_storage_with_history(deps.as_mut().storage, &asset_denom, vec!["1.0", "1.0", "1.0", "1.0", "1.0"]);

        // First check - should set reference
        let price1 = Decimal::from_str("1.0").unwrap();
        check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset_denom.clone(),
            price1,
            threshold,
        )
        .unwrap();

        let breaker1 = ASSET_CIRCUIT_BREAKERS
            .load(deps.as_ref().storage, asset_denom.clone())
            .unwrap();
        let ref1 = breaker1.reference_price.unwrap();

        // Update historical prices to new average
        setup_storage_with_history(deps.as_mut().storage, &asset_denom, vec!["1.05", "1.05", "1.05", "1.05", "1.05"]);

        // Check again with normal price - reference should update
        let price2 = Decimal::from_str("1.05").unwrap();
        check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset_denom.clone(),
            price2,
            threshold,
        )
        .unwrap();

        let breaker2 = ASSET_CIRCUIT_BREAKERS
            .load(deps.as_ref().storage, asset_denom.clone())
            .unwrap();
        let ref2 = breaker2.reference_price.unwrap();

        // Reference should have updated to new average
        assert_ne!(ref1, ref2, "Reference price should update when not frozen");
        assert_eq!(ref2, Decimal::from_str("1.05").unwrap(), "Reference should be new average");
    }

    #[test]
    fn test_zero_price_handling() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let asset_denom = "debit".to_string();
        let threshold = Decimal::percent(10);

        // Zero reference price should not cause panic
        let zero_price = Decimal::zero();
        let is_frozen = check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset_denom.clone(),
            zero_price,
            threshold,
        )
        .unwrap();

        assert!(!is_frozen, "Should handle zero price gracefully");
    }

    #[test]
    fn test_threshold_boundary() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let asset_denom = "debit".to_string();
        let threshold = Decimal::percent(10);

        setup_storage_with_history(deps.as_mut().storage, &asset_denom, vec!["1.0", "1.0", "1.0", "1.0", "1.0"]);

        // Exactly at threshold (10%) - should NOT freeze
        let at_threshold = Decimal::from_str("1.10").unwrap();
        let is_frozen = check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset_denom.clone(),
            at_threshold,
            threshold,
        )
        .unwrap();
        assert!(!is_frozen, "Should not freeze at exactly threshold");

        // Just above threshold - should freeze
        let above_threshold = Decimal::from_str("1.1000001").unwrap();
        let is_frozen = check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset_denom.clone(),
            above_threshold,
            threshold,
        )
        .unwrap();
        assert!(is_frozen, "Should freeze just above threshold");
    }

    #[test]
    fn test_multiple_assets_independent_state() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let threshold = Decimal::percent(10);

        // Setup asset1
        let asset1 = "debit".to_string();
        setup_storage_with_history(deps.as_mut().storage, &asset1, vec!["1.0", "1.0", "1.0", "1.0", "1.0"]);

        // Setup asset2
        let asset2 = "usdc".to_string();
        setup_storage_with_history(deps.as_mut().storage, &asset2, vec!["1.0", "1.0", "1.0", "1.0", "1.0"]);

        // Freeze asset1
        let spike_price = Decimal::from_str("1.15").unwrap();
        check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset1.clone(),
            spike_price,
            threshold,
        )
        .unwrap();

        // Asset2 should remain unfrozen
        let normal_price = Decimal::from_str("1.0").unwrap();
        let is_frozen = check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset2.clone(),
            normal_price,
            threshold,
        )
        .unwrap();

        assert!(!is_frozen, "Asset2 should remain unfrozen");

        // Verify asset1 is frozen, asset2 is not
        let breaker1 = ASSET_CIRCUIT_BREAKERS
            .load(deps.as_ref().storage, asset1)
            .unwrap();
        let breaker2 = ASSET_CIRCUIT_BREAKERS
            .load(deps.as_ref().storage, asset2)
            .unwrap();

        assert!(breaker1.frozen, "Asset1 should be frozen");
        assert!(!breaker2.frozen, "Asset2 should not be frozen");
    }

    #[test]
    fn test_threshold_update() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let asset_denom = "debit".to_string();

        setup_storage_with_history(deps.as_mut().storage, &asset_denom, vec!["1.0", "1.0", "1.0", "1.0", "1.0"]);

        // First check with 10% threshold
        let threshold1 = Decimal::percent(10);
        let price1 = Decimal::from_str("1.12").unwrap(); // 12% - would freeze with 10% threshold
        let is_frozen = check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset_denom.clone(),
            price1,
            threshold1,
        )
        .unwrap();
        assert!(is_frozen, "Should freeze with 10% threshold");

        // Unfreeze first
        let normal_price = Decimal::from_str("1.0").unwrap();
        check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset_denom.clone(),
            normal_price,
            threshold1,
        )
        .unwrap();

        // Now check with 15% threshold - same price should not freeze
        let threshold2 = Decimal::percent(15);
        let is_frozen = check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset_denom.clone(),
            price1,
            threshold2,
        )
        .unwrap();
        assert!(!is_frozen, "Should not freeze with 15% threshold for 12% deviation");
    }

    /// Helper to create a mock querier that returns specific prices for oracle queries
    fn mock_querier_with_prices(prices: Vec<Decimal>) -> MockQuerier {
        let mut querier = MockQuerier::default();
        let prices_clone = prices.clone();
        querier.update_wasm(move |query| {
            match query {
                WasmQuery::Smart { contract_addr: _, msg } => {
                    // Try to deserialize as OracleQueryMsg
                    if let Ok(oracle_query) = cosmwasm_std::from_json::<OracleQueryMsg>(msg) {
                        match oracle_query {
                            OracleQueryMsg::Prices { asset_infos, .. } => {
                                let mut price_responses = vec![];
                                for (i, _asset_info) in asset_infos.iter().enumerate() {
                                    let price = prices_clone.get(i).copied().unwrap_or(Decimal::one());
                                    price_responses.push(PriceResponse {
                                        prices: vec![],
                                        price,
                                        decimals: 6,
                                    });
                                }
                                SystemResult::Ok(ContractResult::Ok(
                                    to_json_binary(&price_responses).unwrap()
                                ))
                            }
                            _ => SystemResult::Ok(ContractResult::Ok(to_json_binary(&true).unwrap())),
                        }
                    } else {
                        SystemResult::Ok(ContractResult::Ok(to_json_binary(&true).unwrap()))
                    }
                }
                _ => SystemResult::Ok(ContractResult::Ok(to_json_binary(&true).unwrap())),
            }
        });
        querier
    }

    #[test]
    fn test_frozen_asset_blocks_withdraw() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let asset_denom = "debit".to_string();
        let threshold = Decimal::percent(10);

        // Setup historical prices at $1.00
        setup_storage_with_history(deps.as_mut().storage, &asset_denom, vec!["1.0", "1.0", "1.0", "1.0", "1.0"]);

        // Freeze the asset with a price spike (15% increase)
        let spike_price = Decimal::from_str("1.15").unwrap();
        check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset_denom.clone(),
            spike_price,
            threshold,
        )
        .unwrap();

        // Verify it's frozen
        let breaker = ASSET_CIRCUIT_BREAKERS
            .load(deps.as_ref().storage, asset_denom.clone())
            .unwrap();
        assert!(breaker.frozen, "Asset should be frozen");

        // Setup config with oracle
        let config = Config {
            owner: Addr::unchecked("owner"),
            staking_contract: None,
            chain_proxy: None,
            debt_auction: None,
            oracle_contract: Some(Addr::unchecked("oracle")),
            liquidity_contract: None,
            discounts_contract: None,
            ltv_disco: Addr::unchecked("ltv_disco"),
            liq_fee: Decimal::percent(5),
            collateral_twap_timeframe: 60,
            credit_twap_timeframe: 60,
            oracle_time_limit: 300,
            cpc_multiplier: Decimal::percent(1),
            debt_minimum: cosmwasm_std::Uint128::zero(),
            base_debt_cap_multiplier: cosmwasm_std::Uint128::from(100u128),
            rate_slope_multiplier: Decimal::percent(1),
            affiliate_fee_max: Decimal::percent(1),
            revenue_distributor: None,
            skip_credit_price_accrual: false,
            liquidation_stat_limit: 1000,
            transmuter_addr: None,
            irm_config: IRMConfig {
                adjustment_speed: Decimal::from_atomics(50u128, 0).unwrap(),
                min_adaptive_rate: Decimal::permille(1),
                max_adaptive_rate: Decimal::from_atomics(1u128, 0).unwrap(),
            },
            points_contract: None,
        };

        // Create mock querier that returns still-high price (keeps it frozen)
        let querier = mock_querier_with_prices(vec![Decimal::from_str("1.14").unwrap()]);
        let querier_wrapper = QuerierWrapper::new(&querier);

        // Create assets to check
        let assets = vec![cAsset {
            asset: Asset {
                info: AssetInfo::NativeToken {
                    denom: asset_denom.clone(),
                },
                amount: cosmwasm_std::Uint128::from(1000u128),
            },
            max_borrow_LTV: Decimal::percent(75),
            max_LTV: Decimal::percent(85),
            pool_info: None,
            rate_index: Decimal::one(),
            peg_rate_index: Decimal::one(),
            force_redemptions: None,
        }];

        // Attempt to check assets - should fail because asset is frozen
        let result = check_assets_not_frozen(
            deps.as_mut().storage,
            querier_wrapper,
            &env,
            &config,
            &assets,
        );

        assert!(result.is_err(), "Should return error when asset is frozen");
        match result.unwrap_err() {
            ContractError::CustomError { val } => {
                assert!(
                    val.contains("frozen"),
                    "Error message should mention frozen asset"
                );
            }
            _ => panic!("Expected CustomError with frozen message"),
        }
    }

    #[test]
    fn test_frozen_asset_blocks_increase_debt() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let asset_denom = "debit".to_string();
        let threshold = Decimal::percent(10);

        // Setup historical prices at $1.00
        setup_storage_with_history(deps.as_mut().storage, &asset_denom, vec!["1.0", "1.0", "1.0", "1.0", "1.0"]);

        // Freeze the asset with a price drop (15% decrease)
        let drop_price = Decimal::from_str("0.85").unwrap();
        check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset_denom.clone(),
            drop_price,
            threshold,
        )
        .unwrap();

        // Verify it's frozen
        let breaker = ASSET_CIRCUIT_BREAKERS
            .load(deps.as_ref().storage, asset_denom.clone())
            .unwrap();
        assert!(breaker.frozen, "Asset should be frozen");

        // Setup config with oracle
        let config = Config {
            owner: Addr::unchecked("owner"),
            staking_contract: None,
            chain_proxy: None,
            debt_auction: None,
            oracle_contract: Some(Addr::unchecked("oracle")),
            liquidity_contract: None,
            discounts_contract: None,
            ltv_disco: Addr::unchecked("ltv_disco"),
            liq_fee: Decimal::percent(5),
            collateral_twap_timeframe: 60,
            credit_twap_timeframe: 60,
            oracle_time_limit: 300,
            cpc_multiplier: Decimal::percent(1),
            debt_minimum: cosmwasm_std::Uint128::zero(),
            base_debt_cap_multiplier: cosmwasm_std::Uint128::from(100u128),
            rate_slope_multiplier: Decimal::percent(1),
            affiliate_fee_max: Decimal::percent(1),
            revenue_distributor: None,
            skip_credit_price_accrual: false,
            liquidation_stat_limit: 1000,
            transmuter_addr: None,
            irm_config: IRMConfig {
                adjustment_speed: Decimal::from_atomics(50u128, 0).unwrap(),
                min_adaptive_rate: Decimal::permille(1),
                max_adaptive_rate: Decimal::from_atomics(1u128, 0).unwrap(),
            },
            points_contract: None,
        };

        // Create mock querier that returns still-low price (keeps it frozen)
        let querier = mock_querier_with_prices(vec![Decimal::from_str("0.86").unwrap()]);
        let querier_wrapper = QuerierWrapper::new(&querier);

        // Create assets to check (collateral assets for increase_debt)
        let assets = vec![cAsset {
            asset: Asset {
                info: AssetInfo::NativeToken {
                    denom: asset_denom.clone(),
                },
                amount: cosmwasm_std::Uint128::from(1000u128),
            },
            max_borrow_LTV: Decimal::percent(75),
            max_LTV: Decimal::percent(85),
            pool_info: None,
            rate_index: Decimal::one(),
            peg_rate_index: Decimal::one(),
            force_redemptions: None,
        }];

        // Attempt to check assets - should fail because asset is frozen
        let result = check_assets_not_frozen(
            deps.as_mut().storage,
            querier_wrapper,
            &env,
            &config,
            &assets,
        );

        assert!(result.is_err(), "Should return error when asset is frozen");
        match result.unwrap_err() {
            ContractError::CustomError { val } => {
                assert!(
                    val.contains("frozen"),
                    "Error message should mention frozen asset"
                );
                assert!(
                    val.contains("debt increases"),
                    "Error message should mention debt increases are disabled"
                );
            }
            _ => panic!("Expected CustomError with frozen message"),
        }
    }

    #[test]
    fn test_unfrozen_asset_allows_operations() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let asset_denom = "debit".to_string();
        let threshold = Decimal::percent(10);

        // Setup historical prices at $1.00
        setup_storage_with_history(deps.as_mut().storage, &asset_denom, vec!["1.0", "1.0", "1.0", "1.0", "1.0"]);

        // Asset is not frozen - price is normal
        let normal_price = Decimal::from_str("1.05").unwrap(); // 5% increase, within threshold
        check_and_update_circuit_breaker(
            deps.as_mut().storage,
            &env,
            asset_denom.clone(),
            normal_price,
            threshold,
        )
        .unwrap();

        // Verify it's not frozen
        let breaker = ASSET_CIRCUIT_BREAKERS
            .may_load(deps.as_ref().storage, asset_denom.clone())
            .unwrap();
        if let Some(breaker) = breaker {
            assert!(!breaker.frozen, "Asset should not be frozen");
        }

        // Setup config with oracle
        let config = Config {
            owner: Addr::unchecked("owner"),
            staking_contract: None,
            chain_proxy: None,
            debt_auction: None,
            oracle_contract: Some(Addr::unchecked("oracle")),
            liquidity_contract: None,
            discounts_contract: None,
            ltv_disco: Addr::unchecked("ltv_disco"),
            liq_fee: Decimal::percent(5),
            collateral_twap_timeframe: 60,
            credit_twap_timeframe: 60,
            oracle_time_limit: 300,
            cpc_multiplier: Decimal::percent(1),
            debt_minimum: cosmwasm_std::Uint128::zero(),
            base_debt_cap_multiplier: cosmwasm_std::Uint128::from(100u128),
            rate_slope_multiplier: Decimal::percent(1),
            affiliate_fee_max: Decimal::percent(1),
            revenue_distributor: None,
            skip_credit_price_accrual: false,
            liquidation_stat_limit: 1000,
            transmuter_addr: None,
            irm_config: IRMConfig {
                adjustment_speed: Decimal::from_atomics(50u128, 0).unwrap(),
                min_adaptive_rate: Decimal::permille(1),
                max_adaptive_rate: Decimal::from_atomics(1u128, 0).unwrap(),
            },
            points_contract: None,
        };

        // Create mock querier that returns normal price
        let querier = mock_querier_with_prices(vec![Decimal::from_str("1.05").unwrap()]);
        let querier_wrapper = QuerierWrapper::new(&querier);

        // Create assets to check
        let assets = vec![cAsset {
            asset: Asset {
                info: AssetInfo::NativeToken {
                    denom: asset_denom.clone(),
                },
                amount: cosmwasm_std::Uint128::from(1000u128),
            },
            max_borrow_LTV: Decimal::percent(75),
            max_LTV: Decimal::percent(85),
            pool_info: None,
            rate_index: Decimal::one(),
            peg_rate_index: Decimal::one(),
            force_redemptions: None,
        }];

        // Attempt to check assets - should succeed because asset is not frozen
        let result = check_assets_not_frozen(
            deps.as_mut().storage,
            querier_wrapper,
            &env,
            &config,
            &assets,
        );

        assert!(result.is_ok(), "Should allow operations when asset is not frozen");
    }
}
