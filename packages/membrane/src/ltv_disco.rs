use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Int128, Uint128};

use crate::types::{DepositDenom, Locked};

/// Time cliff representing a change in daily LVT delta
#[cw_serde]
pub struct LVTTimeCliff {
    /// Timestamp when this cliff becomes active
    pub timestamp: u64,
    /// Daily delta change at this cliff (can be positive or negative)
    pub delta_change: Int128,
}

/// Tracks LVT changes over time for a deposit
#[cw_serde]
pub struct DepositLVTTracking {
    /// Base LVT at reference_time
    pub base_lvt: Uint128,
    /// Reference timestamp for base_lvt and daily_delta calculations.
    /// 
    /// This is the point in time where `base_lvt` and `daily_delta` are known.
    /// All time-cliff calculations use this as the starting point. To calculate LVT
    /// at any other timestamp, we apply the daily delta and process time cliffs
    /// forward (if timestamp > reference_time) or backward (if timestamp < reference_time).
    /// 
    /// Typically set to the current block time when tracking is initialized or updated,
    /// but can be any timestamp. When updating tracking, if the reference_time changes,
    /// the base_lvt is adjusted to the new reference_time using the previous tracking data.
    pub reference_time: u64,
    /// Daily delta at reference_time
    pub daily_delta: Int128,
    /// Time cliffs for this deposit (sorted by timestamp)
    pub time_cliffs: Vec<LVTTimeCliff>,
}

/// Tracks LVT changes over time for a group (aggregate of all deposits)
#[cw_serde]
pub struct GroupLVTTracking {
    /// Base LVT total at reference_time
    pub base_total: Uint128,
    /// Reference timestamp for base_total and base_daily_delta calculations.
    /// 
    /// This is the point in time where `base_total` and `base_daily_delta` are known.
    /// All time-cliff calculations use this as the starting point. To calculate group LVT
    /// at any other timestamp, we apply the daily delta and process time cliffs
    /// forward (if timestamp > reference_time) or backward (if timestamp < reference_time).
    /// 
    /// Typically set to the current block time when tracking is initialized or updated,
    /// but can be any timestamp. When updating tracking, if the reference_time changes,
    /// the base_total is adjusted to the new reference_time using the previous tracking data.
    pub reference_time: u64,
    /// Cumulative daily delta at reference_time
    pub base_daily_delta: Int128,
    /// Time cliffs sorted by timestamp (ascending)
    pub time_cliffs: Vec<LVTTimeCliff>,
}

//NOTES: 
// If the Transmuter is low on CDT, the bad debt fulfillment will error.
// To prevent this from happening we need to keep the CDT balance of the Transmuter high enough. 
// (The transmuter helps do this by setting the target ratio to the mirror of deployed USDC)
// The real problem with this erroring is that it'll block liquidations as well,
// so if we can fix that without delaying the bad debt event, we should.
// Solution: reply on error of the Transmuter's withdrawal to add errored VTs into account & then create a retry function for it.

// HOW DO WE KEEP USERS FROM WITHDRAWING DURING ROCKY PERIODS?
// Solution: We hold a portion of revenue to only disperse post-liquidation.

// HOW DO WE PROTECT CDP USERS FROM BEING LIQUIDATED BY THE DYNAMIC LTVS CHANGING ?
// As long as any capital is deployed, the LTVs will be dynamic 
// but since the ratios can change based on the user withdrawing, there is no LTV param guaranteees.
// Solution A: The CDP contract will slowly accrue the avg LTVs onto the config's base LTVS. (This protects CDP users from withdraws sinking LTV and liquidating users)
// Solution B: If avg LTV or accrue step is going down, we change it once per period at a max of some % (say 5%)




