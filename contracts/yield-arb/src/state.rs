use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Uint128, MessageInfo};
use cw_storage_plus::{Item, Map};

use membrane::{types::UserInfo, yield_arb::{Config, MarketConditions, TVLSnapshot, UserPosition, DeploymentSnapshot}};

pub const CONFIG: Item<Config> = Item::new("config");
pub const USER_POSITIONS: Map<String, Vec<UserPosition>> = Map::new("user_positions");
pub const MARKET_CONDITIONS: Item<Vec<MarketConditions>> = Item::new("market_conditions");
pub const TVL_TRACKER: Item<Vec<TVLSnapshot>> = Item::new("tvl_tracker");
pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");
pub const EXIT_MESSAGE_INFO: Item<MessageInfo> = Item::new("exit_message_info");

pub const TVL_TRACKER_LIMIT: usize = 100usize;
pub const USER_POSITIONS_LIMIT: usize = 100usize;
pub const MARKET_CONDITIONS_LIMIT: usize = 100usize;
pub const MARKET_CONDITIONS_WINDOW: u64 = 86_400; // 1 day in seconds
pub const TVL_TRACKER_WINDOW: u64 = 86_400; // 1 day in seconds

// Temp storage to track the user during multi-step FulfillIntents flow
pub const LOOP_USER: Item<UserInfo> = Item::new("loop_user");
pub const LOOP_CDT_AMOUNT: Item<Uint128> = Item::new("loop_cdt_amount");

/// Deployment snapshot per user (one per user, updated on each loop)
pub const DEPLOYMENT_SNAPSHOTS: Map<String, DeploymentSnapshot> = Map::new("deployment_snapshots");

