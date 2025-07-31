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