/// Instantiate message
#[cw_serde]
pub struct InstantiateMsg {
    /// Contract owner
    pub owner: Option<String>,
    /// CDP contract address
    pub cdp_contract: String,
    /// Deposit denomination
    /// This is made to be either CDT or the Transmuter's vault token.
    /// The Transmuter's vault info will have CDT as the underlying token.
    pub deposit_denom: DepositDenom,
    /// CDT token denomination for revenue distribution and bad debt fulfillment.
    /// CRITICAL: This MUST be the CDT token. Bad debt fulfillment assumes this is CDT.
    /// If this is not the CDT token, bad debt fulfillment will be broken and may lose funds/break state.
    pub cdt_denom: String,
    /// Minimum deposit amount
    pub minimum_deposit: Uint128,
    /// Maximum LTV
    pub max_ltv: Decimal,
    /// Percent of revenue to disperse linearly
    pub percent_to_disperse: Decimal,
    /// Default dispersal window in hours
    pub dispersal_window: u64,
    /// Window for liquidations to activate dispersals (in hours)
    pub activation_window: u64,
    /// Oracle contract for querying asset prices
    pub oracle_contract: String,
    /// Chain proxy contract for executing swaps
    pub chain_proxy_contract: String,
    /// Emissions voting contract address
    pub emissions_voting_contract: Option<String>,
    /// Lock duration ceiling in days (max days you can lock)
    pub lock_duration_ceiling: Option<u64>,
    /// Optional affiliate fee percentage (default 1%)
    pub affiliate_fee: Option<Decimal>,
    /// Optional maximum management fee percentage (default 0%)
    pub max_management_fee: Option<Decimal>,
    /// Optional LTV delta minimum for historical tracking (default 1%)
    pub ltv_delta_minimum: Option<Decimal>,
    /// Points system contract address (optional)
    pub points_system_contract: Option<String>,
    /// Revenue distributor contract address (optional, for querying epoch information)
    pub revenue_distributor: Option<String>,
    /// Auction contract address (optional, for receiving MBRN revenue)
    pub auction_contract: Option<String>,
    /// MBRN denom (optional, for validating MBRN revenue)
    pub mbrn_denom: Option<String>,
}

