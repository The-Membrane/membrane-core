use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use membrane::oracle::Config;
use membrane::types::AssetOracleInfo;


pub const CONFIG: Item<Config> = Item::new("config");
pub const ASSETS: Map<String, Vec<AssetOracleInfo>> = Map::new("assets"); //Asset, Vec of Oracles for each basket
