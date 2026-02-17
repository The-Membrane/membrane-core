use membrane::liq_queue::Config;
use membrane::types::Queue;
use membrane::math::Decimal256;

use cosmwasm_std::{Uint128, Addr};
use cw_storage_plus::{Item, Map};


pub const CONFIG: Item<Config> = Item::new("config");
pub const QUEUES: Map<String, Queue> = Map::new("queue"); //Each asset (String of AssetInfo) has a list of PremiumSlots that make up its Queue
                                                          // epoch_scale_sum key: "bid_for:premium:epoch:scale"
pub const EPOCH_SCALE_SUM: Map<String, Decimal256> =
    Map::new("epoch_scale_sum");

pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");