/// Execute messages
#[cw_serde]
pub enum ExecuteMsg {
    /// Create a new LTV queue for an asset
    CreateQueue {
        asset: String,
    },
    /// Update an existing LTV queue
    UpdateQueue {
        asset: String,
        max_ltv: Option<Decimal>,
        percent_to_disperse: Option<Decimal>,
    },
    /// Submit a backing deposit
    SubmitDeposit {
        deposit_input: BackingDepositInput,
        deposit_owner: Option<String>,
        /// Optional lock information. If provided, deposit is locked on creation
        locked: Option<Locked>,
        /// Optional deposit_id to deposit into a specific existing deposit
        deposit_id: Option<Uint128>,
        /// Optional manager address (can move deposits but cannot withdraw)
        manager: Option<String>,
        /// Optional affiliate address to set when depositing
        affiliate_address: Option<String>,
        /// Optional revenue destination address (if set, claimed revenue goes here instead of deposit.user)
        revenue_destination: Option<String>,
    },
    /// Withdraw a backing deposit (by group)
    WithdrawDeposit {
        asset: String,
        ltv: Decimal,
        max_borrow_ltv: Decimal,
        deposit_id: Uint128,
        amount: Option<Uint128>,
        /// Epoch start time when the deposit was created (required to identify the deposit)
        epoch_start_time: u64,
    },
    /// Lock a deposit for a specified duration
    Lock {
        asset: String,
        ltv: Decimal,
        max_borrow_ltv: Decimal,
        deposit_id: Uint128,
        locked: Locked,
        amount: Option<Uint128>,
        /// Epoch start time when the deposit was created (required to identify the deposit)
        epoch_start_time: u64,
    },
    /// Move a deposit to a different slot/group
    MoveDeposit {
        asset: String,
        ltv: Decimal,
        max_borrow_ltv: Decimal,
        deposit_id: Uint128,
        destination: BackingDepositInput,
        amount: Option<Uint128>,
        /// User address (optional, defaults to sender)
        /// If provided, checks that sender is a manager for this user's deposit
        user: Option<String>,
        /// Epoch start time when the source deposit was created (required to identify the deposit)
        epoch_start_time: u64,
    },
    /// Update deposit settings (owner, manager, revenue_destination)
    /// Only the deposit owner can update these settings
    UpdateDeposit {
        asset: String,
        ltv: Decimal,
        max_borrow_ltv: Decimal,
        deposit_id: Uint128,
        /// Deposit owner address (Some = update owner, None = no change)
        deposit_owner: Option<String>,
        /// Manager address (Some = set/update manager, None = remove manager)
        manager: Option<String>,
        /// Revenue destination address (Some = set/update revenue_destination, None = remove revenue_destination)
        revenue_destination: Option<String>,
        /// Epoch start time when the deposit was created (required to identify the deposit)
        epoch_start_time: u64,
    },
    /// Toggle withdrawals for a deposit
    /// Only the depositor (the address that made the deposit) can call this
    ToggleWithdrawals {
        user: String,
        asset: String,
        ltv: Decimal,
        max_borrow_ltv: Decimal,
        deposit_id: Uint128,
        enabled: bool,
        /// Epoch start time when the deposit was created (required to identify the deposit)
        epoch_start_time: u64,
    },
    /// Add bad debt to an LTV queue (CDP contract only)
    AddBadDebt {
        asset: String,
        amount: Uint128,
    },
    /// Retry failed bad debt fulfillments
    // RetryFailedBadDebt {
    //     asset: String,
    // },
    /// Add revenue to an asset's LTV queue
    AddRevenue {
        asset: String,
    },
    /// Claim accumulated revenue rewards for a specific user and group
    ClaimRevenueForUser {
        user: String,
        asset: String,
        max_ltv: Decimal,
        max_borrow_ltv: Decimal,
        limit: Option<u32>,
        compound_action: Option<CompoundAction>,
    },
    /// Disperse revenue linearly over the active window
    DisperseRevenue {
        asset: String,
    },
    /// Update contract configuration
    UpdateConfig {
        owner: Option<String>,
        cdp_contract: Option<String>,
        deposit_denom: Option<DepositDenom>,
        cdt_denom: Option<String>,
        minimum_deposit: Option<Uint128>,
        percent_to_disperse: Option<Decimal>,
        dispersal_window: Option<u64>,
        activation_window: Option<u64>,
        oracle_contract: Option<String>,
        chain_proxy_contract: Option<String>,
        emissions_voting_contract: Option<String>,
        lock_duration_ceiling: Option<u64>,
        affiliate_fee: Option<Decimal>,
        max_management_fee: Option<Decimal>,
        ltv_delta_minimum: Option<Decimal>,
        points_system_contract: Option<String>,
        revenue_distributor: Option<String>,
        auction_contract: Option<String>,
        mbrn_denom: Option<String>,
    },
    // /// Post a deposit tracker entry for base token tracking
    // PostDepositTrackerEntry {
    //     asset: String,
    //     max_ltv: Decimal,
    //     max_borrow_ltv: Decimal,
    // },
    /// Assures that for deposits & withdrawals the conversion rate is static
    /// Only callable by the contract
    RateAssurance {
        asset: String,
        max_ltv: Decimal,
        max_borrow_ltv: Decimal,
    },
    /// Refresh lock on a deposit (extends locked_until if perpetual_lock is set)
    RefreshLock {
        /// User address (if None, uses info.sender)
        user: Option<String>,
        asset: String,
        ltv: Decimal,
        max_borrow_ltv: Decimal,
        deposit_id: Uint128,
        /// Epoch start time when the deposit was created (required to identify the deposit)
        epoch_start_time: u64,
    },
    /// Set affiliate for a user
    SetAffiliate {
        user: String,
        affiliate_address: String,
        label: Option<String>,
    },
    /// Set manager fee (only callable by managers with active deposits)
    SetManagerFee {
        fee: Decimal,
    },
    /// Clean manager fee state for a manager with no deposits (permissionless)
    CleanManagerFee {
        manager: String,
    },
    /// Add deposit token revenue from auction with per-asset distribution
    /// Adds to total_deposit_tokens to compound for all depositors
    /// Callable by auction contract only
    AddDepositTokenRevenue {
        /// Distribution: which collateral assets earned this revenue
        per_asset_distribution: Vec<crate::types::Asset>,
    },
}

