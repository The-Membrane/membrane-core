use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Uint128, Addr, Decimal};

use crate::types::Owner;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct InstantiateMsg {}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    ///Osmosis Msgs
    CreateDenom {
        subdenom: String,        
        max_supply: Option<Uint128>,
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
    ///
    EditTokenMaxSupply {
        denom: String,
        max_supply: Uint128,
    },
    UpdateConfig {
        owner: Option<Vec<String>>,
        add_owner: bool, //Add or Remove
        debt_auction: Option<String>,
        positions_contract: Option<String>,
        liquidity_contract: Option<String>,
    },
    EditOwner {
        owner: String,
        liquidity_multiplier: Option<Decimal>,
        non_token_contract_auth: Option<bool>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    GetDenom {
        creator_address: String,
        subdenom: String,
    },
    /// For a given pool ID, list all tokens traded on it with current liquidity (spot).
    /// As well as the total number of LP shares and their denom
    PoolState {
        id: u64,
    },
    GetTokenInfo {
        denom: String,
    },
    Config {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Config {
    pub owners: Vec<Owner>,
    pub debt_auction: Option<Addr>,
    pub positions_contract: Option<Addr>,
    pub liquidity_contract: Option<Addr>,
}

// We define a custom struct for each query response
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct GetDenomResponse {
    pub denom: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct TokenInfoResponse {
    pub denom: String,
    pub current_supply: Uint128,
    pub max_supply: Uint128,
}
