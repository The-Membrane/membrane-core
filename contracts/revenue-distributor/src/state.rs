use cw_storage_plus::{Item, Map};
use membrane::revenue_distributor::{Config, RevenuePromise};
use cosmwasm_std::Uint128;
use cw2::ContractVersion;

/// Contract configuration
pub const CONFIG: Item<Config> = Item::new("config");

/// Current revenue promises awaiting distribution
pub const PROMISES: Item<Vec<RevenuePromise>> = Item::new("promises");

/// Failed distributions for tracking (address -> amount)
pub const FAILED_DISTRIBUTIONS: Map<String, u128> = Map::new("failed_distributions");

/// Distribution propagation state for tracking pending promises during distribution
pub const DISTRIBUTION_PROP: Item<Vec<RevenuePromise>> = Item::new("distribution_prop");

/// LTV Disco distribution map: asset denom -> canonical revenue amount
pub const LTV_DISCO_DISTRIBUTION: Map<String, Uint128> = Map::new("ltv_disco_distribution");

/// Temporary propagation queue of asset denoms for vault replies
pub const VAULT_PROPOGATION: Item<Vec<String>> = Item::new("vault_propogation");

// removed per new flow that chains all enter-vaults upfront

/// CDT balance before TakeRevenue call (to calculate actual revenue received)
pub const PRE_TAKE_REVENUE_BALANCE: Item<Uint128> = Item::new("pre_take_revenue_balance");

/// Per-asset revenue saved before TakeRevenue call
pub const PRE_TAKE_REVENUE_PER_ASSET: Item<Vec<membrane::types::Asset>> = Item::new("pre_take_revenue_per_asset");

/// Last distribution timestamp (for window checking)
pub const LAST_DISTRIBUTION_TIME: Item<u64> = Item::new("last_distribution_time");

/// Per-asset revenue accumulation in current epoch: asset denom -> accumulated amount
pub const EPOCH_REVENUE_ACCUMULATION: Map<String, Uint128> = Map::new("epoch_revenue_accumulation");

/// Contract version
pub const CONTRACT: Item<ContractVersion> = Item::new("contract");