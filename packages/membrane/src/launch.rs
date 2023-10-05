use cosmwasm_std::{Addr, Uint128};
use cosmwasm_schema::cw_serde;


#[cw_serde]
pub struct InstantiateMsg {
    /// Pre launch contributors address
    pub pre_launch_contributors: String,
    /// Address receiving pre-launch community allocation
    pub pre_launch_community: Vec<String>,
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
    /// Change lockup duration of a subset of locked deposits.
    ChangeLockDuration {
        /// Amount of uosmo to change lock duration of
        uosmo_amount: Option<Uint128>,
        /// Lock duration of MBRN rewards, in days
        old_lock_up_duration: u64,
        /// Lock duration of MBRN rewards, in days
        new_lock_up_duration: u64,
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
    /// Returns Config
    Config {},
    /// Returns Lockdrop object
    Lockdrop {},
    /// Return Protocol Addresses
    ContractAddresses {},
    /// Returns MBRN lockup distributions
    IncentiveDistribution {},
    /// Returns User incentive distribution
    UserIncentives { user: String },
    /// Returns locked User info
    UserInfo { user: String },
}

#[cw_serde]
pub struct Config {
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
    /// System Discounts contract id
    pub system_discounts_id: u64,
    /// Discount Vault contract id
    pub discount_vault_id: u64, 
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