use std::str::FromStr;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, Uint128, Addr, StdResult};

use pyth_sdk_cw::PriceIdentifier;

use crate::{types::{AssetInfo, OsmosisOracleInfo, OsmosisRouteInfo, PriceInfo, TWAPPoolInfo}, math::{decimal_multiplication, decimal_division, Decimal256, Uint256}};

#[cw_serde]
pub struct InstantiateMsg {
    /// Contract owner, defaults to info.sender
    pub owner: Option<String>
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Execute a swap
    Swap {
        /// Caller
        caller: String,
        // Token In
        token_in: String,
        // Token Out
        token_out: String,
        // Max slippage
        max_slippage: Decimal,
    },
    /// Update contract config
    UpdateConfig {
        /// Contract owner
        owner: Option<String>,
    },
    /// Add a new asset route
    AddRoute {
        /// Caller
        caller: String,
        /// Asset denom
        denom: String,
        /// Asset route info
        route_info: OsmosisRouteInfo,
    },
    /// Edit an existing route
    EditRoute {
        /// Caller
        caller: String,
        /// Asset denom
        denom: String,
        /// Asset route info
        route_info: Option<OsmosisRouteInfo>,
        /// Toggle to remove
        remove: bool,
    },
}

#[cw_serde]
pub enum QueryMsg {
    /// Return contract config
    Config {},
    /// Return list of asset routes
    Routes {
        /// Caller
        caller: String,
        /// List of asset denoms
        asset_infos: Option<Vec<String>> 
    },
}


#[cw_serde]
pub struct Config {
    /// Contract owner
    pub owner: Addr,
}


#[cw_serde]
pub struct MigrateMsg {}