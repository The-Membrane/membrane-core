#![allow(dead_code)]
use cw_storage_plus::{Item, Map};
use membrane::mm_swap::Config;
use membrane::types::{OsmosisRouteInfo, SwapRoute};


// Stores global contract configuration
pub const CONFIG: Item<Config> = Item::new("config");

// Caller-specific asset route infos. Key: (caller, denom)
pub const ROUTES: Map<(String, String), OsmosisRouteInfo> = Map::new("routes");

// Caller-specific swap routes collection.
pub const SWAP_ROUTES: Map<String, Vec<SwapRoute>> = Map::new("swap_routes");
