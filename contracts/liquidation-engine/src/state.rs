use cosmwasm_std::{Addr, Coin, Decimal, Uint128};
use cosmwasm_schema::cw_serde;
use cw_storage_plus::{Item, Map};

use membrane::liquidation_engine::{Config, LiquidationPropagation, LiquidationTimer};

// ============================================================================
// Core State
// ============================================================================

pub const CONFIG: Item<Config> = Item::new("config");

// ============================================================================
// Liquidation Propagation (temporary, used during SubMsg chains)
// ============================================================================

/// Carries state through the multi-step liquidation SubMsg chain.
/// Stored temporarily during a liquidation transaction.
pub const LIQUIDATION_PROPAGATION: Item<LiquidationPropagation> =
    Item::new("liquidation_propagation");

// ============================================================================
// Liquidation Stats
// ============================================================================

pub const LIQUIDATION_STATS: Item<Vec<LiquidationEvent>> =
    Item::new("liquidation_stats");

pub const MAX_LIQUIDATION_STATS: usize = 500;

/// What happened to a position that entered the liquidation flow.
#[cw_serde]
pub enum LiquidationOutcome {
    /// Timer started — position is insolvent but under the immediate threshold.
    /// First call in the delay flow; no collateral is sold yet.
    TimerStarted,
    /// Position had an active timer but became solvent before anyone called
    /// liquidate again (detected on the next liquidate attempt).
    SavedByDelay,
    /// Position exceeded the immediate-liquidation threshold while a timer was
    /// active — the protective window was breached and liquidation proceeded
    /// immediately.
    BrokeWindow,
    /// Timer expired normally and the position was still insolvent — liquidated
    /// after the full delay period.
    DelayExpired,
    /// Position was over the immediate threshold on the first call (no prior
    /// timer) — liquidated immediately with no delay.
    Immediate,
}

/// Simplified snapshot of a single collateral asset in the position at the time
/// of the event, for off-chain analysis.
#[cw_serde]
pub struct PositionAssetSnapshot {
    /// The collateral token (native denom or cw20 addr)
    pub denom: String,
    /// Amount held in the position
    pub amount: Uint128,
    /// USD-equivalent value at the time of the event
    pub value: Decimal,
    /// The asset's max LTV (liquidation threshold)
    pub max_ltv: Decimal,
}

#[cw_serde]
pub struct LiquidationEvent {
    pub position_id: Uint128,
    pub position_owner: Addr,
    pub caller: Addr,
    pub total_debt: Uint128,
    pub total_repaid: Decimal,
    pub bad_debt: Uint128,
    pub caller_fee_value: Decimal,
    pub protocol_fee_value: Decimal,
    pub timestamp: u64,
    /// What happened to this position in the liquidation flow.
    pub outcome: LiquidationOutcome,
    /// LTV at the time of the event
    pub current_ltv: Decimal,
    /// If a timer was involved, when it was originally started (seconds)
    pub timer_started_at: Option<u64>,
    /// Collateral assets in the position at the time of the event
    pub position_assets: Vec<PositionAssetSnapshot>,
}

// ============================================================================
// Liquidation Timers (per-position delay before liquidation can execute)
// ============================================================================

/// Per-position liquidation timers. Key: (position_owner, position_id as string)
pub const LIQUIDATION_TIMERS: Map<(&str, &str), LiquidationTimer> = Map::new("liq_timers");

// ============================================================================
// Reply IDs
// ============================================================================

/// Reply from Debt.AccrueAndReturnDebt
pub const DEBT_ACCRUE_REPLY_ID: u64 = 1u64;
/// Reply from Collateral.RefreshAndAssess
pub const COLLATERAL_ASSESS_REPLY_ID: u64 = 2u64;
/// Reply from Collateral.AuthorizedTransfer (liq queue)
pub const LIQ_QUEUE_TRANSFER_REPLY_ID: u64 = 3u64;
/// Reply from Liq Queue
pub const LIQ_QUEUE_REPLY_ID: u64 = 4u64;
/// Reply from Collateral.AuthorizedSellCollateral (sell wall)
pub const SELL_WALL_REPLY_ID: u64 = 5u64;
/// Reply from Debt.LiquidationDebtUpdate
pub const DEBT_UPDATE_REPLY_ID: u64 = 6u64;
/// Reply from Collateral.LiquidationClaimUpdate
pub const CLAIM_UPDATE_REPLY_ID: u64 = 7u64;
/// Reply from BadDebtCheck callback (error-only, no-op handler)
pub const BAD_DEBT_REPLY_ID: u64 = 8u64;
/// Reply from Collateral.AuthorizedTransfer (fee transfer to liq engine)
pub const FEE_TRANSFER_REPLY_ID: u64 = 9u64;
