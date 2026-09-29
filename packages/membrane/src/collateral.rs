#![allow(non_snake_case)]
use cosmwasm_std::{Addr, Decimal, Uint128};
use cosmwasm_schema::cw_serde;

use crate::oracle::PriceResponse;
use crate::types::{
    cAsset, Asset, AssetInfo, SupplyCap, MultiAssetSupplyCap,
    UserInfo, PoolType, TWAPPoolInfo, IRMConfig,
};

// ============================================================================
// Config
// ============================================================================

#[cw_serde]
pub struct Config {
    /// Contract owner
    pub owner: Addr,
    /// Oracle contract address
    pub oracle_contract: Option<Addr>,
    /// Chain Proxy contract address (for swaps)
    pub chain_proxy: Option<Addr>,
    /// LTV Disco contract address (for dynamic LTV updates)
    pub ltv_disco: Addr,
    /// Debt contract address (authorized caller for withdrawals, etc.)
    pub debt_contract: Option<Addr>,
    /// Liquidation Engine contract address (authorized caller)
    pub liquidation_contract: Option<Addr>,
    /// Collateral TWAP time frame in minutes
    pub collateral_twap_timeframe: u64,
    /// Seconds until oracle failure is accepted
    pub oracle_time_limit: u64,
    /// Proportional gain for upward LTV accrual (default: 5% per day)
    pub ltv_upward_kp: Decimal,
    /// Period for downward LTV shifts in seconds (default: 604,800 = 1 week)
    pub ltv_downward_period: u64,
    /// Fraction of remaining gap applied per downward period (default: 0.40 = 40%)
    pub ltv_downward_decay_rate: Decimal,
    /// Fixed distance between max_LTV and max_borrow_LTV (default: 0.10 = 10%)
    /// borrow_LTV = max_LTV - ltv_borrow_distance
    pub ltv_borrow_distance: Decimal,
}

#[cw_serde]
pub struct UpdateConfig {
    pub owner: Option<String>,
    pub oracle_contract: Option<String>,
    pub chain_proxy: Option<String>,
    pub ltv_disco: Option<String>,
    pub debt_contract: Option<String>,
    pub liquidation_contract: Option<String>,
    pub collateral_twap_timeframe: Option<u64>,
    pub oracle_time_limit: Option<u64>,
    pub ltv_upward_kp: Option<Decimal>,
    pub ltv_downward_period: Option<u64>,
    pub ltv_downward_decay_rate: Option<Decimal>,
    pub ltv_borrow_distance: Option<Decimal>,
}

// ============================================================================
// Basket (Collateral side)
// ============================================================================

/// Collateral-side basket state.
/// Owns collateral_types (authoritative LTV values), supply caps, and position ID counter.
#[cw_serde]
pub struct BasketCollateral {
    /// Basket ID
    pub basket_id: Uint128,
    /// Position ID counter (collateral comes first, so ID allocation lives here)
    pub current_position_id: Uint128,
    /// Available collateral types with authoritative LTV values
    pub collateral_types: Vec<cAsset>,
    /// Per-asset supply caps
    pub collateral_supply_caps: Vec<SupplyCap>,
    /// Multi-asset supply caps
    pub multi_asset_supply_caps: Vec<MultiAssetSupplyCap>,
}

// ============================================================================
// Position (Collateral side)
// ============================================================================

/// Collateral-side position. Split from the monolithic Position struct.
#[cw_serde]
pub struct CollateralPosition {
    /// Position ID (shared with DebtPosition)
    pub position_id: Uint128,
    /// Collateral assets deposited
    pub collateral_assets: Vec<cAsset>,
}

// ============================================================================
// Cross-contract response types
// ============================================================================

/// Response data returned by RefreshAndAssess via reply attributes.
/// Used by Debt and Liquidation contracts to get fresh collateral data.
#[cw_serde]
pub struct CollateralAssessmentResponse {
    /// Collateral assets in the position
    pub collateral_assets: Vec<cAsset>,
    /// Value ratios per collateral asset (value_i / total_value)
    pub ratios: Vec<Decimal>,
    /// Fresh oracle prices per collateral asset
    pub prices: Vec<PriceResponse>,
    /// Weighted average max_LTV across all collateral
    pub avg_max_ltv: Decimal,
    /// Weighted average max_borrow_LTV across all collateral
    pub avg_max_borrow_ltv: Decimal,
    /// Weighted average max_threshold_to_delay across all collateral
    pub avg_max_threshold_to_delay: Decimal,
    /// Whether any collateral asset is frozen by circuit breaker
    pub any_frozen: bool,
    /// List of frozen asset denoms (empty if none frozen)
    pub frozen_assets: Vec<String>,
}

