use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Uint128, Addr, Decimal};

use osmosis_std::types::osmosis::incentives::MsgCreateGauge;

use crate::types::{Owner, SwapRoute};

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
    /// Execute Swaps
    ExecuteSwaps {
        /// Token out
        token_out: String,
        /// Max slippage
        max_slippage: Decimal,
    }
}

#[cw_serde]
pub enum QueryMsg {
    /// Return contract config
    Config {},
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
    /// Return TokenInfoResponse
    GetTokenInfo {
        /// Native token denom
        denom: String,
    },
    /// Return list of swap routes
    GetSwapRoutes { },
}

#[cw_serde]
pub struct GetDenomResponse {
    /// Token full denom
    pub denom: String,
}

#[cw_serde]
pub struct OwnerResponse {
    /// Owner object
    pub owner: Owner,
    /// Liquidity multiplier for debt token token minting caps
    pub liquidity_multiplier: Decimal,
}

#[cw_serde]
pub struct TokenInfoResponse {
    /// Token full denom
    pub denom: String,
    /// Current supply
    pub current_supply: Uint128,
    /// Max supply
    pub max_supply: Uint128,
    /// Burned supply
    pub burned_supply: Uint128,
}

#[cw_serde]
pub struct ContractDenomsResponse {
    /// List of denoms owned by the contract
    pub denoms: Vec<String>,
}

#[cw_serde]
pub struct MigrateMsg {}