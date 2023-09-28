use cw_storage_plus::Item;
use cosmwasm_std::Addr;
use membrane::system_discounts::Config;


pub const CONFIG: Item<Config> = Item::new("config");
pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");