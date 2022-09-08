use std::fmt;

use cosmwasm_std::{Addr, Uint128, Coin, Binary, Decimal};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::{ Asset, cAsset, Position, LiqAsset, SellWallDistribution, AssetInfo, UserInfo, PositionUserInfo, InsolventPosition, TWAPPoolInfo, PoolInfo, SupplyCap };

use cw20::Cw20ReceiveMsg;


//Msg Start
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub oracle_time_limit: u64, //in seconds until oracle failure is acceoted
    pub debt_minimum: Uint128, //Debt minimum value per position
    pub liq_fee: Decimal,
    pub twap_timeframe: u64, //in days
    //Contracts
    pub stability_pool: Option<String>,
    pub dex_router: Option<String>,
    pub staking_contract: Option<String>,
    pub oracle_contract: Option<String>,
    pub interest_revenue_collector: Option<String>,
    pub osmosis_proxy: Option<String>,
    pub debt_auction: Option<String>,
    // // //For Basket creation
    // pub collateral_types: Option<Vec<cAsset>>,
    // pub credit_asset: Option<Asset>,
    // pub credit_price: Option<Decimal>,
    // pub collateral_supply_caps: Option<Vec<Decimal>>,
    // pub base_interest_rate: Option<Decimal>,
    // pub desired_debt_cap_util: Option<Decimal>,
    // pub credit_asset_twap_price_source: Option<TWAPPoolInfo>,
    // pub credit_pool_ids: Option<Vec<u64>>, 
    // pub liquidity_multiplier_for_debt_caps: Option<Decimal>,
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
        interest_revenue_collector: Option<String>,
        liq_fee: Option<Decimal>,
        debt_minimum: Option<Uint128>,
        base_debt_cap_multiplier: Option<Uint128>,
        oracle_time_limit: Option<u64>,
        twap_timeframe: Option<u64>,
        cpc_margin_of_error: Option<Decimal>,
        rate_slope_multiplier: Option<Decimal>,
    },
    Receive(Cw20ReceiveMsg),
    Deposit{
        basket_id: Uint128,
        position_id: Option<Uint128>, //If the user wants to create a new/separate position, no position id is passed         
        position_owner: Option<String>,
    },
    IncreaseDebt { //only works on open positions
        basket_id: Uint128,
        position_id: Uint128,
        amount: Uint128,
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
    LiqRepay {
        credit_asset: Asset,
    },
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
        pool_ids: Option<Vec<u64>>,
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
        max_LTV: Option<Decimal>, //ie liquidation point 
    }, 
    EditAdmin {
        owner: String,
    },
    //Callbacks; Only callable by the contract
    Callback( CallbackMsg ),    
}


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
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
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CallbackMsg {
    BadDebtCheck {
        basket_id: Uint128,
        position_id: Uint128,
        position_owner: Addr,
    },
}


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {},
    GetUserPositions { //All positions from a user 
        basket_id: Option<Uint128>, 
        user: String,
        limit: Option<u32>,
    },
    GetPosition { //Singular position 
        basket_id: Uint128, 
        position_id: Uint128, 
        position_owner: String 
    },
    GetBasketPositions { //All positions in a basket
        basket_id: Uint128,
        start_after: Option<String>,
        limit: Option<u32>,
    },
    GetBasket { basket_id: Uint128 }, //Singular basket  
    GetAllBaskets { //All baskets
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
    //Used internally to test state propagation
    Propagation {},
}

// We define a custom struct for each query response
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct PositionResponse {
    pub position_id: String,
    pub collateral_assets: Vec<cAsset>,
    pub credit_amount: String,
    pub basket_id: String,
    pub avg_borrow_LTV: Decimal,
    pub avg_max_LTV: Decimal,
    
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct PositionsResponse{
    pub user: String,
    pub positions: Vec<Position>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct BasketResponse{
    pub owner: String,
    pub basket_id: String,
    pub current_position_id: String,
    pub collateral_types: Vec<cAsset>, 
    pub collateral_supply_caps: Vec<SupplyCap>,
    pub credit_asset: Asset, 
    pub credit_price: Decimal,
    pub credit_pool_ids: Vec<u64>,
    pub liq_queue: String,
    pub base_interest_rate: Decimal, //Enter as percent, 0.02
    pub liquidity_multiplier: Decimal,
    pub desired_debt_cap_util: Decimal, //Enter as percent, 0.90
    pub pending_revenue: Uint128,
    pub negative_rates: bool, //Allow negative repayment interest or not
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ConfigResponse {
    pub owner: String,
    pub current_basket_id: Uint128,
    pub stability_pool: String,
    pub dex_router: String, //Apollo's router, will need to change msg types if the router changes most likely.
    pub interest_revenue_collector: String,
    pub staking_contract: String,
    pub osmosis_proxy: String,
    pub debt_auction: String,
    pub oracle_contract: String,
    pub liq_fee: Decimal, // 5 = 5%
    pub oracle_time_limit: u64,
    pub debt_minimum: Uint128,
    pub base_debt_cap_multiplier: Uint128,
    pub twap_timeframe: u64,
    pub cpc_margin_of_error: Decimal,
    pub rate_slope_multiplier: Decimal,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct DebtCapResponse{
    pub caps: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct BadDebtResponse{
    pub has_bad_debt: Vec<( PositionUserInfo, Uint128 )>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InsolvencyResponse{
    pub insolvent_positions: Vec<InsolventPosition>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InterestResponse{
    pub credit_interest: Decimal,
    pub negative_rate: bool,
}