
use cosmwasm_std::{Addr, Uint128, Decimal};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::{Asset, LockUp, DebtTokenAsset, AssetInfo};


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,   
    pub positions_contract: String,
    pub accepted_lps: Vec<AssetInfo>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Deposit { },
    Withdraw { 
        withdraw_assets: Vec<Asset>,  //in GAMM share tokens (AssetInfo::NativeToken)  
    },
    ChangeOwner {
        owner: String,        
    },
    EditAcceptedLPs {
        lp: AssetInfo,
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
    //Returns Vec<Asset>
    Deposits {
        limit: Option<u64>,
        start_after: Option<String>, //user
    },
}


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Config {
    pub owner: Addr,
    pub positions_contract: Addr,
    pub accepted_lps: Vec<AssetInfo>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct UserResponse {
    pub user: String,
    pub deposits: Vec<VaultLPs>,
    pub discount_value: Uint128,
}


