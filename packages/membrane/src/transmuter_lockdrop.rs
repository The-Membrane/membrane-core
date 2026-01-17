use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Uint128};

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: String,
    pub transmuter_contract: String,
    pub neutron_proxy: String,
    pub lockdrop_incentive_size: Uint128,
    pub deposit_period_days: u64,
    pub withdrawal_period_days: u64,
    pub deposit_token: String,
    pub minimum_deposit: Uint128,
    pub mbrn_denom: String,
    pub staking_contract: Option<String>,
    pub mars_mirror_contract: Option<String>,
    pub ltv_disco_contract: Option<String>,
    pub discounts_contract: String,
    pub maximum_boost: Decimal,
    pub minimum_lock_days: u64,
    pub emissions_voting_contract: Option<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    StartLockdrop {
        deposit_period_days: Option<u64>,
        withdrawal_period_days: Option<u64>,
    },
    Deposit {
        lock_days: u64,
        intents: Option<Vec<MbrnIntentOption>>,
    },
    Withdraw {
        amount: Uint128,
        lock_days: u64,
    },
    EditLock {
        lock_days: u64,
        new_lock_days: u64,
    },
    CompleteLocks {
        limit: Option<u32>,
    },
    Claim {
        users: Vec<String>,
        mbrn_intent: Option<MbrnClaimIntent>,
    },
    UpdateConfig {
        owner: Option<String>,
        transmuter_contract: Option<String>,
        neutron_proxy: Option<String>,
        lockdrop_incentive_size: Option<Uint128>,
        deposit_period_days: Option<u64>,
        withdrawal_period_days: Option<u64>,
        deposit_token: Option<String>,
        minimum_deposit: Option<Uint128>,
        mbrn_denom: Option<String>,
        staking_contract: Option<String>,
        mars_mirror_contract: Option<String>,
        ltv_disco_contract: Option<String>,
        discounts_contract: Option<String>,
        maximum_boost: Option<Decimal>,
        minimum_lock_days: Option<u64>,
        emissions_voting_contract: Option<String>,
    },
    /// Receive voting result from emissions voting contract
    ReceiveVotingResult {
        /// Graph label to identify which parameter this result is for
        label: String,
        /// Result as Uint128 (for Uint128 graphs, e.g., emission rates)
        result_uint128: Option<Uint128>,
        /// Result as Decimal (for Decimal graphs, e.g., multipliers)
        result_decimal: Option<Decimal>,
    },
}

#[cw_serde]
pub enum QueryMsg {
    Config {},
    CurrentLockdrop {},
    UserDeposits { user: String },
    PendingLocks {},
    UserClaims { user: Option<String>, limit: Option<u32>, start_after: Option<String> },
    LockdropHistory {},
    UserHistory { user: String },
}

#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub transmuter_contract: String,
    pub neutron_proxy: String,
    pub lockdrop_incentive_size: Uint128,
    pub deposit_period_days: u64,
    pub withdrawal_period_days: u64,
    pub deposit_token: String,
    pub minimum_deposit: Uint128,
    pub mbrn_denom: String,
    pub staking_contract: Option<String>,
    pub mars_mirror_contract: Option<String>,
    pub ltv_disco_contract: Option<String>,
    pub discounts_contract: String,
    pub maximum_boost: Decimal,
    pub minimum_lock_days: u64,
    pub emissions_voting_contract: Option<String>,
}

#[cw_serde]
pub struct LockdropState {
    pub start_time: u64,
    pub deposit_end: u64,
    pub withdrawal_end: u64,
    pub total_deposit_points: Option<Uint128>,
}

#[cw_serde]
pub struct UserDeposit {
    pub amount: Uint128,
    pub intended_lock_days: u64,
    pub deposit_time: u64,
    pub intents: Option<Vec<MbrnIntentOption>>,
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

#[cw_serde]
pub struct CurrentLockdropResponse {
    pub lockdrop: Option<LockdropState>,
}

#[cw_serde]
pub struct UserDepositsResponse {
    pub deposits: Vec<UserDeposit>,
}

#[cw_serde]
pub struct PendingLocksResponse {
    pub users: Vec<String>,
}

#[cw_serde]
pub struct UserClaim {
    pub user: String,
    pub claim_amount: Uint128,
}

#[cw_serde]
pub struct UserClaimsResponse {
    pub claims: Vec<UserClaim>,
    pub total: u64,
    pub next_start_after: Option<String>,
}

#[cw_serde]
pub struct LockdropHistoryResponse {
    pub history: Vec<LockdropState>,
}

#[cw_serde]
pub struct UserLockdropHistory {
    pub deposit: Uint128,
    pub running_total_claims: Uint128,
    pub share_of_claims: Decimal,
    pub time: u64,
}

#[cw_serde]
pub struct UserHistoryResponse {
    pub history: Vec<UserLockdropHistory>,
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

