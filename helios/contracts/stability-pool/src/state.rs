use core::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Uint128, Decimal};
use cw_storage_plus::{Item, Map};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub owner: Addr, //A singular positions contract address
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct User {
    //pub user: Addr,
    pub claimable_assets: Vec<Asset>, //Collateral assets earned from liquidations
}


pub const CONFIG: Item<Config> = Item::new("config");
pub const ASSETS: Item<Vec<AssetPool>> = Item::new("assets"); //Acts as the asset WL and the sum of all deposits for said asset
pub const USERS: Map<Addr, User> = Map::new("users"); //Used to map claims to users 
pub const ORACLES: Map<AssetInfo, Addr> = Map::new("oracles");


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct LiqAsset {
    pub info: AssetInfo,
    pub amount: Decimal,
}

impl fmt::Display for LiqAsset {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.amount, self.info)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct cAsset {
    pub asset: Asset, //amount is 0 when adding to basket_contract configor initiator
    pub oracle: String, //This is a String (not an Addr) so it can be used in eMsgs
    pub max_borrow_LTV: Decimal, //aka max borrow LTV
    pub max_LTV: Decimal, //ie liquidation point 
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct UserRatio {
    pub user: Addr,
    pub ratio: Decimal,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Deposit {
    pub user: Addr,
    pub amount: Decimal,

}

impl fmt::Display for Deposit {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.user, self.amount)
    }
}

impl Deposit {

    pub fn equal(&self, deposits: &Vec<Deposit>) -> bool {

        let mut check = false;
        for deposit in deposits.iter(){

            if self.amount == deposit.amount && self.user == deposit.user{
                check = true;
            }
        }

        check
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct AssetPool {
    pub credit_asset: Asset,
    pub liq_premium: Decimal,
    pub deposits: Vec<Deposit>
}

impl fmt::Display for AssetPool {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.credit_asset)
    }
}


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
        write!(f, "{} {}", self.amount, self.info)
    }
}