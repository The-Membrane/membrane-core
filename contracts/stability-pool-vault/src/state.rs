use cosmwasm_schema::cw_serde;

use cosmwasm_std::{Addr, Decimal, Uint128};
use cw_storage_plus::Item;

use membrane::stability_pool_vault::Config;
use membrane::types::{VTClaimCheckpoint, ClaimTracker};


#[cw_serde]
pub struct TokenRateAssurance {
    pub pre_vtokens_per_one: Uint128,
    pub pre_btokens_per_one: Uint128,
}


pub const CONFIG: Item<Config> = Item::new("config");
pub const VAULT_TOKEN: Item<Uint128> = Item::new("vault_token");
pub const DEPOSIT_BALANCE_AT_LAST_CLAIM: Item<Uint128> = Item::new("deposit_balance");
pub const CLAIM_TRACKER: Item<ClaimTracker> = Item::new("claim_tracker");
pub const TOKEN_RATE_ASSURANCE: Item<TokenRateAssurance> = Item::new("token_rate_assurance");

pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");