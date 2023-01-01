
use cosmwasm_std::{Addr, Uint128, Decimal};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::{Asset, AssetInfo, LPPoolInfo, VaultedLP};


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,   
    pub positions_contract: String,
    pub osmosis_proxy: String,
    pub accepted_LPs: Vec<u64>, //Assumption that the LP is 50:50 
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Deposit { },
    Withdraw { 
        withdrawal_assets: Vec<Asset>,  //in GAMM share tokens (AssetInfo::NativeToken)  
    },
    ChangeOwner {
        owner: String,        
    },
    EditAcceptedLPs {
        pool_ids: Vec<u64>,
        remove: bool,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    //Returns Config
    Config { },
    //Returns UserResponse
    User { 
        user: String,
        minimum_deposit_time: Option<u64>, //in days
    },
    //Returns Vec<VaultedLP>
    Deposits {
        limit: Option<u64>, //User limit
        start_after: Option<String>, //user
    },
}


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Config {
    pub owner: Addr,
    pub positions_contract: Addr,
    pub osmosis_proxy: Addr,
    pub accepted_LPs: Vec<LPPoolInfo>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct UserResponse {
    pub user: String,
    pub deposits: Vec<VaultedLP>,
    pub discount_value: Uint128,
}