use membrane::ltv_disco::{Config, LTVQueue, Dispersal, RevenueTrackingEntry, BackingDeposit, RevenueEvent, UserLifetimeRevenueEntry, TVLEntry};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::{Item, Map};


#[cw_serde]
pub struct BadDebtPropagation {
    pub asset: String, 
    //This is in the denom of the deposit token so if its VTs its VTs, if its CDT its CDT
    pub amount: Uint128,
}

#[cw_serde]
pub struct SwapPropagation {
    /// CDT balance before swap to calculate swapped amount
    pub cdt_balance_before: Uint128,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const LTV_QUEUES: Map<String, LTVQueue> = Map::new("ltv_queues"); // Asset , LTVQueue
pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");
pub const DISPERSAL: Map<String, Dispersal> = Map::new("dispersal");

// Revenue tracking: (Asset, MaxLTV, MaxBorrowLTV) -> Vec<RevenueTrackingEntry>
pub const REVENUE_TRACKING: Map<(String, String, String), Vec<RevenueTrackingEntry>> = Map::new("revenue_tracking");

// Rate assurance: (Asset, MaxLTV, MaxBorrowLTV) -> Uint128 (base tokens per 1 trillion vault tokens)
pub const RATE_ASSURANCE: Map<(String, String, String), Uint128> = Map::new("rate_assurance");

pub const PENDING_BAD_DEBT: Map<String, Uint128> = Map::new("pending_bad_debt"); // Asset , Amount
pub const BAD_DEBT_PROPAGATION: Item<BadDebtPropagation> = Item::new("bad_debt_propagation");
pub const SWAP_PROPAGATION: Item<SwapPropagation> = Item::new("swap_propagation"); 

// User claimable revenue removed; using event-based claiming

// Revenue events per group: (Asset, MaxLTV, MaxBorrowLTV) -> Vec<RevenueEvent>
pub const REVENUE_EVENTS: Map<(String, String, String), Vec<RevenueEvent>> = Map::new("revenue_events");

// User lifetime revenue entries (Vec with limit)
pub const USER_LIFETIME_REVENUE: Map<(Addr, String), Vec<UserLifetimeRevenueEntry>> = Map::new("user_lifetime_revenue");

// Backing deposits map for O(1) lookup: composite key "asset:ltv:max_borrow_ltv:user"
pub const BACKING_DEPOSITS: Map<String, BackingDeposit> = Map::new("backing_deposits");

// User deposits index: (User_addr, Asset) -> Vec of deposit keys as strings
pub const USER_DEPOSITS: Map<(Addr, String), Vec<String>> = Map::new("user_deposits");

// Daily TVL tracker: Vec<TVLEntry> with 100 entry limit and daily granularity
pub const DAILY_TVL_TRACKER: Item<Vec<TVLEntry>> = Item::new("daily_tvl_tracker");

// User total deposits tracker: user address (String) -> total deposits (Uint128).
// REdundant bc we could add this to USER_DEPOSITS as a new field and then range with a prefix but 
// we need this to be fast for discount calcs so we aren't overloading the CDP execution costs.
pub const USER_TOTAL_DEPOSITS: Map<String, Uint128> = Map::new("user_total_deposits");

