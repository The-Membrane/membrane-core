
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Uint128};
use crate::types::VaultInfo;


#[cw_serde]
pub struct InstantiateMsg {
    pub cdt_denom: String,
    pub vault_subdenom: String,
    pub deposit_token: VaultInfo,
    pub cdp_contract_addr: String,
    pub osmosis_proxy_contract_addr: String,
    pub oracle_contract_addr: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    EnterVault { },
    ExitVault { },
    // Compound { },
    UpdateConfig {
        owner: Option<String>,
        /// Mainly for testing, we shouldn't change addrs bc we'd lose deposits
        cdp_contract_addr: Option<String>,
        mars_vault_addr: Option<String>,
        //
        osmosis_proxy_contract_addr: Option<String>,
        oracle_contract_addr: Option<String>,
        withdrawal_buffer: Option<Decimal>,
        deposit_cap: Option<Uint128>,
        swap_slippage: Option<Decimal>,
        vault_cost_index: Option<()>
    },
    /// Unloop the vault's CDP position to free up collateral
    /// Only called by the contract. (for withdrawals)
    UnloopCDP {
        /// Amount of collateral to withdraw.
        desired_collateral_withdrawal: Uint128,        
    },
    /// Loop the vault's CDP position to increase collateral
    LoopCDP { },
    //////////////CALLBACKS////////////////
    /// Assures that for deposits & withdrawals the conversion rate is static.
    /// We are trusting that Mars deposits will only go up.
    /// Only callable by the contract
    RateAssurance { exit: bool },
    /// Update the config's total_nonleveraged_vault_tokens
    /// Only callable by the contract
    UpdateNonleveragedVaultTokens { },

}

#[cw_serde]
pub enum QueryMsg {
    /// Return contract config
    Config {},
    VaultTokenUnderlying { vault_token_amount: Uint128 },
    APR {},
}

#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub cdp_contract_addr: Addr,
    pub osmosis_proxy_contract_addr: Addr,
    pub oracle_contract_addr: Addr,
    pub cdt_denom: String,
    pub vault_token: String,
    pub deposit_token: VaultInfo,
    /// % of deposits to keep outside of the CDP to ease withdrawals
    pub withdrawal_buffer: Decimal,
    /// Stores total non-leveraged vault token amount
    pub total_nonleveraged_vault_tokens: Uint128,
    /// Position ID of the vault's CDP position (set in instantiation)
    pub cdp_position_id: Uint128,
    /// Vault debt cap
    /// The CDP contract will have another debt cap but we use this for a static deposit cap so we accurately limit based on liquidity.
    /// UNUSED.
    pub deposit_cap: Uint128,
    pub swap_slippage: Decimal,
    pub vault_cost_index: usize,
}

/// config.witdrawal_buffer's percent of the vault isn't earning the levered APR, just the deposit_token's vault APR
#[cw_serde]
pub struct APRResponse {
    pub week_apr: Option<Decimal>,
    pub month_apr: Option<Decimal>,
    pub three_month_apr: Option<Decimal>,
    pub year_apr: Option<Decimal>,
    pub leverage: Decimal,
    pub cost: Decimal,
}

#[cw_serde]
pub struct MigrateMsg {}