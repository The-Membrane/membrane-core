use cosmwasm_std::{Addr, Decimal, Uint128};
use cosmwasm_schema::cw_serde;


#[cw_serde]
pub struct InstantiateMsg {
    /// Contract owner, defaults to info.sender
    pub owner: Option<String>,
    /// Oracle contract address
    pub oracle_contract: String,
    /// Positions contract address
    pub positions_contract: String,
    /// Staking contract address
    pub staking_contract: String,
    /// Lockdrop contract address
    pub lockdrop_contract: Option<String>,
    /// Discount vault contract address
    pub discount_vault_contract: Option<String>,
    /// LTV Disco contract address
    pub ltv_disco_contract: Option<String>,
    /// Transmuter contract address
    pub transmuter_contract: Option<String>,
    /// Minimum time in network to be eligible for discounts, in days
    pub minimum_time_in_network: u64,
    /// Maximum discount percentage (0.0 to 1.0)
    pub max_discount: Option<Decimal>,
    /// MBRN amount required to reach max discount
    pub mbrn_at_max_discount: Option<Uint128>,
    /// Maximum boost percentage (0.0 to max_boost)
    pub max_boost: Option<Decimal>,
    /// Maximum stable backing discount percentage (0.0 to 1.0)
    pub stable_backing_max_discount: Option<Decimal>,
    /// First month discount percentage (60% of max)
    pub stable_backing_first_month_discount: Option<Decimal>,
    /// Remaining discount percentage (40% of max)
    pub stable_backing_remaining_discount: Option<Decimal>,
    /// Curve duration in days (default 90 days = 3 months)
    pub stable_backing_curve_duration_days: Option<u64>,
    /// First month duration in days (default 30 days)
    pub stable_backing_first_month_days: Option<u64>,
    /// Discountable debt multiplier (default 18x)
    pub stable_backing_discountable_debt_multiplier: Option<u64>,
    /// Transmuter balance multiplier (default 2x = 200%)
    pub stable_backing_transmuter_balance_multiplier: Option<Decimal>,
}

#[cw_serde]
pub enum ExecuteMsg {
    //Updates Config
    UpdateConfig(UpdateConfig),
    /// Set current discount period
    SetDiscountPeriod {
        /// Start time of the discount period (Unix timestamp), if None, start now.
        start_time: Option<u64>,
        /// Duration of the discount period in hours
        duration: u64,
        /// Discount
        discount: Decimal,
    },
    /// Clear current discount period
    ClearDiscountPeriod {},
}

#[cw_serde]
pub enum QueryMsg {
    /// Returns contract config
    Config {},
    //Returns % discount for user
    UserDiscount {
        /// User address
        user: String
    },
    /// Returns % boost for user
    UserBoost {
        /// User address
        user: String
    },
    /// Returns % boost for each intent based on lock duration
    IntentBoosts {
        /// List of intents to calculate boosts for
        intents: Vec<crate::transmuter_lockdrop::MbrnIntentOption>
    },
    /// Returns discount for positions with 100% force_redemption assets based on transmuter deposits
    StableBackingDiscounts {
        /// User address
        user: String,
        /// Debt amount to calculate discount for
        debt_amount: Uint128,
    },
}

#[cw_serde]
pub struct Config {
    /// Contract owner
    pub owner: Addr,
    /// MBRN denom
    pub mbrn_denom: String,
    /// Oracle contract address
    pub oracle_contract: Addr,
    /// Positions contract address
    pub positions_contract: Addr,
    /// Staking contract address
    pub staking_contract: Addr,
    /// Lockdrop contract address
    pub lockdrop_contract: Option<Addr>,
    /// Discount vault contract address
    pub discount_vault_contract: Vec<Addr>,
    /// LTV Disco contract address
    pub ltv_disco_contract: Option<Addr>,
    /// Transmuter contract address
    pub transmuter_contract: Option<Addr>,
    /// Minimum time in network to be eligible for discounts, in days
    pub minimum_time_in_network: u64,
    /// Maximum discount percentage (0.0 to 1.0)
    pub max_discount: Decimal,
    /// MBRN amount required to reach max discount
    pub mbrn_at_max_discount: Uint128,
    /// Maximum boost percentage
    pub max_boost: Decimal,
    /// Maximum stable backing discount percentage (0.0 to 1.0)
    pub stable_backing_max_discount: Decimal,
    /// First month discount percentage (60% of max)
    pub stable_backing_first_month_discount: Decimal,
    /// Remaining discount percentage (40% of max)
    pub stable_backing_remaining_discount: Decimal,
    /// Curve duration in days (default 90 days = 3 months)
    pub stable_backing_curve_duration_days: u64,
    /// First month duration in days (default 30 days)
    pub stable_backing_first_month_days: u64,
    /// Discountable debt multiplier (default 18x)
    pub stable_backing_discountable_debt_multiplier: u64,
    /// Transmuter balance multiplier (default 2x = 200%)
    pub stable_backing_transmuter_balance_multiplier: Decimal,
}

#[cw_serde]
pub struct UpdateConfig {
    /// Contract owner
    pub owner: Option<String>,     
    /// Oracle contract address
    pub oracle_contract: Option<String>,
    /// Positions contract address
    pub positions_contract: Option<String>,
    /// Staking contract address
    pub staking_contract: Option<String>,
    /// Lockdrop contract address
    pub lockdrop_contract: Option<String>,
    /// Discount vault contract address
    pub discount_vault_contract: Option<(String, bool)>, //Addr + Add or remove
    /// LTV Disco contract address
    pub ltv_disco_contract: Option<String>,
    /// Transmuter contract address
    pub transmuter_contract: Option<String>,
    /// Minimum time in network to be eligible for discounts, in days
    pub minimum_time_in_network: Option<u64>,
    /// Maximum discount percentage
    pub max_discount: Option<Decimal>,
    /// MBRN amount required to reach max discount
    pub mbrn_at_max_discount: Option<Uint128>,
    /// Maximum boost percentage
    pub max_boost: Option<Decimal>,
    /// Add or Update a static discount
    pub static_discount: Option<UserDiscountResponse>,
    /// Maximum stable backing discount percentage
    pub stable_backing_max_discount: Option<Decimal>,
    /// First month discount percentage
    pub stable_backing_first_month_discount: Option<Decimal>,
    /// Remaining discount percentage
    pub stable_backing_remaining_discount: Option<Decimal>,
    /// Curve duration in days
    pub stable_backing_curve_duration_days: Option<u64>,
    /// First month duration in days
    pub stable_backing_first_month_days: Option<u64>,
    /// Discountable debt multiplier
    pub stable_backing_discountable_debt_multiplier: Option<u64>,
    /// Transmuter balance multiplier
    pub stable_backing_transmuter_balance_multiplier: Option<Decimal>,
}

#[cw_serde]
pub struct StableBackingDiscountsResponse {
    /// User address
    pub user: String,
    /// Discount percentage (0.0 to 1.0)
    pub discount: Decimal,
}

#[cw_serde]
pub struct UserDiscountResponse {
    /// User address
    pub user: String,
    /// User discount
    pub discount: Decimal,
}

#[cw_serde]
pub struct UserBoostResponse {
    /// User address
    pub user: String,
    /// User boost
    pub boost: Decimal,
}

#[cw_serde]
pub struct IntentBoostsResponse {
    /// List of boosts, one per intent in the same order as input
    pub boosts: Vec<Decimal>,
}

#[cw_serde]
pub struct MigrateMsg {}