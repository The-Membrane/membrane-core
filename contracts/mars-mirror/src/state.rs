use cosmwasm_schema::cw_serde;
use cosmwasm_std::Addr;
use cw_storage_plus::Item;
use membrane::mars_mirror::{Config, MoveProgress};

pub const CONFIG: Item<Config> = Item::new("config");
pub const MOVE_PROGRESS: Item<MoveProgress> = Item::new("move_progress");

