use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, Uint128, Addr};

use crate::types::{AssetInfo, AssetOracleInfo, PriceInfo};

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub osmosis_proxy: String,
    pub positions_contract: Option<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    UpdateConfig {
        owner: Option<String>,
        osmosis_proxy: Option<String>,
        positions_contract: Option<String>,
    },
    AddAsset {
        asset_info: AssetInfo,
        oracle_info: AssetOracleInfo,
    },
    EditAsset {
        asset_info: AssetInfo,
        oracle_info: Option<AssetOracleInfo>,
        remove: bool,
    },
}

#[cw_serde]
pub enum QueryMsg {
    Config {},
    Price {
        asset_info: AssetInfo,
        twap_timeframe: u64,    //in minutes
        //To switch on oracle sources
        //None defaults to 1, which is assumed the USD basket
        basket_id: Option<Uint128>,
    },
    Prices {
        asset_infos: Vec<AssetInfo>,
        twap_timeframe: u64, //in minutes
    },
    Assets { asset_infos: Vec<AssetInfo> },
}


#[cw_serde]
pub struct Config {
    pub owner: Addr, //MBRN Governance
    pub osmosis_proxy: Addr,
    pub positions_contract: Option<Addr>,
}

// We define a custom struct for each query response
#[cw_serde]
pub struct PriceResponse {
    pub prices: Vec<PriceInfo>,
    pub price: Decimal,
}

#[cw_serde]
pub struct AssetResponse {
    pub asset_info: AssetInfo,
    pub oracle_info: Vec<AssetOracleInfo>,
}
