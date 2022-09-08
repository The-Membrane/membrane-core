use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Uint128, Decimal};

use crate::types::{Swap, Asset, UserInfo, RepayPosition, AssetInfo};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    //Start or add to existing auction
    //Callable by the owner (MBRN Governance) or Positions contract
    StartAuction {
        repayment_position_info: UserInfo,
        debt_asset: Asset,
    },
    //Swap for MBRN w/ any open auction's swap_from_asset
    SwapForMBRN { },
    UpdateConfig {
        owner: Option<String>,
        oracle_contract: Option<String>,
        osmosis_proxy: Option<String>,
        mbrn_denom: Option<String>,
        positions_contract: Option<String>,
        twap_timeframe: Option<u64>,
        initial_discount: Option<Decimal>,
        discount_increase_timeframe: Option<u64>, //in seconds
        discount_increase: Option<Decimal>, //% increase
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {},
    OngoingAuction {
        swap_asset: AssetInfo,
    },
}


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct AuctionResponse {
    pub remaining_recapitalization: Uint128,
    pub repayment_positions: Vec<RepayPosition>,  //Repayment amount, Positions info
    pub auction_start_time: u64,
    pub basket_id_price_source: Uint128,
}

