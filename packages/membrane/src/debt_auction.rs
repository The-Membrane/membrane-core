use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, Addr};

use crate::types::{Asset, AssetInfo, UserInfo};

#[cw_serde]
pub struct InstantiateMsg {
    /// Address of the owner
    pub owner: Option<String>,
    /// Address of the oracle contract
    pub oracle_contract: String,
    /// Address of the osmosis proxy contract
    pub osmosis_proxy: String,
    /// Address of the positions contract
    pub positions_contract: String,
    /// Timeframe for MBRN TWAP in minutes
    pub twap_timeframe: u64,
    /// Native Denom of MBRN
    pub mbrn_denom: String,
    /// Initial discount for MBRN
    pub initial_discount: Decimal,
    /// Timeframe for increase of discount in seconds
    pub discount_increase_timeframe: u64,
    /// Increase in discount per unit of timeframe
    pub discount_increase: Decimal, 
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Start or add to ongoing auction.
    /// Callable by the owner (MBRN Governance) or Positions contract.
    StartAuction {
        /// Use auction to repay a position
        repayment_position_info: Option<UserInfo>,
        /// Use auction to buy CDT to send somewhere
        send_to: Option<String>,
        /// Asset being bought by MBRN
        debt_asset: Asset, 
    },
    /// Swap for MBRN with any open auction's swap_from_asset
    SwapForMBRN {},
    /// Remove ongoing auction, primarily for mistakes
    RemoveAuction {
        /// Asset being bought by MBRN
        debt_asset: AssetInfo,
    },
    /// Update config
    UpdateConfig(UpdateConfig),
}

#[cw_serde]
pub enum QueryMsg {
    /// Returns the current config
    Config {},
    /// Returns ongoing auctions
    OngoingAuctions {
        /// Asset being sold 
        debt_asset: Option<AssetInfo>,
        /// Response limiter
        limit: Option<u64>,
        /// Return responses without this asset
        start_without: Option<AssetInfo>,
    },
    /// Returns debt assets that can be used in auctions
    ValidDebtAssets {
        /// Debt asset to specifically query
        debt_asset: Option<AssetInfo>,
        /// Response limiter
        limit: Option<u64>,
        /// Return responses without this asset
        start_without: Option<AssetInfo>,
    },
}

#[cw_serde]
pub struct Config {
    /// Address of the owner
    pub owner: Addr,
    /// Address of the oracle contract
    pub oracle_contract: Addr,
    /// Address of the osmosis proxy contract
    pub osmosis_proxy: Addr,
    /// MBRN Denom
    pub mbrn_denom: String,
    /// Address of the positions contract
    pub positions_contract: Addr,
    /// Timeframe for MBRN TWAP in minutes
    pub twap_timeframe: u64,
    /// Initial discount for MBRN in auction
    pub initial_discount: Decimal,
    /// Timeframe for increase of discount in seconds
    pub discount_increase_timeframe: u64, 
    /// Increase in discount per unit of timeframe
    pub discount_increase: Decimal,       
}

#[cw_serde]
pub struct UpdateConfig {
    /// Address of the owner
    pub owner: Option<String>,
    /// Address of the oracle contract
    pub oracle_contract: Option<String>,
    /// Address of the osmosis proxy contract
    pub osmosis_proxy: Option<String>,
    /// MBRN Denom
    pub mbrn_denom: Option<String>,
    /// Address of the positions contract
    pub positions_contract: Option<String>,
    /// Timeframe for MBRN TWAP in minutes
    pub twap_timeframe: Option<u64>,
    /// Initial discount for MBRN in auction
    pub initial_discount: Option<Decimal>,
    /// Timeframe for increase of discount in seconds
    pub discount_increase_timeframe: Option<u64>, 
    /// Increase in discount per unit of timeframe
    pub discount_increase: Option<Decimal>,
}
