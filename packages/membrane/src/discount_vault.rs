use cosmwasm_std::{Addr, Uint128};
use cosmwasm_schema::cw_serde;

use crate::types::{Asset, LPPoolInfo, VaultedLP};

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: Option<String>,   
    pub positions_contract: String,
    pub osmosis_proxy: String,
    pub accepted_LPs: Vec<u64>, //Assumption that the LP is 50:50 
}

#[cw_serde]
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

#[cw_serde]
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


#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub positions_contract: Addr,
    pub osmosis_proxy: Addr,
    pub accepted_LPs: Vec<LPPoolInfo>,
}

#[cw_serde]
pub struct UserResponse {
    pub user: String,
    pub deposits: Vec<VaultedLP>,
    pub discount_value: Uint128,
}