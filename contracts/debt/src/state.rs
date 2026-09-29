use cosmwasm_std::{Addr, Decimal, Uint128};
use cosmwasm_schema::cw_serde;
use cw_storage_plus::{Item, Map};

use membrane::debt::{BasketDebt, Config, DebtPosition};
use membrane::types::{
    AffiliateData, DebtSplit, Rates, StringEntry, UserDeploymentIntents, UserInfo,
};

// ============================================================================
// Core State
// ============================================================================

pub const CONFIG: Item<Config> = Item::new("config");
pub const BASKET: Item<BasketDebt> = Item::new("basket");
pub const POSITIONS: Map<Addr, Vec<DebtPosition>> = Map::new("positions");
pub const FROZEN: Item<bool> = Item::new("frozen");

// ============================================================================
// Rates & Interest
// ============================================================================

pub const RATES: Item<Rates> = Item::new("rates");
pub const CDT_SUPPLY: Item<Uint128> = Item::new("cdt_supply");

/// Per-asset basket rate indices (maps asset_denom → rate indices).
/// CDP parity: maps to basket.collateral_types[i].rate_index / peg_rate_index
pub const BASKET_RATE_INDICES: Map<String, BasketRateIndex> = Map::new("basket_rate_indices");

#[cw_serde]
pub struct BasketRateIndex {
    pub rate_index: Decimal,
    pub peg_rate_index: Decimal,
}

// ============================================================================
// Historical Data
// ============================================================================

pub const HISTORICAL_INTEREST_RATES: Map<String, Vec<RateTimestamp>> = Map::new("historical_rates");

#[cw_serde]
pub struct RateTimestamp {
    pub rate: String,
    pub timestamp: u64,
}

// ============================================================================
// Affiliates
// ============================================================================

/// Map<position_id, AffiliateData>
pub const AFFILIATES: Map<u128, AffiliateData> = Map::new("affiliates");

// ============================================================================
// Liquidation Stats
// ============================================================================

pub const LIQUIDATION_STATS: Item<Vec<LiquidationEvent>> = Item::new("liquidation_stats");

#[cw_serde]
pub struct LiquidationEvent {
    pub position_id: Uint128,
    pub position_owner: String,
    pub timestamp: u64,
    pub debt_repaid: Uint128,
    pub collateral_liquidated: Decimal,
}

// ============================================================================
// Deployment Venues & Intents
// ============================================================================

pub const ACTIVE_DEPLOYMENT_VENUES: Item<Vec<String>> =
    Item::new("active_deployment_venues");
pub const USER_INTENTS: Map<Addr, UserDeploymentIntents> = Map::new("user_intents");

/// Update the active deployment venues list (add/remove venue addresses).
///
/// CDP parity: maps to cdp/contract.rs::set_active_deployment_venues()
pub fn set_active_deployment_venues(
    storage: &mut dyn cosmwasm_std::Storage,
    venues: Vec<StringEntry>,
) -> cosmwasm_std::StdResult<()> {
    let mut active_deployment_venues = ACTIVE_DEPLOYMENT_VENUES
        .may_load(storage)?
        .unwrap_or_default();

    for venue in venues {
        if venue.remove {
            active_deployment_venues.retain(|v| v != &venue.entry);
        } else {
            if !active_deployment_venues.contains(&venue.entry) {
                active_deployment_venues.push(venue.entry);
            }
        }
    }

    ACTIVE_DEPLOYMENT_VENUES.save(storage, &active_deployment_venues)?;
    Ok(())
}

// ============================================================================
// Reply Propagation State (temporary, used during SubMsg chains)
// ============================================================================

pub const ACCRUE_VALIDATE_PROPAGATION: Item<AccrueValidatePropagation> =
    Item::new("accrue_validate_propagation");
pub const CLOSE_POSITION_PROPAGATION: Item<ClosePositionPropagation> =
    Item::new("close_position_propagation");
pub const DEPLOYABLE_VENUE_PROPAGATION: Item<DeployableVenuePropagation> =
    Item::new("deployable_venue_propagation");
pub const INCREASE_DEBT_PROPAGATION: Item<IncreaseDebtPropagation> =
    Item::new("increase_debt_propagation");
pub const VENUE_RECALL_PROPAGATION: Item<VenueRecallPropagation> =
    Item::new("venue_recall_propagation");

#[cw_serde]
pub struct AccrueValidatePropagation {
    pub position_owner: String,
    pub position_id: Uint128,
    pub remaining_collateral_value: Decimal,
    pub avg_max_borrow_ltv: Decimal,
    pub avg_max_ltv: Decimal,
}

