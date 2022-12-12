use cosmwasm_std::{Addr, Uint128, Decimal};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub positions_contract: String,
    pub apollo_router_contract: String,
    pub max_slippage: Decimal,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    //Deposits asset into a new Position in the Positions contract
    Deposit {
        position_id: Option<Uint128>, //If the user wants to create a new/separate position, no position id is passed
    }, 
    //Looped leverage
    Loop {
        position_id: Uint128,
        num_loops: Option<u64>,
        target_LTV: Decimal,
    },
    //Closes position
    ClosePosition {
        position_id: Uint128,
        max_spread: Decimal,
    },
    UpdateConfig {
        owner: Option<String>,
        positions_contract: Option<String>,
        apollo_router_contract: Option<String>,
        max_slippage: Option<Decimal>,
    },
}
//Position Repayments can be done on the the base Positions contract

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {},
    //Get PositionReponses from user owned Positions in the Positions contract
    GetUserPositions { user: String },
}


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Config {
    pub owner: Addr,
    pub positions_contract: Addr,
    pub apollo_router_contract: Addr,
    pub max_slippage: Decimal,
}
