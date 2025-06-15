
use std::option;

use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Decimal, Uint128};
use crate::{managed_market::{BorrowCap, CollateralParams, MarketParams, RateParams, Config as MarketConfig}, oracle::PriceResponse, types::{AssetOracleInfo, BorrowOptions, ClaimTracker, RangeBounds, RangePositions, RangeTokens, UserInfo, UserIntentState, UserPosition}};

#[cw_serde]
pub struct PendingMarket {
    pub name: String,
    pub socials: Vec<String>,
    pub manager: String,
}

#[cw_serde]
pub struct MarketItem {
    pub name: String,
    pub socials: Vec<String>,
    pub address: String,
}

#[cw_serde]
pub struct ManagerEdit {
    pub add: Option<Vec<String>>,
    pub remove: Option<Vec<String>>,
}

#[cw_serde]
pub struct MarketInstantiation {
    pub manager: Option<String>,
    pub name: String,
    pub socials: Vec<String>,
    pub whitelisted_debt_suppliers: Option<Vec<String>>,
    pub max_slippage: Decimal,
    pub collateral_params: CollateralParams,
    pub rate_params: RateParams,
    pub pool_for_oracle_and_liquidations: AssetOracleInfo,
    pub borrow_fee: Decimal,
    pub whitelisted_collateral_suppliers: Option<Vec<String>>,
    pub pause_option: bool,
    pub debt_supply_cap: Option<Uint128>,
    pub borrow_cap: BorrowCap,
    pub per_user_debt_cap: Option<Uint128>,
    pub debt_minimum: Option<Uint128>,
    pub manager_fee: Option<Decimal>,
}


#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub managed_market_code_id: u64,
    pub manager_whitelist: Vec<Addr>,
    pub osmosis_proxy_contract: Addr,
    pub managed_market_fee: Decimal,
}

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: String,
    pub managed_market_code_id: u64,
    pub manager_whitelist: Vec<String>,
    pub osmosis_proxy_contract: String,
}


#[cw_serde]
pub enum ExecuteMsg {
    /// Update the contract config
    UpdateConfig {
        owner: Option<String>,
        managed_market_code_id: Option<u64>,
        edit_managers: Option<ManagerEdit>,
        managed_market_fee: Option<Decimal>,
    },
    /// Update the market data
    UpdateMarketItem {
        market_address: String,
        /// Owner can update any manager
        manager: Option<String>,
        socials: Option<Vec<String>>,
        name: Option<String>,
        remove: Option<bool>,
    },
    /// Let a manager instantiate a new market
    InstantiateMarket {
        params: MarketInstantiation,
    },
    /// Let a manager migrate an existing market
    MigrateMarket {
        market_address: String,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(Config)]
    Config {},
    #[returns(Vec<String>)]
    MarketsManaged { 
        /// Manager address
        manager: String,
    },
    #[returns(Vec<String>)]
    Managers { 
        start_after: Option<String>,
        limit: Option<u32>,
    },
    //Returns all market params managed by a manager
    #[returns(Vec<MarketData>)]
    MarketParams { 
        /// Manager
        manager: String,
        //Market contract
        start_after: Option<String>,
        //Market limiter
        limit: Option<u32>,
    },
}

#[cw_serde]
pub struct MarketData {
    pub address: String,
    pub name: String,
    pub socials: Vec<String>,
    pub config: MarketConfig,
    pub params: MarketParams,
}

#[cw_serde]
pub struct MigrateMsg {}
