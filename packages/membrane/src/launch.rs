use cosmwasm_std::{Addr, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub labs_addr: String,
    pub apollo_router: String,    
    //Contract IDs
    pub osmosis_proxy_id: u64,
    pub oracle_id: u64,
    pub staking_id: u64,
    pub vesting_id: u64,
    pub governance_id: u64,
    pub positions_id: u64,
    pub stability_pool_id: u64,
    pub liq_queue_id: u64,
    pub liquidity_check_id: u64,
    pub mbrn_auction_id: u64,    
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Lock { 
        lock_up_duration: u64, //in days
    },
    Withdraw { 
        withdrawal_amount: Uint128, 
        lock_up_duration: u64, //in days
    },
    Claim {},
    Launch {},
    UpdateConfig(UpdateConfig),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    //Returns Config
    Config {},
    Lockdrop {},
    IncentiveDistribution {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Config {
    pub mbrn_denom: String,
    pub credit_denom: String,
    pub labs_addr: Addr,
    pub apollo_router: Addr,
    pub mbrn_launch_amount: Uint128,
    //Collateral info    
    pub atom_denom: String,
    pub osmo_denom: String,
    pub usdc_denom: String,
    pub atomosmo_pool_id: u64,
    pub osmousdc_pool_id: u64,
    //Contract IDs
    pub osmosis_proxy_id: u64,
    pub oracle_id: u64,
    pub staking_id: u64,
    pub vesting_id: u64,
    pub governance_id: u64,
    pub positions_id: u64,
    pub stability_pool_id: u64,
    pub liq_queue_id: u64,
    pub liquidity_check_id: u64,
    pub mbrn_auction_id: u64,    
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct UpdateConfig {
    pub mbrn_denom: Option<String>,   
    pub credit_denom: Option<String>,
    pub osmo_denom: Option<String>,
    pub usdc_denom: Option<String>,
}