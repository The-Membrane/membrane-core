
use std::option;

use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Decimal, Uint128};
use crate::{oracle::PriceResponse, types::{AssetOracleInfo, BorrowOptions, RangeBounds, RangePositions, RangeTokens, UserInfo, UserIntentState, UserPosition}};

#[cw_serde]
pub struct InstantiateMsg {
    /// If owner isn't set, we'll set it to Membrane governance in the Manager contract.
    pub owner: String,
    pub osmosis_proxy_contract: String,
    pub whitelisted_debt_suppliers: Option<Vec<String>>,
    pub debt_supply_vault_token: String,

//////////////Market params////////
    pub collateral_params: CollateralParams,
    pub rate_params: RateParams,
    pub pool_for_oracle_and_liquidations: AssetOracleInfo,
    pub borrow_fee: Decimal,
    pub whitelisted_collateral_suppliers: Option<Vec<String>>,
    pub pause_option: bool,
    pub debt_supply_cap: Option<Uint128>,
    pub borrow_cap: BorrowCap,
}


#[cw_serde]
pub enum ExecuteMsg {

    /// Deposit collateral to receive receipt tokens.
    /// Assert:
    /// - Only one asset is sent (error)
    /// - A market exists for that asset (error)
    /// - The contract isn't frozen (error)
    /// - The owner is whitelisted to supply collateral (error)
    /// - State is updated to reflect the deposit
    SupplyCollateral {
        /// Who owns the collateral? Defaults to sender.
        owner: Option<String>
    },
    /// Withdraw collateral to receive deposited tokens back
    /// Assert:
    /// - The contract isn't frozen (error)
    /// - The user has a position in the market (error)
    /// - The user has non-zero collateral (error)
    /// - Update user position to reflect the collateral withdrawal
    /// - Update config to reflect rate accrual
    /// - Send the withdrawn collateral to the user
    WithdrawCollateral {
        /// Market signifier
        collateral_denom: String,
        /// Who is the collateral token going to? Defaults to sender.
        send_to: Option<String>,
        /// Withdraw amount. Defaults to all.
        withdraw_amount: Option<Uint128>,
    },
    /// Deposit debt to receive receipt tokens.
    /// Assert:
    /// - The contract isn't frozen (error)
    /// - The owner is whitelisted to supply debt(error)
    /// - Only one asset is sent & its the debt token (error)
    /// - The amount sent is non-zero (error)
    /// - The amount sent doesn't break the global debt supply cap (error)
    /// - User is minted vault tokens if non-zero
    /// - Update the config.total_debt_tokens to reflect the deposit
    /// - Update vault token supply to reflect the mint & send
    /// - The debt vault token conversion rate is static (post-submsg error)
    SupplyDebt {
        /// Who is the receipt token going to? Defaults to sender.
        send_to: Option<String>
    },
    /// Withdraw debt by sending vault receipt tokens
    /// Assert:
    /// - The contract isn't frozen (error)
    /// - Only one asset is sent & its the debt vault token (error)
    /// - The contract has enough debt token to send for the withdrawal (error)
    /// - Burn the sent vault tokens and send the underlying debt tokens
    /// - Update the config.total_debt_tokens to reflect the withdrawal
    /// - Update vault token supply to reflect the burn & send
    /// - The debt vault token conversion rate is static (post-submsg error)
    WithdrawDebt {
        /// Who is the CDT going to? Defaults to sender.
        send_to: Option<String>
    },
    /// Borrow CDT from the market & add it as debt to the user's position
    /// Assert:
    /// - The contract isn't frozen (error)
    /// - The user has a position in the market (error)
    /// - The borrow doesn't break the debt cap (forced minimum)
    /// - The user has enough collateral to borrow the requested amount (forced minimum)
    /// - User state is updated to reflect the borrow
    /// - The market state is updated to reflect the borrow
    /// - The borrowed amount is sent to the user
    Borrow {
        /// Market signifier
        collateral_denom: String,
        /// Who is the CDT going to? Defaults to sender.
        send_to: Option<String>,
        /// Borrow amount or ltv
        borrow_amount: BorrowOptions,
    },
    Repay { 
        /// Market signifier
        collateral_denom: String 
    },
    Liquidate {
        /// Market signifier
        collateral_denom: String,
        position_owner: String,
        /// Toggle if you want to take the caller's fee or not.
        /// Advise managers not to take the fee.
        take_fee: bool
    },
    ///Accrue a users position to keep debt supplier earnings up to date.
    Accrue {
        collateral_denom: String,
        position_owner: String,
    },
    //
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
        manager_fee: Option<Decimal>,
        whitelisted_debt_suppliers: Option<Option<Vec<String>>>,
        debt_supply_cap: Option<Option<Uint128>>,
    },
    /// Update the market config
    UpdateMarket {
        collateral_denom: String,
        max_borrow_LTV: Option<Decimal>,
        liquidation_LTV: Option<LTVRamp>,
        rate_params: Option<RateParams>,
        borrow_fee: Option<Decimal>,
        whitelisted_collateral_suppliers: Option<Option<Vec<String>>>,
        borrow_cap: Option<BorrowCap>,
        max_slippage: Option<Decimal>,
        pool_for_oracle_and_liquidations: Option<AssetOracleInfo>,
    },
    /// Assures that for deposits & withdrawals the conversion rate is static.
    /// Only callable by the contract
    RateAssurance { },
    /// Callback
    GetTotalDepositTokens { },
    CheckBadDebt { },   
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(Config)]
    Config {},
    #[returns(PriceResponse)]
    GetCollateralPrice { asset: String },
    #[returns(PriceResponse)]
    GetDebtPrice { },
    #[returns(Decimal)]
    GetCurrentInterestRate {  
        /// Market signifier
        collateral_denom: String,
    },
    #[returns(())]
    TestDebtAllowance { 
        /// Market signifier
        collateral_denom: String,
        potential_total_debt: Option<Uint128>
     },
     #[returns(Vec<UserPositionResponse>)]
    GetUserPositions { 
        /// Market signifier
        collateral_denom: String,
        user: Option<String>,
        start_after: Option<String>,
        limit: Option<u32>,
    },
}


