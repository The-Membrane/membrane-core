
use std::option;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Uint128};
use crate::types::{AssetOracleInfo, RangeBoundUserIntents, RangeBounds, RangePositions, RangeTokens, UserInfo, UserIntentState, BorrowOptions};

#[cw_serde]
pub struct InstantiateMsg {
    /// If owner isn't set, we'll set it to Membrane governance in the Manager contract.
    pub owner: String,
    pub osmosis_proxy_contract: String,
    pub collateral_params: CollateralParams,
    pub rate_params: RateParams,
    pub pool_for_oracle_and_liquidations: AssetOracleInfo,
    pub borrow_fee: Decimal,
    pub whitelisted_collateral_suppliers: Option<Vec<String>>,
    pub whitelisted_debt_suppliers: Option<Vec<String>>,
    pub debt_supply_vault_token: String,
    pub pause_option: bool,
    pub supply_cap: Option<Uint128>,
    pub borrow_cap: BorrowCap,
}


#[cw_serde]
pub enum ExecuteMsg {
    SupplyCollateral {
        /// Who owns the collateral? Defaults to sender.
        owner: Option<String>
    },
    WithdrawCollateral {
        /// Who is the collateral token going to? Defaults to sender.
        send_to: Option<String>,
        /// Withdraw amount. Defaults to all.
        withdraw_amount: Option<Uint128>,
    },
    SupplyDebt {
        /// Who is the receipt token going to? Defaults to sender.
        send_to: Option<String>
    },
    WithdrawDebt {
        /// Who is the CDT going to? Defaults to sender.
        send_to: Option<String>
    },
    Borrow {
        /// Who is the CDT going to? Defaults to sender.
        send_to: Option<String>,
        /// Borrow amount or ltv
        borrow_amount: BorrowOptions,
    },
    Repay { },
    Liquidate {
        position_owner: String,
        /// Toggle if you want to take the caller's fee or not.
        /// Advise managers not to take the fee.
        take_fee: bool
    },
    ClosePosition {
        position_owner: String,
        close_percentage: Option<Decimal>,
        max_spread: Decimal,
        /// Who to send excess CDT from the spread coverage & available collateral if fully closed. Defaults to sender.
        send_to: Option<String>,
    },
    /// Update the contract config
    UpdateConfig {
        owner: Option<String>,
        osmosis_proxy_contract_addr: Option<String>,
        pause_actions: Option<bool>,
        max_borrow_LTV: Option<Decimal>,
        liquidation_LTV: Option<LTVRamp>,
        rate_params: Option<RateParams>,
        borrow_fee: Option<Decimal>,
    },
    /// Assures that for deposits & withdrawals the conversion rate is static.
    /// Only callable by the contract
    RateAssurance { },
    /// Callback
    GetTotalDepositTokens { },
    CheckBadDebt { },   
}

#[cw_serde]
pub enum QueryMsg {
    /// Return contract config
    Config {},
    GetCollateralPrice { asset: String },
    GetDebtPrice { },
    GetCurrentInterestRate { },
    TestDebtAllowance { potential_total_debt: Uint128 },
}

#[cw_serde]
pub struct UserIntentResponse {
    pub user: String,
    pub intent: UserIntentState
}
#[cw_serde]
pub struct RateKinkParams {
    pub rate_mulitplier: Decimal,
    pub kink_starting_point_ratio: Decimal,
}

#[cw_serde]
pub struct RateParams {
    pub base_rate: Decimal,
    /// If this is None, the base rate becomes a fixed rate.
    /// It's hard to attract capital with a fixed rate & .
    pub rate_kink: Option<RateKinkParams>,
    pub rate_max: Decimal,
}

#[cw_serde]
pub struct CollateralParams {
    pub collateral_asset: String,
    pub max_borrow_LTV: Decimal,
    pub liquidation_LTV: Decimal,
}

#[cw_serde]
pub struct LTVRamp {
    pub new_LTV: Decimal,
    pub duration_in_hours: u64,
}

#[cw_serde]
pub struct RateIndex {
    pub rate_index: Decimal,
    pub last_accrued: u64,
}

#[cw_serde]
pub struct BorrowCap {
    pub fixed_cap: Option<Uint128>,
    /// Cap borrows based on current liquidatibility thru the oracle pools
    pub cap_borrows_by_liquidity: bool
}

#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub osmosis_proxy_contract: Addr,
    pub collateral_params: CollateralParams,
    pub rate_params: RateParams,
    pub global_rate_index: RateIndex,
    /// This includes supplied CDT & CDT accrued from interest to make sure debt suppliers always withdraw their full share.
    pub total_debt_tokens: Uint128,
    /// This is the total amount of debt that has been borrowed.
    pub total_borrowed: Uint128,
    pub pool_for_oracle_and_liquidations: AssetOracleInfo,
    pub borrow_fee: Decimal,
    ///Set Whitelists to None to disable new capital
    pub whitelisted_collateral_suppliers: Option<Vec<String>>,
    ///Set Whitelists to None to disable new capital
    pub whitelisted_debt_suppliers: Option<Vec<String>>,
    pub debt_supply_vault_token: String,
    pub supply_cap: Option<Uint128>,
    pub borrow_cap: BorrowCap,
    pub bad_debt: Uint128,
    pub manager_fee: Decimal,
    //Max slippage for liquidation swaps. If the swaps fail, liquidations fail. 
    //If the swap quality is bad, we get inefficient liquidations & bad debt.
    pub max_slippage: Decimal,
}

#[cw_serde]
pub struct MigrateMsg {}
