use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Uint128, Decimal};

use crate::types::{Asset, AssetPool, LiqAsset, cAsset, AssetInfo, Deposit};


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PositionUserInfo{
    pub basket_id: Uint128,
    pub position_id: Uint128,
}
pub struct InstantiateMsg {
    pub asset_pool: Option<AssetPool>,
    pub owner: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Deposit { //Deposit a list of accepted assets
        user: Option<String>, 
        assets: Vec<Asset> 
    },
    Withdraw { //Withdraw a list of accepted assets 
        assets: Vec<Asset>
    }, 
    Liquidate { //Use assets from an Asset pool to liquidate for contract owner (Positions Contract)
        credit_asset: LiqAsset,
        // position_id: Uint128,
        // basket_id: Uint128,
        // position_owner: String,
    }, 
    ClaimAs { //Claim ALL liquidation revenue, claim_as is a contract address
        claim_as: Option<String>,
        deposit_to: Option<PositionUserInfo>,
    }, 
    AddPool { //Adds an asset pool 
        asset_pool: AssetPool 
    },
    Distribute { //Distributes liquidated funds to users
        distribution_assets: Vec<cAsset>,
        credit_asset: AssetInfo,
        credit_price: Decimal,
    } 
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    //Check if the amount of said asset is liquidatible
    CheckLiquidatible { 
        asset: LiqAsset 
    }, 
    //User deposits in 1 AssetPool
    AssetDeposits{ 
        user: String, 
        asset_info: AssetInfo 
    }, 
    //Check if user has any claimable assets
    UserClaims{ 
        user: String 
    }, 
    //Returns asset pool info
    AssetPool{ 
        asset_info: AssetInfo 
    },
}

// We define a custom struct for each query response
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct LiquidatibleResponse {
    pub leftover: Decimal,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct DepositResponse {
    pub asset: AssetInfo,
    pub deposits: Vec<Deposit>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ClaimsResponse {
    pub claims: Vec<Asset>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct PoolResponse {
    pub credit_asset: Asset,
    pub liq_premium: Decimal,
    pub deposits: Vec<Deposit>
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cw20HookMsg {
    Distribute { //Distributes liquidated funds to users
        distribution_assets: Vec<cAsset>,
        credit_asset: AssetInfo,
    } 
} 