// ============================================================================
// Execute Messages
// ============================================================================

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub oracle_contract: Option<String>,
    pub chain_proxy: Option<String>,
    pub ltv_disco: String,
    pub debt_contract: Option<String>,
    pub liquidation_contract: Option<String>,
    pub collateral_twap_timeframe: u64,
    pub oracle_time_limit: u64,
    /// Initial basket creation
    pub create_basket: CreateBasket,
    /// Proportional gain for upward LTV accrual (default: 5% per day)
    pub ltv_upward_kp: Option<Decimal>,
    /// Period for downward LTV shifts in seconds (default: 604,800 = 1 week)
    pub ltv_downward_period: Option<u64>,
    /// Fraction of remaining gap applied per downward period (default: 0.40 = 40%)
    pub ltv_downward_decay_rate: Option<Decimal>,
    /// Fixed distance between max_LTV and max_borrow_LTV (default: 0.10 = 10%)
    pub ltv_borrow_distance: Option<Decimal>,
}

#[cw_serde]
pub struct CreateBasket {
    pub basket_id: Uint128,
    pub collateral_types: Vec<cAsset>,
    pub collateral_supply_caps: Vec<SupplyCap>,
    pub multi_asset_supply_caps: Vec<MultiAssetSupplyCap>,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Update contract config
    UpdateConfig(UpdateConfig),

    // === User operations ===

    /// Deposit collateral into a position (funds sent with msg)
    Deposit {
        position_id: Option<Uint128>,
        position_owner: Option<String>,
        affiliate_address: Option<String>,
    },
    /// Withdraw collateral from a position.
    /// Calls Debt.AccrueAndValidate via SubMsg, then processes on reply.
    Withdraw {
        position_id: Uint128,
        assets: Vec<Asset>,
        send_to: Option<String>,
    },

    // === Admin ===

    /// Edit basket collateral config
    EditBasket(EditBasket),
    /// Edit a cAsset's params (including LTV values)
    EditcAsset {
        asset: AssetInfo,
        max_borrow_LTV: Option<Decimal>,
        max_LTV: Option<Decimal>,
        max_threshold_to_delay: Option<Decimal>,
    },
    /// Collateral rate assurance check (self-callback)
    CollateralRateAssurance {
        collateral_denoms: Option<Vec<String>>,
    },
    /// Set frozen state (replicated, syncs to Debt contract)
    SetFrozen {
        frozen: bool,
    },

    // === Authorized calls from Debt contract ===

    /// Refresh oracle prices & return collateral assessment via reply attributes.
    /// Only callable by Debt or Liquidation contract.
    RefreshAndAssess {
        position_owner: String,
        position_id: Uint128,
    },
    /// Execute a withdrawal after Debt validates solvency.
    /// Only callable by Debt contract.
    AuthorizedWithdraw {
        position_id: Uint128,
        position_owner: String,
        assets: Vec<Asset>,
        send_to: String,
    },
    /// Sell collateral for ClosePosition / liquidation.
    /// Only callable by Debt or Liquidation contract.
    AuthorizedSellCollateral {
        position_id: Uint128,
        position_owner: String,
        max_spread: Decimal,
        send_proceeds_to: String,
        /// If provided, sell only these specific amounts instead of all remaining.
        /// Used by liquidation engine to sell only leftover_repayment worth (CDP parity).
        sell_amounts: Option<Vec<Asset>>,
    },
    /// Activate supply cap tracking when a position first takes on debt.
    /// Only callable by Debt contract.
    ActivateSupplyCaps {
        position_id: Uint128,
        position_owner: String,
    },
    /// Deactivate supply cap tracking when a position's debt reaches zero.
    /// Only callable by Debt contract.
    DeactivateSupplyCaps {
        position_id: Uint128,
        position_owner: String,
    },

    // === Authorized calls from Liquidation contract ===

    /// Update collateral claims after liquidation.
    /// Only callable by Liquidation contract.
    LiquidationClaimUpdate {
        position_id: Uint128,
        position_owner: String,
        collateral_reductions: Vec<Asset>,
    },
    /// Transfer collateral to the liq queue during liquidation.
    /// Only callable by Liquidation contract.
    AuthorizedTransfer {
        position_id: Uint128,
        position_owner: String,
        assets: Vec<Asset>,
        recipient: String,
    },

