use membrane::types::{FeeEvent, StakeDeposit, StakeDistributionLog};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Uint128, Addr};
use cw_storage_plus::Item;

use membrane::staking::Config;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Totals {
    pub stakers: Uint128,
    pub vesting_contract: Uint128,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const TOTALS: Item<Totals> = Item::new("totals");
pub const STAKED: Item<Vec<StakeDeposit>> = Item::new("stake"); //Stack of deposits
pub const FEE_EVENTS: Item<Vec<FeeEvent>> = Item::new("fee_events"); //<timestamp, asset> //The amount saved is the amount of the asset per MBRN staked
pub const INCENTIVE_SCHEDULING: Item<StakeDistributionLog> = Item::new("stake_incentives_log"); 

pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");