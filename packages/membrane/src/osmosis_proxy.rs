use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::Uint128;

use crate::types::Swap;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    CreateDenom {
        subdenom: String,
    },
    ChangeAdmin {
        denom: String,
        new_admin_address: String,
    },
    MintTokens {
        denom: String,
        amount: Uint128,
        mint_to_address: String,
    },
    BurnTokens {
        denom: String,
        amount: Uint128,
        burn_from_address: String,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    GetDenom {
        creator_address: String,
        subdenom: String,
    },
    //This will be replaced by TWAP but is here for testing
    SpotPrice { asset: String },
}

// We define a custom struct for each query response
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct GetDenomResponse {
    pub denom: String,
}