use cw_storage_plus::{Item, Map};
use cosmwasm_std::Addr;
use membrane::market_manager::{Config, MarketItem, PendingMarket};


pub const CONFIG: Item<Config> = Item::new("config");
//Manager, Market Addresses
pub const MANAGED_MARKETS: Map<String, Vec<MarketItem>> = Map::new("managed_markets");
pub const PENDING_MARKET: Item<PendingMarket> = Item::new("pending_market");

pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");