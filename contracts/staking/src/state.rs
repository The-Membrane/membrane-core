use membrane::{types::{FeeEvent, StakeDeposit, StakeDistributionLog, DelegationInfo, Delegate}, staking::Totals};

use cosmwasm_std::{Uint128, Addr, Coin, Decimal};
use cw_storage_plus::{Item, Map};

use membrane::staking::Config;

pub const CONFIG: Item<Config> = Item::new("config");
pub const STAKING_TOTALS: Item<Totals> = Item::new("totals"); 
pub const STAKED: Map<Addr, Vec<StakeDeposit>> = Map::new("stake"); //Stack of staking deposits
pub const DELEGATIONS: Map<Addr, DelegationInfo> = Map::new("delegations"); //Info for each user's delegations (sent and received)
pub const DELEGATE_CLAIMS: Map<Addr, (Vec<Coin>, Uint128)> = Map::new("delegate_claims"); //Staking rewards that can be claimed by a delegate
pub const FEE_EVENTS: Item<Vec<FeeEvent>> = Item::new("fee_events"); //<timestamp, asset> //The amount saved is the amount of the asset per MBRN staked
pub const INCENTIVE_SCHEDULING: Item<StakeDistributionLog> = Item::new("stake_incentives_log"); 
/// Filled with info of addresses that want to be delegates
pub const DELEGATE_INFO: Item<Vec<Delegate>> = Item::new("delegate_info"); 

//Vesting specific
pub const VESTING_STAKE_TIME: Item<u64> = Item::new("vesting_stake_time"); //The time to use for vesting contract claims
pub const VESTING_REV_MULTIPLIER: Item<Decimal> = Item::new("vesting_rev_multiplier"); //The multiplier to use for vesting contract claims

pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");