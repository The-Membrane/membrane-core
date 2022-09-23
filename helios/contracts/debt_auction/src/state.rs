
use cosmwasm_std::{ Addr, Decimal, Uint128 };
use cw_storage_plus::{Item, Map};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use membrane::types::{ RepayPosition, AssetInfo };

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {    
    pub owner: Addr,
    pub oracle_contract: Addr,
    pub osmosis_proxy: Addr,
    pub mbrn_denom: String,
    pub positions_contract: Addr,
    pub twap_timeframe: u64,
    pub initial_discount: Decimal,
    pub discount_increase_timeframe: u64, //in seconds
    pub discount_increase: Decimal, //% increase
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Auction {  
    pub remaining_recapitalization: Uint128,
    pub repayment_positions: Vec<RepayPosition>,  //Repayment amount, Positions info
    pub auction_start_time: u64,
    pub basket_id_price_source: Uint128,
}


pub const CONFIG: Item<Config> = Item::new("config");
pub const ASSETS: Item<Vec<AssetInfo>> = Item::new("assets");
pub const ONGOING_AUCTIONS: Map<String, Auction> = Map::new("ongoing_auctions");

