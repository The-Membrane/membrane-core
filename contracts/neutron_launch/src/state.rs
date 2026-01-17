use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use membrane::{neutron_launch::Config, types::{UserRatio, Lockdrop, LockedUser}};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct LaunchAddrs {
    pub neutron_proxy: Addr,
    pub oracle: Addr,
    pub staking: Addr,
    pub vesting: Addr,
    pub positions: Addr,
    pub liq_queue: Addr,
    pub mbrn_auction: Addr,    
    pub discount_vault: Addr,
    pub system_discounts: Addr,
    pub ltv_disco: Addr,
    pub transmuter: Addr,
    pub revenue_distributor: Addr,
    pub transmuter_lockdrop: Addr,
    pub yield_arb: Addr,
    pub mars_vault_token: Addr,
    pub points_system: Addr,
    pub emissions_voting: Addr,
}

pub const CONFIG: Item<Config> = Item::new("config");

//Launch
pub const ADDRESSES: Item<LaunchAddrs> = Item::new("addresses");