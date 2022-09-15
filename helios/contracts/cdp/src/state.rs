use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Uint128, Decimal};
use cw_storage_plus::{Item, Map};

//use crate::msg::{AssetInfo, Asset};

use membrane::types::{Asset, Basket, Position, SellWallDistribution, UserInfo};


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
    pub liq_fee: Decimal, //Enter as percent, 0.01
    pub collateral_twap_timeframe: u64, //in minutes
    pub credit_twap_timeframe: u64, //in minutes
    pub oracle_time_limit: u64, //in seconds until oracle failure is accepted. Think of it as how many blocks you allow the oracle to fail for.
    //% difference btwn credit TWAP and repayment price before the interest changes
    //Set to 100 if you want to turn off the PID
    pub cpc_margin_of_error: Decimal, 
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
pub struct RepayPropagation {
    pub per_asset_repayment: Vec<Decimal>,
    pub liq_queue_leftovers: Decimal, //List of repayments
    pub stability_pool: Decimal, //Value of repayment
    pub sell_wall_distributions: Vec<SellWallDistribution>,
    pub positions_contract: Addr,
    //So the sell wall knows who to repay to
    pub position_id: Uint128,
    pub basket_id: Uint128,
    pub position_owner: Addr,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct WithdrawPropagation {
    pub positions_prev_collateral: Vec<Asset>, //Amount of collateral in the position before the withdrawal
    pub withdraw_amounts: Vec<Uint128>,
    pub contracts_prev_collateral_amount: Vec<Uint128>,
    pub position_info: UserInfo,
    pub reply_order: Vec<usize>,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const POSITIONS: Map<(String, Addr), Vec<Position>> = Map::new("positions"); //basket_id, owner
pub const BASKETS: Map<String, Basket> = Map::new("baskets");
pub const CREDIT_MULTI: Map<String, Decimal> = Map::new("credit_multipliers");

pub const REPAY: Item<RepayPropagation> = Item::new("repay_propagation");
pub const WITHDRAW: Item<WithdrawPropagation> = Item::new("withdraw_propagation");
