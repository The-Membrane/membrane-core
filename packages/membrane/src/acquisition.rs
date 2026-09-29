use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Uint128};

/// Configuration for the acquisition accrual + control system model.
///
/// ## How it works
/// - A base acquisition rate accrues MBRN into a "pool" over time while utilization >= target.
/// - Once the pool hits `max_mbrn_emission`, CDP base_interest_rate is bumped by `bump_rate`
///   every `bump_interval_seconds`. Below target utilization, bump_rate decreases at
///   `reduction_speed_multiplier`x faster.
/// - A control system mutates `base_acquisition_rate` based on how efficiently MBRN spend
///   attracts new deposits. Rolling cumulative efficiency comparison: if improving → lower rate,
///   if worsening → higher rate. Mutations proportional to delta from previous measurement.
/// - At the end of the withdrawal period, if realized MBRN/deposit ratio exceeds the
///   `efficiency_threshold` above baseline, total emission is reduced to clamp at that threshold.
/// - Rate is capped at `max_mbrn_emission / deposit_period_seconds`. Max rate change per
///   mutation = `max_rate_change_per_mutation`. No mutations until day 1 of deposit period ends.
#[cw_serde]
pub struct AcquisitionModelConfig {
    /// Per-second MBRN emission rate. Pool grows at `rate * elapsed_seconds`.
    pub base_acquisition_rate: Decimal,
    /// Initial pool seed / rate calibration basis. Default 1_000_000_000 uMBRN.
    pub initial_acquisition_amount: Uint128,
    /// Max MBRN emittable per window. Triggers bump_rate mechanism when reached.
    pub max_mbrn_emission: Uint128,
    /// Transmuter utilization target (mirrored). Pool only accrues at or above this.
    pub target_utilization: Decimal,
    /// Per-interval bump increment (e.g. 0.1% = Decimal::permille(1)).
    /// bump_rate += this each bump_interval when above target and pool maxed.
    pub bump_increment: Decimal,
    /// How often bump_rate changes, in seconds. Default 17280 (4.8 hours).
    pub bump_interval_seconds: u64,
    /// How much faster bump_rate decreases below target util. Default 2x.
    /// Reduction interval = bump_interval / this.
    pub reduction_speed_multiplier: Decimal,
    /// Max acceptable efficiency delta from rolling avg. Default 20%.
    /// At withdrawal end, emission is clamped if MBRN/deposit exceeds baseline * (1 + threshold).
    pub efficiency_threshold: Decimal,
    /// Max % the acquisition rate can change per control system mutation.
    /// Defaults to efficiency_threshold value.
    pub max_rate_change_per_mutation: Decimal,
    /// Maximum bump_rate allowed. Caps CDP interest rate inflation. Default 100% (Decimal::one()).
    pub max_bump_rate: Decimal,
    /// Minimum seconds between worsening efficiency mutations. Default 28800 (8 hours).
    /// Improving mutations (rate decreases) bypass this cooldown.
    pub mutation_cooldown_seconds: u64,
    /// Duration in seconds over which accrual ramps from start_fraction to 100%. Default 86400 (24h).
    pub accrual_ramp_seconds: u64,
    /// Starting fraction of the acquisition rate during ramp period. Default 0.25 (25%).
    /// Must be in (0, 1].
    pub accrual_ramp_start_fraction: Decimal,
}

/// Live state of the acquisition model for the current window.
/// Updated on every AccruePool call, deposit, and withdraw.
#[cw_serde]
pub struct AcquisitionModelState {
    /// Current per-second emission rate (mutated by control system).
    pub current_acquisition_rate: Decimal,
    /// Total MBRN accrued in pool this window.
    pub accrued_pool: Uint128,
    /// Last time accrual was computed.
    pub last_accrual_time: u64,
    /// Whether pool has hit max_mbrn_emission.
    pub pool_maxed: bool,
    /// Current bump_rate = cumulative bump sent to CDP.
    /// Increases by bump_increment each bump_interval above target util.
    /// Decreases by bump_increment each reduction_interval below target util. Floor 0.
    pub bump_rate: Decimal,
    /// Last time bump_rate was adjusted.
    pub last_bump_time: u64,
    /// Whether first deposit triggered window timers.
    pub window_timers_started: bool,
    /// Timestamp of first deposit (timers start here).
    pub first_deposit_time: Option<u64>,
    /// Number of deposit events for efficiency tracking.
    pub deposit_event_count: u64,
    /// Sum of all deposit amounts this window.
    pub total_new_deposits: Uint128,
    /// Efficiency (total_deposits / accrued_pool) at the previous mutation tick.
    pub last_mutation_efficiency: Option<Decimal>,
    /// Time of last control system mutation.
    pub last_mutation_time: Option<u64>,
    /// Whether the efficiency clamp has been applied at end of withdrawal.
    pub efficiency_clamped: bool,
}

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: String,
    pub transmuter_contract: String,
    pub neutron_proxy: String,
    pub cdp_contract: String,
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
    /// Acquisition model configuration
    pub acquisition_config: AcquisitionModelConfig,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Owner starts a new acquisition window. Pool begins accruing at target util.
    StartAcquisitionWindow {},
    /// User deposits during deposit period. First deposit triggers window timers.
    Deposit {
        intents: Option<Vec<MbrnIntentOption>>,
        /// Optional affiliate address for referral tracking.
        /// Stored on deposit; affiliate earns points when revenue is distributed.
        affiliate_address: Option<String>,
        /// Optional label for affiliate campaign tracking.
        affiliate_label: Option<String>,
    },
    /// User withdraws during withdrawal period.
    Withdraw {
        amount: Uint128,
    },
    /// Accrue the MBRN pool and apply bump_rate changes. Permissionless.
    /// Called by transmuter notification hook or anyone.
    AccruePool {},
    /// Claim deposit ownership after cliff. Transfers transmuter + disco deposit to user.
    /// If user didn't send to Disco, mints MBRN directly at claim time.
    Claim {
        window_id: u64,
        mbrn_intent: Option<MbrnClaimIntent>,
    },
    ClaimForUser {
        user: String,
        window_id: u64,
        mbrn_intent: Option<MbrnClaimIntent>,
    },
    /// After withdrawal period ends, optionally send MBRN rewards to Disco for a user.
    /// User chooses Disco route. If not called, user claims MBRN directly post-cliff.
    SendAcquisitionRewardsToDisco {
        window_id: u64,
        mbrn_intent: Option<MbrnClaimIntent>,
    },
    UpdateConfig {
        owner: Option<String>,
        transmuter_contract: Option<String>,
        neutron_proxy: Option<String>,
        cdp_contract: Option<String>,
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
        acquisition_config: Option<AcquisitionModelConfig>,
    },
    /// Self-callback: verifies transmuter ACQ_DT matches acquisition total_deposit_amount.
    /// Detects exploits that steal acquisition deposits.
    AcquisitionRateAssurance {},
}

