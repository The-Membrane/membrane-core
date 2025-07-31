use std::str::FromStr;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, Uint128, Addr, StdResult};

use pyth_sdk_cw::PriceIdentifier;

use crate::{types::{AssetInfo, OsmosisOracleInfo, PriceInfo, TWAPPoolInfo}, math::{decimal_multiplication, decimal_division, Decimal256, Uint256}};

#[cw_serde]
pub struct InstantiateMsg {
    /// Contract owner, defaults to info.sender
    pub owner: Option<String>,
    /// Pyth (chain-specific) address
    pub pyth_address: Option<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Update contract config
    UpdateConfig {
        /// Contract owner
        owner: Option<String>,
        /// Pyth (chain-specific) address
        pyth_address: Option<String>,
    },
    /// Add a new asset
    AddAsset {
        /// Asset info
        asset_info: String,
        /// Asset's oracle info
        oracle_info: OsmosisOracleInfo,
        /// Caller that we're saving the asset under
        caller: String,
    },
    /// Edit an existing asset
    EditAsset {
        /// Asset info
        asset_info: String,
        /// Asset's oracle info. Replaces existing oracle info.
        oracle_info: Option<OsmosisOracleInfo>,
        /// Caller that we're editing the asset for
        caller: String,
        /// Toggle to remove
        remove: bool,
    },
}

#[cw_serde]
pub enum QueryMsg {
    /// Return contract config
    Config {},
    /// Returns twap prices
    Prices {
        /// Caller 
        caller: String,
        /// Asset infos
        asset_infos: Vec<String>,
        /// Timeframe in minutes
        twap_timeframe: u64,
        /// (Pyth) Oracle time limit in seconds
        oracle_time_limit: u64,
    },
    /// Return list of asset oracle info
    Assets {
        /// List of asset infos
        asset_infos: Vec<AssetInfo>,
        /// Caller
        caller: String,
    },
}


#[cw_serde]
pub struct Config {
    /// Contract owner
    pub owner: Addr,
    /// Pyth (chain-specific) address
    pub pyth_address: Option<Addr>,
}


#[cw_serde]
pub struct MigrateMsg {}