/// Query messages
#[cw_serde]
pub enum QueryMsg {
    /// Get contract configuration
    Config {},
    /// Get LTV queue(s) for asset(s)
    /// If `assets` is non-empty, returns queues for those specific assets.
    /// If `assets` is empty, returns all queues (paginated with `limit`/`start_after`).
    GetLTVQueue {
        assets: Vec<String>,
        limit: Option<u32>,
        start_after: Option<String>,
    },
    /// Get backing deposit by group and deposit ID
    GetBackingDeposit { 
        user: String, 
        asset: String, 
        ltv: Decimal, 
        max_borrow_ltv: Decimal,
        deposit_id: Uint128,
    },
    /// Get backing deposits by user
    GetBackingDepositsByUser {
        user: String,
        asset: String,
        limit: Option<u32>,
        start_after: Option<Uint128>,
    },
    /// Get average LTVs for assets
    GetAverageLTVs { assets: Vec<String> },
    /// Check if the LTV Disco can handle bad debt for an asset
    CanHandleBadDebt {
        asset: String,
        amount: Uint128,
    },
    /// Cumulative Revenue queries
    GetCumulativeRevenue {
        asset: String,
        max_ltv: Option<Decimal>,
        max_borrow_ltv: Option<Decimal>,
    },
    /// Get pending claims for a user across their deposits
    PendingClaims { user: String, asset: String },
    /// Get user lifetime revenue summary
    GetUserLifetimeRevenue { user: String, asset: String },
    /// Get revenue events for a group
    GetRevenueEvents { asset: String, max_ltv: Decimal, max_borrow_ltv: Decimal },
    /// Get all assets that have LTV queues
    GetAssets {},
    /// Get daily TVL tracker history
    GetDailyTVL {},
    /// Get daily LTV tracker history for an asset
    GetDailyLTV { asset: String },
    /// Get user's total deposits
    UserTotalDeposits { user: String },
    /// Get user's locked deposits
    GetLockedDeposits { user: String },
    /// Get all user deposits across all assets
    GetAllUserDeposits { user: String },
    /// Get affiliates for a user
    GetAffiliates { user: String },
    /// Convert vault tokens to deposit tokens for a specific deposit group
    VaultTokenConversion {
        asset: String,
        ltv: Decimal,
        max_borrow_ltv: Decimal,
        vault_tokens: Uint128,
    },
    /// Convert deposit tokens to vault tokens for a specific deposit group
    DepositTokenConversion {
        asset: String,
        ltv: Decimal,
        max_borrow_ltv: Decimal,
        deposit_tokens: Uint128,
    },
    /// Get managed deposit keys for a manager (paginated)
    GetManagedDepositKeys {
        manager: String,
        limit: Option<u32>,
        start_after: Option<String>,
    },
    /// Get total insurance (MBRN deposits + pending rewards)
    /// Returns total in CDT if oracle available, otherwise returns separate values
    GetTotalInsurance {},
    /// Get manager fee for a specific manager
    GetManagerFee {
        manager: String,
    },
    /// Get daily insurance tracker history for an asset
    GetDailyInsurance {
        asset: String,
    },
}

/// Response for LTV queue query
#[cw_serde]
pub struct LTVQueueResponse {
    pub queues: Vec<(String, LTVQueue)>,
}

/// Response for backing deposit query
#[cw_serde]
pub struct BackingDepositResponse {
    pub deposit: BackingDeposit,
}

/// Response for backing deposits by user query
#[cw_serde]
pub struct BackingDepositsByUserResponse {
    pub deposits: Vec<BackingDeposit>,
}

/// Response for average LTV queries
#[cw_serde]
pub struct AverageLTVsResponse {
    pub average_max_ltv: Decimal,
    pub average_max_borrow_ltv: Decimal,
}

/// Response for managed deposit keys query
#[cw_serde]
pub struct ManagedDepositKeysResponse {
    pub keys: Vec<String>,
    pub total: u64,
    pub next_start_after: Option<String>,
}

/// Response for total insurance query
#[cw_serde]
pub enum TotalInsuranceResponse {
    /// Oracle conversion succeeded - return total insurance in CDT
    WithOracle {
        total_insurance: Uint128,
    },
    /// Oracle conversion failed - return pending CDT and per-asset deposit totals
    WithoutOracle {
        pending_cdt: Uint128,
        mbrn_deposit_totals: Vec<(String, Uint128)>,
    },
}

/// Response for manager fee query
#[cw_serde]
pub struct ManagerFeeResponse {
    /// Manager address
    pub manager: String,
    /// Manager fee percentage (0 if not set)
    pub fee: Decimal,
}

#[cw_serde]
pub struct VTGrowthResponse {
    pub timestamp: u64,
    pub amount: Uint128,
}

/// Pending claims aggregate response
#[cw_serde]
pub struct PendingClaimsResponse {
    pub user: String,
    pub asset: String,
    pub claims: Vec<DepositPendingClaim>,
}

/// Response for claimable revenue query (deprecated, use PendingClaims instead)
#[cw_serde]
pub struct ClaimableRevenueResponse {
    pub amount: Uint128,
}

