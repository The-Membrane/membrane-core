use membrane::liq_queue::Config;
use membrane::types::Queue;

use cosmwasm_std::{Decimal, Uint128, Addr};
use cw_storage_plus::{Item, Map};


pub const CONFIG: Item<Config> = Item::new("config");
pub const QUEUES: Map<String, Queue> = Map::new("queue"); //Each asset (String of AssetInfo) has a list of PremiumSlots that make up its Queue
                                                          //(bid_for, premium, epoch, scale) -> sum_snapshot
pub const EPOCH_SCALE_SUM: Map<(String, Uint128, Uint128, Uint128), Decimal> =
    Map::new("epoch_scale_sum");

pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");