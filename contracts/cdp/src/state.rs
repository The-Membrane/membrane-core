use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Decimal, Uint128};
use cw_storage_plus::{Item, Map};


use membrane::types::{Asset, Basket, Position, UserInfo, AssetInfo};
use membrane::positions::Config;


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct LiquidationPropagation {
    pub per_asset_repayment: Vec<Decimal>,
    pub liq_queue_leftovers: Decimal, //List of repayments
    pub stability_pool: Decimal,      //Value of repayment
    pub sell_wall_distributions: Vec<(AssetInfo, Decimal)>,
    pub user_repay_amount: Decimal,
    pub positions_contract: Addr,
    //So the sell wall knows who to repay to
    pub position_info: UserInfo,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct WithdrawPropagation {
    pub positions_prev_collateral: Vec<Asset>, //Amount of collateral in the position before the withdrawal
    pub withdraw_amounts: Vec<Uint128>,
    pub contracts_prev_collateral_amount: Vec<Uint128>,
    pub position_info: UserInfo,
    pub reply_order: Vec<usize>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ClosePositionPropagation {
    pub withdrawn_assets: Vec<Asset>,
    pub position_info: UserInfo,
    pub send_to: Option<String>,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const BASKET: Item<Basket> = Item::new("basket"); 
pub const POSITIONS: Map<Addr, Vec<Position>> = Map::new("positions"); //owner

//Reply State Propagations
pub const WITHDRAW: Item<WithdrawPropagation> = Item::new("withdraw_propagation");
pub const LIQUIDATION: Item<LiquidationPropagation> = Item::new("repay_propagation");
pub const CLOSE_POSITION: Item<ClosePositionPropagation> = Item::new("close_position_propagation");
