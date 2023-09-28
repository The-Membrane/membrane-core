use membrane::types::Recipient;
use membrane::vesting::Config;
use cosmwasm_std::Addr;
use cw_storage_plus::Item;


pub const CONFIG: Item<Config> = Item::new("config");
pub const RECIPIENTS: Item<Vec<Recipient>> = Item::new("recipients");
pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");