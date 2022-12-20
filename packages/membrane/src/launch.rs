use cosmwasm_std::{Addr, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub labs_addr: String,
    pub apollo_router: String,    
    //Collateral info    
    pub atom_denom: String,
    pub osmo_denom: String,
    pub usdc_denom: String,
    pub atomosmo_pool_id: String,
    pub atomusdc_pool_id: String,
    pub osmousdc_pool_id: String,
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
    UpdateConfig(UpdateConfig),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    //Returns Config
    Config {},
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
    pub atomosmo_pool_id: String,
    pub atomusdc_pool_id: String,
    pub osmousdc_pool_id: String,
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
    pub owner: Option<String>,  
    pub mbrn_denom: Option<String>,   
}