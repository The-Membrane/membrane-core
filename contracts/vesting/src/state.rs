use membrane::types::Recipient;
use membrane::vesting::Config;

use cw_storage_plus::Item;


pub const CONFIG: Item<Config> = Item::new("config");
pub const RECIPIENTS: Item<Vec<Recipient>> = Item::new("recipients");
