use cosmwasm_schema::cw_serde;

use cosmwasm_std::{Decimal, Uint128, Addr, Coin};

use crate::types::{Asset, AssetPool, Deposit, UserInfo};

#[cw_serde]
pub struct InstantiateMsg {
    /// Contract owner, defaults to info.sender
    pub owner: Option<String>,
    /// Asset pool instance for the debt token
    pub asset_pool: AssetPool,
    /// Incentive rate for users
    pub incentive_rate: Option<Decimal>,
    /// Max incentives 
    pub max_incentives: Option<Uint128>,
    /// Minimum bid amount
    pub minimum_deposit_amount: Uint128,
    /// Osmosis Proxy contract address
    pub osmosis_proxy: String,
    /// Positions contract address
    pub positions_contract: String,
    /// MBRN denom
    pub mbrn_denom: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Update contract config
    UpdateConfig(UpdateConfig),
    /// Deposit the debt token into the pool
    Deposit {
        /// User address, defaults to info.sender
        user: Option<String>,
    },
    /// Unstake/Withdraw deposits from the pool
    Withdraw {
        /// Debt token amount 
        amount: Uint128,
    },
    /// Restake unstak(ed/ing) assets
    Restake {
        /// Debt token amount
        restake_amount: Decimal,
    },
    /// Claim ALL liquidation revenue && MBRN incentives
    ClaimRewards {},
    /// Use assets from an Asset pool to liquidate for a Position (Positions Contract)
    Liquidate {
        /// Liquidation amount
        liq_amount: Decimal,
    },
    /// Positions contract distributes liquidated funds to users
    Distribute {
        /// Assets to distribute
        distribution_assets: Vec<Asset>,
        /// Distribution asset ratios
        distribution_asset_ratios: Vec<Decimal>,
        /// Amount to distribute for
        distribute_for: Uint128,
    },
    /// Allow the Positions contract to use user funds to repay for themselves
    Repay {
        /// User position info
        user_info: UserInfo,
        /// Repayment asset
        repayment: Asset,
    },
}

#[cw_serde]
pub enum QueryMsg {
    /// Returns contract config
    Config {},
    /// Returns amount of unclaimed incentives for a user
    UnclaimedIncentives { 
        /// User address
        user: String 
    },
    /// Returns capital ahead of frontmost user deposit
    CapitalAheadOfDeposit { 
        /// User address
        user: String 
    },
    /// Check if the amount of debt asset is liquidatible
    CheckLiquidatible { 
        /// Debt token amount
        amount: Decimal
    },
    /// Returns user's claimable assets
    UserClaims { 
        /// User address
        user: String 
    },
    /// Returns AssetPool
    AssetPool { 
        /// User address
        user: Option<String>,
        /// Deposit limit
        deposit_limit: Option<u32>,
        /// Deposit to start after
        start_after: Option<u32>,        
    },
}

#[cw_serde]
pub struct Config {
    /// Contract owner
    pub owner: Addr,
    /// Incentive rate for deposits
    pub incentive_rate: Decimal,
    /// Max incentives
    pub max_incentives: Uint128,
    /// Unstaking period in days
    pub unstaking_period: u64,
    /// Minimum bid amount
    pub minimum_deposit_amount: Uint128,
    /// MBRN denom
    pub mbrn_denom: String,
    /// Osmosis Proxy contract address
    pub osmosis_proxy: Addr,
    /// Positions contract address
    pub positions_contract: Addr,
}

#[cw_serde]
pub struct UpdateConfig {
    /// Contract owner
    pub owner: Option<String>,
    /// Incentive rate for deposits
    pub incentive_rate: Option<Decimal>,
    /// Max incentives
    pub max_incentives: Option<Uint128>,
    /// Unstaking period in days
    pub unstaking_period: Option<u64>,
    /// Minimum bid amount
    pub minimum_deposit_amount: Option<Uint128>,
    /// Osmosis Proxy contract address
    pub osmosis_proxy: Option<String>,
    /// Positions contract address
    pub positions_contract: Option<String>,
    /// MBRN denom
    pub mbrn_denom: Option<String>,
}

#[cw_serde]
pub struct LiquidatibleResponse {
    /// Amount that can't be liquidated
    pub leftover: Decimal,
}

#[cw_serde]
pub struct ClaimsResponse {
    /// Claimable assets
    pub claims: Vec<Coin>,
}

#[cw_serde]
pub struct DepositPositionResponse {
    /// Deposit position
    pub deposit: Deposit,
    /// Capital ahead of deposit
    pub capital_ahead: Decimal,
}
