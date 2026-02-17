use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Timestamp, Uint128};
use crate::types::LiqAsset;

/// Standardized graph labels for emissions-voting
pub const TOTAL_EMISSIONS_GRAPH_LABEL: &str = "transmuter_total_emissions";
pub const ACQUISITION_PERCENTAGE_GRAPH_LABEL: &str = "transmuter_acquisition_percentage";

///This follows PSM risk insofar as if USDC depegs, this transmuter will become USDC only.
/// In the event that arbitragers use this to arb during a depeg, which is possible.
/// Luckily tho, if CDT has no market value, then swapped USDC to CDT is worthless.
/// And we aren't incentivizing market LPs for CDT, just this connection.
/// 
/// But if we grow & there is external liquidity for CDT, it means any CDT in this LP is at risk to be sold.
/// Sold using any LPs down to USDC's price, creating a depeg of market price. 
/// The only way it doesn't depeg is if there is more liquidity than CDT in this venue but since the main flow
/// is CDT -> USDC -> DeFi, this will be filled with CDT a lot.
/// 
/// So instead we'll add a whitelisting & rate-limiting mechanism to rate-limit any non-whitelisted addresses.
/// And then add a global rate-limit threshold for all non-whitelisted addresses to reduce CDTs price tracking of USDC.
/// 
/// We also have no answer for if TVL gets pulled and deployed capital can't be transmuted back to CDT.
/// Solution: If TVL gets pulled while its used, there will be higher APRs for users.

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub tokenfactory_contract: Option<Addr>,
    pub discounts_contract: String,
    pub cdp_contract: String,
    pub deposit_pair: AssetPair,
    pub composition_leeway: Decimal,
    pub cdt_target_ratio: Decimal, //probably set to 0%, which means no CDT needed.
    pub usage_fee: Option<Decimal>,
    /// Paired asset utilization threshold at which usage_fee activates (default 90%)
    pub usage_fee_utilization_threshold: Option<Decimal>,
    pub swap_history_cap: u32,
    pub volume_history_cap: u32,
    /// Optional per-address sliding window in seconds (default 8 hours)
    pub rate_limit_window_secs: Option<u64>,
    /// Optional percent of total deposit value allowed per address within window (default 5%)
    pub rate_limit_threshold: Option<Decimal>,
    /// Optional revenue distributor contract address
    pub revenue_distributor_addr: Option<String>,
    /// Optional revenue distribution ratios (Vec<LiqAsset>)
    pub revenue_distributions: Option<Vec<LiqAsset>>,
    /// Optional allowlist set at instantiate
    pub allowlist: Option<Vec<String>>, 
    /// Optional separate rate limit threshold for allowlisted addresses
    pub allowlist_rate_limit_threshold: Option<Decimal>,
    /// Optional global rate limit window in seconds for all non-whitelisted addresses (default 24 hours)
    pub global_rate_limit_window_secs: Option<u64>,
    /// Optional global rate limit threshold as percentage of total deposits for all non-whitelisted addresses (default 20%)
    pub global_rate_limit_threshold: Option<Decimal>,
    /// Maximum lock days allowed for vault tokens
    pub lock_ceiling: u64,
    /// Affiliate fee percentage (required, default 1%)
    pub affiliate_fee: Decimal,
    /// Whether to send swap fees to revenue distributor (true) or keep them in contract (false)
    pub send_swap_fee: Option<bool>,
    /// Percentage of swap fees to send to revenue distributor (0% = none, 100% = all). Default 20%
    pub revenue_distributor_fee_percentage: Option<Decimal>,
    /// Optional emissions voting contract address
    pub emissions_voting_contract: Option<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    UpdateConfig {
        owner: Option<String>,
        deposit_pair: Option<AssetPair>,
        composition_leeway: Option<Decimal>,
        cdt_target_ratio: Option<Decimal>,
        tokenfactory_contract: Option<Addr>,
        discounts_contract: Option<String>,
        cdp_contract: Option<String>,
        usage_fee: Option<Decimal>,
        /// Paired asset utilization threshold at which usage_fee activates
        usage_fee_utilization_threshold: Option<Decimal>,
        swap_history_cap: Option<u32>,
        volume_history_cap: Option<u32>,
        /// Optional per-address sliding window in seconds
        rate_limit_window_secs: Option<u64>,
        /// Optional percent of total deposit value allowed per address within window
        rate_limit_threshold: Option<Decimal>,
        /// Update allowlist entries (add/remove)
        allowlist: Option<Vec<crate::types::StringEntry>>, 
        /// Optional separate rate limit threshold for allowlisted addresses
        allowlist_rate_limit_threshold: Option<Decimal>,
        /// Optional global rate limit window in seconds for all non-whitelisted addresses
        global_rate_limit_window_secs: Option<u64>,
        /// Optional global rate limit threshold as percentage of total deposits for all non-whitelisted addresses
        global_rate_limit_threshold: Option<Decimal>,
        /// Optional revenue distributor contract address
        revenue_distributor_addr: Option<String>,
        /// Optional revenue distribution ratios (Vec<LiqAsset>)
        revenue_distributions: Option<Vec<crate::types::DistributionEntry>>,
        /// Maximum lock days allowed for vault tokens
        lock_ceiling: Option<u64>,
        /// Affiliate fee percentage (required)
        affiliate_fee: Decimal,
        /// Whether to send swap fees to revenue distributor (true) or keep them in contract (false)
        send_swap_fee: Option<bool>,
        /// Percentage of swap fees to send to revenue distributor (0% = none, 100% = all). Default 20%
        revenue_distributor_fee_percentage: Option<Decimal>,
        /// Optional emissions voting contract address
        emissions_voting_contract: Option<String>,
    },
    EnterVault {
        recipient: Option<String>,
        /// Optional lock days for vault tokens (must be <= lock_ceiling)
        lock_days: Option<u64>,
        /// Optional affiliate address to set when depositing
        affiliate_address: Option<String>,
    },
    DepositFee {},
    ExitVault {
        recipient: Option<String>,
        withdraw_as: Option<String>,
        /// Optional user address to exit for (only contract can use this)
        user: Option<String>,
        /// Optional deposit ID to withdraw from (if provided, withdraws only from this deposit)
        deposit_id: Option<Uint128>,
        /// Optional amount to withdraw (only used when deposit_id is provided)
        amount: Option<Uint128>,
    },
    Lock {
        /// Amount of deposits to lock (locks across deposits starting from oldest first)
        amount: Uint128,
        /// Number of days to lock the deposits (must be <= lock_ceiling)
        lock_days: u64,
    },
    Transmute {
        recipient: Option<String>,
    },
    UpdateVolumeWindow {},
    /// Assures that for deposits & withdrawals the conversion rate is static
    /// Only callable by the contract
    RateAssurance {},
    /// Set affiliate for a user
    SetAffiliate {
        user: String,
        affiliate_address: String,
        label: Option<String>,
    },
    /// Add current vault token conversion rate to history
    AddToRateHistory {},
    /// Repay user debt (for deployable venue interface)
    RepayUserDebt {
        /// User info
        user_info: crate::types::UserInfo,
        /// Repayment amount
        repayment: Uint128,
    },
    /// Distribute retention emissions (creates daily event)
    DistributeRetentionEmissions {},
    /// Claim accumulated retention emissions rewards
    ClaimRetentionEmissions {},
    /// Transfer ownership of a deposit from one user to another
    TransferDepositOwnership {
        user: String,
        deposit_id: Uint128,
        new_owner: String,
    },
}

