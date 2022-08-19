use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Uint128, Coin, Decimal};

use crate::{types::Swap};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    CreateDenom {
        subdenom: String,
        basket_id: String,
    },
    ChangeAdmin {
        denom: String,
        new_admin_address: String,
    },
    MintTokens {
        denom: String,
        amount: Uint128,
        mint_to_address: String,
    },
    BurnTokens {
        denom: String,
        amount: Uint128,
        burn_from_address: String,
    },
    // ExitPool {
    //     sender: String,
    //     pool_id: u64,
    //     share_in_amount: Uint128,
    //     token_out_mins: Vec<Coin>,
    // },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    GetDenom {
        creator_address: String,
        subdenom: String,
    },
    //This will be replaced by TWAP but is here for testing
    SpotPrice { asset: String },
    /// For a given pool ID, list all tokens traded on it with current liquidity (spot).
    /// As well as the total number of LP shares and their denom
    PoolState { id: u64 },
    // Returns the accumulated historical TWAP of the given base asset and quote asset.
    // CONTRACT: start_time should be based on Unix time millisecond.
    ArithmeticTwapToNow {
        id: u64,
        quote_asset_denom: String,
        base_asset_denom: String,
        start_time: i64,
    },
}

// We define a custom struct for each query response
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct GetDenomResponse {
    pub denom: String,
}

// #[derive(Serialize, Deserialize, Clone, PartialEq, JsonSchema, Debug)]
// pub struct ArithmeticTwapToNowResponse {
//     pub twap: Decimal,
// }