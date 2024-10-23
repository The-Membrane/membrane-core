use cosmwasm_schema::cw_serde;
use cw_storage_plus::Item;
use cosmwasm_std::{Addr, Decimal};
use membrane::system_discounts::{Config, UserDiscountResponse};

pub const CONFIG: Item<Config> = Item::new("config");
pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");
//Addresses that get static discounts
pub const STATIC_DISCOUNTS: Item<Vec<UserDiscountResponse>> = Item::new("static_discounts");