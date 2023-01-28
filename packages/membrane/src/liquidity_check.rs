use cosmwasm_std::{Addr, Decimal};
use cosmwasm_schema::cw_serde;

use crate::types::{AssetInfo, LiquidityInfo};

#[cw_serde]
pub struct InstantiateMsg {
    /// Contract owner, defaults to info.sender
    pub owner: Option<String>,
    /// Osmosis Proxy contract address
    pub osmosis_proxy: String,
    /// Positions contract address
    pub positions_contract: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Add a new asset
    AddAsset {
        /// Asset liquidity info
        asset: LiquidityInfo,
    },
    /// Edit an existing asset
    EditAsset {
        /// Asset liquidity info
        asset: LiquidityInfo,
    },
    /// Remove an asset 
    RemoveAsset {
        /// Asset liquidity info
        asset: AssetInfo,
    },
    /// Update contract config
    UpdateConfig {
        /// Contract owner
        owner: Option<String>,
        /// Osmosis Proxy contract address
        osmosis_proxy: Option<String>,
        /// Positions contract address
        positions_contract: Option<String>,
        /// Stableswap liquidity multiplier
        stableswap_multiplier: Option<Decimal>,
    },
}

#[cw_serde]
pub enum QueryMsg {
    /// Return contract config
    Config {},  
    /// Return list of asset liquidity info
    Assets {
        /// Asset info to specific, i.e. 1 response
        asset_info: Option<AssetInfo>,
        /// Response limit
        limit: Option<u64>,
        /// Asset info to start after
        start_after: Option<AssetInfo>,
    },
    /// Return asset liquidity 
    Liquidity {
        /// Asset info
        asset: AssetInfo,
    },
}

#[cw_serde]
pub struct Config {
    /// Contract owner
    pub owner: Addr,
    /// Osmosis Proxy contract address
    pub osmosis_proxy: Addr,
    /// Positions contract address
    pub positions_contract: Addr,
    /// Stableswap liquidity multiplier
    pub stableswap_multiplier: Decimal,
}
