
use cosmwasm_std::{Addr, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::{LPPoolInfo, DebtTokenAsset, Asset, AssetInfo};


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,   
    pub osmosis_proxy: String,
    pub positions_contract: String,    
    pub staking_contract: String,
    pub basket_id: Uint128,
    pub locked_lp: LPPoolInfo,
    pub lock_up_ceiling: Option<u64>, //in days
    pub deposit_period: Option<u64>, //in days
    pub withdrawal_period: Option<u64>, //in days
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Lock { 
        lock_up_duration: u64, //in days
    },
    Withdraw { 
        withdrawal_amount: Uint128,  //in GAMM share tokens (AssetInfo::NativeToken)  
        lock_up_duration: u64, //in days
    },
    //ClaimRewards { },
    UpdateConfig {
        owner: Option<String>,        
        lock_up_ceiling: Option<u64>,
        deposit_period: Option<u64>,
        withdrawal_period: Option<u64>,
        basket_id: Option<Uint128>,
        locked_lp: Option<LPPoolInfo>,
        osmosis_proxy: Option<String>,
        positions_contract: Option<String>,
    },
    StartLockdrop {
        num_of_incentives: Option<Uint128>,
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
        minimum_lock: Option<u64>,
    },
    //Returns Uint128
    TotalDeposits { },
    //Returns Vec<LockDistributionResponse>
    LockupDistribution { },
}


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Config {
    pub owner: Addr,
    pub osmosis_proxy: Addr,
    pub positions_contract: Addr,
    pub basket_id: Uint128,
    pub locked_lp: LPPoolInfo,
    pub mbrn_denom: AssetInfo,
    pub num_of_incentives: Uint128,
    pub lock_up_ceiling: u64, //in days
    pub deposit_period: u64, //in days
    pub withdrawal_period: u64, //in days
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct UserResponse {
    pub user: String,
    pub total_debt_token: DebtTokenAsset,
    pub lock_up_distributions: Vec<LockDistributionResponse>, 
    pub incentives: Asset,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct LockDistributionResponse {
    pub locked_lp: Asset,
    pub lock_up_duration: u64,
}


