use cosmwasm_std::{Addr, Uint128, Decimal};
use cw_storage_plus::{Item, Map};

use membrane::margin_proxy::Config;
use membrane::cdp::PositionResponse;

pub const CONFIG: Item<Config> = Item::new("config");
pub const USERS: Map<Addr, Vec<Uint128>> = Map::new("assets"); //position_id

//Reply Propogations
pub const COMPOSITION_CHECK: Item<PositionResponse> = Item::new("composition_check"); 
pub const NEW_POSITION_INFO: Item<Addr> = Item::new("new_position_info"); //User
pub const NUM_OF_LOOPS: Item<Option<u64>> = Item::new("num_of_loops");
pub const LOOP_PARAMETERS: Item<(Addr, Decimal)> = Item::new("loop_parameters"); //User, target_LTV

pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");