    // === Dynamic LTV ===

    /// Update basket LTVs based on Disco average LTVs. Permissionless.
    UpdateBasketLTVs {},

    // === Callbacks ===
    Callback(CallbackMsg),
}

#[cw_serde]
pub enum CallbackMsg {
    /// Post-withdrawal supply cap update
    WithdrawCallback {
        position_id: Uint128,
        position_owner: String,
    },
}

#[cw_serde]
pub struct EditBasket {
    /// Add new cAsset
    pub added_cAsset: Option<cAsset>,
    /// Supply caps for each collateral
    pub collateral_supply_caps: Option<Vec<SupplyCap>>,
    /// Supply caps for asset groups
    pub multi_asset_supply_caps: Option<Vec<MultiAssetSupplyCap>>,
}

// ============================================================================
// Query Messages
// ============================================================================

#[cw_serde]
pub enum QueryMsg {
    /// Returns the contract's config
    Config {},
    /// Returns collateral positions
    GetCollateralPositions {
        start_after: Option<String>,
        limit: Option<u32>,
        user_info: Option<UserInfo>,
        user: Option<String>,
    },
    /// Returns the collateral basket info
    GetBasket {},
    /// Returns historical oracle prices for an asset
    GetHistoricalOraclePrices { asset: String },
    /// Check if assets are in volatile windows
    CheckVolatilityWindow { assets: Vec<String> },
    /// Simulate liquidation market sales for slippage estimation
    SimulateLiquidation {
        collateral_to_sell: Vec<cosmwasm_std::Coin>,
        target_denom: String,
    },
    /// Get circuit breaker status for assets
    GetCircuitBreakerStatus { assets: Vec<String> },
    /// Get frozen state
    GetFrozen {},
    /// Get position collateral value and LTV info from stored prices.
    /// Used by Debt contract's fulfill_intents to compute deployment venue LTV allocations.
    GetPositionValue {
        position_owner: String,
        position_id: Uint128,
    },
    /// Get historical LTV snapshots for an asset
    GetHistoricalLTVs {
        asset_denom: String,
        start_time: Option<u64>,
        end_time: Option<u64>,
        limit: Option<u32>,
    },
    /// Get pending LTV shift info for an asset
    GetLTVShiftInfo {
        asset_denom: String,
    },
}

// ============================================================================
// Response types
// ============================================================================

#[cw_serde]
pub struct CollateralPositionsResponse {
    pub user: String,
    pub positions: Vec<CollateralPositionResponse>,
}

#[cw_serde]
pub struct CollateralPositionResponse {
    pub position_id: Uint128,
    pub collateral_assets: Vec<cAsset>,
    pub cAsset_ratios: Vec<Decimal>,
}

/// Response for GetPositionValue query.
/// Returns the total value of a position's collateral using stored prices,
/// along with weighted average LTV limits.
#[cw_serde]
pub struct PositionValueResponse {
    pub total_value: Decimal,
    pub avg_max_borrow_ltv: Decimal,
    pub avg_max_ltv: Decimal,
    pub avg_max_threshold_to_delay: Decimal,
}

// ============================================================================
// LTV Update response types
// ============================================================================

/// Historical LTV snapshot for an asset
#[cw_serde]
pub struct LTVSnapshot {
    pub timestamp: u64,
    pub max_ltv: Decimal,
    pub max_borrow_ltv: Decimal,
    /// The current (active) liquidation LTV at the time of this snapshot
    pub current_ltv: Decimal,
    /// The pending (staged) liquidation LTV at the time of this snapshot, if any
    pub pending_ltv: Option<Decimal>,
    /// The current borrow LTV at the time of this snapshot
    pub current_borrow_ltv: Decimal,
}

/// Response for GetHistoricalLTVs query
#[cw_serde]
pub struct HistoricalLTVResponse {
    pub asset_denom: String,
    pub snapshots: Vec<LTVSnapshot>,
}

/// Response for GetLTVShiftInfo query
#[cw_serde]
pub struct LTVShiftInfoResponse {
    /// Unix timestamp when next shift can be applied (0 if no staged shift)
    pub next_shift_time: u64,
    /// Seconds remaining until shift can be applied (0 if ready or no staged shift)
    pub time_until_shift: u64,
    /// Staged downward max_LTV value (None if no pending shift)
    pub staged_max_ltv: Option<Decimal>,
    /// Whether deposits are blocked because Disco returned 0 for this asset
    pub disco_blocked: bool,
}

#[cw_serde]
pub struct MigrateMsg {}
