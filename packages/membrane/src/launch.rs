use cosmwasm_std::{Addr, Uint128};
use cosmwasm_schema::cw_serde;


#[cw_serde]
pub struct InstantiateMsg {
    /// Emergent Labs multisig address
    pub labs_addr: String,
    /// Apollo router address
    pub apollo_router: String,
    /// Osmosis Proxy contract id
    pub osmosis_proxy_id: u64,
    /// Oracle contract id
    pub oracle_id: u64,
    /// Staking contract id
    pub staking_id: u64,
    /// Vesting contract id
    pub vesting_id: u64,
    /// Governance contract id
    pub governance_id: u64,
    /// Positions contract id
    pub positions_id: u64,
    /// Stability Pool contract id
    pub stability_pool_id: u64,
    /// Liquidity Queue contract id
    pub liq_queue_id: u64,
    /// Liquidity Check contract id
    pub liquidity_check_id: u64,
    /// MBRN Auction contract id
    pub mbrn_auction_id: u64,
    /// Margin Proxy contract id
    pub margin_proxy_id: u64,
    /// System Discounts contract id
    pub system_discounts_id: u64,
    /// Discount Vault contract id
    pub discount_vault_id: u64,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Deposit OSMO to earned locked MBRN rewards for a specified duration
    Lock {
        /// Lock duration of MBRN rewards, in days
        lock_up_duration: u64, 
    },
    /// Withdraw OSMO from a specified lockup duration
    Withdraw {
        /// OSMO amount to withdraw
        withdrawal_amount: Uint128,
        /// Lock duration of MBRN rewards, in days 
        lock_up_duration: u64,
    },
    /// Claim MBRN rewards from a specified lockup duration.
    /// Must be past the lockup duration to claim rewards.
    Claim {},
    /// Create MBRN & CDT LPs.
    /// Incentivize CDT stableswap.
    /// Deposit into MBRN OSMO LP.
    Launch {},
    /// Update Config
    UpdateConfig(UpdateConfig),
}

#[cw_serde]
pub enum QueryMsg {
    //Returns Config
    Config {},
    Lockdrop {},
    IncentiveDistribution {},
}

#[cw_serde]
pub struct Config {
    pub mbrn_denom: String,
    pub credit_denom: String,
    pub labs_addr: Addr,
    pub apollo_router: Addr,
    pub mbrn_launch_amount: Uint128,
    //Collateral info    
    pub atom_denom: String,
    pub osmo_denom: String,
    pub usdc_denom: String,
    pub atomosmo_pool_id: u64,
    pub osmousdc_pool_id: u64,
    //Contract IDs
    pub osmosis_proxy_id: u64,
    pub oracle_id: u64,
    pub staking_id: u64,
    pub vesting_id: u64,
    pub governance_id: u64,
    pub positions_id: u64,
    pub stability_pool_id: u64,
    pub liq_queue_id: u64,
    pub liquidity_check_id: u64,
    pub mbrn_auction_id: u64,     
    pub margin_proxy_id: u64,   
    pub system_discounts_id: u64,
    pub discount_vault_id: u64, 
}

#[cw_serde]
pub struct UpdateConfig {
    pub mbrn_denom: Option<String>,   
    pub credit_denom: Option<String>,
    pub osmo_denom: Option<String>,
    pub usdc_denom: Option<String>,
}