#![allow(dead_code)]
use cosmwasm_schema::cw_serde;
use cw_storage_plus::{Item, Map};

use membrane::tokenfactory::Config;

// Stores global contract configuration
pub const CONFIG: Item<Config> = Item::new("config");

// Track denoms that were created by this contract
// key: denom string, value: bool (placeholder)
pub const DENOMS: Map<String, bool> = Map::new("denoms");
