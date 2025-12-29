use membrane::ltv_disco::{Config, LTVQueue, Dispersal, RevenueTrackingEntry, BackingDeposit, RevenueEvent, UserLifetimeRevenueEntry, TVLEntry, LTVEntry, LockedDeposit};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Uint128, Decimal};
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

/// State tracking for compound swap operations
/// 
/// This struct is saved before initiating a compound swap and loaded in the reply handler
/// to distribute newly received deposit tokens back to the contributing deposits.
/// 
/// # Usage Flow
/// 1. claim_revenue_for_user saves this with deposit contributions and balance before swap
/// 2. Swap executes via neutron_proxy (asynchronously)
/// 3. Reply handler loads this, calculates new tokens, and distributes proportionally
#[cw_serde]
pub struct CompoundPropagation {
    /// Deposit keys and their CDT contribution amounts
    /// Format: Vec<(deposit_key_string, cdt_amount)>
    /// Used to calculate proportional distribution of new deposit tokens
    pub deposit_contributions: Vec<(String, Uint128)>,
    /// Deposit token balance before swap to calculate new tokens
    /// CRITICAL: Used to determine how many NEW tokens were received (current - before)
    pub deposit_token_balance_before: Uint128,
    /// Asset for compound operations (e.g., "uusd")
    pub asset: String,
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
pub const COMPOUND_PROPAGATION: Item<CompoundPropagation> = Item::new("compound_propagation"); 

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

// Daily LTV tracker per asset: Asset -> Vec<LTVEntry> with 100 entry limit
pub const DAILY_LTV_TRACKER: Map<String, Vec<LTVEntry>> = Map::new("daily_ltv_tracker");

// User total deposits tracker: user address (String) -> total deposits (Uint128).
// REdundant bc we could add this to USER_DEPOSITS as a new field and then range with a prefix but 
// we need this to be fast for discount calcs so we aren't overloading the CDP execution costs.
pub const USER_TOTAL_DEPOSITS: Map<String, Uint128> = Map::new("user_total_deposits");

// Locked deposits tracker: user address -> Vec of locked deposits with identifying info
pub const USER_LOCKED_DEPOSITS: Map<Addr, Vec<LockedDeposit>> = Map::new("user_locked_deposits");

// Manager -> Vec<deposit_key> (only keys, no full deposit info)
pub const MANAGED_DEPOSITS: Map<Addr, Vec<String>> = Map::new("managed_deposits");

// Manager -> Decimal (fee percentage)
pub const MANAGER_FEE: Map<Addr, Decimal> = Map::new("manager_fee");

// ================= Affiliates =================
/// Affiliates map: user address -> Vec<AffiliateData>
pub const AFFILIATES: Map<String, Vec<membrane::types::AffiliateData>> = Map::new("affiliates");
/// Maximum number of affiliates per user
pub const AFFILIATE_LIMIT: usize = 10;

// Helper functions for managed deposits
use cosmwasm_std::Storage;

pub fn add_managed_deposit(
    storage: &mut dyn Storage,
    manager: &Addr,
    deposit_key: String,
) -> Result<(), cosmwasm_std::StdError> {
    let mut keys = MANAGED_DEPOSITS
        .may_load(storage, manager.clone())?
        .unwrap_or_default();
    if !keys.contains(&deposit_key) {
        keys.push(deposit_key);
        MANAGED_DEPOSITS.save(storage, manager.clone(), &keys)?;
    }
    Ok(())
}

pub fn remove_managed_deposit(
    storage: &mut dyn Storage,
    manager: &Addr,
    deposit_key: &str,
) -> Result<(), cosmwasm_std::StdError> {
    let mut keys = MANAGED_DEPOSITS
        .may_load(storage, manager.clone())?
        .unwrap_or_default();
    keys.retain(|k| k != deposit_key);
    if keys.is_empty() {
        MANAGED_DEPOSITS.remove(storage, manager.clone());
    } else {
        MANAGED_DEPOSITS.save(storage, manager.clone(), &keys)?;
    }
    Ok(())
}

pub fn update_managed_deposit_key(
    storage: &mut dyn Storage,
    manager: &Addr,
    old_key: &str,
    new_key: String,
) -> Result<(), cosmwasm_std::StdError> {
    let mut keys = MANAGED_DEPOSITS
        .may_load(storage, manager.clone())?
        .unwrap_or_default();
    if let Some(pos) = keys.iter().position(|k| k == old_key) {
        keys[pos] = new_key;
        MANAGED_DEPOSITS.save(storage, manager.clone(), &keys)?;
    }
    Ok(())
}

