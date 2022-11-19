
use cosmwasm_std::{Addr, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::{LockUp, DebtTokenAsset, Asset};


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,   
    pub lock_up_ceiling: Option<u64>,
    pub osmosis_proxy: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Deposit { 
        lock_up_duration: u64, //in days
    },
    Withdraw { 
        amount: Uint128,  //in GAMM share tokens (AssetInfo::NativeToken)  
    },
    ClaimRewards { },
    UpdateConfig {
        owner: Option<String>,        
        lock_up_ceiling: Option<u64>,
        osmosis_proxy: Option<String>,
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
    },
    //Returns Uint128
    TotalDepositsPerLP { },
    //Returns Vec<Asset>
    TotalDeposits { },
    //Returns Vec<LockUp>
    LockUpDistribution { },
}


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Config {
    pub owner: Addr,
    pub osmosis_proxy: Addr,
    pub lock_up_ceiling: u64, //in days
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct UserResponse {
    pub user: String,
    pub total_debt_token: DebtTokenAsset,
    pub deposits: Vec<Asset>,
    pub lock_up_distributions: Vec<LockUp>, 
    pub accrued_incentives: Asset,
}


