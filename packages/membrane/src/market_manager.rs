
use std::option;

use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Decimal, Uint128};
use crate::{managed_market::{BorrowCap, CollateralParams, RateParams}, oracle::PriceResponse, types::{AssetOracleInfo, BorrowOptions, ClaimTracker, RangeBounds, RangePositions, RangeTokens, UserInfo, UserIntentState, UserPosition}};

#[cw_serde]
pub struct PendingMarket {
    pub name: String,
    pub manager: String,
}

#[cw_serde]
pub struct MarketItem {
    pub name: String,
    pub address: String,
}

#[cw_serde]
pub struct ManagerEdit {
    pub add: Option<Vec<String>>,
    pub remove: Option<Vec<String>>,
}

#[cw_serde]
pub struct MarketInstantiation {
    pub name: String,
    pub whitelisted_debt_suppliers: Option<Vec<String>>,
    // pub debt_supply_vault_token: String,
    pub collateral_params: CollateralParams,
    pub rate_params: RateParams,
    pub pool_for_oracle_and_liquidations: AssetOracleInfo,
    pub borrow_fee: Decimal,
    pub whitelisted_collateral_suppliers: Option<Vec<String>>,
    pub pause_option: bool,
    pub debt_supply_cap: Option<Uint128>,
    pub borrow_cap: BorrowCap,
    pub per_user_debt_cap: Option<Uint128>,
}


#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub managed_market_code_id: u64,
    pub manager_whitelist: Vec<Addr>,
    pub osmosis_proxy_contract: Addr,
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
    },
    /// Let a manager instantiate a new market
    InstantiateMarket {
        params: MarketInstantiation,
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
}


#[cw_serde]
pub struct MigrateMsg {}
