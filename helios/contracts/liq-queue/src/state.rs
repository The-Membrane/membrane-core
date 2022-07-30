use core::fmt;

use cosmwasm_bignumber::{Uint256, Decimal256};
use membrane::types::{ AssetInfo, Asset, Queue };
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Uint128, Decimal, CanonicalAddr};
use cw_storage_plus::{Item, Map};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub owner: Addr, //A singular positions contract address
    pub added_assets: Option<Vec<AssetInfo>>,
    pub waiting_period: u64, //Wait period is at max doubled due to slot_total calculation
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct User {
    pub claimable_assets: Vec<Asset>, //Collateral assets earned from liquidations
}

// #[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
// pub struct StoreSum {
//     pub bid_for: CanonicalAddr,
//     pub premium: u8,
//     pub current_epoch: Uint128,
//     pub current_scale: Uint128,
//     pub sum_snapshot: Decimal256,
// }




pub const CONFIG: Item<Config> = Item::new("config");
pub const QUEUES: Map<String, Queue> = Map::new("queue"); //Each asset (String of AssetInfo) has a list of PremiumSlots that make up its Queue
//(bid_for, premium, epoch, scale) -> sum_snapshot
pub const EPOCH_SCALE_SUM: Map<(String, Uint128, Uint128, Uint128), Decimal> = Map::new("epoch_scale_sum"); 

//Use oracle contract w/ cAsset info instead
pub const ORACLES: Map<AssetInfo, Addr> = Map::new("oracles");

