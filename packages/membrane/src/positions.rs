use std::fmt;

use cosmwasm_std::{Addr, Uint128, Coin, Binary, Decimal};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::{ Asset, cAsset, Position, LiqAsset, SellWallDistribution, AssetInfo, UserInfo, PositionUserInfo, InsolventPosition };

use cw20::Cw20ReceiveMsg;


//Msg Start
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub oracle_time_limit: u64, //in seconds until oracle failure is acceoted
    pub debt_minimum: Uint128, //Debt minimum value per position
    pub liq_fee: Decimal,
    //Contracts
    pub stability_pool: Option<String>,
    pub dex_router: Option<String>,
    pub liq_fee_collector: Option<String>,
    pub interest_revenue_collector: Option<String>,
    pub osmosis_proxy: Option<String>,
    pub debt_auction: Option<String>,
    //For Basket creation
    pub collateral_types: Option<Vec<cAsset>>,
    pub credit_asset: Option<Asset>,
    pub credit_price: Option<Decimal>,
    pub credit_interest: Option<Decimal>,
    pub collateral_supply_caps: Option<Vec<Uint128>>,
    pub base_interest_rate: Option<Decimal>,
    pub desired_debt_cap_util: Option<Decimal>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Receive(Cw20ReceiveMsg),
    Deposit{
        assets: Vec<AssetInfo>,
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
        credit_asset: Asset,
        credit_price: Option<Decimal>,
        credit_interest: Option<Decimal>,
        collateral_supply_caps: Option<Vec<Uint128>>,
        base_interest_rate: Option<Decimal>,
        desired_debt_cap_util: Option<Decimal>,
        
    },
    EditBasket {
        basket_id: Uint128,
        added_cAsset: Option<cAsset>,
        owner: Option<String>,
        credit_interest: Option<Decimal>,
        liq_queue: Option<String>,
        pool_ids: Option<Vec<u64>>,
        liquidity_multiplier: Option<Decimal>,
        collateral_supply_caps: Option<Vec<Uint128>>,
        base_interest_rate: Option<Decimal>,
        desired_debt_cap_util: Option<Decimal>,
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
    pub collateral_supply_caps: Vec<Uint128>,
    pub credit_asset: Asset, 
    pub credit_price: String,
    pub credit_interest: String,
    pub debt_pool_ids: Vec<u64>,
    pub debt_liquidity_multiplier_for_caps: Decimal, //Ex: 5 = debt cap at 5x liquidity.
    pub liq_queue: String,
    pub base_interest_rate: Decimal, //Enter as percent, 0.02
    pub desired_debt_cap_util: Decimal, //Enter as percent, 0.90
    pub pending_revenue: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ConfigResponse {
    pub owner: String,
    pub current_basket_id: Uint128,
    pub stability_pool: String,
    pub dex_router: String, //Apollo's router, will need to change msg types if the router changes most likely.
    pub liq_fee_collector: String,
    pub interest_revenue_collector: String,
    pub osmosis_proxy: String,
    pub debt_auction: String,
    pub liq_fee: Decimal, // 5 = 5%
    pub oracle_time_limit: u64,
    pub debt_minimum: Uint128,
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