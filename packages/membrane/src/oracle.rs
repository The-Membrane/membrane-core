use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Decimal, Uint128};

use crate::types::{AssetInfo, AssetOracleInfo, PriceInfo};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub osmosis_proxy: String,
    pub positions_contract: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {},
    Price {
        asset_info: AssetInfo,
        twap_timeframe: u64,        //in minutes
        basket_id: Option<Uint128>, //To switch oracle sources
    },
    Prices {
        asset_infos: Vec<AssetInfo>,
        twap_timeframe: u64, //in minutes
    },
    Asset {
        asset_info: AssetInfo,
    },
    Assets {
        asset_infos: Vec<AssetInfo>,
    },
}

// We define a custom struct for each query response
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct PriceResponse {
    pub prices: Vec<PriceInfo>,
    pub avg_price: Decimal,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct AssetResponse {
    pub asset_info: AssetInfo,
    pub oracle_info: Vec<AssetOracleInfo>,
}
