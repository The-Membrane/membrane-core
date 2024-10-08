
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, StdError, Uint128};

pub const DEFAULT_VAULT_TOKENS_PER_STAKED_BASE_TOKEN: Uint128 = Uint128::new(1_000_000);

#[cw_serde]
pub struct InstantiateMsg {
    pub vault_subdenom: String,
    pub deposit_token: String,
    pub stability_pool_contract: String,
    pub osmosis_proxy_contract: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    EnterVault { },
    ExitVault { },
    Compound { },
    UpdateConfig {
        owner: Option<String>,
        percent_to_keep_liquid: Option<Decimal>,
        compound_activation_fee: Option<Uint128>,
        min_time_before_next_compound: Option<u64>,
    },
    /// Assures that for deposits & withdrawals the conversion rate is static
    /// & for compounds the conversion rate increases
    /// Only callable by the contract
    RateAssurance {
        deposit_or_withdraw: bool,
        compound: bool,
    },
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
    pub vault_token: String,
    pub deposit_token: String,
    //Deposit token tally that includes tokens in the vault
    pub total_deposit_tokens: Uint128,
    //Ratio to keep outside of the vault strategy for easy withdrawals
    //Only applicable bc the strategy has an unstaking period
    pub percent_to_keep_liquid: Decimal,
    //Amount of deposit tokens sent to the caller of the compound msg
    pub compound_activation_fee: Uint128,
    //Maximum Compound frequency in seconds
    pub min_time_before_next_compound: u64,
    pub stability_pool_contract: Addr,
    pub osmosis_proxy_contract: Addr,
}

#[cw_serde]
pub struct APRResponse {
    pub week_apr: Option<Decimal>,
    pub month_apr: Option<Decimal>,
    pub three_month_apr: Option<Decimal>,
    pub year_apr: Option<Decimal>,
}

#[cw_serde]
pub struct MigrateMsg {}

/// Converts an amount of base_tokens to an amount of vault_tokens.
pub fn calculate_vault_tokens(
    base_tokens: Uint128,
    total_staked_amount: Uint128,
    vault_token_supply: Uint128,
) -> Result<Uint128, StdError> {
    let vault_tokens = if total_staked_amount.is_zero() {
        base_tokens.checked_mul(DEFAULT_VAULT_TOKENS_PER_STAKED_BASE_TOKEN)?
    } else {
        vault_token_supply.multiply_ratio(base_tokens, total_staked_amount)
    };

    Ok(vault_tokens)
}

/// Converts an amount of vault_tokens to an amount of base_tokens.
pub fn calculate_base_tokens(
    vault_tokens: Uint128,
    total_staked_amount: Uint128,
    vault_token_supply: Uint128,
) -> Result<Uint128, StdError> {
    let base_tokens = if vault_token_supply.is_zero() {
        vault_tokens.checked_div(DEFAULT_VAULT_TOKENS_PER_STAKED_BASE_TOKEN)?
    } else {
        total_staked_amount.multiply_ratio(vault_tokens, vault_token_supply)
    };

    Ok(base_tokens)
}