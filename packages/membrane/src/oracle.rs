use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, Uint128, Addr};

use crate::types::{AssetInfo, AssetOracleInfo, PriceInfo};

#[cw_serde]
pub struct InstantiateMsg {
    /// Contract owner, defaults to info.sender
    pub owner: Option<String>,
    /// Osmosis Proxy contract address
    pub osmosis_proxy: String,
    /// Positions contract address
    pub positions_contract: Option<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Update contract config
    UpdateConfig {
        /// Contract owner
        owner: Option<String>,
        /// Osmosis Proxy contract address
        osmosis_proxy: Option<String>,
        /// Positions contract address
        positions_contract: Option<String>,
    },
    /// Add a new asset
    AddAsset {
        /// Asset info
        asset_info: AssetInfo,
        /// Asset's oracle info
        oracle_info: AssetOracleInfo,
    },
    /// Edit an existing asset
    EditAsset {
        /// Asset info
        asset_info: AssetInfo,
        /// Asset's oracle info
        oracle_info: Option<AssetOracleInfo>,
        /// Toggle to remove
        remove: bool,
    },
}

#[cw_serde]
pub enum QueryMsg {
    /// Return contract config
    Config {},
    /// Returns twap price
    Price {
        /// Asset info
        asset_info: AssetInfo,
        /// Timeframe in minutes
        twap_timeframe: u64,
        /// To switch on oracle sources.
        /// None defaults to 1, which is assumed the USD basket.
        basket_id: Option<Uint128>,
    },
    /// Returns twap prices
    Prices {
        /// Asset infos
        asset_infos: Vec<AssetInfo>,
        /// Timeframe in minutes
        twap_timeframe: u64,
    },
    /// Return asset oracle info
    Assets {
        /// List of asset infos
        asset_infos: Vec<AssetInfo> 
    },
}


#[cw_serde]
pub struct Config {
    /// Contract owner
    pub owner: Addr,
    /// Osmosis Proxy contract address
    pub osmosis_proxy: Addr,
    /// Positions contract address
    pub positions_contract: Option<Addr>,
}

#[cw_serde]
pub struct PriceResponse {
    /// List of PriceInfo from different sources
    pub prices: Vec<PriceInfo>,
    /// Median price
    pub price: Decimal,
}

#[cw_serde]
pub struct AssetResponse {
    /// Asset info
    pub asset_info: AssetInfo,
    /// Asset's list of oracle info
    pub oracle_info: Vec<AssetOracleInfo>,
}
