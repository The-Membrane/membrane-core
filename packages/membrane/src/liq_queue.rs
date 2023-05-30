use cosmwasm_schema::cw_serde;

use cosmwasm_std::{Decimal, Addr, Uint128};

use crate::math::{Decimal256, Uint256};
use crate::types::{AssetInfo, Bid, BidInput, Asset};

#[cw_serde]
pub struct InstantiateMsg {
    /// Contract owner
    pub owner: Option<String>,
    /// Positions contract address
    pub positions_contract: String,
    /// Waiting period before bids are activated
    pub waiting_period: u64, 
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Deposit an accepted asset to create a bid
    SubmitBid {
        /// Bid info 
        bid_input: BidInput,
        /// Bidder address, defaults to msg.sender
        bid_owner: Option<String>,
    },
    /// Withdraw assets from a bid
    RetractBid {
        /// Bid id
        bid_id: Uint128,
        /// Asset being bid for
        bid_for: AssetInfo,
        /// Amount to withdraw, None = withdraw full bid
        amount: Option<Uint256>,
    },
    /// Use bids to fulfll liquidation of Position Contract basket. Called by Positions
    Liquidate {
        /// Basket credit price, sent from Position's contract
        credit_price: Decimal,
        /// Collateral price, sent from Position's contract
        collateral_price: Decimal,
        /// Collateral amount
        collateral_amount: Uint256,
        /// Collateral asset info to bid_for
        bid_for: AssetInfo,
        /// Position id to liquidate
        position_id: Uint128,
        /// Position owner 
        position_owner: String,
    },
    /// Claim liquidated assets
    ClaimLiquidations {
        /// Collateral asset info the bid was for
        bid_for: AssetInfo,
        /// Bid ids to claim, none = all bids in the collateral's queue
        bid_ids: Option<Vec<Uint128>>,
    },
    /// Add a new queue
    AddQueue {
        /// Asset to bid for
        bid_for: AssetInfo,
        /// Max premium to pay for a slot. 
        /// A slot for each premium is created when queue is created.
        max_premium: Uint128, 
        /// Minimum bid amount. Unlocks waiting bids if total_bids is less than.
        bid_threshold: Uint256,
    },
    /// Update a queue
    UpdateQueue {
        /// To signal which queue to edit. You can't edit the bid_for asset.
        bid_for: AssetInfo, 
        /// Max premium to pay for a slot
        max_premium: Option<Uint128>,
        /// Minimum bid amount. Unlocks waiting bids if total_bids is less than.
        bid_threshold: Option<Uint256>,
    },
    /// Update contract config
    UpdateConfig {
        /// Contract owner
        owner: Option<String>,
        /// Positions contract address
        positions_contract: Option<String>,
        /// Waiting period before bids are activated
        waiting_period: Option<u64>,
    },
}


#[cw_serde]
pub enum QueryMsg {
    /// Returns contract config
    Config {},
    /// Returns BidResponse
    Bid {
        /// Bid for asset 
        bid_for: AssetInfo,
        /// Bid id
        bid_id: Uint128,
    },
    /// Returns BidResponse for a user's bids in a queue
    BidsByUser {
        /// Bid for asset
        bid_for: AssetInfo,
        /// User address
        user: String,
        /// Response limit
        limit: Option<u32>,
        /// Start after bid id
        start_after: Option<Uint128>,
    },
    /// Returns QueueResponse
    Queue {
        /// Bid for asset
        bid_for: AssetInfo,
    },
    /// Returns multiple QueueResponses
    Queues {
        /// Start after bid_for asset
        start_after: Option<AssetInfo>,
        /// Response limit
        limit: Option<u8>,
    },
    /// Check if the amount of said asset is liquidatible. Returns LiquidatibleResponse.
    CheckLiquidatible {
        /// Bid for asset
        bid_for: AssetInfo,
        /// Collateral price
        collateral_price: Decimal,
        /// Collateral amount
        collateral_amount: Uint256,
        /// Credit asset info
        credit_info: AssetInfo,
        /// Credit price
        credit_price: Decimal,
    },
    /// Returns User's claimable assetss
    UserClaims {
        /// User address
        user: String,
    },
    /// Returns SlotResponse
    PremiumSlot {
        /// Bid for asset
        bid_for: AssetInfo,
        /// Premium slot. Taken as % 50 = 50%.
        premium: u64, 
    },
    /// Returns multiple SlotResponse
    PremiumSlots {
        /// Bid for asset
        bid_for: AssetInfo,
        /// Start after premium value taken as a %, ( 50 = 50%)
        start_after: Option<u64>, 
        /// Response limit
        limit: Option<u8>,
    },
}


#[cw_serde]
pub struct Config {
    /// Contract owner
    pub owner: Addr,
    /// Positions contract address
    pub positions_contract: Addr,
    /// Available assets to bid for
    pub added_assets: Option<Vec<AssetInfo>>,
    /// Waiting period before bids are activated to prevent bots frontrunning bids.
    /// Wait period is at max doubled due to slot_total calculation.
    pub waiting_period: u64, 
    /// Bid with asset
    pub bid_asset: AssetInfo,
}

#[cw_serde]
pub struct SlotResponse {
    /// List of bids
    pub bids: Vec<Bid>,
    /// Liquidation premium 
    pub liq_premium: String,
    /// Sum Snapshot
    pub sum_snapshot: String,
    /// Product Snapshot
    pub product_snapshot: String,
    /// Total bid amount
    pub total_bid_amount: String,
    /// Current epoch
    pub current_epoch: Uint128,
    /// Current scale
    pub current_scale: Uint128,
    /// Collateral residue
    pub residue_collateral: String,
    /// Bid residue
    pub residue_bid: String,
}

#[cw_serde]
pub struct BidResponse {
    /// User address
    pub user: String,
    /// Bid id
    pub id: Uint128,
    /// Bid amount
    pub amount: Uint256,
    /// Liquidation premium
    pub liq_premium: u8,
    /// Product Snapshot
    pub product_snapshot: Decimal256,
    /// Sum Snapshot
    pub sum_snapshot: Decimal256,
    /// Pending liquidated collateral
    pub pending_liquidated_collateral: Uint256,
    /// Wait period end time, in seconds
    pub wait_end: Option<u64>,
    /// Epoch snapshot
    pub epoch_snapshot: Uint128,
    /// Scale snapshot
    pub scale_snapshot: Uint128,
}

#[cw_serde]
pub struct ClaimsResponse {
    /// Bid for asset
    pub bid_for: String,
    /// Claimable collateral
    pub pending_liquidated_collateral: Uint256,
}

#[cw_serde]
pub struct LiquidatibleResponse {
    /// Non-liquidatible collateral
    pub leftover_collateral: String,
    /// Total debt repaid
    pub total_debt_repaid: String,
}

#[cw_serde]
pub struct QueueResponse {
    /// Bid for asset
    pub bid_asset: Asset,
    /// Max premium
    pub max_premium: Uint128,
    /// Current bid id
    pub current_bid_id: Uint128,
    /// Minimum bid amount
    pub bid_threshold: Uint256,
}
