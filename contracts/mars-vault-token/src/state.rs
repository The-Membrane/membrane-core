use cosmwasm_schema::cw_serde;

use cosmwasm_std::{Addr, Decimal, Uint128};
use cw_storage_plus::Item;

use membrane::mars_vault_token::Config;


#[cw_serde]
pub struct APRInstance {
    pub apr_per_second: Decimal,
    pub time_since_last_update: u64,
    pub apr_of_this_update: Decimal,
}

#[cw_serde]
pub struct APRTracker {
    pub aprs: Vec<APRInstance>,
    pub last_updated: u64,
    pub last_total_deposit: Uint128,
}

#[cw_serde]
pub struct TokenRateAssurance {
    pub pre_vtokens_per_one: Uint128,
    pub pre_btokens_per_one: Uint128,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const VAULT_TOKEN: Item<Uint128> = Item::new("vault_token");
pub const APR_TRACKER: Item<APRTracker> = Item::new("apr_tracker");
pub const TOKEN_RATE_ASSURANCE: Item<TokenRateAssurance> = Item::new("token_rate_assurance");

pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");