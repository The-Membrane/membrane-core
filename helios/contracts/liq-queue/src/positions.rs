use std::fmt;

use cosmwasm_std::{Addr, Uint128, Coin, Binary, Decimal};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{cw20::Cw20ReceiveMsg, state::Asset};



#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Receive(Cw20ReceiveMsg),
    Deposit{
        assets: Vec<Asset>,
        position_owner: Option<String>,
        basket_id: Uint128,
        position_id: Option<Uint128>, //If the user wants to create a new/separate position, no position id is passed         
    },
    Repay {
        basket_id: Uint128,
        position_id: Uint128,
        position_owner: Option<String>, //If not the sender
        credit_asset: Asset,
    },
    LiqRepay {
        credit_asset: Asset,
    },
  
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cw20HookMsg {
    Deposit {
        basket_id: Uint128,
        position_owner: Option<String>,
        position_id: Option<Uint128>,
    },
    Repay {
        basket_id: Uint128,
        position_id: Uint128,
        position_owner: Option<String>, //If not the sender
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct RepayFee {
    pub fee: Decimal,
    pub ratio: Decimal,
}

    