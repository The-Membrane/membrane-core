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
    /// Address of the governance contract
    pub governance_contract: String,
    /// Address of the staking contract
    pub staking_contract: String,
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
        /// If CDT, recapitalize bad debt
        /// If not, use auction to sell fees for a desired asset
        auction_asset: Asset,
    },
    /// Swap for discounted MBRN in any open CDT debt auction
    SwapForMBRN {},
    /// Swap for discounted fees with the configuration's desired asset
    SwapForFee { auction_asset: AssetInfo },
    /// Remove ongoing CDT auction, primarily for mistakes
    RemoveAuction {},
    /// Update config
    UpdateConfig(UpdateConfig),
}

#[cw_serde]
pub enum QueryMsg {
    /// Returns the current config
    Config {},
    /// Returns DebtAuction info
    DebtAuction {},
    /// Returns ongoing FeeAuctions
    OngoingFeeAuctions {
        /// Asset being sold 
        auction_asset: Option<AssetInfo>,
        /// Response limiter
        limit: Option<u64>,
        /// Return responses without this asset
        start_after: Option<u64>,
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
    /// CDT Denom
    pub cdt_denom: String,
    /// Asset to be bought by FeeAuctions
    pub desired_asset: String,
    /// Address of the positions contract
    pub positions_contract: Addr,
    /// Address of the governance contract
    pub governance_contract: Addr,
    /// Address of the staking contract
    pub staking_contract: Addr,
    /// Timeframe for MBRN TWAP in minutes
    pub twap_timeframe: u64,
    /// Initial discount for MBRN in auction
    pub initial_discount: Decimal,
    /// Timeframe for increase of discount in seconds
    pub discount_increase_timeframe: u64, 
    /// Increase in discount per unit of timeframe
    pub discount_increase: Decimal,
    /// Toggle sending FeeAuction assets to stakers instead of governance
    pub send_to_stakers: bool,
    
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
    /// CDT Denom
    pub cdt_denom: Option<String>,
    /// Asset to be bought by FeeAuctions
    pub desired_asset: Option<String>,
    /// Address of the positions contract
    pub positions_contract: Option<String>,
    /// Address of the governance contract
    pub governance_contract: Option<String>,
    /// Address of the staking contract
    pub staking_contract: Option<String>,
    /// Timeframe for MBRN TWAP in minutes
    pub twap_timeframe: Option<u64>,
    /// Initial discount for MBRN in auction
    pub initial_discount: Option<Decimal>,
    /// Timeframe for increase of discount in seconds
    pub discount_increase_timeframe: Option<u64>, 
    /// Increase in discount per unit of timeframe
    pub discount_increase: Option<Decimal>,
    /// Toggle sending FeeAuction assets to stakers instead of governance
    pub send_to_stakers: Option<bool>,
}

#[cw_serde]
pub struct MigrateMsg {}