#[cw_serde]
pub enum QueryMsg {
    Config {},
    VaultInfo {},
    TransmuteHistory { start_after: Option<u64>, limit: Option<u32> },
    VolumeHistory { start_after: Option<u64>, limit: Option<u32> },
    VolumeWindow {},
    /// Current deployed paired_asset amount from allowlisted deployment venues
    DeployedPairedAsset {},
    /// Effective target ratio for CDT after considering deployed paired asset
    EffectiveTarget {},
    /// Rate limit status for multiple addresses with optional pagination
    RateLimitMany { addresses: Option<Vec<String>>, start_after: Option<u64>, limit: Option<u32> },
    /// Current global rate limit status for all non-whitelisted addresses
    GlobalRateLimit {},
    /// Get affiliates for a user
    GetAffiliates { user: String },
    /// Get vault token conversion rate history
    RateHistory { start_after: Option<u64>, limit: Option<u32> },
    /// Get user deposits with timestamps and lock information
    UserDeposits { user: String },
    /// Get user's retrievable CDT (for deployable venue interface)
    RetrievableCDT { user: String },
    /// Get user's retention emissions claimable amount
    UserRetentionEmissions { user: String },
    /// Get global retention weight
    GlobalRetentionWeight {},
    /// Get emissions configuration
    EmissionsConfig {},
    /// Get current deposit ID for a user (next ID that will be assigned)
    CurrentDepositId { user: String },
    /// Get deposit by ID for a user
    DepositById { user: String, deposit_id: Uint128 },
}

