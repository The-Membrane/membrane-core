#![allow(non_snake_case)]
use cosmwasm_std::{Addr, Decimal, Uint128};
use cosmwasm_schema::cw_serde;

use crate::oracle::PriceResponse;
use crate::types::{
    cAsset, Asset, AssetInfo, RateSegment, DeploymentVenue, DeploymentIntent,
    UserDeploymentIntents, DebtSplit, CreditAssetBreakdown, PendingRevenue,
    Rates, IRMConfig, PoolType, TWAPPoolInfo, UserInfo, InsolventPosition,
    AffiliateData,
};

// ============================================================================
// Config
// ============================================================================

#[cw_serde]
pub struct Config {
    /// Contract owner
    pub owner: Addr,
    /// Collateral contract address
    pub collateral_contract: Option<Addr>,
    /// Liquidation Engine contract address
    pub liquidation_contract: Option<Addr>,
    /// Staking contract address
    pub staking_contract: Option<Addr>,
    /// Chain Proxy contract address
    pub chain_proxy: Option<Addr>,
    /// Debt auction contract address
    pub debt_auction: Option<Addr>,
    /// Oracle contract address
    pub oracle_contract: Option<Addr>,
    /// Liquidity Check contract address
    pub liquidity_contract: Option<Addr>,
    /// System Discounts contract address
    pub discounts_contract: Option<Addr>,
    /// Revenue Distributor contract address
    pub revenue_distributor: Option<Addr>,
    /// Transmuter contract address for peg_debt swaps
    pub transmuter_addr: Option<Addr>,
    /// Points contract address for management points
    pub points_contract: Option<Addr>,
    /// Liquidation fee as percent
    pub liq_fee: Decimal,
    /// Credit TWAP time frame in minutes
    pub credit_twap_timeframe: u64,
    /// Augment the rate of increase per % difference for the redemption rate
    pub cpc_multiplier: Decimal,
    /// Debt minimum value per position
    pub debt_minimum: Uint128,
    /// Debt minimum multiplier for base debt cap
    pub base_debt_cap_multiplier: Uint128,
    /// Interest rate 2nd Slope multiplier
    pub rate_slope_multiplier: Decimal,
    /// Affiliate fee max
    pub affiliate_fee_max: Decimal,
    /// Skip credit price accrual
    pub skip_credit_price_accrual: bool,
    /// Maximum number of liquidation stats stored
    pub liquidation_stat_limit: u64,
    /// AdaptiveCurveIRM configuration
    pub irm_config: IRMConfig,
    /// Acquisition contract address (authorized to set bump rate)
    pub acquisition_contract: Option<Addr>,
}

#[cw_serde]
pub struct UpdateConfig {
    pub owner: Option<String>,
    pub collateral_contract: Option<String>,
    pub liquidation_contract: Option<String>,
    pub staking_contract: Option<String>,
    pub chain_proxy: Option<String>,
    pub debt_auction: Option<String>,
    pub oracle_contract: Option<String>,
    pub liquidity_contract: Option<String>,
    pub discounts_contract: Option<String>,
    pub revenue_distributor: Option<String>,
    pub transmuter_addr: Option<String>,
    pub points_contract: Option<String>,
    pub liq_fee: Option<Decimal>,
    pub credit_twap_timeframe: Option<u64>,
    pub cpc_multiplier: Option<Decimal>,
    pub debt_minimum: Option<Uint128>,
    pub base_debt_cap_multiplier: Option<Uint128>,
    pub rate_slope_multiplier: Option<Decimal>,
    pub affiliate_fee_max: Option<Decimal>,
    pub skip_credit_price_accrual: Option<bool>,
    pub liquidation_stat_limit: Option<u64>,
    pub irm_config: Option<IRMConfig>,
    pub acquisition_contract: Option<String>,
}

// ============================================================================
// Basket (Debt side)
// ============================================================================

