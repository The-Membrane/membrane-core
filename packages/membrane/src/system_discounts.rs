use cosmwasm_std::{Addr, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub basket_id: Uint128,
    pub oracle_contract: String,
    pub positions_contract: String,
    pub staking_contract: String,
    pub stability_pool_contract: String,
    pub lockdrop_contract: String,
    pub discount_vault_contract: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    UpdateConfig {
        owner: Option<String>,  
        mbrn_denom: Option<String>,      
        basket_id: Option<Uint128>,
        oracle_contract: Option<String>,
        positions_contract: Option<String>,
        staking_contract: Option<String>,
        stability_pool_contract: Option<String>,
        lockdrop_contract: Option<String>,
        discount_vault_contract: Option<String>,
    },
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
    pub basket_id: Uint128, //Used to find credit price & to query user's outstanding debt
    pub oracle_contract: Addr,
    pub positions_contract: Addr,
    pub staking_contract: Addr,
    pub stability_pool_contract: Addr,
    pub lockdrop_contract: Addr,
    pub discount_vault_contract: Addr,
}
