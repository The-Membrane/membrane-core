use membrane::types::{AssetPool, User};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Decimal, Uint128};
use cw_storage_plus::{Item, Map};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Config {
    pub owner: Addr, //Positions contract address
    pub incentive_rate: Decimal,
    pub max_incentives: Uint128,
    //% of Supply desired in the SP.
    //Incentives decrease as it gets closer
    pub desired_ratio_of_total_credit_supply: Decimal,
    pub unstaking_period: u64, // in days
    pub mbrn_denom: String,
    pub osmosis_proxy: Addr,
    pub positions_contract: Addr,
    pub dex_router: Option<Addr>,
    pub max_spread: Option<Decimal>, //max_spread for the router, mainly claim_as swaps
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Propagation {
    pub repaid_amount: Uint128,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const ASSETS: Item<Vec<AssetPool>> = Item::new("assets"); //Acts as the asset WL and the sum of all deposits for said asset
pub const PROP: Item<Propagation> = Item::new("propagation");
pub const INCENTIVES: Item<Uint128> = Item::new("incentives_total");
pub const USERS: Map<Addr, User> = Map::new("users"); //Used to map claims to users
