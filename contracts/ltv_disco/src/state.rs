use membrane::ltv_disco::{Config, LTVQueue, Dispersal, BaseTokenTrackingEntry};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Uint128, Uint256};
use cw_storage_plus::{Item, Map};


#[cw_serde]
pub struct BadDebtPropagation {
    pub asset: String, 
    //This is in the denom of the deposit token so if its VTs its VTs, if its CDT its CDT
    pub amount: Uint128,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const LTV_QUEUES: Map<String, LTVQueue> = Map::new("ltv_queues"); // Asset , LTVQueue
pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");
pub const DISPERSAL: Map<String, Dispersal> = Map::new("dispersal");

//Asset, MaxLTV, MaxBorrowLTV
pub const BASE_TOKEN_TRACKING: Map<(String, String, String), Vec<BaseTokenTrackingEntry>> = Map::new("base_token_tracking"); // (Asset, MaxLTV, MaxBorrowLTV), Vec<BaseTokenTrackingEntry>

pub const PENDING_BAD_DEBT: Map<String, Uint128> = Map::new("pending_bad_debt"); // Asset , Amount
pub const BAD_DEBT_PROPAGATION: Item<BadDebtPropagation> = Item::new("bad_debt_propagation"); 

