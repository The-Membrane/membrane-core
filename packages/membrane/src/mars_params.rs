use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Decimal, Uint128};

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: String,
    pub risk_manager: Option<String>,
    pub address_provider: String,
    pub max_perp_params: u8,
}

#[cw_serde]
pub enum ExecuteMsg {
    UpdateOwner(String),
    UpdateRiskManager(String),
    ResetRiskManager(),
    UpdateConfig {
        address_provider: Option<String>,
        max_perp_params: Option<u8>,
    },
    UpdateAssetParams(AssetParamsUpdate),
    UpdateVaultConfig(String),
    UpdatePerpParams(String),
    EmergencyUpdate(String),
    UpdateManagedVaultConfig(String),
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(Option<AssetParams>)]
    AssetParams {
        denom: String,
    },
}

#[cw_serde]
pub struct ConfigResponse {
    pub address_provider: String,
    pub max_perp_params: u8,
}

#[cw_serde]
pub struct ManagedVaultConfigResponse {
    pub min_creation_fee_in_uusd: u128,
    pub code_ids: Vec<u64>,
    pub blacklisted_vaults: Vec<String>,
}

#[cw_serde]
pub struct TotalDepositResponse {
    pub denom: String,
    pub cap: Uint128,
    pub amount: Uint128,
}

#[cw_serde]
pub enum AssetParamsUpdate {
    AddOrUpdate {
        params: AssetParamsUnchecked,
    },
}

// AssetParams types - simplified for our use case
#[cw_serde]
pub struct CmSettings {
    pub whitelisted: bool,
    pub withdraw_enabled: bool,
    pub hls: Option<String>, // Simplified to String for serialization
}

#[cw_serde]
pub struct RedBankSettings {
    pub deposit_enabled: bool,
    pub borrow_enabled: bool,
    pub withdraw_enabled: bool,
}

#[cw_serde]
pub struct LiquidationBonus {
    pub starting_lb: Decimal,
    pub slope: Decimal,
    pub min_lb: Decimal,
    pub max_lb: Decimal,
}

#[cw_serde]
pub struct InterestRateModel {
    pub optimal_utilization_rate: Decimal,
    pub base: Decimal,
    pub slope_1: Decimal,
    pub slope_2: Decimal,
}

#[cw_serde]
pub struct AssetParams {
    pub denom: String,
    pub credit_manager: CmSettings,
    pub red_bank: RedBankSettings,
    pub max_loan_to_value: Decimal,
    pub liquidation_threshold: Decimal,
    pub liquidation_bonus: LiquidationBonus,
    pub protocol_liquidation_fee: Decimal,
    pub deposit_cap: Uint128,
    pub close_factor: Decimal,
    pub reserve_factor: Decimal,
    pub interest_rate_model: InterestRateModel,
}

#[cw_serde]
pub struct AssetParamsUnchecked {
    pub denom: String,
    pub credit_manager: CmSettings,
    pub red_bank: RedBankSettings,
    pub max_loan_to_value: Decimal,
    pub liquidation_threshold: Decimal,
    pub liquidation_bonus: LiquidationBonus,
    pub protocol_liquidation_fee: Decimal,
    pub deposit_cap: Uint128,
    pub close_factor: Decimal,
    pub reserve_factor: Decimal,
    pub interest_rate_model: InterestRateModel,
}
