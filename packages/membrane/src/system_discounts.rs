use cosmwasm_std::Addr;
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
    /// Stability pool contract address
    pub stability_pool_contract: String,
    /// Lockdrop contract address
    pub lockdrop_contract: Option<String>,
    /// Discount vault contract address
    pub discount_vault_contract: Option<String>,
    /// Minimum time in network to be eligible for discounts, in days
    pub minimum_time_in_network: u64,
}

#[cw_serde]
pub enum ExecuteMsg {
    //Updates Config
    UpdateConfig(UpdateConfig),
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
    /// Stability pool contract address
    pub stability_pool_contract: Addr,
    /// Lockdrop contract address
    pub lockdrop_contract: Option<Addr>,
    /// Discount vault contract address
    pub discount_vault_contract: Option<Addr>,
    /// Minimum time in network to be eligible for discounts, in days
    pub minimum_time_in_network: u64,
}

#[cw_serde]
pub struct UpdateConfig {
    /// Contract owner
    pub owner: Option<String>,
    /// MBRN denom
    pub mbrn_denom: Option<String>,      
    /// Oracle contract address
    pub oracle_contract: Option<String>,
    /// Positions contract address
    pub positions_contract: Option<String>,
    /// Staking contract address
    pub staking_contract: Option<String>,
    /// Stability pool contract address
    pub stability_pool_contract: Option<String>,
    /// Lockdrop contract address
    pub lockdrop_contract: Option<String>,
    /// Discount vault contract address
    pub discount_vault_contract: Option<String>,
    /// Minimum time in network to be eligible for discounts, in days
    pub minimum_time_in_network: Option<u64>,
}