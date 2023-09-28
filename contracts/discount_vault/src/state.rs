use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};

use membrane::discount_vault::Config;
use membrane::types::VaultUser;

pub const CONFIG: Item<Config> = Item::new("config");
pub const USERS: Map<Addr, VaultUser> = Map::new("vault_users");

pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");