#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub tokenfactory_contract: Option<Addr>,
    // System discounts contract
    pub discounts_contract: String,
    pub cdp_contract: String,
    pub deposit_pair: AssetPair,
    pub composition_leeway: Decimal,
    /// Target ratio for CDT of total deposits
    /// If the target ratio is 0%, then we will not require any CDT on deposits...
    /// ...unless the deployed paired asset is greater than 0.
    pub cdt_target_ratio: Decimal,
    /// Usage fee applied only to CDT→paired_asset swaps when paired_asset utilization
    /// exceeds the threshold. We're adding friction once LP inventory gets low to try
    /// and retain optional exit for LPs. Slows the velocity of liquidity consumption
    /// & compensates LPs for being last in line.
    /// If set to 100% it blocks non-CDP & non-deployable venue CDT→paired_asset usage entirely.
    pub usage_fee: Decimal,
    /// Paired asset utilization threshold (0-1) at which usage_fee activates. Default 90%.
    /// Utilization = 1 - (paired_asset_balance / total_deposit_value).
    /// Fee only kicks in when paired_asset is scarce (utilization >= this value).
    pub usage_fee_utilization_threshold: Decimal,
    pub swap_history_cap: u32,
    pub volume_history_cap: u32,
    /// Per-address sliding-window rate limit configuration
    pub rate_limit_window_secs: u64,
    pub rate_limit_threshold: Decimal,
    /// Allowlist of addresses (strings) with a separate threshold
    pub allowlist: Vec<String>,
    pub allowlist_rate_limit_threshold: Decimal,
    /// Global rate limit window in seconds for all non-whitelisted addresses
    pub global_rate_limit_window_secs: u64,
    /// Global rate limit threshold as percentage of total deposits for all non-whitelisted addresses
    pub global_rate_limit_threshold: Decimal,
    /// Revenue distributor contract address (optional)
    pub revenue_distributor_addr: Option<Addr>,
    /// Revenue distribution ratios (Vec<LiqAsset>)
    pub revenue_distributions: Vec<LiqAsset>,
    /// Maximum lock days allowed for vault tokens
    pub lock_ceiling: u64,
    /// Optional affiliate fee percentage (default 1%)
    pub affiliate_fee: Decimal,
    /// Whether to send swap fees to revenue distributor (true) or keep them in contract (false)
    pub send_swap_fee: bool,
    /// Percentage of swap fees to send to revenue distributor (0% = none, 100% = all). Default 20%
    pub revenue_distributor_fee_percentage: Decimal,
    /// Optional emissions voting contract address
    pub emissions_voting_contract: Option<Addr>,
}


#[cw_serde]
pub struct AssetPair {
    pub cdt: String,
    pub paired_asset: String,
}

