
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Uint128};
use crate::types::{LiqAsset, DistributionEntry};


#[cw_serde]
pub struct InstantiateMsg {
    pub vault_subdenom: String,
    pub deposit_token: String,
    pub mars_redbank_addr: String,
    pub transmuter_addr: String,
    pub revenue_distributor_addr: String,
    pub cdt_denom: String,
    pub cdp_contract_addr: String,
    pub revenue_distributions: Vec<LiqAsset>,
}

#[cw_serde]
pub enum ExecuteMsg {
    EnterVault { },
    ExitVault { },
    // Compound { },
    UpdateConfig {
        owner: Option<String>,
        /// Mainly for testing, we shouldn't change addrs bc we'd lose deposits
        mars_redbank_addr: Option<String>,
        transmuter_addr: Option<String>,
        revenue_distributor_addr: Option<String>,
        vault_cost: Option<VaultCost>,
        cdt_denom: Option<String>,
        cdp_contract_addr: Option<String>,
        revenue_distributions: Option<Vec<DistributionEntry>>,
    },
    ///APRs are calculated for every deposit and withdrawla but if you want something up to date
    /// you must crank.
    CrankAPR { },
    /// Collect accrued vault costs by burning revenue vault tokens and swapping to CDT
    CollectCost { },
    /// Assures that for deposits & withdrawals the conversion rate is static.
    /// We are trusting that Mars deposits will only go up.
    /// Only callable by the contract
    RateAssurance { },
    /// Update CDP with calculated vault cost
    UpdateCDPCosts { },
}

#[cw_serde]
pub enum QueryMsg {
    /// Return contract config
    Config {},
    VaultTokenUnderlying { vault_token_amount: Uint128 },
    DepositTokenConversion { deposit_token_amount: Uint128 },
    APR {},
    Cost {},
}

#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub mars_redbank_addr: Addr,
    pub vault_token: String,
    pub deposit_token: String,
    //Deposit token tally that includes tokens in the vault
    pub total_deposit_tokens: Uint128,
    //Vault Cost
    pub vault_cost: VaultCost,
    pub transmuter_addr: Addr,
    pub revenue_distributor_addr: Addr,
    pub cdt_denom: String,
    pub cdp_contract_addr: Addr,
    pub vault_cost_index: usize,
    pub revenue_distributions: Vec<LiqAsset>,
}


#[cw_serde]
pub struct VaultCost {
    pub static_cost: Option<Decimal>,
    //If set, the vault will subtract the ceiling from the queried Mars yield to calculate the cost.
    pub yield_ceiling: Option<Decimal>,
}

//AVERAGE APR PER TIME PERIOD
#[cw_serde]
pub struct APRResponse {
    pub week_apr: Option<Decimal>,
    pub month_apr: Option<Decimal>,
    pub three_month_apr: Option<Decimal>,
    pub year_apr: Option<Decimal>,
}
#[cw_serde]
pub struct MigrateMsg {}