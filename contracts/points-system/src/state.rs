use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Uint128};
use cw_storage_plus::{Item, Map};

use membrane::{points_system::{ClaimCheck, Config, UserStats, VaultConversionRate}, types::PointsMultipliers};

#[cw_serde]
pub struct LiquidationPropagation {
    ///CDP's Pre-Liquidation CDT SUPPLY
    pub pre_liq_CDT: Uint128,
    ///Liquidator address
    pub liquidator: Addr,
    ///Liquidatee address
    pub liquidatee: Addr,
}


#[cw_serde]
pub struct VaultInfo {
    ///Vault Address
    pub vault_address: String,
    //Saves denom for a single vault token
    pub vault_token_denom: String,
    //Saves decimal for a single vault token
    pub single_vault_token: Uint128,
}



pub const CONFIG: Item<Config> = Item::new("config");
pub const USER_STATS: Map<Addr, UserStats> = Map::new("user_stats"); 
pub const CLAIM_CHECK: Item<ClaimCheck> = Item::new("claim_check");
pub const LIQ_PROPAGATION: Item<LiquidationPropagation> = Item::new("cdp_balances");
pub const USER_VAULT_CONVERSION_RATES: Map<Addr, Vec<VaultConversionRate>> = Map::new("user_vault_conversion_rates");
pub const VAULT_INFO: Item<Vec<VaultInfo>> = Item::new("vault_info");
pub const POINTS_MULTIPLIERS: Item<PointsMultipliers> = Item::new("points_multipliers");

pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");
// Store pending user for reply handlers (simple - just need to know who to give points to)
pub const PENDING_USER: Item<Addr> = Item::new("pending_user");
