use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, Addr};

use crate::types::{Asset, AssetInfo, UserInfo};

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub oracle_contract: String,
    pub osmosis_proxy: String,
    pub positions_contract: String,
    pub twap_timeframe: u64,
    pub mbrn_denom: String,
    pub initial_discount: Decimal,
    pub discount_increase_timeframe: u64,
    pub discount_increase: Decimal, //Increase in discount per unit of timeframe
}

#[cw_serde]
pub enum ExecuteMsg {
    //Start or add to ongoing auction
    //Callable by the owner (MBRN Governance) or Positions contract
    StartAuction {
        //Repay a position
        repayment_position_info: Option<UserInfo>,
        //or buy CDT to send somewhere
        send_to: Option<String>,
        //Asset being bought by MBRN
        debt_asset: Asset, 
    },
    //Swap for MBRN w/ any open auction's swap_from_asset
    SwapForMBRN {},
    //Remove ongoing auction
    //Mostly for potential mistakes
    RemoveAuction {
        debt_asset: AssetInfo,
    },
    UpdateConfig(UpdateConfig),
}

#[cw_serde]
pub enum QueryMsg {
    Config {},
    OngoingAuctions {
        debt_asset: Option<AssetInfo>,
        limit: Option<u64>,
        start_without: Option<AssetInfo>,
    },
    ValidDebtAssets {
        debt_asset: Option<AssetInfo>,
        limit: Option<u64>,
        start_without: Option<AssetInfo>,
    },
}

#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub oracle_contract: Addr,
    pub osmosis_proxy: Addr,
    pub mbrn_denom: String,
    pub positions_contract: Addr,
    pub twap_timeframe: u64,
    pub initial_discount: Decimal,
    pub discount_increase_timeframe: u64, //in seconds
    pub discount_increase: Decimal,       //% increase
}

#[cw_serde]
pub struct UpdateConfig {
    pub owner: Option<String>,
    pub oracle_contract: Option<String>,
    pub osmosis_proxy: Option<String>,
    pub mbrn_denom: Option<String>,
    pub positions_contract: Option<String>,
    pub twap_timeframe: Option<u64>,
    pub initial_discount: Option<Decimal>,
    pub discount_increase_timeframe: Option<u64>, //in seconds
    pub discount_increase: Option<Decimal>,       //% increase
}
