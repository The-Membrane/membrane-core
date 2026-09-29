#![allow(non_snake_case)]
use cosmwasm_std::{Addr, Coin, Decimal, Uint128};
use cosmwasm_schema::cw_serde;

use crate::types::Asset;

// ============================================================================
// Config
// ============================================================================

#[cw_serde]
pub struct Config {
    /// Contract owner
    pub owner: Addr,
    /// Collateral contract address
    pub collateral_contract: Addr,
    /// Debt contract address
    pub debt_contract: Addr,
    /// Liquidation Queue contract address
    pub liq_queue: Option<Addr>,
    /// Chain Proxy contract address (for sell wall)
    pub chain_proxy: Option<Addr>,
    /// Debt Auction contract address (for bad debt MBRN auctions)
    pub debt_auction: Option<Addr>,
    /// Staking contract address (for fee routing)
    pub staking_contract: Option<Addr>,
    /// Liquidation fee as percent
    pub liq_fee: Decimal,
    /// Credit token denom (used for LQ CollateralForDebt query).
    /// When set, enables precise LQ transfer amounts to prevent over-transfer.
    pub credit_denom: Option<String>,
    /// Delay in seconds before a flagged position can be liquidated. Default: 28800 (8hr)
    pub liquidation_delay: u64,
}

#[cw_serde]
pub struct UpdateConfig {
    pub owner: Option<String>,
    pub collateral_contract: Option<String>,
    pub debt_contract: Option<String>,
    pub liq_queue: Option<String>,
    pub chain_proxy: Option<String>,
    pub debt_auction: Option<String>,
    pub staking_contract: Option<String>,
    pub liq_fee: Option<Decimal>,
    pub credit_denom: Option<String>,
    pub liquidation_delay: Option<u64>,
}

// ============================================================================
// Execute Messages
// ============================================================================

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub collateral_contract: String,
    pub debt_contract: String,
    pub liq_queue: Option<String>,
    pub chain_proxy: Option<String>,
    pub debt_auction: Option<String>,
    pub staking_contract: Option<String>,
    pub liq_fee: Decimal,
    pub credit_denom: Option<String>,
    pub liquidation_delay: Option<u64>,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Update contract config
    UpdateConfig(UpdateConfig),

    /// Liquidate an insolvent position.
    /// Orchestrates: accrue debt → assess collateral → liq queue → sell wall → debt update → claim update → bad debt check.
    Liquidate {
        position_id: Uint128,
        position_owner: String,
    },

    // === Callbacks ===
    Callback(CallbackMsg),
}

#[cw_serde]
pub enum CallbackMsg {
    /// Bad debt check after liquidation completes
    BadDebtCheck {
        position_id: Uint128,
        position_owner: Addr,
    },
}

// ============================================================================
// Query Messages
// ============================================================================

#[cw_serde]
pub enum QueryMsg {
    /// Returns the contract's config
    Config {},
    /// Simulate a liquidation to estimate costs
    SimulateLiquidation {
        position_id: Uint128,
        position_owner: String,
    },
    /// Returns liquidation history
    GetLiquidationStats {
        start_after: Option<u32>,
        limit: Option<u32>,
    },
    /// Returns the liquidation timer for a specific position
    GetLiquidationTimer {
        position_id: Uint128,
        position_owner: String,
    },
    /// Returns the timestamp of the most recent liquidation event (or timer start)
    /// involving a specific collateral asset. Used by Disco to enforce unstaking lockout.
    GetLastLiquidationTimestampForAsset {
        asset: String,
    },
}

// ============================================================================
// Response types
// ============================================================================

#[cw_serde]
pub struct SimulateLiquidationResponse {
    /// Whether the position is currently insolvent
    pub is_insolvent: bool,
    /// Current LTV of the position
    pub current_ltv: Decimal,
    /// Amount of debt that would be repaid
    pub repay_amount: Uint128,
    /// Caller fee in collateral value
    pub caller_fee: Decimal,
    /// Protocol fee in collateral value
    pub protocol_fee: Decimal,
    /// Whether the position has an active (non-expired) liquidation timer
    pub has_active_timer: bool,
    /// When the active timer expires (block time in seconds)
    pub timer_expiry: Option<u64>,
    /// Whether calling liquidate would start a timer (vs immediate liquidation)
    pub would_start_timer: bool,
}

#[cw_serde]
pub struct LiquidationStatsResponse {
    pub events: Vec<LiquidationEventResponse>,
    pub total_count: u32,
}

/// What happened to a position that entered the liquidation flow.
#[cw_serde]
pub enum LiquidationOutcomeResponse {
    /// Timer started — first call, position under immediate threshold
    TimerStarted,
    /// Position had timer but became solvent (saved by the delay)
    SavedByDelay,
    /// Position broke the immediate threshold while timer was active
    BrokeWindow,
    /// Timer expired, position still insolvent — liquidated after delay
    DelayExpired,
    /// Over immediate threshold on first call — liquidated with no delay
    Immediate,
}

