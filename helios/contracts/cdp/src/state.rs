use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Uint128, Decimal};
use cw_storage_plus::{Item, Map};

use crate::msg::{AssetInfo, Asset};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct cAsset {
    pub asset: Asset, //amount is 0 when adding to basket_contract configor initiator
    pub oracle: String, //This is a String (not an Addr) so it can be used in eMsgs
    pub max_borrow_LTV: Decimal, //aka max borrow LTV
    pub max_LTV: Decimal, //ie liquidation point 
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct LiqAsset {
    pub info: AssetInfo,
    pub amount: Decimal,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Position {
    pub position_id: Uint128,
    pub collateral_assets: Vec<cAsset>,
    pub avg_borrow_LTV: Decimal,
    pub avg_max_LTV: Decimal,
    pub credit_amount: Decimal,
    pub basket_id: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub owner: Addr,
    pub current_basket_id: Uint128,
    pub stability_pool: Option<Addr>,
    pub liq_queue: Option<Addr>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Basket {
    pub owner: Addr,
    pub basket_id: Uint128,
    pub current_position_id: Uint128,
    pub collateral_types: Vec<cAsset>, 
    pub credit_asset: Asset, //Depending on type of token we use for credit this.info will be an Addr or denom (Cw20 or Native token respectively)
    pub credit_price: Option<Decimal>, //This is credit_repayment_price, not market price
    pub credit_interest: Option<Decimal>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct RepayPropagation {
    pub liq_queue: Decimal,
    pub stability_pool: Decimal,
    pub sell_wall: Decimal,
    pub position_id: Uint128,
    pub basket_id: Uint128,
    pub position_owner: Addr,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct RepayFee {
    pub fee: Decimal,
    pub ratio: Decimal,
}


pub const CONFIG: Item<Config> = Item::new("config");
pub const POSITIONS: Map<(String, Addr), Vec<Position>> = Map::new("positions"); //basket_id, owner
pub const BASKETS: Map<String, Basket> = Map::new("baskets");

pub const REPAY: Item<RepayPropagation> = Item::new("repay_propagation");

//LIQUIDATION QUEUE
//....