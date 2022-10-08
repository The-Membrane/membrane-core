use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Decimal, Addr, Uint128};
use cw20::Cw20ReceiveMsg;

use crate::math::{Decimal256, Uint256};
use crate::types::{AssetInfo, Bid, BidInput};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub positions_contract: String,
    pub waiting_period: u64, //seconds
    pub basket_id: Option<Uint128>,
    pub bid_asset: Option<AssetInfo>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Receive(Cw20ReceiveMsg),
    SubmitBid {
        //Deposit a list of accepted assets
        bid_input: BidInput,
        bid_owner: Option<String>,
    },
    RetractBid {
        //Withdraw a list of accepted assets
        bid_id: Uint128,
        bid_for: AssetInfo,
        amount: Option<Uint256>, //If none, retracts full bid
    },
    Liquidate {
        //Use bids to fulfll liquidation of Position Contract basket. Called by Positions
        credit_price: Decimal,     //Sent from Position's contract
        collateral_price: Decimal, //Sent from Position's contract
        collateral_amount: Uint256,
        bid_for: AssetInfo,
        bid_with: AssetInfo,
        basket_id: Uint128,
        position_id: Uint128,
        position_owner: String,
    },
    ClaimLiquidations {
        bid_for: AssetInfo,
        bid_ids: Option<Vec<Uint128>>, //None = All bids in the queue
    },
    AddQueue {
        bid_for: AssetInfo,
        max_premium: Uint128, //A slot for each premium is created when queue is created
        bid_threshold: Uint256, //Minimum bid amount. Unlocks waiting bids if total_bids is less than.
    },
    UpdateQueue {
        bid_for: AssetInfo, //To signla which queue to edit. You can't edit the bid_for asset.
        max_premium: Option<Uint128>,
        bid_threshold: Option<Uint256>,
    },
    UpdateConfig {
        owner: Option<String>,
        positions_contract: Option<String>,
        waiting_period: Option<u64>,
        basket_id: Option<Uint128>,
    },
}


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {},
    Bid {
        bid_for: AssetInfo,
        bid_id: Uint128,
    },
    BidsByUser {
        bid_for: AssetInfo,
        user: String,
        limit: Option<u32>,
        start_after: Option<Uint128>,
    },
    Queue {
        bid_for: AssetInfo,
    },
    Queues {
        start_after: Option<AssetInfo>,
        limit: Option<u8>,
    },
    //Check if the amount of said asset is liquidatible
    //Position's contract is sending its basket.credit_price
    CheckLiquidatible {
        bid_for: AssetInfo,
        collateral_price: Decimal,
        collateral_amount: Uint256,
        credit_info: AssetInfo,
        credit_price: Decimal,
    },
    UserClaims {
        user: String,
    }, //Check if user has any claimable assets
    PremiumSlot {
        bid_for: AssetInfo,
        premium: u64, //Taken as %. 50 = 50%
    },
    PremiumSlots {
        bid_for: AssetInfo,
        start_after: Option<u64>, //Start after a premium value taken as a %.( 50 = 50%)
        limit: Option<u8>,
    },
}


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Config {
    pub owner: Addr, //Governance
    pub positions_contract: Addr,
    pub added_assets: Option<Vec<AssetInfo>>,
    pub waiting_period: u64, //Wait period is at max doubled due to slot_total calculation
    pub bid_asset: AssetInfo,
}

// We define a custom struct for each query response
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct SlotResponse {
    pub bids: Vec<Bid>,
    pub liq_premium: String,
    pub sum_snapshot: String,
    pub product_snapshot: String,
    pub total_bid_amount: String,
    pub current_epoch: Uint128,
    pub current_scale: Uint128,
    pub residue_collateral: String,
    pub residue_bid: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct ClaimsResponse {
    pub bid_for: String,
    pub pending_liquidated_collateral: Uint256,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct LiquidatibleResponse {
    pub leftover_collateral: String,
    pub total_credit_repaid: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct QueueResponse {
    pub bid_asset: String,
    pub max_premium: String,
    pub current_bid_id: String,
    pub bid_threshold: String,
}
