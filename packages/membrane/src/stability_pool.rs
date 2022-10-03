use cw20::Cw20ReceiveMsg;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Decimal, Uint128};

use crate::types::{Asset, AssetInfo, AssetPool, Deposit, LiqAsset, PositionUserInfo, UserInfo};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub asset_pool: Option<AssetPool>,
    pub incentive_rate: Option<Decimal>,
    pub max_incentives: Option<Uint128>,
    pub desired_ratio_of_total_credit_supply: Option<Decimal>,
    pub osmosis_proxy: String,
    pub positions_contract: String,
    pub mbrn_denom: String,
    pub dex_router: Option<String>,
    pub max_spread: Option<Decimal>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    UpdateConfig {
        owner: Option<String>,
        incentive_rate: Option<Decimal>,
        max_incentives: Option<Uint128>,
        desired_ratio_of_total_credit_supply: Option<Decimal>,
        unstaking_period: Option<u64>,
        osmosis_proxy: Option<String>,
        positions_contract: Option<String>,
        mbrn_denom: Option<String>,
        dex_router: Option<String>,
        max_spread: Option<Decimal>,
    },
    Receive(Cw20ReceiveMsg),
    Deposit {
        //Deposit a list of accepted assets
        user: Option<String>,
        assets: Vec<AssetInfo>,
    },
    Withdraw {
        // Unstake/Withdraw a list of accepted assets
        assets: Vec<Asset>,
    },
    Restake {
        //Restake unstak(ed/ing) assets
        restake_asset: LiqAsset,
    },
    Claim {
        //Claim ALL liquidation revenue && MBRN incentives
        claim_as_native: Option<String>,      //Native FullDenom
        claim_as_cw20: Option<String>,        //Contract Address
        deposit_to: Option<PositionUserInfo>, //Deposit to Position in CDP contract
    },
    ////Only callable by the owner////
    AddPool {
        //Adds an asset pool
        asset_pool: AssetPool,
    },
    Liquidate {
        //Use assets from an Asset pool to liquidate for a Position (Positions Contract)
        credit_asset: LiqAsset,
    },
    Distribute {
        //Distributes liquidated funds to users
        distribution_assets: Vec<Asset>,
        distribution_asset_ratios: Vec<Decimal>,
        credit_asset: AssetInfo,
        distribute_for: Uint128,
    },
    //Allow the Positions contract to use user funds to repay for themselves
    Repay {
        user_info: UserInfo,
        repayment: Asset,
    },
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cw20HookMsg {
    Distribute {
        //Distributes liquidated funds to users
        credit_asset: AssetInfo,
        distribute_for: Uint128,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {},
    //Get current MBRN incentive rate
    Rate {
        asset_info: AssetInfo,
    },
    //Query unclaimed incentives for a user
    UnclaimedIncentives {
        user: String,
        asset_info: AssetInfo,
    },
    //Query capital ahead of user deposits
    CapitalAheadOfDeposit {
        user: String,
        asset_info: AssetInfo,
    },
    //Check if the amount of said asset is liquidatible
    CheckLiquidatible { asset: LiqAsset },
    //User deposits in 1 AssetPool
    AssetDeposits { user: String, asset_info: AssetInfo },
    //Check if user has any claimable assets
    UserClaims { user: String },
    //Returns asset pool info
    AssetPool { asset_info: AssetInfo },
}

// We define a custom struct for each query response
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct LiquidatibleResponse {
    pub leftover: Decimal,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct DepositResponse {
    pub asset: AssetInfo,
    pub deposits: Vec<Deposit>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct ClaimsResponse {
    pub claims: Vec<Asset>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct PoolResponse {
    pub credit_asset: Asset,
    pub liq_premium: Decimal,
    pub deposits: Vec<Deposit>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct DepositPositionResponse {
    pub deposit: Deposit,
    pub capital_ahead: Decimal,
}