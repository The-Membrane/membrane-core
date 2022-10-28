use crate::types::{Asset, AssetInfo};

use cosmwasm_std::{Addr, Binary, Decimal, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// ## Description
/// This structure describes the basic settings for creating a contract.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct InstantiateMsg {
    /// Apollo Collector contract address
    pub apollo_collector: String,

    pub blocks_till_price_expires: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    /// Swap where start asset is a native token
    Swap {
        to: SwapToAssetsInput,
        max_spread: Option<Decimal>,
        recipient: Option<String>,
        hook_msg: Option<Binary>,
    },
}

/// CW20 Hook
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cw20HookMsg {
    Swap {
        to: SwapToAssetsInput,
        max_spread: Option<Decimal>,
        recipient: Option<String>,
        hook_msg: Option<Binary>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
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
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    /// Config returns controls settings that specified in custom [`ConfigResponse`] structure
    Config {},
    /// Get a list of all pairs on router
    Pairs {
        limit: Option<u32>,
        start_after: Option<AssetInfo>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum SwapToAssetsInput {
    /// Swap to single asset
    Single(AssetInfo),

    /// Swap to multiple assets, given weights of assets to allocate return to
    Multi(Vec<Asset>),
}
