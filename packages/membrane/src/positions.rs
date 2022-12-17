use cosmwasm_std::{Addr, Decimal, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::{
    cAsset, Asset, AssetInfo, InsolventPosition, Position, PositionUserInfo,
    SupplyCap, MultiAssetSupplyCap, TWAPPoolInfo, UserInfo,
};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub oracle_time_limit: u64, //in seconds until oracle failure is acceoted
    pub debt_minimum: Uint128,  //Debt minimum value per position
    pub liq_fee: Decimal,
    pub collateral_twap_timeframe: u64, //in minutes
    pub credit_twap_timeframe: u64,     //in minutes
    //Contracts
    pub stability_pool: Option<String>,
    pub dex_router: Option<String>,
    pub staking_contract: Option<String>,
    pub oracle_contract: Option<String>,
    pub osmosis_proxy: Option<String>,
    pub debt_auction: Option<String>,
    pub liquidity_contract: Option<String>,
    pub discounts_contract: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    UpdateConfig(UpdateConfig),
    Deposit {
        position_id: Option<Uint128>, //If the user wants to create a new/separate position, no position id is passed
        position_owner: Option<String>,
    },
    //Increase debt by an amount or to a LTV
    IncreaseDebt {
        position_id: Uint128,
        amount: Option<Uint128>,
        LTV: Option<Decimal>,
        mint_to_addr: Option<String>,
    },
    Withdraw {
        position_id: Uint128,
        assets: Vec<Asset>,
        send_to: Option<String>, //If not the sender
    },
    Repay {
        position_id: Uint128,
        position_owner: Option<String>, //If not the sender
        send_excess_to: Option<String>, //If not the sender
    },
    LiqRepay {},
    Liquidate {
        position_id: Uint128,
        position_owner: String,
    },
    ClosePosition {
        position_id: Uint128,
        max_spread: Decimal,
        send_to: Option<String>,
    },
    Accrue { position_id: Uint128 },
    MintRevenue {
        send_to: Option<String>, 
        repay_for: Option<UserInfo>, //Repay for a position w/ the revenue
        amount: Option<Uint128>,
    },
    //Non-USD denominated baskets don't work due to the debt minimum
    CreateBasket {
        basket_id: Uint128,
        collateral_types: Vec<cAsset>,
        credit_asset: Asset, //Creates native denom for Asset
        credit_price: Decimal,
        base_interest_rate: Option<Decimal>,
        credit_pool_ids: Vec<u64>, //For liquidity measuring
        liquidity_multiplier_for_debt_caps: Option<Decimal>, //Ex: 5 = debt cap at 5x liquidity
        liq_queue: Option<String>,
    },
    EditBasket(EditBasket),
    EditcAsset {
        asset: AssetInfo,
        //Editables
        max_borrow_LTV: Option<Decimal>, //aka what u can borrow up to
        max_LTV: Option<Decimal>,        //ie liquidation point
    },
    EditAdmin {
        owner: String,
    },
    //Callbacks; Only callable by the contract
    Callback(CallbackMsg),
}


