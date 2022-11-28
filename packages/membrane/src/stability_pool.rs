use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Decimal, Uint128, Addr};

use crate::types::{Asset, AssetInfo, AssetPool, Deposit, UserInfo};

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
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    UpdateConfig(UpdateConfig),
    Deposit {
        //Deposit the pool asset
        user: Option<String>,
        asset: AssetInfo,
    },
    Withdraw {
        // Unstake/Withdraw a list of accepted assets
        asset: Asset,
    },
    Restake {
        //Restake unstak(ed/ing) assets
        restake_asset: Decimal,
    },
    //Claim ALL liquidation revenue && MBRN incentives
    Claim {},
    Liquidate {
        //Use assets from an Asset pool to liquidate for a Position (Positions Contract)
        liq_amount: Decimal,
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
pub enum QueryMsg {
    //Returns Config
    Config {},
    //Get current MBRN incentive rate
    //Returns Decimal
    Rate { },
    //Query unclaimed incentives for a user
    //Returns Uint128
    UnclaimedIncentives { user: String },
    //Query capital ahead of frontmost user deposit
    //Returns DepositPositionResponse
    CapitalAheadOfDeposit { user: String },
    //Check if the amount of said asset is liquidatible
    //Returns LiquidatibleResponse
    CheckLiquidatible { amount: Decimal },
    //User deposits in the AssetPool
    //Returns Vec<Deposit>
    AssetDeposits { user: String },
    //Check if user has any claimable assets
    //Returns ClaimsResponse
    UserClaims { user: String },
    //Returns AssetPool
    AssetPool { },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Config {
    pub owner: Addr, //Governance contract address
    pub incentive_rate: Decimal,
    pub max_incentives: Uint128,
    //% of Supply desired in the SP.
    //Incentives decrease as it gets closer
    pub desired_ratio_of_total_credit_supply: Decimal,
    pub unstaking_period: u64, // in days
    pub mbrn_denom: String,
    pub osmosis_proxy: Addr,
    pub positions_contract: Addr,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct UpdateConfig {
    owner: Option<String>,
    incentive_rate: Option<Decimal>,
    max_incentives: Option<Uint128>,
    desired_ratio_of_total_credit_supply: Option<Decimal>,
    unstaking_period: Option<u64>,
    osmosis_proxy: Option<String>,
    positions_contract: Option<String>,
    mbrn_denom: Option<String>,
}

// We define a custom struct for each query response
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct LiquidatibleResponse {
    pub leftover: Decimal,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct ClaimsResponse {
    pub claims: Vec<Asset>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct DepositPositionResponse {
    pub deposit: Deposit,
    pub capital_ahead: Decimal,
}