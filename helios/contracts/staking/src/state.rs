use membrane::types::{FeeEvent, StakeDeposit};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Decimal, Uint128};
use cw_storage_plus::Item;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub owner: Addr, //MBRN Governance
    pub mbrn_denom: String,
    pub staking_rate: Decimal,
    //Wait period between deposits & ability to earn fee events
    pub fee_wait_period: u64,  //in days
    pub unstaking_period: u64, //days
    pub positions_contract: Option<Addr>,
    pub builders_contract: Option<Addr>,
    pub osmosis_proxy: Option<Addr>,
    pub dex_router: Option<Addr>,
    pub max_spread: Option<Decimal>, //max_spread for the router, mainly claim_as swaps
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Totals {
    pub stakers: Uint128,
    pub builders_contract: Uint128,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const TOTALS: Item<Totals> = Item::new("totals");
pub const STAKED: Item<Vec<StakeDeposit>> = Item::new("stake"); //Stack of deposits
                                                                //The amount saved is the amount of the asset per MBRN staked
pub const FEE_EVENTS: Item<Vec<FeeEvent>> = Item::new("fee_events"); //<timestamp, asset>