#[cw_serde]
pub struct ClosePositionPropagation {
    pub position_info: UserInfo,
    pub close_percentage: Decimal,
    pub max_spread: Decimal,
    pub send_to: Option<String>,
}

#[cw_serde]
pub struct DeployableVenuePropagation {
    pub position_info: UserInfo,
    pub venue_address: String,
    pub deployed_amount: Uint128,
}

#[cw_serde]
pub struct VenueRecallPropagation {
    pub position_owner: String,
    pub position_id: Uint128,
    /// Venue addresses in SubMsg order; front is popped on each reply.
    pub venues: Vec<String>,
}

#[cw_serde]
pub struct IncreaseDebtPropagation {
    pub position_owner: String,
    pub position_id: Uint128,
    pub amount: Option<Uint128>,
    pub ltv: Option<Decimal>,
    pub mint_to_addr: Option<String>,
    pub peg_debt: bool,
    pub debt_split: Option<DebtSplit>,
}

// ============================================================================
// Reply IDs
// ============================================================================

pub const COLLATERAL_REFRESH_REPLY_ID: u64 = 1u64;
pub const CLOSE_POSITION_SELL_REPLY_ID: u64 = 2u64;
pub const CLOSE_POSITION_WITHDRAW_REPLY_ID: u64 = 3u64;
pub const DEPLOYABLE_VENUE_REPLY_ID: u64 = 5u64;
pub const REVENUE_REPLY_ID: u64 = 6u64;
/// Reply ID for venue recall during liquidation (reply_always).
/// Uses VENUE_RECALL_PROPAGATION to track which venues are pending,
/// so the reply handler can pop the queue and set failed_liquidation = true on failure.
pub const VENUE_RECALL_REPLY_ID: u64 = 7u64;

// ============================================================================
// Constants
// ============================================================================

pub const MAX_RATE_HISTORY_ENTRIES: usize = 365;

// ============================================================================
// Helper Functions
// ============================================================================

/// Returns DebtPosition & index in user's list
pub fn get_target_position(
    storage: &dyn cosmwasm_std::Storage,
    valid_position_owner: Addr,
    position_id: Uint128,
) -> Result<(usize, DebtPosition), crate::ContractError> {
    let positions: Vec<DebtPosition> = match POSITIONS.load(storage, valid_position_owner) {
        Err(_) => return Err(crate::ContractError::NoUserPositions {}),
        Ok(positions) => positions,
    };

    match positions
        .into_iter()
        .enumerate()
        .find(|(_i, x)| x.position_id == position_id)
    {
        Some(position) => Ok(position),
        None => Err(crate::ContractError::NonExistentPosition { id: position_id }),
    }
}

/// Replace DebtPosition data in state
pub fn update_position(
    storage: &mut dyn cosmwasm_std::Storage,
    valid_position_owner: Addr,
    new_position: DebtPosition,
) -> cosmwasm_std::StdResult<()> {
    POSITIONS.update(
        storage,
        valid_position_owner,
        |old_positions| -> cosmwasm_std::StdResult<Vec<DebtPosition>> {
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
                }
                None => Err(cosmwasm_std::StdError::generic_err("Invalid position owner")),
            }
        },
    )?;
    Ok(())
}

/// Save a rate snapshot to historical interest rates.
/// CDP parity: maps to cdp/state.rs::update_historical_interest_rates()
pub fn update_historical_interest_rates(
    storage: &mut dyn cosmwasm_std::Storage,
    env: cosmwasm_std::Env,
    asset_info_string: String,
    rate: Decimal,
) -> Result<(), crate::ContractError> {
    let mut history = HISTORICAL_INTEREST_RATES
        .may_load(storage, asset_info_string.clone())?
        .unwrap_or_default();

    history.push(RateTimestamp {
        rate: rate.to_string(),
        timestamp: env.block.time.seconds(),
    });

    // Trim to max entries
    if history.len() > MAX_RATE_HISTORY_ENTRIES {
        let excess = history.len() - MAX_RATE_HISTORY_ENTRIES;
        history.drain(..excess);
    }

    HISTORICAL_INTEREST_RATES.save(storage, asset_info_string, &history)?;
    Ok(())
}

/// Get total debt across all segments for a DebtPosition
pub fn get_total_debt(position: &DebtPosition) -> Uint128 {
    let regular: Uint128 = position
        .rate_segments
        .iter()
        .map(|s| s.amount)
        .sum();
    let peg: Uint128 = position
        .peg_rate_segments
        .iter()
        .map(|s| s.amount)
        .sum();
    regular + peg
}
