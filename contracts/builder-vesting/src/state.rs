use membrane::types::{Allocation, Asset};
use membrane::builder_vesting::Config;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::Item;


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Receiver {
    pub receiver: Addr,
    pub allocation: Option<Allocation>,
    pub claimables: Vec<Asset>,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const RECEIVERS: Item<Vec<Receiver>> = Item::new("receivers");
