use cw_storage_plus::Item;

use membrane::system_discounts::Config;


pub const CONFIG: Item<Config> = Item::new("config");