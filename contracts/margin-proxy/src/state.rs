use cosmwasm_std::{Addr, Uint128, Decimal};
use cw_storage_plus::{Item, Map};

use membrane::margin_proxy::Config;
use membrane::positions::PositionResponse;

pub const CONFIG: Item<Config> = Item::new("config");
pub const USERS: Map<Addr, Vec<(Uint128, Uint128)>> = Map::new("assets"); //Basket_id, position_id

//Reply Propogations
pub const COMPOSITION_CHECK: Item<(PositionResponse, Uint128)> = Item::new("composition_check"); //Response, basket_id
pub const NEW_POSITION_INFO: Item<(Addr, Uint128)> = Item::new("new_position_info"); //User, basket_id
pub const NUM_OF_LOOPS: Item<Option<u64>> = Item::new("num_of_loops");
pub const LOOP_PARAMETERS: Item<(Addr, Decimal)> = Item::new("loop_parameters"); //User, target_LTV
