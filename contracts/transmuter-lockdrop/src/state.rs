use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, StdResult, Storage, Uint128};
use cw_storage_plus::{Item, Map};

use membrane::transmuter_lockdrop::{Config, LockdropState, UserDeposit, MbrnIntentOption, UserLockdropHistory};

pub const CONFIG: Item<Config> = Item::new("config");
pub const CURRENT_LOCKDROP: Item<LockdropState> = Item::new("current_lockdrop");
pub const USER_DEPOSITS: Map<String, Vec<UserDeposit>> = Map::new("user_deposits");
pub const PENDING_LOCKS: Map<String, Vec<UserDeposit>> = Map::new("pending_locks");
pub const USER_INTENTS: Map<String, Vec<MbrnIntentOption>> = Map::new("user_intents");
pub const LOCKDROP_HISTORY: Item<Vec<LockdropState>> = Item::new("lockdrop_history");
pub const USER_LOCKDROP_HISTORY: Map<String, Vec<UserLockdropHistory>> = Map::new("user_lockdrop_history");
pub const MAX_HISTORY_LIMIT: usize = 9;

