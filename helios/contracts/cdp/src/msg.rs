use std::fmt;

use cosmwasm_std::{Addr, Uint128, Coin, Binary, Decimal};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::state::{cAsset, Position, LiqAsset, RepayFee};

//TODO: add cw20
use crate::cw20::Cw20ReceiveMsg;




#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssetInfo {
    Token{
        address: Addr,
    },
    NativeToken{
        denom: String,
    },
}

impl fmt::Display for AssetInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AssetInfo::NativeToken { denom } => write!(f, "{}", denom),
            AssetInfo::Token { address } => write!(f, "{}", address),
        }
    }
}

impl AssetInfo {

    pub fn is_native_token(&self) -> bool {
        match self {
            AssetInfo::NativeToken { .. } => true,
            AssetInfo::Token { .. } => false,
        }
    }

    pub fn equal(&self, asset: &AssetInfo) -> bool {
        match self {
            AssetInfo::Token { address, .. } => {
                let self_addr = address;
                match asset {
                    AssetInfo::Token { address, .. } => self_addr == address,
                    AssetInfo::NativeToken { .. } => false,
                }
            }
            AssetInfo::NativeToken { denom, .. } => {
                let self_denom = denom;
                match asset {
                    AssetInfo::Token { .. } => false,
                    AssetInfo::NativeToken { denom, .. } => self_denom == denom,
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Asset{
    pub info: AssetInfo,
    pub amount: Uint128,
}

impl fmt::Display for Asset {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}{}", self.amount, self.info)
    }
}


//Msg Start
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub collateral_types: Option<Vec<cAsset>>,
    pub credit_asset: Option<Asset>,
    pub credit_price: Option<Decimal>,
    pub credit_interest: Option<Decimal>,
    pub basket_owner: Option<String>,
}

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
        credit_asset: Asset,
    },
    LiqRepay {
        credit_asset: Asset,
        collateral_asset: Option<Asset>, //Only used by the liquidation queue since it repays by specific assets
        fee_ratios: Option<Vec<RepayFee>>,  //Used by liq_queue to provide list of ratios of repay amount in said fee 
    },
    CreateBasket {
        owner: Option<String>,
        collateral_types: Vec<cAsset>,
        credit_asset: Asset,
        credit_price: Option<Decimal>,
        credit_interest: Option<Decimal>,
    },
    EditBasket {
        basket_id: Uint128,
        added_cAsset: Option<cAsset>,
        owner: Option<String>,
        credit_interest: Option<Decimal>,
    }, 
    EditAdmin {
        owner: String,
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
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    GetUserPositions { //All positions from a user
        basket_id: Option<Uint128>, 
        user: String
    },
    GetPosition { //Singular position
        position_id: Uint128, 
        basket_id: Uint128, 
        user: String 
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
}

// We define a custom struct for each query response
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct PositionResponse {
    pub position_id: String,
    pub collateral_assets: Vec<cAsset>,
    pub avg_borrow_LTV: String,
    pub avg_max_LTV: String,
    pub credit_amount: String,
    pub basket_id: String,
    
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
    pub credit_asset: Asset, 
    pub credit_price: String,
    pub credit_interest: String,
}
