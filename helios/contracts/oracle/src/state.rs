use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use membrane::types::AssetOracleInfo;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub owner: Addr, //MBRN Governance
    pub osmosis_proxy: Addr,
    pub positions_contract: Option<Addr>,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const ASSETS: Map<String, Vec<AssetOracleInfo>> = Map::new("assets"); //Asset, Vec of Oracles for each basket
