use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use membrane::liquidity_check::Config;
use membrane::types::LiquidityInfo;


pub const CONFIG: Item<Config> = Item::new("config");
pub const ASSETS: Map<String, LiquidityInfo> = Map::new("assets");
