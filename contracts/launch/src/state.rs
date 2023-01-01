use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::Item;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use membrane::{launch::Config, types::{Deposit, AssetInfo, UserRatio, Lockdrop}};

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
    pub discount_vault: Addr,    
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


pub const CONFIG: Item<Config> = Item::new("config");

//Lockdrop
pub const LOCKDROP: Item<Lockdrop> = Item::new("lockdrop");
pub const INCENTIVE_RATIOS: Item<Vec<UserRatio>> = Item::new("incentive_ratios");

//Launch
pub const ADDRESSES: Item<LaunchAddrs> = Item::new("addresses");
pub const CREDIT_POOL_IDS: Item<CreditPools> = Item::new("credit_pools");