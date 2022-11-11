use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::{Item, Map};

use membrane::osmosis_proxy::Config;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct TokenInfo {
    pub current_supply: Uint128,
    pub max_supply: Option<Uint128>,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const TOKENS: Map<String, TokenInfo> = Map::new("tokens"); //AssetInfo, TokenInfo