#[cw_serde]
pub struct DepositPendingClaim {
    pub max_ltv: Decimal,
    pub max_borrow_ltv: Decimal,
    pub pending_amount: Uint128,
}

/// Migrate message
#[cw_serde]
pub struct MigrateMsg {}

/// Compound action for revenue claims
/// 
/// Allows users to control compound behavior on a per-claim basis.
/// Can be used to:
/// - One-time compound: compound_now = true, set_ongoing = false
/// - Set ongoing intent: compound_now = true/false, set_ongoing = true
/// - Override deposit setting: compound_now = true overrides deposit.compound_claims
#[cw_serde]
pub struct CompoundAction {
    /// Whether to compound this claim
    /// If true, deposits will compound this claim regardless of their compound_claims setting
    /// Priority: compound_now > deposit.compound_claims
    pub compound_now: bool,
    /// Whether to set compound_claims as ongoing intent on deposits
    /// If true, all deposits will have compound_claims set to true for future claims
    /// This allows users to "turn on" automatic compounding permanently
    pub set_ongoing: bool,
    /// Optional recipient address for the claim
    /// If provided, the claim will be sent to this address instead of the user
    pub recipient_address: Option<String>,
}


/// Configuration for the LTV Discount contract
#[cw_serde]
pub struct Config {
    /// Contract owner
    pub owner: Addr,
    /// CDP contract address for querying basket information
    pub cdp_contract: Addr,
    /// Deposit denomination.
    /// This is made to be either CDT or the Transmuter's vault token.
    /// The Transmuter's vault info will have CDT as the underlying token.
    pub deposit_denom: DepositDenom,
    /// CDT token denomination for revenue distribution and bad debt fulfillment.
    /// CRITICAL: This MUST be the CDT token. Bad debt fulfillment assumes this is CDT.
    /// If this is not the CDT token, bad debt fulfillment will be broken and may lose funds/break state.
    pub cdt_denom: String,
    /// Minimum deposit amount
    pub minimum_deposit: Uint128,
    /// Max LTV
    pub max_ltv: Decimal,
    /// Percent of revenue to disperse linearly
    pub percent_to_disperse: Decimal,
    /// Dispersal window (in hours) 
    /// We disperse accumulated liquidation event jackpot over a window in order to pay users 
    /// for taking the risk of the current liquidation period. Because liquidation events tend to be 
    /// longer than a single liquidation, we need to disperse over a window instead of immediately.
    /// Any dispersable revenue that comes in after the dispersal starts is saved for the next event.
    pub dispersal_window: u64,
    /// Window for liquidations to activate dispersals (in hours)
    /// When querying for liquidations we need to know how far back we'll accept a liquidation to activate dispersal.
    pub activation_window: u64,
    /// Oracle contract for querying asset prices
    pub oracle_contract: Addr,
    /// Chain proxy contract for executing swaps to convert collateral to CDT
    pub chain_proxy_contract: Addr,
    /// Emissions Voting contract address
    pub emissions_voting_contract: Option<Addr>,
    /// Lock duration ceiling in days (max days you can lock)
    pub lock_duration_ceiling: u64,
    /// Affiliate fee percentage (default 1%)
    pub affiliate_fee: Decimal,
    /// Maximum management fee percentage (default 0%)
    pub max_management_fee: Decimal,
    /// LTV delta minimum for historical tracking (default 1%)
    pub ltv_delta_minimum: Decimal,
    /// Points system contract address (optional, for awarding points to managers)
    pub points_system_contract: Option<Addr>,
    /// Revenue distributor contract address (for querying epoch information)
    pub revenue_distributor: Option<Addr>,
    /// Auction contract address (for receiving MBRN revenue)
    pub auction_contract: Option<Addr>,
    /// MBRN denom (for validating MBRN revenue)
    pub mbrn_denom: Option<String>,
}

#[cw_serde]
pub struct Dispersal {
    pub total_to_disperse: Uint128,
    pub dispersal_window: u64,
    pub active_dispersal: ActiveDispersal,
    pub pending_dispersal: Uint128
}

