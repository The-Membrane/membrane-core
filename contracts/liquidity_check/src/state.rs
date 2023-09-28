use cw_storage_plus::{Item, Map};
use cosmwasm_std::Addr;
use membrane::liquidity_check::Config;
use membrane::types::LiquidityInfo;


pub const CONFIG: Item<Config> = Item::new("config");
pub const ASSETS: Map<String, LiquidityInfo> = Map::new("assets");

pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");