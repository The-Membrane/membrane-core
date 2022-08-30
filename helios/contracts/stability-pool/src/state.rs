use core::fmt;

use membrane::types::{AssetPool, User, AssetInfo};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Uint128, Decimal};
use cw_storage_plus::{Item, Map};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub owner: Addr, //Positions contract address
    pub incentive_rate: Decimal,
    //% of Supply desired in the SP. 
    //Incentives decrease as it gets closer
    pub desired_ratio_of_total_credit_supply: Decimal,
    pub mbrn_denom: String,
    pub osmosis_proxy: Addr,
    pub dex_router: Option<Addr>,
    pub max_spread: Option<Decimal>, //max_spread for the router, mainly claim_as swaps
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Propagation {
    pub repaid_amount: Uint128,
}


pub const CONFIG: Item<Config> = Item::new("config");
pub const ASSETS: Item<Vec<AssetPool>> = Item::new("assets"); //Acts as the asset WL and the sum of all deposits for said asset
pub const PROP: Item<Propagation> = Item::new("propagation");

pub const USERS: Map<Addr, User> = Map::new("users"); //Used to map claims to users 


