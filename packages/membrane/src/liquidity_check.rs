use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::{AssetInfo, LiquidityInfo};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub osmosis_proxy: String,
    pub positions_contract: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    AddAsset {
        asset: LiquidityInfo,
    },
    EditAsset {
        asset: LiquidityInfo,
    },
    RemoveAsset {
        asset: AssetInfo,
    },
    UpdateConfig {
        owner: Option<String>,
        osmosis_proxy: Option<String>,
        positions_contract: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {},
    Assets {
        asset_info: Option<AssetInfo>,
        limit: Option<u64>,
        start_after: Option<AssetInfo>,
    },
    Liquidity {
        asset: AssetInfo,
    },
}
