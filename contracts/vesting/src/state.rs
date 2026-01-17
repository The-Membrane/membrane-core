use membrane::types::{Recipient, VestingPeriod};
use membrane::vesting::Config;
use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::{Item, Map};
use cosmwasm_schema::cw_serde;


pub const CONFIG: Item<Config> = Item::new("config");
pub const RECIPIENTS: Item<Vec<Recipient>> = Item::new("recipients");
pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");

/// Map: (user_addr, week_id) -> VestingSchedule
pub const VESTING_SCHEDULES: Map<(String, u64), VestingSchedule> = Map::new("vesting_schedules");

/// Total MBRN mint liability (tracks amount_to_mint, not old token amount)
pub const OLD_MBRN_RECEIVED: Item<Uint128> = Item::new("old_mbrn_received");

/// Total MBRN minted
pub const MINTED_MBRN: Item<Uint128> = Item::new("minted_mbrn");

/// Total remaining unminted liabilities (sum of mbrn_to_mint - amount_withdrawn across all schedules)
pub const TOTAL_OLD_MBRN: Item<Uint128> = Item::new("total_old_mbrn");

/// Neutron-proxy address (authorization)
pub const NEUTRON_PROXY: Item<Addr> = Item::new("neutron_proxy");

#[cw_serde]
pub struct VestingSchedule {
    /// User address
    pub user: Addr,
    /// Week ID (epoch_seconds / SECONDS_IN_WEEK)
    pub week_id: u64,
    /// Total MBRN to mint for this schedule
    pub mbrn_to_mint: Uint128,
    /// Amount already withdrawn
    pub amount_withdrawn: Uint128,
    /// Vesting start time (first transmutation in week)
    pub start_time: u64,
    /// Vesting parameters
    pub vesting_period: VestingPeriod,
    /// Number of transmutations grouped
    pub transmutation_count: u64,
}