use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Decimal, Uint128};

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub cdt_denom: String,
    pub usdc_denom: String,
    pub mars_vault_addr: String,
    pub cdp_contract_addr: String,
    pub transmuter_addr: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Enter the vault - creates CDP position for user
    EnterVault {
        leave_vault_tokens_in_vault: Option<crate::types::LeaveTokens>,
    },
    /// Repay user debt
    RepayUserDebt {
        user_info: crate::types::UserInfo,
        repayment: Uint128,
    },
    /// Update market conditions manually
    UpdateMarketConditions {},
    /// Update contract config
    UpdateConfig {
        owner: Option<String>,
        cdt_denom: Option<String>,
        usdc_denom: Option<String>,
        mars_vault_addr: Option<String>,
        cdp_contract_addr: Option<String>,
        transmuter_addr: Option<String>,
        vault_cost_index: Option<usize>,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    /// Get user's retrievable CDT (always returns 0)
    #[returns(Uint128)]
    RetrievableCDT {
        user: String,
    },
    /// Get the underlying token amount for a given vault token amount
    #[returns(Uint128)]
    VaultTokenUnderlying { 
        vault_token_amount: Uint128 
    },
    /// Get the vault token conversion for a given deposit token amount
    #[returns(Uint128)]
    DepositTokenConversion { 
        deposit_token_amount: Uint128 
    },
    /// Get user positions with pagination
    #[returns(Vec<UserPosition>)]
    GetUserPositions {
        user: Option<String>,
        limit: Option<u32>,
        start_after: Option<u64>,
    },
    /// Get market conditions with pagination (most recent first)
    #[returns(Vec<MarketConditions>)]
    GetMarketConditions {
        limit: Option<u32>,
        start_after: Option<u64>,
    },
    /// Get TVL history with pagination
    #[returns(Vec<TVLSnapshot>)]
    GetTVLHistory {
        limit: Option<u32>,
        start_after: Option<u64>,
    },
    /// Get contract config
    #[returns(Config)]
    Config {},
    /// Get deployment snapshot for a user
    #[returns(Option<DeploymentSnapshot>)]
    GetDeploymentSnapshot {
        user: String,
    },
}

#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub cdt_denom: String,
    pub usdc_denom: String,
    pub vault_token_denom: String,
    pub mars_vault_addr: Addr,
    pub cdp_contract_addr: Addr,
    pub transmuter_addr: Addr,
    pub vault_cost_index: usize,
}

#[cw_serde]
pub struct UserPosition {
    pub user: Addr,
    pub collateral_amount: Uint128,
    pub debt_amount: Uint128,
    pub position_id: Uint128,
    pub timestamp: u64,
}

#[cw_serde]
pub struct MarketConditions {
    pub cdt_mint_cost: Decimal,
    pub vault_apr: Decimal,
    pub vault_cost: Decimal,
    pub timestamp: u64,
}

#[cw_serde]
pub struct TVLSnapshot {
    pub tvl: Uint128,
    pub timestamp: u64,
}

#[cw_serde]
pub struct DeploymentSnapshot {
    /// Collateral assets at time of initial deployment (saved once)
    pub collateral_assets: Vec<crate::types::cAsset>,
    /// Block time when initial deployment occurred (saved once)
    pub block_time: u64,
    /// Amount of CDT that was looped (cumulative, updated on each loop)
    pub amount_looped: Uint128,
    /// Amount of debt taken (current total debt, updated on each loop)
    pub debt_taken: Uint128,
}

#[cw_serde]
pub struct MigrateMsg {}

