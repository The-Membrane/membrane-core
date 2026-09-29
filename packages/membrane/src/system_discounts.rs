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
    /// Osmosis proxy contract address (for supply queries)
    pub osmosis_proxy: String,
    /// Minimum time in network to be eligible for discounts, in days
    pub minimum_time_in_network: u64,
    /// Maximum discount percentage (0.0 to 1.0), default 30%
    pub max_discount: Option<Decimal>,
    /// Maximum boost percentage (0.0 to max_boost)
    pub max_boost: Option<Decimal>,
    /// Ratio of MBRN supply required for max discount (default 0.01 = 1%)
    pub ratio_of_mbrn_supply_at_max_discount: Option<Decimal>,
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
        intents: Vec<crate::acquisition::MbrnIntentOption>
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
    /// Osmosis proxy contract address
    pub osmosis_proxy: Addr,
    /// Minimum time in network to be eligible for discounts, in days
    pub minimum_time_in_network: u64,
    /// Maximum discount percentage (0.0 to 1.0)
    pub max_discount: Decimal,
    /// Maximum boost percentage
    pub max_boost: Decimal,
    /// Ratio of MBRN supply required to reach max discount (e.g. 0.01 = 1%)
    pub ratio_of_mbrn_supply_at_max_discount: Decimal,
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
    /// Osmosis proxy contract address
    pub osmosis_proxy: Option<String>,
    /// Minimum time in network to be eligible for discounts, in days
    pub minimum_time_in_network: Option<u64>,
    /// Maximum discount percentage
    pub max_discount: Option<Decimal>,
    /// Maximum boost percentage
    pub max_boost: Option<Decimal>,
    /// Add or Update a static discount
    pub static_discount: Option<UserDiscountResponse>,
    /// Ratio of MBRN supply required for max discount
    pub ratio_of_mbrn_supply_at_max_discount: Option<Decimal>,
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
pub struct MigrateMsg {
    /// Osmosis proxy contract address (required for migration)
    pub osmosis_proxy: String,
    /// Ratio of MBRN supply at max discount (default 0.01 if not provided)
    pub ratio_of_mbrn_supply_at_max_discount: Option<Decimal>,
    /// Max discount override (default 30% if not provided)
    pub max_discount: Option<Decimal>,
}
