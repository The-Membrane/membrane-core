use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Uint128, Decimal};

use crate::state::{Asset, AssetPool, LiqAsset, cAsset, AssetInfo};


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Liquidate { //Use bids to fulfll liquidation of Position Contract basket
        credit_price: Decimal, //Sent from Position's contract
        collateral_price: Decimal, //Sent from Position's contract
        collateral_amount: Uint128,
        bid_for: AssetInfo,
        credit_info: AssetInfo,
    }, 
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cw20HookMsg {
    // Liquidate { //Use bids to fulfll liquidation of Position Contract basket
    //     credit_price: Decimal, //Sent from Position's contract
    //     collateral_price: Decimal, //Sent from Position's contract
    // }, 
}


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    //Check if the amount of said asset is liquidatible
    //Position's contract is sending its basket.credit_price
    CheckLiquidatible { 
        bid_for: AssetInfo,
        collateral_price: Decimal,
        collateral_amount: Uint128,
        credit_info: AssetInfo,
        credit_price: Decimal,
    },
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct LiquidatibleResponse {
    pub leftover: Uint128
}

