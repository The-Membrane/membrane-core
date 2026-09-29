use cosmwasm_std::{Addr, Decimal, Env, StdResult, Storage, Uint128};
use cosmwasm_schema::cw_serde;
use cw_storage_plus::{Item, Map};

use membrane::collateral::{BasketCollateral, CollateralPosition, Config, LTVSnapshot};
use membrane::oracle::PriceResponse;
use membrane::types::{Asset, UserInfo};

// ============================================================================
// Core State
// ============================================================================

pub const CONFIG: Item<Config> = Item::new("config");
pub const BASKET: Item<BasketCollateral> = Item::new("basket");
pub const POSITIONS: Map<Addr, Vec<CollateralPosition>> = Map::new("positions");
pub const FROZEN: Item<bool> = Item::new("frozen");

// ============================================================================
// Price Tracking (written by get_cAsset_ratios - mutable operations)
// ============================================================================

pub const VOLATILITY: Map<String, CollateralVolatility> = Map::new("volatility");
pub const STORED_PRICES: Map<String, StoredPrice> = Map::new("stored_prices");

/// Per-asset volatility tracking
#[cw_serde]
pub struct CollateralVolatility {
    pub index: Decimal,
    /// Speed of volatility (price_change / time_elapsed) - used for index calculation
    pub volatility_list: Vec<Decimal>,
    /// Raw price change percentages - used for comparative rate calculation
    pub raw_volatility_list: Vec<Decimal>,
}

/// Cached oracle price for an asset
#[cw_serde]
pub struct StoredPrice {
    pub price: PriceResponse,
    pub last_time_updated: u64,
}

// ============================================================================
// Historical Data
// ============================================================================

pub const HISTORICAL_ORACLE_PRICES: Map<String, Vec<PriceTimestamp>> = Map::new("historical_oracle");

#[cw_serde]
pub struct PriceTimestamp {
    pub price: String,
    pub timestamp: u64,
}

// ============================================================================
// Circuit Breaker
// ============================================================================

pub const ASSET_CIRCUIT_BREAKERS: Map<String, AssetCircuitBreaker> = Map::new("asset_circuit_breakers");

/// Per-asset circuit breaker configuration and state
#[cw_serde]
pub struct AssetCircuitBreaker {
    /// Is the asset currently frozen due to abnormal price moves?
    pub frozen: bool,
    /// Timestamp (in seconds) when the asset was frozen
    pub frozen_at: u64,
    /// Allowed price deviation from the reference price before freezing
    pub price_deviation_threshold: Decimal,
    /// Reference price used to measure deviation
    pub reference_price: Option<Decimal>,
}

// ============================================================================
// Collateral Rate Assurance
// ============================================================================

pub const COLLATERAL_RATE_ASSURANCE: Map<String, CollateralRateAssurance> = Map::new("collateral_rate_assurance");

#[cw_serde]
pub struct CollateralRateAssurance {
    pub collateral_denom: String,
    pub pre_collateral_per_one: Uint128,
}

/// Snapshot current collateral-per-state-unit ratios and create a self-callback
/// message to verify rates haven't changed after the operation completes.
///
/// CDP parity: maps to cdp/state.rs::create_collateral_rate_assurance()
pub fn create_collateral_rate_assurance(
    storage: &mut dyn cosmwasm_std::Storage,
    querier: cosmwasm_std::QuerierWrapper,
    env: &cosmwasm_std::Env,
    collateral_denoms: Vec<String>,
    basket: &membrane::collateral::BasketCollateral,
) -> cosmwasm_std::StdResult<Vec<cosmwasm_std::CosmosMsg>> {
    let mut valid_denoms = Vec::new();

    for denom in collateral_denoms {
        // Get current contract balance
        let current_balance = querier.query_balance(
            env.contract.address.to_string(),
            denom.clone(),
        )?.amount;

        // Get current supply from basket supply caps
        let current_supply = basket.collateral_supply_caps.iter()
            .find(|cap| cap.asset_info.to_string() == denom)
            .map(|cap| cap.current_supply)
            .unwrap_or(Uint128::zero());

        // Calculate collateral-per-state-unit: balance * 1_000_000 / supply
        let collateral_per_one = if current_supply.is_zero() {
            Uint128::zero()
        } else {
            current_balance.multiply_ratio(Uint128::new(1_000_000), current_supply)
        };

        COLLATERAL_RATE_ASSURANCE.save(storage, denom.clone(), &CollateralRateAssurance {
            collateral_denom: denom.clone(),
            pre_collateral_per_one: collateral_per_one,
        })?;

        valid_denoms.push(denom);
    }

    let mut msgs = Vec::new();
    if !valid_denoms.is_empty() {
        msgs.push(cosmwasm_std::CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: cosmwasm_std::to_json_binary(
                &membrane::collateral::ExecuteMsg::CollateralRateAssurance {
                    collateral_denoms: Some(valid_denoms),
                },
            )?,
            funds: vec![],
        }));
    }

    Ok(msgs)
}

// ============================================================================
// Freeze Timer
// ============================================================================

pub const FREEZE_TIMER: Item<Timer> = Item::new("freeze_timer");

#[cw_serde]
pub struct Timer {
    pub start_time: u64,
    pub end_time: u64,
}

// ============================================================================
// Reply Propagation State (temporary, used during SubMsg chains)
// ============================================================================

pub const WITHDRAW_PROPAGATION: Item<WithdrawPropagation> = Item::new("withdraw_propagation");
pub const CLOSE_POSITION_PROPAGATION: Item<ClosePositionPropagation> = Item::new("close_position_propagation");
pub const SELL_COLLATERAL_PROPAGATION: Item<SellCollateralPropagation> = Item::new("sell_collateral_propagation");

