use cosmwasm_std::{Addr, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub oracle_contract: String,
    pub positions_contract: String,
    pub staking_contract: String,
    pub stability_pool_contract: String,
    pub lockdrop_contract: String,
    pub discount_vault_contract: String,
    pub minimum_time_in_network: u64, //in days
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    UpdateConfig(UpdateConfig),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    //Returns Config
    Config {},
    //Returns Decimal
    UserDiscount { user: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Config {
    pub owner: Addr,
    pub mbrn_denom: String,
    pub oracle_contract: Addr,
    pub positions_contract: Addr,
    pub staking_contract: Addr,
    pub stability_pool_contract: Addr,
    pub lockdrop_contract: Addr,
    pub discount_vault_contract: Addr,
    pub minimum_time_in_network: u64, //in days
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct UpdateConfig {
    pub owner: Option<String>,  
    pub mbrn_denom: Option<String>,      
    pub oracle_contract: Option<String>,
    pub positions_contract: Option<String>,
    pub staking_contract: Option<String>,
    pub stability_pool_contract: Option<String>,
    pub lockdrop_contract: Option<String>,
    pub discount_vault_contract: Option<String>,
    pub minimum_time_in_network: Option<u64>, //in days
}