#[cw_serde]
pub struct UserPositionResponse {
    pub user: String,
    pub position: UserPosition
}

// #[cw_serde]
// pub struct UserIntentResponse {
//     pub user: String,
//     pub intent: UserIntentState
// }
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
    pub global_rate_index: RateIndex,
    /// This includes supplied CDT & CDT accrued from interest to make sure debt suppliers always withdraw their full share.
    pub total_debt_tokens: Uint128,
    pub bad_debt: Uint128,
    pub debt_supply_cap: Option<Uint128>,
    pub debt_supply_vault_token: String,
    ///Set Whitelists to None to disable new capital
    pub whitelisted_debt_suppliers: Option<Vec<String>>,
    pub manager_fee: Decimal,
}


#[cw_serde]
pub struct MarketParams {
    pub collateral_params: CollateralParams,
    pub rate_params: RateParams,
    pub market_rate_index: RateIndex,
    /// This is the total amount of debt that has been borrowed.
    pub total_borrowed: Uint128,
    pub pool_for_oracle_and_liquidations: AssetOracleInfo,
    pub borrow_fee: Decimal,
    ///Set Whitelists to None to disable new capital
    pub whitelisted_collateral_suppliers: Option<Vec<String>>,
    pub borrow_cap: BorrowCap,
    //Max slippage for liquidation swaps. If the swaps fail, liquidations fail. 
    //If the swap quality is bad, we get inefficient liquidations & bad debt.
    pub max_slippage: Decimal,
}

#[cw_serde]
pub struct MigrateMsg {}
