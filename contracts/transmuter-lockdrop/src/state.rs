use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, StdResult, Storage, Uint128};
use cw_storage_plus::{Item, Map};

use membrane::transmuter_lockdrop::{Config, AcquisitionWindow, AcquisitionDeposit, MbrnIntentOption};

pub const CONFIG: Item<Config> = Item::new("config");
pub const CURRENT_WINDOW_ID: Item<u64> = Item::new("current_window_id");
pub const CURRENT_ACQUISITION_WINDOW: Item<AcquisitionWindow> = Item::new("current_acquisition_window");
pub const USER_ACQUISITION_DEPOSITS: Map<(String, u64), AcquisitionDeposit> = Map::new("user_acquisition_deposits");
pub const USER_INTENTS: Map<String, Vec<MbrnIntentOption>> = Map::new("user_intents");

