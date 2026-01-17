use cosmwasm_std::{Addr, Uint128};
use cosmwasm_schema::cw_serde;


#[cw_serde]
pub struct InstantiateMsg {
    /// Neutron Proxy contract id
    pub neutron_proxy_id: u64,
    /// Neutron Oracle contract id
    pub neutron_oracle_id: u64,
    /// Staking contract id
    pub staking_id: u64,
    /// Vesting contract id
    pub vesting_id: u64,
    /// Positions contract id
    pub positions_id: u64,
    /// Liquidity Queue contract id
    pub liq_queue_id: u64,
    /// MBRN Auction contract id
    pub mbrn_auction_id: u64,
    /// System Discounts contract id
    pub system_discounts_id: u64,
    /// Discount Vault contract id
    pub discount_vault_id: u64,
    /// LTV Disco contract id
    pub ltv_disco_id: u64,
    /// Transmuter contract id
    pub transmuter_id: u64,
    /// Revenue Distributor contract id
    pub revenue_distributor_id: u64,
    /// Transmuter Lockdrop contract id
    pub transmuter_lockdrop_id: u64,
    /// Yield Arb contract id
    pub yield_arb_id: u64,
    /// Mars Vault Token contract id
    pub mars_vault_token_id: u64,
    /// Points System contract id
    pub points_system_id: u64,
    /// Emissions Voting contract id
    pub emissions_voting_id: u64,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Update Config
    UpdateConfig(UpdateConfig),
}

#[cw_serde]
pub enum QueryMsg {
    /// Returns Config
    Config {},
    /// Return Protocol Addresses
    ContractAddresses {},
}

#[cw_serde]
pub struct Config {
    /// Contract owner
    pub owner: Addr,
    /// MBRN token denom
    pub mbrn_denom: String,
    /// Basket credit asset denom
    pub credit_denom: String,
    /// Pre launch contributors address
    pub pre_launch_contributors: Addr,
    /// Address receiving pre-launch community allocation
    pub pre_launch_community: Vec<String>,
    /// Apollo router address
    pub apollo_router: Addr,
    /// Amount of MBRN for launch incentives & LPs
    pub mbrn_launch_amount: Uint128,
    /// Osmosis ATOM denom
    pub atom_denom: String,
    /// OSMO denom
    pub osmo_denom: String,
    /// Axelar USDC denom
    pub usdc_denom: String,
    /// ATOM/OSMO pool id
    pub atomosmo_pool_id: u64,
    /// USDC/OSMO pool id
    pub osmousdc_pool_id: u64,
    /// Neutron Proxy contract id
    pub neutron_proxy_id: u64,
    /// Neutron Oracle contract id
    pub neutron_oracle_id: u64,
    /// Staking contract id
    pub staking_id: u64,
    /// Vesting contract id
    pub vesting_id: u64,
    /// Positions contract id
    pub positions_id: u64,
    /// Liquidity Queue contract id
    pub liq_queue_id: u64,
    /// MBRN Auction contract id
    pub mbrn_auction_id: u64,   
    /// System Discounts contract id
    pub system_discounts_id: u64,
    /// Discount Vault contract id
    pub discount_vault_id: u64, 
    /// LTV Disco contract id
    pub ltv_disco_id: u64,
    /// Transmuter contract id
    pub transmuter_id: u64,
    /// Revenue Distributor contract id
    pub revenue_distributor_id: u64,
    /// Transmuter Lockdrop contract id
    pub transmuter_lockdrop_id: u64,
    /// Yield Arb contract id
    pub yield_arb_id: u64,
    /// Mars Vault Token contract id
    pub mars_vault_token_id: u64,
    /// Points System contract id
    pub points_system_id: u64,
    /// Emissions Voting contract id
    pub emissions_voting_id: u64,
}

#[cw_serde]
pub struct UpdateConfig {
    /// MBRN token denom
    pub mbrn_denom: Option<String>,   
    /// Basket credit asset denom
    pub credit_denom: Option<String>,
    /// OSMO denom
    pub osmo_denom: Option<String>,
    /// Axelar USDC denom
    pub usdc_denom: Option<String>,
}

#[cw_serde]
pub struct MigrateMsg {}

