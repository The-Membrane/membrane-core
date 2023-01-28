use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Uint128, Addr, Decimal};

use crate::types::Owner;

#[cw_serde]
pub struct InstantiateMsg {}

#[cw_serde]
pub enum ExecuteMsg {
    /// Create a new native token denom
    CreateDenom {
        /// Subdenom of the token
        subdenom: String,        
        /// Max supply of the token.
        /// Enforced by the contract, not Osmosis.
        max_supply: Option<Uint128>,
    },
    /// Change the admin of a denom
    ChangeAdmin {
        /// Native token denom
        denom: String,
        /// New admin address
        new_admin_address: String,
    },
    /// Mint tokens of a denom owned by the contract
    MintTokens {
        /// Native token denom
        denom: String,
        /// Amount to mint
        amount: Uint128,
        /// Mint to address
        mint_to_address: String,
    },
    /// Burn tokens
    BurnTokens {
        /// Native token denom
        denom: String,
        /// Amount to burn
        amount: Uint128,
        /// Burn from address
        burn_from_address: String,
    },
    /// Edit the max supply of a denom
    EditTokenMaxSupply {
        /// Native token denom
        denom: String,
        /// New max supply
        max_supply: Uint128,
    },
    /// Update contract config
    UpdateConfig {
        /// List of owners
        owners: Option<Vec<Owner>>,
        /// Toggle to add or remove list of owners
        add_owner: bool,
        /// Debt auction contract address
        debt_auction: Option<String>,
        /// Positions contract address
        positions_contract: Option<String>,
        /// Liquidity contract address
        liquidity_contract: Option<String>,
    },
    /// Edit owner params & permissions
    EditOwner {
        /// Owner address
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
    /// Return contract config
    Config {},
    /// Return Owner
    GetOwner {
        /// Owner address
        owner: String
    },
    /// Return GetDenomResponse
    GetDenom {
        /// Denom creator address
        creator_address: String,
        /// Subdenom of the token
        subdenom: String,
    },
    /// Return list of denoms owned by the contract
    GetContractDenoms {
        /// Response limit
        limit: Option<u32>,
    },
    /// For a given pool ID, list all tokens traded on it with current liquidity (spot).
    /// As well as the total number of LP shares and their denom.
    /// Queried from Osmosis.
    PoolState {
        /// Pool ID
        id: u64,
    },
    /// Return TokenInfoResponse
    GetTokenInfo {
        /// Native token denom
        denom: String,
    },
}

#[cw_serde]
pub struct Config {
    /// List of owners
    pub owners: Vec<Owner>,
    /// Debt auction contract address
    pub debt_auction: Option<Addr>,
    /// Positions contract address
    pub positions_contract: Option<Addr>,
    /// Liquidity contract address
    pub liquidity_contract: Option<Addr>,
}

#[cw_serde]
pub struct GetDenomResponse {
    /// Token full denom
    pub denom: String,
}

#[cw_serde]
pub struct TokenInfoResponse {
    /// Token full denom
    pub denom: String,
    /// Current supply
    pub current_supply: Uint128,
    /// Max supply
    pub max_supply: Uint128,
}
