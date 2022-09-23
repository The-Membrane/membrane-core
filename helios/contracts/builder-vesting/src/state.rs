
use membrane::types::{ Allocation, Asset };
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::Item;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub owner: Addr, //Governance Contract
    pub initial_allocation: Uint128,
    pub mbrn_denom: String,
    pub osmosis_proxy: Addr,
    pub staking_contract: Addr,
}


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Receiver {
    pub receiver: Addr,
    pub allocation: Option<Allocation>,  
    pub claimables: Vec<Asset>,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const RECEIVERS: Item<Vec<Receiver>> = Item::new("receivers");