/// Debt-side basket state. Owns credit/debt accounting.
#[cw_serde]
pub struct BasketDebt {
    /// Basket ID
    pub basket_id: Uint128,
    /// Credit asset breakdown by rate type
    pub credit_asset: CreditAssetBreakdown,
    /// Credit redemption price (not market price)
    pub credit_price: PriceResponse,
    /// Pending revenue available to mint
    pub pending_revenue: PendingRevenue,
    /// Pending bad debt
    pub pending_bad_debt: Uint128,
    /// Liquidation queue contract address
    pub liq_queue: Option<Addr>,
    /// True if the credit oracle was set
    pub oracle_set: bool,
    /// Toggle to allow revenue to be distributed
    pub distribute_revenue: bool,
}

// ============================================================================
// Position (Debt side)
// ============================================================================

/// Per-asset rate index for tracking accrual per collateral type.
/// Maps to cAsset.rate_index / peg_rate_index in the monolithic CDP.
#[cw_serde]
pub struct AssetRateIndex {
    /// Asset denomination string
    pub asset_denom: String,
    /// Rate index for regular debt (synced from basket)
    pub rate_index: Decimal,
    /// Rate index for peg debt (synced from basket)
    pub peg_rate_index: Decimal,
    /// Value ratio of this asset in the position (updated on refresh)
    pub ratio: Decimal,
}

/// Debt-side position. Split from the monolithic Position struct.
/// Shares position_id with CollateralPosition.
#[cw_serde]
pub struct DebtPosition {
    /// Position ID (shared with CollateralPosition)
    pub position_id: Uint128,
    /// Rate segments (variable and fixed rate debt)
    pub rate_segments: Vec<RateSegment>,
    /// Peg debt rate segments (USDC debt via transmuter)
    #[serde(default)]
    pub peg_rate_segments: Vec<RateSegment>,
    /// Deployed to venues
    pub deployed_to: Vec<DeploymentVenue>,
    /// Interest waiting to be paid
    pub pending_interest: Uint128,
    /// Total interest paid lifetime
    pub total_interest_accrued: Uint128,
    /// Initial debt when entering a volatile window (for management points)
    #[serde(default)]
    pub vol_window_initial_debt: Option<Uint128>,
    /// Per-asset rate indices for computing accrual.
    /// CDP parity: maps to position.collateral_assets[i].rate_index
    #[serde(default)]
    pub collateral_rate_indices: Vec<AssetRateIndex>,
}

// ============================================================================
// Execute Messages
// ============================================================================

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub collateral_contract: Option<String>,
    pub liquidation_contract: Option<String>,
    pub staking_contract: Option<String>,
    pub chain_proxy: Option<String>,
    pub debt_auction: Option<String>,
    pub oracle_contract: Option<String>,
    pub liquidity_contract: Option<String>,
    pub discounts_contract: Option<String>,
    pub revenue_distributor: Option<String>,
    pub transmuter_addr: Option<String>,
    pub points_contract: Option<String>,
    pub liq_fee: Decimal,
    pub credit_twap_timeframe: u64,
    pub cpc_multiplier: Decimal,
    pub debt_minimum: Uint128,
    pub base_debt_cap_multiplier: Uint128,
    pub rate_slope_multiplier: Decimal,
    pub affiliate_fee_max: Decimal,
    pub irm_config: IRMConfig,
    pub acquisition_contract: Option<String>,
    /// Initial basket debt creation
    pub create_basket: CreateBasketDebt,
}

#[cw_serde]
pub struct CreateBasketDebt {
    pub basket_id: Uint128,
    /// Creates native denom for credit_asset
    pub credit_asset: Asset,
    /// Credit redemption price
    pub credit_price: Decimal,
    /// Base collateral interest rate
    pub base_interest_rate: Option<Decimal>,
    /// Credit pool infos for liquidity measuring
    pub credit_pool_infos: Vec<PoolType>,
    /// Liquidation queue contract
    pub liq_queue: Option<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Update contract config
    UpdateConfig(UpdateConfig),

    // === User operations ===

