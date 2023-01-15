use cosmwasm_std::{Addr, Decimal};
use cosmwasm_schema::cw_serde;

use crate::types::{AssetInfo, LiquidityInfo};

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub osmosis_proxy: String,
    pub positions_contract: String,
}

#[cw_serde]
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
        stableswap_multiplier: Option<Decimal>,
    },
}

#[cw_serde]
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

#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub osmosis_proxy: Addr,
    pub positions_contract: Addr,
    pub stableswap_multiplier: Decimal,
}
