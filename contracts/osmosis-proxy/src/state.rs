use cosmwasm_schema::cw_serde;

use cosmwasm_std::{Uint128, Decimal, Addr, Coin};
use cw_storage_plus::{Item, Map};

use membrane::osmosis_proxy::Config;
use membrane::types::{SwapRoute, TransmutationPair};

#[cw_serde]
pub struct TokenInfo {
    /// Current minted supply
    pub current_supply: Uint128,
    /// Max supply 
    pub max_supply: Option<Uint128>,
    /// Burned supply
    pub burned_supply: Uint128,
}

#[cw_serde]
pub struct PendingTokenInfo {
    /// Chosen subdenom
    pub subdenom: String,
    /// Max supply
    pub max_supply: Option<Uint128>,
}


#[cw_serde]
pub struct SwapInfo {
    pub swapper: Addr,
    pub prev_balances: Vec<Coin>,
    pub token_out: String,
    pub max_slippage: Decimal,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const TOKENS: Map<String, TokenInfo> = Map::new("tokens"); //AssetInfo, TokenInfo
pub const PENDING: Item<PendingTokenInfo> = Item::new("pending_denoms");
pub const SWAP_ROUTES: Item<Vec<SwapRoute>> = Item::new("swap_routes");
pub const SWAP_INFO: Item<SwapInfo> = Item::new("swap_info");
pub const TRANSMUTATION_PAIRS: Item<Vec<TransmutationPair>> = Item::new("transmutation_pairs");

#[cw_serde]
pub struct SupplySnapshot {
    /// Block time (seconds)
    pub timestamp: u64,
    /// Current supply at snapshot
    pub current_supply: Uint128,
    /// Burned supply at snapshot
    pub burned_supply: Uint128,
}

/// Supply history per denom: Map<denom, Vec<SupplySnapshot>>
pub const SUPPLY_HISTORY: Map<String, Vec<SupplySnapshot>> = Map::new("supply_history");