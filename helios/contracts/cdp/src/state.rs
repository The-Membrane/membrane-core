use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Uint128, Decimal};
use cw_storage_plus::{Item, Map};

//use crate::msg::{AssetInfo, Asset};

use membrane::types::{Asset, AssetInfo, RepayPropagation, Basket, Position};


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct LiqAsset {
    pub info: AssetInfo,
    pub amount: Decimal,
}


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub owner: Addr,
    pub current_basket_id: Uint128,
    pub stability_pool: Option<Addr>,
    pub dex_router: Option<Addr>, //Apollo's router, will need to change msg types if the router changes most likely.
    pub liq_fee_collector: Option<Addr>,
    pub interest_revenue_collector: Option<Addr>,
    pub osmosis_proxy: Option<Addr>,
    pub debt_auction: Option<Addr>,
    pub liq_fee: Decimal, //Enter as percent, 0.02
    pub oracle_time_limit: u64, //in seconds until oracle failure is accepted. Think of it as how many blocks you allow the oracle to fail for.
    //This needs to be large enough so that USDC positions are profitable to liquidate, 1-2% of debt value needs to be more than gas fees assuming ~98% LTV.
    pub debt_minimum: Uint128, //Debt minimum value per position. 
    
}


pub const CONFIG: Item<Config> = Item::new("config");
pub const POSITIONS: Map<(String, Addr), Vec<Position>> = Map::new("positions"); //basket_id, owner
pub const BASKETS: Map<String, Basket> = Map::new("baskets");
pub const REPAY: Item<RepayPropagation> = Item::new("repay_propagation");