#[cw_serde]
pub struct ActiveDispersal {
    pub dispersal_start: u64,
    pub amount_dispersed: Uint128,
}
/// LTV Queue containing slots for different LTV ranges
#[cw_serde]
pub struct LTVQueue {
    /// Vector of LTV slots, sorted by LTV value
    pub slots: Vec<MaxLTVSlot>,
    /// Minimum LTV for this queue
    pub borrow_ltv: DecimalMinMax,
    /// Maximum LTV for this queue
    pub liquidation_ltv: DecimalMinMax,
    /// Current deposit ID counter
    pub current_deposit_id: Uint128,
    /// Optional percent of revenue to disperse for this queue
    /// If None, uses the global config value
    pub percent_to_disperse: Option<Decimal>,
}

#[cw_serde]
pub struct DecimalMinMax {
    /// Minimum 
    pub min: Decimal,
    /// Maximum
    pub max: Decimal,
}

/// Individual LTV slot containing backing deposits grouped by maxBorrowLTV
#[cw_serde]
pub struct MaxLTVSlot {
    /// LTV value for this slot
    pub ltv: Decimal,
    /// Backing deposits grouped by maxBorrowLTV
    pub deposit_groups: Vec<MaxBorrowLTVGroup>,
    /// Total deposit tokens in this slot (for ease of tracking, not used for calculations)
    pub total_deposit_tokens: Uint128,
    /// Total bad debt in this slot (for ease of tracking, not used for calculations)
    pub bad_debt: Uint128,
}

/// Group of deposits with the same maxBorrowLTV
#[cw_serde]
pub struct MaxBorrowLTVGroup {
    /// Max borrow LTV for this group
    pub max_borrow_ltv: Decimal,
    /// Total deposit tokens for this group
    pub total_deposit_tokens: Uint128,
    /// Total vault tokens for this group
    pub total_vault_tokens: Uint128,
    /// Total locked vault tokens for this group (sum of all deposits' locked_vault_tokens)
    pub total_locked_vault_tokens: Uint128,
    /// Total unused locked vault tokens (lost weight from late deposits + contract deposits)
    /// This tracks locked vault tokens that should be excluded from revenue distribution:
    /// 1. Lost weight from deposits made late in the epoch (time penalty)
    /// 2. Full weight of contract-owned deposits (from early withdrawal penalties)
    /// This field resets to zero at the start of each new epoch
    pub total_unused_locked_vault_tokens: Uint128,
    /// Epoch start time for which total_unused_locked_vault_tokens applies
    /// When this changes, we know a new epoch started
    pub effective_epoch_start: Option<u64>,
    /// LVT tracking for calculating LVT at any timestamp
    pub lvt_tracking: GroupLVTTracking,
}

/// Backing deposit similar to Bid but for LTV slots
#[cw_serde]
pub struct BackingDeposit {
    /// User address
    pub user: Addr,
    /// Deposit amount (vault tokens)
    pub vault_tokens: Uint128,
    /// Boosted vault tokens (vault_tokens * (lock_days + 1)) used for revenue calculations
    pub locked_vault_tokens: Uint128,
    /// Chosen max borrow LTV for sorting within slot
    pub max_borrow_ltv: Decimal,
    /// Last timestamp the user claimed revenue for this deposit
    pub last_claimed: u64,
    /// Lock information (if locked)
    pub locked: Option<Locked>,
    /// Timestamp when deposit was created (for boost calculations)
    pub start_time: u64,
    /// Timestamp when deposit was made within the current epoch (for epoch-based discounting)
    /// None for deposits made before epoch tracking was implemented
    pub deposit_time: Option<u64>,
    /// Whether to automatically compound claimed revenue back into this deposit
    /// 
    /// If true, this deposit will automatically compound its claimed revenue on every claim.
    /// The claimed CDT is swapped to deposit tokens via neutron_proxy and added back to the deposit.
    /// 
    /// Can be set via CompoundAction.set_ongoing or manually by the contract owner.
    /// Defaults to false for new deposits.
    pub compound_claims: bool,
    /// Optional manager address
    /// Manager can move deposits but cannot withdraw
    pub manager: Option<Addr>,
    /// Address that made the deposit (None for self-deposits, Some for deposits made on behalf of others)
    /// First depositor for a position is saved and cannot be changed
    pub depositor: Option<Addr>,
    /// Whether withdrawals are enabled for this deposit
    /// Only the depositor can toggle this setting
    pub withdrawals_enabled: bool,
    /// LVT tracking for calculating this deposit's LVT at any timestamp
    pub lvt_tracking: DepositLVTTracking,
    /// Optional revenue destination address
    /// If set, claimed revenue will be sent to this address instead of deposit.user
    pub revenue_destination: Option<Addr>,
}

