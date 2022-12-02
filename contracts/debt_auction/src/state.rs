use cw_storage_plus::{Item, Map};

use membrane::types::{AssetInfo, Auction};
use membrane::debt_auction::Config;

pub const CONFIG: Item<Config> = Item::new("config");
pub const ASSETS: Item<Vec<AssetInfo>> = Item::new("assets");
pub const ONGOING_AUCTIONS: Map<String, Auction> = Map::new("ongoing_auctions"); //AssetInfo, Auction
