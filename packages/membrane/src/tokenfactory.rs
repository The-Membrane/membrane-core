use std::str::FromStr;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, Uint128, Addr, StdResult};

use pyth_sdk_cw::PriceIdentifier;

use crate::{types::{AssetInfo, OsmosisOracleInfo, OsmosisRouteInfo, PriceInfo, TWAPPoolInfo}, math::{decimal_multiplication, decimal_division, Decimal256, Uint256}};
use osmosis_std::types::{cosmos::base::v1beta1::Coin, osmosis::tokenfactory::v1beta1::{self as TokenFactory}};

#[cw_serde]
pub struct InstantiateMsg {
    /// Contract owner, defaults to info.sender
    pub owner: Option<String>
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Update contract config
    UpdateConfig {
        /// Contract owner
        owner: Option<String>,
    },
    /// TokenFactory messages
    CreateDenom {
        subdenom: String,
    },
    MintTokens {
        amount: Option<Coin>,
        mint_to_address: String,
    },
    BurnTokens { }
}

#[cw_serde]
pub enum QueryMsg {
    /// Return contract config
    Config {},
    /// Return list of asset routes
    Denoms {
        /// Limit
        limit: Option<u32>,
        /// Start after
        start_after: Option<String>,
    },
}


#[cw_serde]
pub struct Config {
    /// Contract owner
    pub owner: Addr,
}


#[cw_serde]
pub struct MigrateMsg {}

use cosmwasm_std::{CosmosMsg};
use osmosis_std::types::{
    cosmos::base::v1beta1::Coin as OsmosisCoin,
    osmosis::tokenfactory::v1beta1::{MsgCreateDenom, MsgMint, MsgBurn},
};

/// Build a create denom CosmosMsg. If `contract` is Some, calls the external TokenFactory contract via Wasm; 
/// otherwise emits the native MsgCreateDenom.
pub fn create_denom_msg(contract: Option<Addr>, sender: &str, subdenom: &str) -> CosmosMsg {
    match contract {
        Some(addr) => CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute {
            contract_addr: addr.to_string(),
            msg: cosmwasm_std::to_binary(&ExecuteMsg::CreateDenom { subdenom: subdenom.to_string() }).unwrap(),
            funds: vec![],
        }),
        None => MsgCreateDenom {
            sender: sender.to_string(),
            subdenom: subdenom.to_string(),
        }
        .into(),
    }
}

/// Build a mint tokens CosmosMsg
pub fn mint_msg(contract: Option<Addr>, sender: &str, denom: &str, amount: cosmwasm_std::Uint128, to: &str) -> CosmosMsg {
    match contract {
        Some(addr) => CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute {
            contract_addr: addr.to_string(),
            msg: cosmwasm_std::to_binary(&ExecuteMsg::MintTokens {
                amount: Some(OsmosisCoin { denom: denom.to_string(), amount: amount.to_string() }),
                mint_to_address: to.to_string(),
            }).unwrap(),
            funds: vec![],
        }),
        None => MsgMint {
            sender: sender.to_string(),
            amount: Some(OsmosisCoin { denom: denom.to_string(), amount: amount.to_string() }),
            mint_to_address: to.to_string(),
        }
        .into(),
    }
}

/// Build a burn tokens CosmosMsg
pub fn burn_msg(contract: Option<Addr>, sender: &str, denom: &str, amount: cosmwasm_std::Uint128, burn_from: &str) -> CosmosMsg {
    match contract {
        Some(addr) => CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute {
            contract_addr: addr.to_string(),
            msg: cosmwasm_std::to_binary(&ExecuteMsg::BurnTokens {}).unwrap(),
            funds: vec![cosmwasm_std::Coin { denom: denom.to_string(), amount }],
        }),
        None => MsgBurn {
            sender: sender.to_string(),
            amount: Some(OsmosisCoin { denom: denom.to_string(), amount: amount.to_string() }),
            burn_from_address: burn_from.to_string(),
        }
        .into(),
    }
} 