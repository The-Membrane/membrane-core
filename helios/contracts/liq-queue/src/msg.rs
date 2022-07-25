use cosmwasm_bignumber::{Uint256, Decimal256};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Uint128, Decimal};

use crate::{state::{Asset, LiqAsset, cAsset, AssetInfo, BidInput, Bid, PremiumSlot}, cw20::Cw20ReceiveMsg};


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PositionUserInfo{
    pub basket_id: Uint128,
    pub position_id: Uint128,
}
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub waiting_period: u64, //seconds
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Recieve(Cw20ReceiveMsg),
    SubmitBid { //Deposit a list of accepted assets
        bid_input: BidInput,
        bid_owner: Option<String>,
    },
    RetractBid { //Withdraw a list of accepted assets 
        bid_id: Uint128,
        bid_for: AssetInfo,
        amount: Option<Uint128>, //If none, retracts full bid
    }, 
    Liquidate { //Use bids to fulfll liquidation of Position Contract basket
        credit_price: Decimal, //Sent from Position's contract
        collateral_price: Decimal, //Sent from Position's contract
        collateral_amount: Uint128,
        bid_for: AssetInfo,
        credit_info: AssetInfo,
    }, 
    UpdateConfig{
        owner: Option<String>,
        waiting_period: Option<u64>,
    },
    AddQueue{    
        bid_for: AssetInfo,
        bid_asset: AssetInfo, //This should always be the same credit_asset but will leave open for flexibility
        max_premium: Uint128, //A slot for each premium is created when queue is created
        bid_threshold: Uint256, //Minimum bid amount. Unlocks waiting bids if total_bids is less than.
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cw20HookMsg {
    // Liquidate { //Use bids to fulfll liquidation of Position Contract basket
    //     credit_price: Decimal, //Sent from Position's contract
    //     collateral_price: Decimal, //Sent from Position's contract
    //     collateral_amount: Uint128,
    // }, 
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {},
    Bid {
        bid_for: AssetInfo, 
        bid_id: Uint128, 
    },
    //Check if the amount of said asset is liquidatible
    //Position's contract is sending its basket.credit_price
    CheckLiquidatible { 
        bid_for: AssetInfo,
        collateral_price: Decimal,
        collateral_amount: Uint128,
        credit_info: AssetInfo,
        credit_price: Decimal,
    },
    AssetDeposits{ user: String, asset_info: AssetInfo }, //User deposits in 1 AssetPool
    UserClaims{ user: String }, //Check if user has any claimable assets
    AssetPool{ asset_info: AssetInfo }, //Returns asset pool info
}

// We define a custom struct for each query response
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct SlotResponse {
    pub liq_premium: String,
    pub sum_snapshot: String,
    pub product_snapshot: String,
    pub total_bid_amount: String,
    pub current_epoch: Uint128,
    pub current_scale: Uint128,
    pub residue_collateral: String,
    pub residue_bid: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ConfigResponse {
    pub owner: String, 
    pub waiting_period: u64,
    pub added_assets: Vec<AssetInfo>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct BidResponse {
    pub user: String,
    pub id: Uint128,
    pub amount: Uint256,
    pub liq_premium: u8,
    pub product_snapshot: Decimal256,
    pub sum_snapshot: Decimal256,
    pub pending_liquidated_collateral: Uint256,
    pub wait_end: Option<u64>,
    pub epoch_snapshot: Uint128,
    pub scale_snapshot: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ClaimsResponse {
    pub bid_for: String,
    pub pending_liquidated_collateral: Uint256
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct LiquidatibleResponse {
    pub leftover_collateral: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct QueueResponse {
    pub bid_asset: String,
    pub max_premium: String, 
    pub slots: Vec<PremiumSlot>,
    pub current_bid_id: String,
    pub bid_threshold: String,
}
