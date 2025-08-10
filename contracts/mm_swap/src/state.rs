#![allow(dead_code)]
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal};
use cw_storage_plus::{Item, Map};
use membrane::mm_swap::Config;
use membrane::types::{OsmosisRouteInfo, SwapRoute};

#[cw_serde]
pub struct SwapInfo {
    pub swapper: Addr,
    //The state identifier
    pub caller: String,
    pub token_out: String,
    pub max_slippage: Decimal,
}

// Stores global contract configuration
pub const CONFIG: Item<Config> = Item::new("config");

// Caller-specific asset route infos. Key: (caller, denom)
pub const ROUTES: Map<(String, String), OsmosisRouteInfo> = Map::new("routes");

// Caller-specific swap routes collection.
pub const SWAP_ROUTES: Map<String, Vec<SwapRoute>> = Map::new("swap_routes");
pub const SWAP_INFO: Item<SwapInfo> = Item::new("swap_info");
