use cosmwasm_schema::cw_serde;

use cosmwasm_std::{Decimal, Uint128, Addr};

use crate::types::{Asset, AssetPool, Deposit, UserInfo};

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub asset_pool: AssetPool,
    pub incentive_rate: Option<Decimal>,
    pub max_incentives: Option<Uint128>,
    pub osmosis_proxy: String,
    pub positions_contract: String,
    pub mbrn_denom: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    UpdateConfig(UpdateConfig),
    Deposit {
        //Deposit the pool asset
        user: Option<String>,
    },
    Withdraw {
        // Unstake/Withdraw
        amount: Uint128,
    },
    Restake {
        //Restake unstak(ed/ing) assets
        restake_amount: Decimal,
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
        distribute_for: Uint128,
    },
    //Allow the Positions contract to use user funds to repay for themselves
    Repay {
        user_info: UserInfo,
        repayment: Asset,
    },
}

#[cw_serde]
pub enum QueryMsg {
    //Returns Config
    Config {},
    //Query unclaimed incentives for a user
    //Returns Uint128
    UnclaimedIncentives { user: String },
    //Query capital ahead of frontmost user deposit
    //Returns DepositPositionResponse
    CapitalAheadOfDeposit { user: String },
    //Check if the amount of said asset is liquidatible
    //Returns LiquidatibleResponse
    CheckLiquidatible { amount: Decimal },
    //Check if user has any claimable assets
    //Returns ClaimsResponse
    UserClaims { user: String },
    //Returns AssetPool
    AssetPool { 
        user: Option<String>,
        deposit_limit: Option<u32>,
        start_after: Option<u32>,        
    },
}

#[cw_serde]
pub struct Config {
    pub owner: Addr, //Governance contract address
    pub incentive_rate: Decimal,
    pub max_incentives: Uint128,
    pub unstaking_period: u64, // in days
    pub mbrn_denom: String,
    pub osmosis_proxy: Addr,
    pub positions_contract: Addr,
}

#[cw_serde]
pub struct UpdateConfig {
    pub owner: Option<String>,
    pub incentive_rate: Option<Decimal>,
    pub max_incentives: Option<Uint128>,
    pub unstaking_period: Option<u64>,
    pub osmosis_proxy: Option<String>,
    pub positions_contract: Option<String>,
    pub mbrn_denom: Option<String>,
}

// We define a custom struct for each query response
#[cw_serde]
pub struct LiquidatibleResponse {
    pub leftover: Decimal,
}

#[cw_serde]
pub struct ClaimsResponse {
    pub claims: Vec<Asset>,
}

#[cw_serde]
pub struct DepositPositionResponse {
    pub deposit: Deposit,
    pub capital_ahead: Decimal,
}
