use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Decimal, Uint128};
use cw_storage_plus::{Item, Map};


use membrane::types::{Asset, Basket, Position, SellWallDistribution, UserInfo};
use membrane::positions::Config;


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct RepayPropagation {
    pub per_asset_repayment: Vec<Decimal>,
    pub liq_queue_leftovers: Decimal, //List of repayments
    pub stability_pool: Decimal,      //Value of repayment
    pub sell_wall_distributions: Vec<SellWallDistribution>,
    pub user_repay_amount: Decimal,
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
pub const BASKETS: Map<String, Basket> = Map::new("baskets"); //basket_id Basket
pub const CREDIT_MULTI: Map<String, Decimal> = Map::new("credit_multipliers"); //basket_id, multiplier

pub const REPAY: Item<RepayPropagation> = Item::new("repay_propagation");
pub const WITHDRAW: Item<WithdrawPropagation> = Item::new("withdraw_propagation");