    /// Increase debt of a position
    IncreaseDebt {
        position_id: Uint128,
        amount: Option<Uint128>,
        LTV: Option<Decimal>,
        mint_to_addr: Option<String>,
        deployment_intent: Option<DeploymentIntent>,
        debt_split: Option<DebtSplit>,
        rollover_updates: Option<Vec<(u8, bool)>>,
        peg_debt: Option<bool>,
    },
    /// Repay debt of a position
    Repay {
        position_id: Uint128,
        position_owner: Option<String>,
        send_excess_to: Option<String>,
        debt_split: Option<DebtSplit>,
    },
    /// Accrue interest for positions
    Accrue {
        position_owner: Option<String>,
        position_ids: Vec<Uint128>,
    },
    /// Close a position by selling collateral and repaying debt
    ClosePosition {
        position_id: Uint128,
        close_percentage: Option<Decimal>,
        max_spread: Decimal,
        send_to: Option<String>,
    },
    /// Set user deployment intents
    SetUserIntents {
        deployment_intent: DeploymentIntent,
    },
    /// Fulfill minting intents
    FulfillIntents { users: Vec<String> },
    /// Set deployment venue for a position
    SetDeploymentVenue {
        position_id: Uint128,
        venue_address: String,
        initial_deployed_debt_amount: Option<Uint128>,
    },

    // === Basket/Config edits ===

    /// Edit basket debt config (credit pool infos, liq queue, base_interest_rate, etc.)
    EditBasket(EditBasketDebt),
    /// Set affiliate for a position
    SetAffiliate {
        position_id: Uint128,
        affiliate_address: String,
        affiliate_fee: Decimal,
        label: Option<String>,
    },
    /// Fulfill bad debt with CDT
    FulfillBadDebt {},
    /// Take revenue from pending_revenue
    TakeRevenue {},
    /// Check and clear position's volatile window debt delta (called by Points contract)
    CheckAndClearDebtDelta {
        position_owner: String,
        position_id: Uint128,
    },
    /// Set frozen state (replicated, synced with Collateral)
    SetFrozen {
        frozen: bool,
    },

    // === Authorized calls from Collateral contract ===

    /// Notify of a new deposit (for accrual + affiliate setup).
    /// Only callable by Collateral contract.
    NotifyDeposit {
        position_id: Uint128,
        position_owner: String,
        affiliate_address: Option<String>,
    },
    /// Accrue interest and validate solvency for a withdrawal.
    /// Returns debt data via reply attributes. Errors if insolvent.
    /// Only callable by Collateral contract.
    AccrueAndValidate {
        position_owner: String,
        position_id: Uint128,
        /// Remaining collateral value after proposed withdrawal
        remaining_collateral_value: Decimal,
        /// Weighted average max_borrow_LTV of remaining collateral
        avg_max_borrow_ltv: Decimal,
        /// Weighted average max_LTV of remaining collateral
        avg_max_ltv: Decimal,
    },

    // === Authorized calls from Liquidation contract ===

    /// Accrue and return debt data for liquidation assessment.
    /// Only callable by Liquidation contract.
    AccrueAndReturnDebt {
        position_owner: String,
        position_id: Uint128,
    },
    /// Update debt after liquidation repayment.
    /// Only callable by Liquidation contract.
    LiquidationDebtUpdate {
        position_id: Uint128,
        position_owner: String,
        repay_amount: Uint128,
    },
    /// Record bad debt after liquidation.
    /// Only callable by Liquidation contract.
    RecordBadDebt {
        position_id: Uint128,
        position_owner: String,
        bad_debt_amount: Uint128,
    },

    // === Acquisition contract ===

    /// Set the acquisition bump rate from the acquisition contract.
    /// Only callable by the configured acquisition_contract.
    /// Applied ONLY to peg (USDC) debt: peg_effective_rate = smoothed_rate + (acquisition_bump_rate * 1/max_LTV).
    /// Regular (CDT) debt is NOT affected.
    SetAcquisitionBumpRate {
        rate: Decimal,
    },

    // === Callbacks ===
    Callback(CallbackMsg),
}

#[cw_serde]
pub enum CallbackMsg {
    /// Close position callback after venue repayments
    ClosePositionCallback {
        position_id: Uint128,
        position_owner: String,
    },
}

