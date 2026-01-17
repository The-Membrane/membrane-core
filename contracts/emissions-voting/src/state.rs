use cw_storage_plus::{Item, Map};

use membrane::emissions_voting::{Config, Graph, UserVote, PeriodResult};

/// Contract configuration
pub const CONFIG: Item<Config> = Item::new("config");

/// Map of graph label -> Graph
pub const GRAPHS: Map<&str, Graph> = Map::new("graphs");

/// Map of (graph_label, user_addr) -> UserVote
/// Stores each user's current vote on each graph
pub const USER_VOTES: Map<(&str, &str), UserVote> = Map::new("user_votes");

/// Map of graph_label -> total_voting_power
/// Aggregate voting power across all votes for a graph
pub const GRAPH_TOTALS: Map<&str, u128> = Map::new("graph_totals");

/// Map of graph_label -> weighted_sum (stored as String to support both Uint128 and Decimal)
/// For Uint128 graphs: sum of (voted_value * voting_power)
/// For Decimal graphs: sum of (voted_value * voting_power) as Decimal
/// This allows O(1) weighted average calculation
pub const WEIGHTED_SUMS_UINT128: Map<&str, u128> = Map::new("weighted_sums_uint128");
pub const WEIGHTED_SUMS_DECIMAL: Map<&str, String> = Map::new("weighted_sums_decimal");

/// Map of graph_label -> Vec<PeriodResult>
/// Historical results for each graph
pub const PERIOD_HISTORY: Map<&str, Vec<PeriodResult>> = Map::new("period_history");

/// Ownership transfer pending address
pub const OWNERSHIP_TRANSFER: Item<cosmwasm_std::Addr> = Item::new("ownership_transfer");
