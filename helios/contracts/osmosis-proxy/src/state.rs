
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::{Item, Map};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub owner: Addr,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct TokenInfo {
    pub current_supply: Uint128,
    pub max_supply: Uint128,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const TOKENS: Map<String, TokenInfo> = Map::new("tokens");