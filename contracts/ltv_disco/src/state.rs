use membrane::ltv_disco::{Config, AssetQueue, RevenueTrackingEntry, BackingDeposit, RevenueEvent, UserLifetimeRevenueEntry, TVLEntry, DepositEntry, UnstakeRequest};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::{Item, Map};


#[cw_serde]
pub struct BadDebtPropagation {
    pub asset: String,
    pub amount: Uint128,
}

/// State tracking for compound swap operations
#[cw_serde]
pub struct CompoundPropagation {
    /// Deposit keys and their CDT contribution amounts
    /// Format: Vec<(deposit_key_string, cdt_amount)>
    pub deposit_contributions: Vec<(String, Uint128)>,
    /// Deposit token balance before swap
    pub deposit_token_balance_before: Uint128,
    /// Asset for compound operations
    pub asset: String,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const ASSET_QUEUES: Map<String, AssetQueue> = Map::new("asset_queues");
pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");

// Revenue tracking: (Asset, SlotStr) -> Vec<RevenueTrackingEntry>
pub const REVENUE_TRACKING: Map<(String, String), Vec<RevenueTrackingEntry>> = Map::new("revenue_tracking_v2");

// Rate assurance: (Asset, SlotStr) -> Uint128 (base tokens per 1 trillion vault tokens)
pub const RATE_ASSURANCE: Map<(String, String), Uint128> = Map::new("rate_assurance_v2");

pub const PENDING_BAD_DEBT: Map<String, Uint128> = Map::new("pending_bad_debt");
pub const BAD_DEBT_PROPAGATION: Item<BadDebtPropagation> = Item::new("bad_debt_propagation");
pub const COMPOUND_PROPAGATION: Item<CompoundPropagation> = Item::new("compound_propagation");

// Revenue events per slot: (Asset, SlotStr) -> Vec<RevenueEvent>
pub const REVENUE_EVENTS: Map<(String, String), Vec<RevenueEvent>> = Map::new("revenue_events_v2");

// User lifetime revenue entries
pub const USER_LIFETIME_REVENUE: Map<(Addr, String), Vec<UserLifetimeRevenueEntry>> = Map::new("user_lifetime_revenue");

// Backing deposits: "asset:slot:user:deposit_id" -> BackingDeposit
pub const BACKING_DEPOSITS: Map<String, BackingDeposit> = Map::new("backing_deposits_v2");

// User deposits index: (User_addr, Asset) -> Vec of deposit keys
pub const USER_DEPOSITS: Map<(Addr, String), Vec<String>> = Map::new("user_deposits_v2");

// Daily TVL tracker
pub const DAILY_TVL_TRACKER: Item<Vec<TVLEntry>> = Item::new("daily_tvl_tracker");

// Daily deposit tracker per asset (renamed from insurance tracker)
pub const DAILY_DEPOSIT_TRACKER: Map<String, Vec<DepositEntry>> = Map::new("daily_deposit_tracker");

// User total deposits tracker
pub const USER_TOTAL_DEPOSITS: Map<String, Uint128> = Map::new("user_total_deposits");

// Manager -> Vec<deposit_key>
pub const MANAGED_DEPOSITS: Map<Addr, Vec<String>> = Map::new("managed_deposits_v2");

// Manager -> Decimal (fee percentage)
pub const MANAGER_FEE: Map<Addr, cosmwasm_std::Decimal> = Map::new("manager_fee");

// Affiliates map: user address -> Vec<AffiliateData>
pub const AFFILIATES: Map<String, Vec<membrane::types::AffiliateData>> = Map::new("affiliates");
pub const AFFILIATE_LIMIT: usize = 10;

// Unstake requests: "asset:slot:user:deposit_id" -> UnstakeRequest
pub const UNSTAKE_REQUESTS: Map<String, UnstakeRequest> = Map::new("unstake_requests");

// User unstake requests index: (User_addr, Asset) -> Vec of unstake request keys
pub const USER_UNSTAKE_REQUESTS: Map<(Addr, String), Vec<String>> = Map::new("user_unstake_requests");


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
