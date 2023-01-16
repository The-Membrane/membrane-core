use cosmwasm_std::Addr;
use cosmwasm_schema::cw_serde;


#[cw_serde]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub oracle_contract: String,
    pub positions_contract: String,
    pub staking_contract: String,
    pub stability_pool_contract: String,
    pub lockdrop_contract: Option<String>,
    pub discount_vault_contract: Option<String>,
    pub minimum_time_in_network: u64, //in days
}

#[cw_serde]
pub enum ExecuteMsg {
    UpdateConfig(UpdateConfig),
}

#[cw_serde]
pub enum QueryMsg {
    //Returns Config
    Config {},
    //Returns Decimal
    UserDiscount { user: String },
}

#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub mbrn_denom: String,
    pub oracle_contract: Addr,
    pub positions_contract: Addr,
    pub staking_contract: Addr,
    pub stability_pool_contract: Addr,
    pub lockdrop_contract: Option<Addr>,
    pub discount_vault_contract: Option<Addr>,
    pub minimum_time_in_network: u64, //in days
}

#[cw_serde]
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