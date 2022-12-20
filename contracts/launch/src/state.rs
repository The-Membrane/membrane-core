use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::Item;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use membrane::{launch::Config, types::{Deposit, AssetInfo}};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct LaunchAddrs {
    pub osmosis_proxy: Addr,
    pub oracle: Addr,
    pub staking: Addr,
    pub vesting: Addr,
    pub governance: Addr,
    pub positions: Addr,
    pub stability_pool: Addr,
    pub liq_queue: Addr,
    pub liquidity_check: Addr,
    pub mbrn_auction: Addr,    
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct CreditPools {
    pub stableswap: u64,
    pub atom: u64,
    pub osmo: u64,
}

impl CreditPools {
    pub fn to_vec(&self) -> Vec<u64>{
        return vec![self.stableswap, self.atom, self.osmo]
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Lockdrop {
    pub locked_users: Vec<LockedUser>,
    pub num_of_incentives: Uint128,
    pub locked_asset: AssetInfo,    
    pub lock_up_ceiling: u64, //in days
    pub deposit_end: u64, //5 days
    pub withdrawal_end: u64, //2 days
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct LockedUser {
    pub user: String,
    pub deposits: Vec<Lock>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Lock {
    pub deposit: Uint128,
    pub lock_up_duration: u64, //in days
}


pub const CONFIG: Item<Config> = Item::new("config");

pub const LOCKDROP: Item<Lockdrop> = Item::new("lockdrop");
pub const ADDRESSES: Item<LaunchAddrs> = Item::new("addresses");
pub const CREDIT_POOL_IDS: Item<CreditPools> = Item::new("credit_pools");