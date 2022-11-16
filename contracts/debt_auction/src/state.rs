use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::{Item, Map};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use membrane::types::{AssetInfo, RepayPosition, AuctionRecipient};
use membrane::debt_auction::Config;


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Auction {
    pub remaining_recapitalization: Uint128,
    pub repayment_positions: Vec<RepayPosition>, //Repayment amount, Positions info
    pub send_to: Vec<AuctionRecipient>,
    pub auction_start_time: u64,
    pub basket_id_price_source: Uint128,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const ASSETS: Item<Vec<AssetInfo>> = Item::new("assets");
pub const ONGOING_AUCTIONS: Map<String, Auction> = Map::new("ongoing_auctions"); //AssetInfo, Auction