/// Input for creating a backing deposit
#[cw_serde]
pub struct BackingDepositInput {
    /// Asset to deposit for
    pub asset: String,
    /// Chosen LTV slot
    pub ltv: Decimal,
    /// Chosen max borrow LTV
    pub max_borrow_ltv: Decimal,
    /// Epoch start time for deposit key creation and lookup
    /// If None, will be queried from revenue distributor or use current time
    pub epoch_start_time: Option<u64>,
}

/// Revenue tracking entry with timestamp
#[cw_serde]
pub struct RevenueTrackingEntry {
    /// Timestamp when this entry was created
    pub timestamp: u64,
    /// Total cumulative revenue at this timestamp
    pub total_revenue: Uint128,
}

// Event-based revenue tracking per group
#[cw_serde]
pub struct RevenueEvent {
    pub timestamp: u64,
    /// Epoch start time when this event was created (for claim-time discount logic)
    pub epoch_start: u64,
    /// Epoch end time when this event was created (to check if deposit was made within this epoch)
    pub epoch_end: u64,
    // Revenue per 1 locked vault token
    pub amount_per_locked_vt: Decimal,
    // Remaining total to be claimed from this event
    pub amount_to_be_claimed: Uint128,
}

/// User lifetime revenue entry
#[cw_serde]
pub struct UserLifetimeRevenueEntry {
    pub timestamp: u64,
    pub total_claimed: Uint128,
}

/// TVL tracking entry
#[cw_serde]
pub struct TVLEntry {
    pub timestamp: u64,
    pub total_deposit_tokens: Uint128,
}

/// LTV tracking entry
#[cw_serde]
pub struct LTVEntry {
    pub timestamp: u64,
    pub average_max_ltv: Decimal,
    pub average_max_borrow_ltv: Decimal,
}

/// Insurance tracking entry per asset (raw components, no oracle conversion)
#[cw_serde]
pub struct InsuranceEntry {
    pub timestamp: u64,
    /// Pending CDT from this asset's dispersal (active undisbursed + pending)
    pub pending_cdt: Uint128,
    /// Total deposit tokens backing this asset
    pub deposit_tokens: Uint128,
}

/// Response for assets query
#[cw_serde]
pub struct AssetsResponse {
    pub assets: Vec<String>,
}

/// Response for daily TVL query
#[cw_serde]
pub struct DailyTVLResponse {
    pub entries: Vec<TVLEntry>,
}

/// Response for daily LTV query
#[cw_serde]
pub struct DailyLTVResponse {
    pub entries: Vec<LTVEntry>,
}

/// Response for daily insurance query
#[cw_serde]
pub struct DailyInsuranceResponse {
    pub entries: Vec<InsuranceEntry>,
}

/// Response for user total deposits query
#[cw_serde]
pub struct UserTotalDepositsResponse {
    pub total_deposits: Uint128,
}

/// Locked deposit with identifying information to reconstruct the deposit key
#[cw_serde]
pub struct LockedDeposit {
    /// Asset identifier
    pub asset: String,
    /// LTV value for this deposit
    pub ltv: Decimal,
    /// Max borrow LTV for this deposit
    pub max_borrow_ltv: Decimal,
    /// Deposit ID
    pub deposit_id: Uint128,
    /// The backing deposit
    pub deposit: BackingDeposit,
}

/// Response for locked deposits query
#[cw_serde]
pub struct LockedDepositsResponse {
    pub locked_deposits: Vec<LockedDeposit>,
}

/// Response for all user deposits query
#[cw_serde]
pub struct AllUserDepositsResponse {
    pub deposits: Vec<UserDepositInfo>,
}

/// User deposit information with identifying details
#[cw_serde]
pub struct UserDepositInfo {
    /// Asset identifier
    pub asset: String,
    /// LTV value for this deposit
    pub ltv: Decimal,
    /// Max borrow LTV for this deposit
    pub max_borrow_ltv: Decimal,
    /// Deposit ID
    pub deposit_id: Uint128,
    /// The backing deposit
    pub deposit: BackingDeposit,
    /// Deposit tokens (converted from vault tokens)
    pub deposit_tokens: Uint128,
}