// NOTE: Since CallbackMsg are always sent by the contract itself, we assume all types are already
// validated and don't do additional checks. E.g. user addresses are Addr instead of String
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CallbackMsg {
    BadDebtCheck {
        position_id: Uint128,
        position_owner: Addr,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {},
    // GetUserPositions {
    //     //All positions from a user
    //     user: String,
    //     limit: Option<u32>,
    // },
    // GetPosition {
    //     //Singular position
    //     position_id: Uint128,
    //     position_owner: String,
    // },
    // GetBasketPositions {
    //     //All positions in a basket
    //     start_after: Option<String>,
    //     limit: Option<u32>,
    // },
    GetBasket { }, //Singular basket
    //GetBasketDebtCaps { },
    //GetBasketBadDebt { },
    //GetPositionInsolvency {
    //     position_id: Uint128,
    //     position_owner: String,
    // },
    //GetCreditRate { },
    //GetCollateralInterest { },
    //Used internally to test state propagation
    Propagation {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub owner: Addr,
    pub stability_pool: Option<Addr>,
    pub dex_router: Option<Addr>, //Apollo's router, will need to change msg types if the router changes
    pub staking_contract: Option<Addr>,
    pub osmosis_proxy: Option<Addr>,
    pub debt_auction: Option<Addr>,
    pub oracle_contract: Option<Addr>,
    pub liquidity_contract: Option<Addr>,
    pub discounts_contract: Option<Addr>,
    pub liq_fee: Decimal,               //Enter as percent, 0.01
    pub collateral_twap_timeframe: u64, //in minutes
    pub credit_twap_timeframe: u64,     //in minutes
    pub oracle_time_limit: u64, //in seconds until oracle failure is accepted. Think of it as how many blocks you allow the oracle to fail for.
    //Augment the rate of increase per % difference for the redemption rate
    pub cpc_multiplier: Decimal,
    //This needs to be large enough so that USDC positions are profitable to liquidate,
    //1-2% of liquidated debt (max -> borrow_LTV) needs to be more than gas fees assuming ~98% LTV.
    pub debt_minimum: Uint128, //Debt minimum value per position.
    //Debt Minimum multiplier for base debt cap
    //ie; How many users do we want at 0 credit liquidity?
    pub base_debt_cap_multiplier: Uint128,
    //Interest rate 2nd Slope multiplier
    pub rate_slope_multiplier: Decimal,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct UpdateConfig {
    pub owner: Option<String>,
    pub stability_pool: Option<String>,
    pub dex_router: Option<String>,
    pub osmosis_proxy: Option<String>,
    pub debt_auction: Option<String>,
    pub staking_contract: Option<String>,
    pub oracle_contract: Option<String>,
    pub liquidity_contract: Option<String>,
    pub discounts_contract: Option<String>,
    pub liq_fee: Option<Decimal>,
    pub debt_minimum: Option<Uint128>,
    pub base_debt_cap_multiplier: Option<Uint128>,
    pub oracle_time_limit: Option<u64>,
    pub credit_twap_timeframe: Option<u64>,
    pub collateral_twap_timeframe: Option<u64>,
    pub cpc_multiplier: Option<Decimal>,
    pub rate_slope_multiplier: Option<Decimal>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct EditBasket {
    pub added_cAsset: Option<cAsset>,
    pub liq_queue: Option<String>,
    pub credit_pool_ids: Option<Vec<u64>>, //For liquidity measuring
    pub liquidity_multiplier: Option<Decimal>,
    pub collateral_supply_caps: Option<Vec<SupplyCap>>,
    pub multi_asset_supply_caps: Option<Vec<MultiAssetSupplyCap>>,
    pub base_interest_rate: Option<Decimal>,
    pub credit_asset_twap_price_source: Option<TWAPPoolInfo>,
    pub negative_rates: Option<bool>, //Allow negative repayment interest or not
    pub cpc_margin_of_error: Option<Decimal>,
    pub frozen: Option<bool>,
    pub rev_to_stakers: Option<bool>,
}

// We define a custom struct for each query response
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct PositionResponse {
    pub position_id: Uint128,
    pub collateral_assets: Vec<cAsset>,
    //Allows front ends to get ratios using the same oracles
    //Useful for users who want to deposit or withdraw at the current ratio
    pub cAsset_ratios: Vec<Decimal>,
    pub credit_amount: Uint128,
    pub basket_id: Uint128,
    pub avg_borrow_LTV: Decimal,
    pub avg_max_LTV: Decimal,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct PositionsResponse {
    pub user: String,
    pub positions: Vec<Position>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct BadDebtResponse {
    pub has_bad_debt: Vec<(PositionUserInfo, Uint128)>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct InsolvencyResponse {
    pub insolvent_positions: Vec<InsolventPosition>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct InterestResponse {
    pub credit_interest: Decimal,
    pub negative_rate: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct CollateralInterestResponse {
    pub rates: Vec<(AssetInfo, Decimal)>,
}
