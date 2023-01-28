use cw_storage_plus::{Item, Map};
use membrane::liquidity_check::Config;
use membrane::types::LiquidityInfo;


pub const CONFIG: Item<Config> = Item::new("config");
pub const ASSETS: Map<String, LiquidityInfo> = Map::new("assets");
