use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Uint128, Addr};
use cw_storage_plus::{Item, Map};

use membrane::discount_vault::Config;
use membrane::types::{VaultUser, VaultedLP, LPDeposit, AssetInfo};


pub const CONFIG: Item<Config> = Item::new("config");
pub const USERS: Map<Addr, VaultUser> = Map::new("vault_users");
