
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
        //
        osmosis_proxy_contract_addr: Option<String>,
        oracle_contract_addr: Option<String>,
        self_debt_cap: Option<Uint128>,
        swap_slippage: Option<Decimal>,
        vault_cost_index: Option<bool>
    },
    ///APRs are calculated for every deposit and withdrawla but if you want something up to date
    /// you must crank.
    CrankAPR { },
    /// Unloop the vault's CDP position to free up collateral
    /// Called by the contract for withdrawals.
    /// Called by external to unloop & retain profitability.
    UnloopCDP {
        /// Amount of collateral to withdraw.
        /// Only callable by the contract/
        desired_collateral_withdrawal: Option<Uint128>,
        /// Max loops to run
        /// This caps msgs sent (i.e. gas) and prevents infinite loops
        loop_max: Option<u32>,
    },
    /// Loop the vault's CDP position to increase collateral
    LoopCDP {
        /// Max loops to run
        /// This caps msgs sent (i.e. gas) and prevents infinite loops
        loop_max: Option<u32>,
    },
    //////////////CALLBACKS////////////////
    /// Assures that for deposits & withdrawals the conversion rate is static.
    /// We are trusting that Mars deposits will only go up.
    /// Only callable by the contract
    RateAssurance { },
    /// Check Loop profitability & Deposit excess VT into the CDP
    /// Only callable by the contract
    PostLoopMaintenance { },
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
    /// Stores total non-leveraged vault token amount
    pub total_nonleveraged_vault_tokens: Uint128,
    /// Position ID of the vault's CDP position (set in instantiation)
    pub cdp_position_id: Uint128,
    /// Vault debt cap
    /// The CP contract will have another debt cap but we use this for a static debt cap so we accurately limit based on liquidity.
    pub self_debt_cap: Uint128,
    pub swap_slippage: Decimal,
    pub vault_cost_index: usize,
}

#[cw_serde]
pub struct MigrateMsg {}