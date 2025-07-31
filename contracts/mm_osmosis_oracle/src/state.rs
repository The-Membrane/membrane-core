use cw_storage_plus::{Item, Map};
use cosmwasm_std::Addr;
use membrane::mm_oracle::Config;
use membrane::types::OsmosisOracleInfo;


pub const CONFIG: Item<Config> = Item::new("config");
pub const ASSETS: Map<(String, String), OsmosisOracleInfo> = Map::new("assets"); //(Caller address, Asset), Oracles for each basket

pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");
