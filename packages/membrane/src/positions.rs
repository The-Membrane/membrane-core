use cosmwasm_std::{Addr, Decimal, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::{
    cAsset, Asset, AssetInfo, InsolventPosition, Position, PositionUserInfo, SellWallDistribution,
    SupplyCap, TWAPPoolInfo, UserInfo,
};

use cw20::Cw20ReceiveMsg;

//Msg Start
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
    pub interest_revenue_collector: Option<String>,
    pub osmosis_proxy: Option<String>,
    pub debt_auction: Option<String>,
    pub liquidity_contract: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    UpdateConfig {
        owner: Option<String>,
        stability_pool: Option<String>,
        dex_router: Option<String>,
        osmosis_proxy: Option<String>,
        debt_auction: Option<String>,
        staking_contract: Option<String>,
        oracle_contract: Option<String>,
        liquidity_contract: Option<String>,
        interest_revenue_collector: Option<String>,
        liq_fee: Option<Decimal>,
        debt_minimum: Option<Uint128>,
        base_debt_cap_multiplier: Option<Uint128>,
        oracle_time_limit: Option<u64>,
        credit_twap_timeframe: Option<u64>,
        collateral_twap_timeframe: Option<u64>,
        cpc_margin_of_error: Option<Decimal>,
        cpc_multiplier: Option<Decimal>,
        rate_slope_multiplier: Option<Decimal>,
    },
    Receive(Cw20ReceiveMsg),
    Deposit {
        basket_id: Uint128,
        position_id: Option<Uint128>, //If the user wants to create a new/separate position, no position id is passed
        position_owner: Option<String>,
    },
    IncreaseDebt {
        //only works on open positions
        basket_id: Uint128,
        position_id: Uint128,
        amount: Uint128,
        mint_to_addr: Option<String>,
    },
    Withdraw {
        basket_id: Uint128,
        position_id: Uint128,
        assets: Vec<Asset>,
    },
    Repay {
        basket_id: Uint128,
        position_id: Uint128,
        position_owner: Option<String>, //If not the sender
    },
    LiqRepay {},
    Liquidate {
        basket_id: Uint128,
        position_id: Uint128,
        position_owner: String,
    },
    MintRevenue {
        basket_id: Uint128,
        send_to: Option<String>, //Defaults to config.interest_revenue_collector
        repay_for: Option<UserInfo>, //Repay for a position w/ the revenue
        amount: Option<Uint128>,
    },
    CreateBasket {
        owner: Option<String>,
        collateral_types: Vec<cAsset>,
        credit_asset: Asset, //Creates native denom for Asset
        credit_price: Decimal,
        base_interest_rate: Option<Decimal>,
        desired_debt_cap_util: Option<Decimal>,
        credit_pool_ids: Vec<u64>, //For liquidity measuring
        liquidity_multiplier_for_debt_caps: Option<Decimal>, //Ex: 5 = debt cap at 5x liquidity
        liq_queue: Option<String>,
    },
    EditBasket {
        basket_id: Uint128,
        added_cAsset: Option<cAsset>,
        owner: Option<String>,
        liq_queue: Option<String>,
        credit_pool_ids: Option<Vec<u64>>, //For liquidity measuring
        liquidity_multiplier: Option<Decimal>,
        collateral_supply_caps: Option<Vec<SupplyCap>>,
        base_interest_rate: Option<Decimal>,
        desired_debt_cap_util: Option<Decimal>,
        credit_asset_twap_price_source: Option<TWAPPoolInfo>,
        negative_rates: Option<bool>, //Allow negative repayment interest or not
    },
    //Clone basket. Reset supply_caps. Sets repayment price to new oracle price.
    //When using this to add a new UoA:
    // Add logic to change oracle quote asset in Oracle contract
    //
    //Note: Edit pool_ids if desired
    CloneBasket {
        basket_id: Uint128,
    },
    EditcAsset {
        basket_id: Uint128,
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cw20HookMsg {
    Deposit {
        basket_id: Uint128,
        position_owner: Option<String>,
        position_id: Option<Uint128>,
    },
}

// NOTE: Since CallbackMsg are always sent by the contract itself, we assume all types are already
// validated and don't do additional checks. E.g. user addresses are Addr instead of String
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CallbackMsg {
    BadDebtCheck {
        basket_id: Uint128,
        position_id: Uint128,
        position_owner: Addr,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {},
    GetUserPositions {
        //All positions from a user
        basket_id: Option<Uint128>,
        user: String,
        limit: Option<u32>,
    },
    GetPosition {
        //Singular position
        basket_id: Uint128,
        position_id: Uint128,
        position_owner: String,
    },
    GetBasketPositions {
        //All positions in a basket
        basket_id: Uint128,
        start_after: Option<String>,
        limit: Option<u32>,
    },
    GetBasket {
        basket_id: Uint128,
    }, //Singular basket
    GetAllBaskets {
        //All baskets
        start_after: Option<Uint128>,
        limit: Option<u32>,
    },
    GetBasketDebtCaps {
        basket_id: Uint128,
    },
    GetBasketBadDebt {
        basket_id: Uint128,
    },
    GetBasketInsolvency {
        basket_id: Uint128,
        start_after: Option<String>,
        limit: Option<u32>,
    },
    GetPositionInsolvency {
        basket_id: Uint128,
        position_id: Uint128,
        position_owner: String,
    },
    GetBasketInterest {
        basket_id: Uint128,
    },
    GetCollateralInterest {
        basket_id: Uint128,
    },
    //Used internally to test state propagation
    Propagation {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub owner: Addr,
    pub current_basket_id: Uint128,
    pub stability_pool: Option<Addr>,
    pub dex_router: Option<Addr>, //Apollo's router, will need to change msg types if the router changes most likely.
    pub interest_revenue_collector: Option<Addr>,
    pub staking_contract: Option<Addr>,
    pub osmosis_proxy: Option<Addr>,
    pub debt_auction: Option<Addr>,
    pub oracle_contract: Option<Addr>,
    pub liquidity_contract: Option<Addr>,
    pub liq_fee: Decimal,               //Enter as percent, 0.01
    pub collateral_twap_timeframe: u64, //in minutes
    pub credit_twap_timeframe: u64,     //in minutes
    pub oracle_time_limit: u64, //in seconds until oracle failure is accepted. Think of it as how many blocks you allow the oracle to fail for.
    //% difference btwn credit TWAP and repayment price before the interest changes
    //Set to 100 if you want to turn off the PID
    pub cpc_margin_of_error: Decimal,
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

// We define a custom struct for each query response
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct PositionResponse {
    pub position_id: Uint128,
    pub collateral_assets: Vec<cAsset>,
    //Allows front ends to get ratios using the smae oracles
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct BasketResponse {
    pub owner: String,
    pub basket_id: String,
    pub current_position_id: String,
    pub collateral_types: Vec<cAsset>,
    pub collateral_supply_caps: Vec<SupplyCap>,
    pub credit_asset: Asset,
    pub credit_price: Decimal,
    pub liq_queue: String,
    pub base_interest_rate: Decimal, //Enter as percent, 0.02
    pub liquidity_multiplier: Decimal,
    pub desired_debt_cap_util: Decimal, //Enter as percent, 0.90
    pub pending_revenue: Uint128,
    pub negative_rates: bool, //Allow negative repayment interest or not
}


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct PropResponse {
    pub liq_queue_leftovers: Decimal,
    pub stability_pool: Decimal,
    pub sell_wall_distributions: Vec<SellWallDistribution>,
    pub positions_contract: String,
    //So the sell wall knows who to repay to
    pub position_id: Uint128,
    pub basket_id: Uint128,
    pub position_owner: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct DebtCapResponse {
    pub caps: String,
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
