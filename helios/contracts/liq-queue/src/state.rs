use membrane::liq_queue::Config;
use membrane::types::{AssetInfo, Queue};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Decimal, Uint128};
use cw_storage_plus::{Item, Map};


pub const CONFIG: Item<Config> = Item::new("config");
pub const QUEUES: Map<String, Queue> = Map::new("queue"); //Each asset (String of AssetInfo) has a list of PremiumSlots that make up its Queue
                                                          //(bid_for, premium, epoch, scale) -> sum_snapshot
pub const EPOCH_SCALE_SUM: Map<(String, Uint128, Uint128, Uint128), Decimal> =
    Map::new("epoch_scale_sum");