#[cw_serde]
pub struct WithdrawPropagation {
    pub positions_prev_collateral: Vec<Asset>,
    pub withdraw_amounts: Vec<Uint128>,
    pub contracts_prev_collateral_amount: Vec<Uint128>,
    pub position_info: UserInfo,
}

#[cw_serde]
pub struct ClosePositionPropagation {
    pub withdrawn_assets: Vec<Asset>,
    pub position_info: UserInfo,
    pub send_to: Option<String>,
}

#[cw_serde]
pub struct SellCollateralPropagation {
    pub collateral_sold: Vec<cosmwasm_std::Coin>,
    pub position_info: UserInfo,
}

// ============================================================================
// Reply IDs
// ============================================================================

pub const WITHDRAW_REPLY_ID: u64 = 4u64;
pub const SELL_COLLATERAL_REPLY_ID: u64 = 7u64;
pub const DEBT_ACCRUE_VALIDATE_REPLY_ID: u64 = 10u64;
pub const REFRESH_ASSESS_REPLY_ID: u64 = 11u64;

// ============================================================================
// Constants
// ============================================================================

pub const MAX_POSITIONS_PER_USER: usize = 9;
pub const MAX_ORACLE_ENTRIES: usize = 365;
pub const SECONDS_PER_DAY: u64 = 86400;
pub const MAX_LTV_HISTORY_ENTRIES: usize = 365;

// ============================================================================
// LTV Update Tracking
// ============================================================================

pub const LTV_UPDATE_TRACKERS: Map<String, LTVUpdateTracker> = Map::new("ltv_update_trackers");
pub const LTV_HISTORY: Map<String, Vec<LTVSnapshot>> = Map::new("ltv_history");

/// Admin-set baseline max_LTV per asset. Drift target when Disco returns 0.
/// Initialized from cAsset.max_LTV on first tracker creation; updated via EditcAsset.
pub const MANUAL_MAX_LTVS: Map<String, Decimal> = Map::new("manual_max_ltvs");

/// Per-asset flag: true when Disco returned 0 for this asset.
/// Blocks new deposits via supply cap breach logic in risk_engine.
pub const DISCO_BLOCKED: Map<String, bool> = Map::new("disco_blocked");

/// Per-asset tracking state for the dynamic LTV update state machine.
/// Tracks update timing and staged downward shift values.
#[cw_serde]
pub struct LTVUpdateTracker {
    /// Timestamp of last update call (used for both upward and downward timing)
    pub last_update_timestamp: u64,
    /// Staged downward max_LTV value (waiting for delay period to elapse)
    pub staged_max_ltv: Option<Decimal>,
    /// Timestamp when the staged downward value was first set (timer start)
    pub staged_timestamp: Option<u64>,
}

/// Record a historical LTV snapshot for an asset.
/// Only records one snapshot per day. Prunes to MAX_LTV_HISTORY_ENTRIES.
pub fn record_ltv_snapshot(
    storage: &mut dyn Storage,
    env: &Env,
    asset_denom: &str,
    max_ltv: Decimal,
    max_borrow_ltv: Decimal,
    current_ltv: Decimal,
    pending_ltv: Option<Decimal>,
    current_borrow_ltv: Decimal,
) -> StdResult<()> {
    let mut history = LTV_HISTORY
        .may_load(storage, asset_denom.to_string())?
        .unwrap_or_default();

    let current_time = env.block.time.seconds();

    // Only record if at least one day has passed since last entry
    if let Some(last) = history.last() {
        if current_time.saturating_sub(last.timestamp) < SECONDS_PER_DAY {
            return Ok(());
        }
    }

    history.push(LTVSnapshot {
        timestamp: current_time,
        max_ltv,
        max_borrow_ltv,
        current_ltv,
        pending_ltv,
        current_borrow_ltv,
    });

    // Prune oldest entries if over limit
    if history.len() > MAX_LTV_HISTORY_ENTRIES {
        let excess = history.len() - MAX_LTV_HISTORY_ENTRIES;
        history.drain(..excess);
    }

    LTV_HISTORY.save(storage, asset_denom.to_string(), &history)?;
    Ok(())
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Returns CollateralPosition & index in user's list
pub fn get_target_position(
    storage: &dyn cosmwasm_std::Storage,
    valid_position_owner: Addr,
    position_id: Uint128,
) -> Result<(usize, CollateralPosition), crate::ContractError> {
    let positions: Vec<CollateralPosition> = match POSITIONS.load(storage, valid_position_owner) {
        Err(_) => return Err(crate::ContractError::NoUserPositions {}),
        Ok(positions) => positions,
    };

    match positions.into_iter().enumerate().find(|(_i, x)| x.position_id == position_id) {
        Some(position) => Ok(position),
        None => Err(crate::ContractError::NonExistentPosition { id: position_id }),
    }
}

/// Replace CollateralPosition data in state
pub fn update_position(
    storage: &mut dyn cosmwasm_std::Storage,
    valid_position_owner: Addr,
    new_position: CollateralPosition,
) -> cosmwasm_std::StdResult<()> {
    POSITIONS.update(
        storage,
        valid_position_owner,
        |old_positions| -> cosmwasm_std::StdResult<Vec<CollateralPosition>> {
            match old_positions {
                Some(old_positions) => {
                    let new_positions = old_positions
                        .into_iter()
                        .map(|stored_position| {
                            if stored_position.position_id == new_position.position_id {
                                new_position.clone()
                            } else {
                                stored_position
                            }
                        })
                        .collect();
                    Ok(new_positions)
                },
                None => Err(cosmwasm_std::StdError::generic_err("Invalid position owner")),
            }
        },
    )?;
    Ok(())
}
