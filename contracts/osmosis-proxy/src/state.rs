use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::Uint128;
use cw_storage_plus::{Item, Map};

use membrane::osmosis_proxy::Config;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct TokenInfo {
    /// Current minted supply
    pub current_supply: Uint128,
    /// Max supply 
    pub max_supply: Option<Uint128>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct PendingTokenInfo {
    /// Chosen subdenom
    pub subdenom: String,
    /// Max supply
    pub max_supply: Option<Uint128>,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const TOKENS: Map<String, TokenInfo> = Map::new("tokens"); //AssetInfo, TokenInfo
pub const PENDING: Item<PendingTokenInfo> = Item::new("pending_denoms");