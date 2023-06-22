use membrane::types::{FeeEvent, StakeDeposit, StakeDistributionLog, DelegationInfo, Asset};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Uint128, Addr, Coin};
use cw_storage_plus::{Item, Map};

use membrane::staking::Config;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Totals {
    pub stakers: Uint128,
    pub vesting_contract: Uint128,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const STAKING_TOTALS: Item<Totals> = Item::new("totals"); 
pub const STAKED: Map<Addr, Vec<StakeDeposit>> = Map::new("stake"); //Stack of staking deposits
pub const DELEGATIONS: Map<Addr, DelegationInfo> = Map::new("delegations"); //Info for each user's delegations (sent and received)
pub const DELEGATE_CLAIMS: Map<Addr, (Vec<Coin>, Uint128)> = Map::new("delegate_claims"); //Staking rewards that can be claimed by a delegate
pub const FEE_EVENTS: Item<Vec<FeeEvent>> = Item::new("fee_events"); //<timestamp, asset> //The amount saved is the amount of the asset per MBRN staked
pub const INCENTIVE_SCHEDULING: Item<StakeDistributionLog> = Item::new("stake_incentives_log"); 

pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");