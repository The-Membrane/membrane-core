
use membrane::stability_pool::Config;
use membrane::types::{AssetPool, FeeEvent, User};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::{Item, Map};


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Propagation {
    pub repaid_amount: Uint128,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const ASSET: Item<AssetPool> = Item::new("asset"); //Acts as the asset WL and the sum of all deposits for said asset
pub const PROP: Item<Propagation> = Item::new("propagation");
pub const INCENTIVES: Item<Uint128> = Item::new("incentives_total");
pub const USERS: Map<Addr, User> = Map::new("users"); //Used to map claims to users
pub const OUTSTANDING_FEES: Item<Vec<FeeEvent>> = Item::new("outstanding_fees"); //Used to track fees in the credit asset that need to be paid out to users 

pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");