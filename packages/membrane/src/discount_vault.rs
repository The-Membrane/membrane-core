use cosmwasm_std::{Addr, Uint128};
use cosmwasm_schema::cw_serde;

use crate::types::{Asset, LPPoolInfo, VaultedLP};

#[cw_serde]
pub struct InstantiateMsg {
    /// Address of the owner
    pub owner: Option<String>,
    /// Address of the positions contract
    pub positions_contract: String,
    /// Address of the osmosis proxy contract
    pub osmosis_proxy: String,
    /// List of accepted Osmosis LP pool ids, assumption that the LP is 50:50 
    pub accepted_LPs: Vec<u64>, 
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Deposit LP tokens into the vault
    Deposit { },
    /// Withdraw LP tokens from the vault
    Withdraw { 
        /// Amount of LP tokens to withdraw.
        /// In GAMM share tokens (AssetInfo::NativeToken).
        withdrawal_assets: Vec<Asset>,  
    },
    /// Chanfe the owner of the contract
    ChangeOwner {
        /// New owner
        owner: String,        
    },
    /// Edit the accepted LPs
    EditAcceptedLPs {
        /// Pool ids to act upon
        pool_ids: Vec<u64>,
        /// Add or remove
        remove: bool,
    },
}

#[cw_serde]
pub enum QueryMsg {
    /// Returns Config
    Config { },
    /// Returns user deposits & total value (UserResponse)
    User { 
        /// User to query
        user: String,
        /// Minimum deposit time to filter for, in days
        minimum_deposit_time: Option<u64>, //in days
    },
    /// Returns list of Deposits (Vec<VaultedLP>)
    Deposits {
        /// User limiter
        limit: Option<u64>,
        /// Start after this user
        start_after: Option<String>,
    },
}


#[cw_serde]
pub struct Config {
    /// Address of the owner
    pub owner: Addr,
    /// Address of the positions contract
    pub positions_contract: Addr,
    /// Address of the osmosis proxy contract
    pub osmosis_proxy: Addr,
    /// List of accepted Osmosis LP pool ids, assumption that the LP is 50:50
    pub accepted_LPs: Vec<LPPoolInfo>,
}

#[cw_serde]
pub struct UserResponse {
    /// User address
    pub user: String,
    /// User deposits
    pub deposits: Vec<VaultedLP>,
    /// Total value of user deposits
    pub discount_value: Uint128,
}