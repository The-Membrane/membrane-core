use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Uint128, Addr, Decimal};

use crate::types::Owner;

#[cw_serde]
pub struct InstantiateMsg {}

#[cw_serde]
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
        owner: Option<Vec<Owner>>,
        add_owner: bool, //Add or Remove
        debt_auction: Option<String>,
        positions_contract: Option<String>,
        liquidity_contract: Option<String>,
    },
    EditOwner {
        owner: String,
        /// Liquidity multiplier for debt caps.
        /// Ex: 5 = debt cap at 5x liquidity
        liquidity_multiplier: Option<Decimal>,
        /// Distribute cap space from Stability Pool liquidity
        stability_pool_ratio: Option<Decimal>,
        /// Toggle authority over non-token contract state
        non_token_contract_auth: Option<bool>,
    },
}

#[cw_serde]
pub enum QueryMsg {
    Config {},
    GetOwner { owner: String },
    GetDenom {
        creator_address: String,
        subdenom: String,
    },
    GetContractDenoms {
        limit: Option<u32>,
    },
    /// For a given pool ID, list all tokens traded on it with current liquidity (spot).
    /// As well as the total number of LP shares and their denom
    PoolState {
        id: u64,
    },
    GetTokenInfo {
        denom: String,
    },
}

#[cw_serde]
pub struct Config {
    pub owners: Vec<Owner>,
    pub debt_auction: Option<Addr>,
    pub positions_contract: Option<Addr>,
    pub liquidity_contract: Option<Addr>,
}

// We define a custom struct for each query response
#[cw_serde]
pub struct GetDenomResponse {
    pub denom: String,
}

#[cw_serde]
pub struct TokenInfoResponse {
    pub denom: String,
    pub current_supply: Uint128,
    pub max_supply: Uint128,
}