#[cw_serde]
pub struct SwapRecord {
    pub offered_asset: String,
    pub offered_amount: Uint128,
    pub received_asset: String,
    pub received_amount: Uint128,
    pub block_time: Timestamp,
}

#[cw_serde]
pub struct VolumeWindow {
    pub cdt_swapped: Uint128,
    pub cdt_received: Uint128,
    pub paired_asset_swapped: Uint128,
    pub paired_asset_received: Uint128,
    pub block_time: Timestamp,
    pub cumulative_volume: Uint128,
}

#[cw_serde]
pub struct RateHistoryEntry {
    pub conversion_rate: Uint128,
    pub timestamp: Timestamp,
}


#[cw_serde]
pub struct VaultInfoResponse {
    pub total_deposit_value: Uint128,
    pub deposit_total: Uint128,
    pub cdt_balance: Uint128,
    pub paired_asset_balance: Uint128,
}

#[cw_serde]
pub struct VolumeHistoryResponse {
    pub records: Vec<VolumeWindow>,
    pub total: u64,
    pub next_start_after: Option<u64>,
}

#[cw_serde]
pub struct TransmuteHistoryResponse {
    pub records: Vec<SwapRecord>,
    pub total: u64,
    pub next_start_after: Option<u64>,
}

#[cw_serde]
pub struct RateHistoryResponse {
    pub records: Vec<RateHistoryEntry>,
    pub total: u64,
    pub next_start_after: Option<u64>,
}

#[cw_serde]
pub struct VolumeWindowResponse {
    pub window: VolumeWindow,
}

#[cw_serde]
pub struct RateLimitStatus {
    /// Net signed flow in base units within window (positive means USDC->CDT net)
    pub net_flow_base: i128,
    /// Absolute threshold amount in base units derived from total deposits and active threshold
    pub threshold_base: Uint128,
    /// Whether address is allowlisted
    pub is_allowlisted: bool,
    /// Remaining headroom before blocking (0 if exceeded); base units
    pub remaining_base: Uint128,
    /// Number of stored flow entries currently within the window for this address
    pub entries_count: u64,
}

#[cw_serde]
pub struct RateLimitStatusResponse {
    pub address: String,
    pub status: RateLimitStatus,
}

#[cw_serde]
pub struct RateLimitManyResponse {
    pub records: Vec<RateLimitStatusResponse>,
    pub total: u64,
    pub next_start_after: Option<u64>,
}

#[cw_serde]
pub struct DeployedPairedAssetResponse {
    pub amount: Uint128,
}

#[cw_serde]
pub struct EffectiveTargetResponse {
    pub target: Decimal,
}

#[cw_serde]
pub struct GlobalRateLimitResponse {
    pub net_flow_base: i128,
    pub threshold_base: Uint128,
    pub remaining_base: Uint128,
    pub entries_count: u64,
}

#[cw_serde]
pub struct UserDeposit {
    /// Deposit ID (unique identifier for this deposit)
    pub deposit_id: Uint128,
    /// Deposit amount (1:1 tracking, CDT + paired asset)
    pub amount: Uint128,
    /// Timestamp when deposit was made
    pub deposit_time: u64,
    /// Lock information (similar to ltv_disco structure)
    pub locked: Option<crate::types::Locked>,
    /// Timestamp when deposit/lock was created (for discount curve calculation)
    pub start_time: u64,
}

#[cw_serde]
pub struct UserDepositsResponse {
    pub deposits: Vec<UserDeposit>,
}

#[cw_serde]
pub struct UserRetentionEmissionsResponse {
    pub claimable: Uint128,
}

#[cw_serde]
pub struct GlobalRetentionWeightResponse {
    pub weight: Uint128,
}

#[cw_serde]
pub struct EmissionsConfigResponse {
    pub emissions_voting_contract: Option<Addr>,
}

#[cw_serde]
pub struct CurrentDepositIdResponse {
    pub deposit_id: Uint128,
}

#[cw_serde]
pub struct DepositByIdResponse {
    pub deposit: UserDeposit,
}
