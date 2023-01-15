use crate::types::{Asset, AssetInfo};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Binary, Decimal, Uint128};

/// ## Description
/// This structure describes the basic settings for creating a contract.
#[cw_serde]
pub struct InstantiateMsg {
    /// Apollo Collector contract address
    pub apollo_collector: String,

    pub blocks_till_price_expires: Option<u64>,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Swap where start asset is a native token
    Swap {
        to: SwapToAssetsInput,
        max_spread: Option<Decimal>,
        recipient: Option<String>,
        hook_msg: Option<Binary>,
    },
}


#[cw_serde]
pub enum RouterCallbackMsg {
    /// Check the swap amount is exceed minimum_receive
    AssertMinimumReceive {
        /// Asset info (Native or Token)
        target_asset: AssetInfo,
        /// Previous Balance before swap
        target_balance_before_swap: Uint128,
        /// Expected minimum to receive
        minimum_receive: Uint128,
        /// To Addr
        recipient: String,
    },
    SendTokens {
        token: AssetInfo,
        recipient: Addr,
        amount: Option<Uint128>,
        /// percentage of amount to send
        amount_pct: Option<Decimal>,
        hook_msg: Option<Binary>,
    },
}

/// ## Description
/// This structure describes the query messages of the contract.
#[cw_serde]
pub enum QueryMsg {
    /// Config returns controls settings that specified in custom [`ConfigResponse`] structure
    Config {},
    /// Get a list of all pairs on router
    Pairs {
        limit: Option<u32>,
        start_after: Option<AssetInfo>,
    },
}

#[cw_serde]
pub enum SwapToAssetsInput {
    /// Swap to single asset
    Single(AssetInfo),

    /// Swap to multiple assets, given weights of assets to allocate return to
    Multi(Vec<Asset>),
}