/// Snapshot of a collateral asset in the position at event time.
#[cw_serde]
pub struct PositionAssetSnapshotResponse {
    pub denom: String,
    pub amount: Uint128,
    pub value: Decimal,
    pub max_ltv: Decimal,
}

#[cw_serde]
pub struct LiquidationEventResponse {
    pub position_id: Uint128,
    pub position_owner: String,
    pub caller: String,
    pub total_debt: Uint128,
    pub total_repaid: Decimal,
    pub bad_debt: Uint128,
    pub caller_fee_value: Decimal,
    pub protocol_fee_value: Decimal,
    pub timestamp: u64,
    /// Liquidation outcome category
    pub outcome: LiquidationOutcomeResponse,
    /// LTV at the time of the event
    pub current_ltv: Decimal,
    /// When the timer was started (if applicable)
    pub timer_started_at: Option<u64>,
    /// Collateral assets in the position
    pub position_assets: Vec<PositionAssetSnapshotResponse>,
}

// ============================================================================
// Liquidation Timer
// ============================================================================

/// Per-position liquidation timer. Started on first liquidation call
/// when the position is insolvent but under the immediate-liquidation threshold.
#[cw_serde]
pub struct LiquidationTimer {
    /// Block time (seconds) when the timer was started
    pub start_time: u64,
    /// The caller who started the timer
    pub caller: Addr,
}

#[cw_serde]
pub struct LiquidationTimerResponse {
    /// Whether a timer exists for this position
    pub has_timer: bool,
    /// When the timer was started (block time in seconds)
    pub start_time: Option<u64>,
    /// When the timer expires (block time in seconds)
    pub expiry_time: Option<u64>,
    /// Address of the caller who started the timer
    pub caller: Option<String>,
}

#[cw_serde]
pub struct LastLiquidationTimestampForAssetResponse {
    /// The most recent liquidation-related timestamp for this asset.
    /// max(event.timestamp, event.timer_started_at) across matching events.
    /// None if no liquidation events have occurred for this asset.
    pub timestamp: Option<u64>,
}

// ============================================================================
// Liquidation propagation state (internal, used for SubMsg chain)
// ============================================================================

/// Carries state through the liquidation SubMsg chain.
/// Stored temporarily in the Liquidation contract during a liquidation tx.
#[cw_serde]
pub struct LiquidationPropagation {
    /// Per-asset repayment amounts
    pub per_asset_repayment: Vec<Decimal>,
    /// Liq queue repayment
    pub liq_queue_repayment: Decimal,
    /// Protocol fee value (in collateral value terms)
    pub protocol_fee_value: Decimal,
    /// Collateral ratios (don't change during liquidation)
    pub cAsset_ratios: Vec<Decimal>,
    /// Collateral prices
    pub cAsset_prices: Vec<crate::oracle::PriceResponse>,
    /// Collateral assets in position
    pub collateral_assets: Vec<crate::types::cAsset>,
    /// Total debt of position
    pub total_debt: Uint128,
    /// Credit price
    pub credit_price: crate::oracle::PriceResponse,
    /// Caller fee value paid so far
    pub caller_fee_value_paid: Decimal,
    /// Per-asset caller fee amounts in collateral tokens (computed in Step 2, used in Step 9)
    pub caller_fee_assets: Vec<Coin>,
    /// Per-asset protocol fee amounts in collateral tokens (stub for future routing)
    pub protocol_fee_assets: Vec<Coin>,
    /// Per-asset sell wall amounts (computed in build_sell_wall_submsg, used in Step 6 claim reduction)
    pub sell_wall_amounts: Vec<crate::types::Asset>,
    /// Total repaid so far
    pub total_repaid: Decimal,
    /// Amount of CDT being recalled from deployment venues.
    /// CDP parity: return value of get_deployable_venues_user_repay_amount().
    /// Subtracted from leftover credit in build_sell_wall_submsg so less collateral is sold.
    pub venue_recall_amount: Uint128,
    /// Position owner
    pub position_owner: Addr,
    /// Position ID
    pub position_id: Uint128,
    /// Liquidation caller (info.sender from liquidate())
    pub caller: Addr,
    /// If true, this is a timer-start call. After fee extraction, return early
    /// instead of proceeding to liq queue/sell wall.
    pub timer_start_only: bool,
    /// Liquidation outcome category (set in Step 2, consumed in Step 7)
    pub liquidation_outcome: Option<String>,
    /// If a timer was involved, when it was originally started
    pub timer_started_at: Option<u64>,
    /// LTV at the time of the collateral assessment
    pub current_ltv: Decimal,
}

#[cw_serde]
pub struct MigrateMsg {}
