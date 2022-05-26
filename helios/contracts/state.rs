use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::{Item, Map};

use crate::msg::{AssetInfo, Asset};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct cAsset {
    pub asset: Asset, //amount is 0 when adding to basket_contract config
    pub oracle: Addr, 
    pub collateral_LTV: Uint128,

}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Position {
    pub position_id: Uint128,
    pub collateral_assets: Vec<cAsset>,
    pub avg_LTV: Uint128,
    pub credit_amount: Uint128,
    pub basket_id: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub owner: Addr,
    pub current_basket_id: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Basket {
    pub owner: Addr,
    pub current_position_id: Uint128,
    pub basket_contract: Addr,
    pub collateral_types: Vec<cAsset>, //Goverance creates cAssets that are added to the Config
    pub credit_asset: Asset, //Depending on type of token we use for credit this will be an Addr or denom (Cw20 or Native token respectively)
    pub repayment_price: Option<Uint128>,
}



pub const CONFIG: Item<Config> = Item::new("config");
pub const POSITIONS: Map<(String, Addr), Vec<Position>> = Map::new("positions");
pub const BASKETS: Map<String, Basket> = Map::new("baskets");


//LIQUIDATION QUEUE
//....