#[cw_serde]
pub struct EditBasketDebt {
    /// Liquidation Queue
    pub liq_queue: Option<String>,
    /// Credit pool info for liquidity measuring
    pub credit_pool_infos: Option<Vec<PoolType>>,
    /// Credit TWAP price source
    pub credit_asset_twap_price_source: Option<TWAPPoolInfo>,
    /// Base interest rate
    pub base_interest_rate: Option<Decimal>,
    /// Toggle negative redemption rates
    pub negative_rates: Option<bool>,
    /// CPC margin of error
    pub cpc_margin_of_error: Option<Decimal>,
    /// Toggle revenue distribution
    pub distribute_revenue: Option<bool>,
    /// Take revenue
    pub take_revenue: Option<Uint128>,
}

// ============================================================================
// Query Messages
// ============================================================================

#[cw_serde]
pub enum QueryMsg {
    /// Returns the contract's config
    Config {},
    /// Returns debt positions
    GetDebtPositions {
        start_after: Option<String>,
        limit: Option<u32>,
        user_info: Option<UserInfo>,
        user: Option<String>,
    },
    /// Returns full positions (queries Collateral for collateral data)
    GetFullPositions {
        start_after: Option<String>,
        limit: Option<u32>,
        user_info: Option<UserInfo>,
        user: Option<String>,
    },
    /// Returns the debt basket info
    GetBasket {},
    /// Returns credit redemption rate
    GetCreditRate {},
    /// Returns collateral interest rates
    GetCollateralInterest {},
    /// Returns rates store
    GetRates {},
    /// Simulate LTV mint for a position
    SimulateMint {
        position_info: UserInfo,
        LTV: Decimal,
    },
    /// Get user deployment intents
    GetUserIntent {
        start_after: Option<String>,
        limit: Option<u32>,
        users: Vec<String>,
    },
    /// Get affiliates for a position
    GetAffiliates { position_id: Uint128 },
    /// Get active deployment venues
    GetActiveDeploymentVenues {
        venue: Option<String>,
        start_after: Option<String>,
        limit: Option<u32>,
    },
    /// Returns liquidation stats
    GetLiquidationStats {
        start_after: Option<u64>,
        limit: Option<u32>,
    },
    /// Returns historical interest rates
    GetHistoricalInterestRates { asset: String },
    /// Get frozen state
    GetFrozen {},
}

// ============================================================================
// Response types
// ============================================================================

/// Combined position view (debt queries Collateral for collateral data)
#[cw_serde]
pub struct FullPosition {
    pub position_id: Uint128,
    pub collateral_assets: Vec<cAsset>,
    pub cAsset_ratios: Vec<Decimal>,
    pub credit_amount: Uint128,
    pub rate_segments: Vec<RateSegment>,
    pub peg_rate_segments: Vec<RateSegment>,
    pub avg_borrow_LTV: Decimal,
    pub avg_max_LTV: Decimal,
    pub deployed_to: Vec<DeploymentVenue>,
    pub pending_interest: Uint128,
    pub total_interest_accrued: Uint128,
}

#[cw_serde]
pub struct DebtPositionResponse {
    pub position_id: Uint128,
    pub credit_amount: Uint128,
    pub rate_segments: Vec<RateSegment>,
    pub peg_rate_segments: Vec<RateSegment>,
    pub deployed_to: Vec<DeploymentVenue>,
    pub pending_interest: Uint128,
    pub total_interest_accrued: Uint128,
}

#[cw_serde]
pub struct DebtPositionsResponse {
    pub user: String,
    pub positions: Vec<DebtPositionResponse>,
}

/// Data returned by AccrueAndReturnDebt via reply attributes
#[cw_serde]
pub struct AccrueDebtResponse {
    pub total_debt: Uint128,
    pub credit_price: PriceResponse,
    pub rate_segments: Vec<RateSegment>,
    pub peg_rate_segments: Vec<RateSegment>,
    /// Amount of CDT being recalled from deployment venues during liquidation.
    /// CDP parity: maps to return value of get_deployable_venues_user_repay_amount().
    /// The liq engine subtracts this from the amount that needs collateral-side liquidation.
    #[serde(default)]
    pub venue_recall_amount: Uint128,
}

#[cw_serde]
pub struct MigrateMsg {}
