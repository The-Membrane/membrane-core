
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Uint128};
use crate::types::{AssetOracleInfo, RangeBoundUserIntents, RangeBounds, RangePositions, RangeTokens, UserInfo, UserIntentState};


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
    pub borrow_cap: Option<Uint128>,
}


#[cw_serde]
pub enum ExecuteMsg {
    /// Enter the vault 100% CDT
    EnterVault {
        // leave_vault_tokens_in_vault: Option<LeaveTokens>,
    },
    /// Exit vault in the current ratio of assets owned (LP + balances)
    /// The App can swap into a single token and give value options based on swap rate.
    ExitVault {
        send_to: Option<String>,
        swap_to_cdt: bool
    },
    /// Deposits CDT revenue into the contract. 
    /// We use a msg enum bc the CDP needs it.
    DepositFee { },
    ManageVault { rebalance_sale_max: Option<Decimal> },
    /// Withdraws the floor position 
    WithdrawFloorPosition {  },
    /// Withdraws CDT from the ceiling to swap to USDC to deposit into the floor
    // BolsterFloorWithSwaps { max_swap_amount: Option<Uint128> },
    /// Set intents for a user. They must send vault tokens or have a non-zero balance in state.
    /// NOTE: We don't use the asset price initiation.
    SetUserIntents { 
        intents: Option<RangeBoundUserIntents>,
        // reduce_vault_tokens: Option<ReduceTokens>,
    },
    /// Fulfill intents for a user. Send fees to the caller.
    FulFillUserIntents { user: String },
    /// Let CDP contract use VTs in user intents to repay debt.
    RepayUserDebt { 
        user_info: UserInfo,
        repayment: Uint128, 
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
    ///Saves the current base token claim for 1 vault token
    CrankRealizedAPR { },
    /// Assures that for deposits & withdrawals the conversion rate is static.
    /// Only callable by the contract
    RateAssurance { },
    /// Callback
    GetTotalDepositTokens { },
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
}

#[cw_serde]
pub struct MigrateMsg {}
