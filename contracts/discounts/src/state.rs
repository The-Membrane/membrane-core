use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use membrane::discounts::Config;


pub const CONFIG: Item<Config> = Item::new("config");