#[cw_serde]
pub enum QueryMsg {
    Config {},
    AcquisitionConfig {},
    CurrentAcquisitionWindow {},
    ActiveAcquisitionWindow {},
    UserAcquisitionDeposit { user: String, window_id: u64 },
    /// Returns the live acquisition model state (pool, rates, efficiency).
    AcquisitionModelState {},
    /// Returns historical snapshots of the acquisition model's control system.
    /// If `window_id` is provided, filters to entries from that window only.
    AcquisitionHistory { window_id: Option<u64> },
}

#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub transmuter_contract: String,
    pub neutron_proxy: String,
    pub cdp_contract: String,
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
    /// Timestamp when StartAcquisitionWindow was called.
    pub start_time: u64,
    /// End of deposit period. Set to u64::MAX until first deposit triggers timers.
    pub deposit_end: u64,
    /// End of withdrawal period. Set to u64::MAX until first deposit triggers timers.
    pub withdrawal_end: u64,
    pub deposit_period_days: u64,
    pub total_deposit_amount: Uint128,
    /// Final MBRN budget (set to accrued_pool at withdrawal end, may be reduced by efficiency clamp).
    pub acquisition_budget: Uint128,
}

#[cw_serde]
pub struct AcquisitionDeposit {
    pub amount: Uint128,
    pub deposit_time: u64,
    pub vested_at: u64,
    /// Whether user chose to send MBRN to Disco (true) or will claim directly (false/None).
    pub sent_to_disco: Option<bool>,
    /// Calculated MBRN share for this user (set after withdrawal period).
    pub mbrn_claim_amount: Option<Uint128>,
    /// Disco deposit ID if rewards were sent to Disco
    pub disco_deposit_id: Option<Uint128>,
    /// Amount of MBRN claimed and sent to Disco
    pub claimed_mbrn_amount: Option<Uint128>,
    /// Asset deposited to Disco (needed for clawback)
    pub disco_asset: Option<String>,
    /// Slot used for Disco deposit (needed for clawback)
    pub disco_slot: Option<u8>,
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

#[cw_serde]
pub struct AcquisitionConfigResponse {
    pub config: AcquisitionModelConfig,
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

#[cw_serde]
pub struct AcquisitionModelStateResponse {
    pub state: Option<AcquisitionModelState>,
}

/// Snapshot of the acquisition model's control system at a point in time.
/// Recorded on every `accrue_pool_internal` call for historical tracking.
#[cw_serde]
pub struct AcquisitionHistoryEntry {
    /// Which acquisition window this entry belongs to.
    pub window_id: u64,
    pub timestamp: u64,
    pub current_acquisition_rate: Decimal,
    pub bump_rate: Decimal,
    pub accrued_pool: Uint128,
    pub pool_maxed: bool,
    /// Transmuter utilization at accrual time.
    pub utilization: Decimal,
    /// Efficiency (total_deposits / accrued_pool) at last mutation, if any.
    pub efficiency: Option<Decimal>,
    pub total_new_deposits: Uint128,
}

#[cw_serde]
pub struct AcquisitionHistoryResponse {
    pub history: Vec<AcquisitionHistoryEntry>,
}

/// MBRN claim intent action
/// Similar to CompoundAction in Disco - supports one-time action or ongoing intent
#[cw_serde]
pub struct MbrnClaimIntent {
    /// Whether to apply intent for this claim
    pub apply_now: bool,
    /// Whether to set this as ongoing intent for future claims
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
        /// Slot number (1-9) for the Disco deposit
        slot: u8,
    },
    /// Send MBRN to a specified address
    SendToAddress {
        /// Address to receive the MBRN
        address: String,
    },
}
