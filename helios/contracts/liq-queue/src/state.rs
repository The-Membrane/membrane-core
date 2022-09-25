use membrane::types::{AssetInfo, Queue};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Decimal, Uint128};
use cw_storage_plus::{Item, Map};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub owner: Addr, //Governance
    pub positions_contract: Addr,
    pub added_assets: Option<Vec<AssetInfo>>,
    pub waiting_period: u64, //Wait period is at max doubled due to slot_total calculation
    pub bid_asset: AssetInfo,
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
pub const EPOCH_SCALE_SUM: Map<(String, Uint128, Uint128, Uint128), Decimal> =
    Map::new("epoch_scale_sum");
