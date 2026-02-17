use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Uint128};

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: String,
    pub transmuter_contract: String,
    pub neutron_proxy: String,
    pub deposit_period_days: u64,
    pub withdrawal_period_days: u64,
    pub deposit_token: String,
    pub minimum_deposit: Uint128,
    pub mbrn_denom: String,
    pub staking_contract: Option<String>,
    pub mars_mirror_contract: Option<String>,
    pub ltv_disco_contract: Option<String>,
    pub discounts_contract: String,
    pub emissions_voting_contract: Option<String>,
    pub cliff_period_days: u64,
}

#[cw_serde]
pub enum ExecuteMsg {
    StartAcquisitionWindow { },
    Deposit {
        intents: Option<Vec<MbrnIntentOption>>,
    },
    Withdraw {
        amount: Uint128,
    },
    Claim {
        window_id: u64,
        mbrn_intent: Option<MbrnClaimIntent>,
    },
    ClaimForUser {
        user: String,
        window_id: u64,
        mbrn_intent: Option<MbrnClaimIntent>,
    },
    SendAcquisitionRewardsToDisco {
        window_id: u64,
        mbrn_intent: Option<MbrnClaimIntent>,
    },
    UpdateConfig {
        owner: Option<String>,
        transmuter_contract: Option<String>,
        neutron_proxy: Option<String>,
        deposit_period_days: Option<u64>,
        withdrawal_period_days: Option<u64>,
        deposit_token: Option<String>,
        minimum_deposit: Option<Uint128>,
        mbrn_denom: Option<String>,
        staking_contract: Option<String>,
        mars_mirror_contract: Option<String>,
        ltv_disco_contract: Option<String>,
        discounts_contract: Option<String>,
        emissions_voting_contract: Option<String>,
        cliff_period_days: Option<u64>,
    },
}

#[cw_serde]
pub enum QueryMsg {
    Config {},
    CurrentAcquisitionWindow {},
    ActiveAcquisitionWindow {},
    UserAcquisitionDeposit { user: String, window_id: u64 },
}

#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub transmuter_contract: String,
    pub neutron_proxy: String,
    pub deposit_period_days: u64,
    pub withdrawal_period_days: u64,
    pub deposit_token: String,
    pub minimum_deposit: Uint128,
    pub mbrn_denom: String,
    pub staking_contract: Option<String>,
    pub mars_mirror_contract: Option<String>,
    pub ltv_disco_contract: Option<String>,
    pub discounts_contract: String,
    pub emissions_voting_contract: Option<String>,
    pub cliff_period_seconds: u64,
}

#[cw_serde]
pub struct AcquisitionWindow {
    pub window_id: u64,
    pub start_time: u64,
    pub deposit_end: u64,
    pub withdrawal_end: u64,
    pub deposit_period_days: u64,
    pub total_deposit_amount: Uint128,
    pub acquisition_budget: Uint128,
}

#[cw_serde]
pub struct AcquisitionDeposit {
    pub deposit_id: Uint128,
    pub amount: Uint128,
    pub deposit_time: u64,
    pub vested_at: u64,
    /// Disco deposit ID if rewards were sent to Disco
    pub disco_deposit_id: Option<Uint128>,
    /// Amount of MBRN claimed and sent to Disco
    pub claimed_mbrn_amount: Option<Uint128>,
    /// Asset deposited to Disco (needed for clawback)
    pub disco_asset: Option<String>,
    /// LTV used for Disco deposit (needed for clawback)
    pub disco_ltv: Option<Decimal>,
    /// Max borrow LTV used for Disco deposit (needed for clawback)
    pub disco_max_borrow_ltv: Option<Decimal>,
    /// Epoch start time for Disco deposit (needed for clawback)
    pub disco_epoch_start_time: Option<u64>,
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

#[cw_serde]
pub struct CurrentAcquisitionWindowResponse {
    pub window: Option<AcquisitionWindow>,
}

#[cw_serde]
pub struct ActiveAcquisitionWindowResponse {
    pub window: Option<AcquisitionWindow>,
}

#[cw_serde]
pub struct UserAcquisitionDepositResponse {
    pub deposit: Option<AcquisitionDeposit>,
}

/// MBRN claim intent action
/// Similar to CompoundAction in Disco - supports one-time action or ongoing intent
#[cw_serde]
pub struct MbrnClaimIntent {
    /// Whether to apply intent for this claim
    /// If true, applies intent regardless of ongoing setting
    pub apply_now: bool,
    /// Whether to set this as ongoing intent for future claims
    /// If true, future claims will automatically use these intents
    pub set_ongoing: bool,
    /// Intent distribution ratios (must sum to 1.0)
    pub intents: Vec<MbrnIntentOption>,
}

#[cw_serde]
pub struct MbrnIntentOption {
    /// Intent type
    pub intent_type: MbrnIntentType,
    /// Ratio of claimed MBRN to allocate (0.0 to 1.0)
    pub ratio: Decimal,
    /// Optional lock information if intent supports locking
    pub lock: Option<crate::types::Locked>,
}

#[cw_serde]
pub enum MbrnIntentType {
    /// Stake MBRN in staking contract
    Stake {},
    /// Deposit into Disco via Mars Mirror contract
    DepositViaMarsMirror {
        /// Asset to deposit (must be valid Disco asset)
        asset: String,
        /// LTV to target (optional, can be auto-selected by mirror)
        target_ltv: Option<Decimal>,
        /// Max borrow LTV to target (optional, can be auto-selected by mirror)
        target_max_borrow_ltv: Option<Decimal>,
    },
    /// Send MBRN to a specified address
    SendToAddress {
        /// Address to receive the MBRN
        address: String,
    },
}

