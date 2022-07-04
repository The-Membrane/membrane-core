use cosmwasm_std::{Decimal, Uint128};
use schemars::JsonSchema;
use serde::{Serialize, Deserialize};

use crate::{msg::{Asset, AssetInfo}, state::{LiqAsset, cAsset}};

//Structs needed to interact w/ the Stability Pool

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cw20HookMsg {
    Liquidate { //Calculate and execute liquidations
        credit_asset: Asset,
    },
    Distribute { //Distributes liquidated funds to users
        distribution_assets: Vec<cAsset>,
        credit_asset: AssetInfo,
    } 
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Distribute { //Distributes liquidated funds to users
        distribution_assets: Vec<cAsset>,
        credit_asset: AssetInfo,
        credit_price: Decimal,
    },
    Liquidate {
        credit_asset: LiqAsset,
        position_id: Uint128,
        basket_id: Uint128,
        position_owner: String,
    } 
}

// We define a custom struct for each query response
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct LiquidatibleResponse {
    pub leftover: Decimal,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    CheckLiquidatible { asset: LiqAsset }, //Check if the amount of said asset is liquidatible
}