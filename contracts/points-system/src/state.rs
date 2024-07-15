use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::{Item, Map};

use membrane::points_system::{Config, ClaimCheck, UserStats};

#[cw_serde]
pub struct LiquidationPropagation {
    ///CDP's Pre-Liquidation CDT SUPPLY
    pub pre_liq_CDT: Uint128,
    ///Liquidator address
    pub liquidator: Addr,
    ///Liquidatee address
    pub liquidatee: Addr,
}


pub const CONFIG: Item<Config> = Item::new("config");
pub const USER_STATS: Map<Addr, UserStats> = Map::new("user_stats"); 
pub const CLAIM_CHECK: Item<ClaimCheck> = Item::new("claim_check");
pub const LIQ_PROPAGATION: Item<LiquidationPropagation> = Item::new("cdp_balances");

pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");
