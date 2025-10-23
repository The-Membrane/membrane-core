use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Timestamp, Uint128};


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
    pub revenue_contract: String,
    pub cdp_contract: String,
    pub vault_subdenom: String,
    pub deposit_pair: AssetPair,
    pub composition_leeway: Decimal,
    pub asset_a_to_b_rate: Decimal,
    pub target_ratio: Decimal, //probably set to 0%, which means no CDT needed.
    pub usage_fee: Option<Decimal>,
    pub swap_history_cap: u32,
    pub volume_history_cap: u32,
    /// Optional per-address sliding window in seconds (default 8 hours)
    pub rate_limit_window_secs: Option<u64>,
    /// Optional percent of total deposit value allowed per address within window (default 5%)
    pub rate_limit_threshold: Option<Decimal>,
    /// Optional allowlist set at instantiate
    pub allowlist: Option<Vec<String>>, 
    /// Optional separate rate limit threshold for allowlisted addresses
    pub allowlist_rate_limit_threshold: Option<Decimal>,
    /// Optional global rate limit window in seconds for all non-whitelisted addresses (default 24 hours)
    pub global_rate_limit_window_secs: Option<u64>,
    /// Optional global rate limit threshold as percentage of total deposits for all non-whitelisted addresses (default 20%)
    pub global_rate_limit_threshold: Option<Decimal>,
}

#[cw_serde]
pub enum ExecuteMsg {
    UpdateConfig {
        owner: Option<String>,
        deposit_pair: Option<AssetPair>,
        composition_leeway: Option<Decimal>,
        asset_a_to_b_rate: Option<Decimal>,
        target_ratio: Option<Decimal>,
        tokenfactory_contract: Option<Addr>,
        cdp_contract: Option<String>,
        revenue_contract: Option<String>,
        usage_fee: Option<Decimal>,
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
    },
    EnterVault {
        recipient: Option<String>,
    },
    DepositFee {},
    ExitVault {
        recipient: Option<String>,
        withdraw_as: Option<String>,
    },
    Transmute {
        recipient: Option<String>,
    },
    UpdateVolumeWindow {},
    /// Assures that for deposits & withdrawals the conversion rate is static
    /// Only callable by the contract
    RateAssurance {},
}

#[cw_serde]
pub enum QueryMsg {
    Config {},
    VaultInfo {},
    VaultTokenUnderlying { vault_token_amount: Uint128 },
    DepositTokenConversion { deposit_token_amount: Uint128 },
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
}

#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub tokenfactory_contract: Option<Addr>,
    pub revenue_contract: String,
    pub cdp_contract: String,
    pub vault_token: String,
    pub deposit_pair: AssetPair,
    pub composition_leeway: Decimal,
    pub asset_a_to_b_rate: Decimal,
    pub target_ratio: Decimal,
    /// Usage fee for any usage that isn't from the CDP or a deployable venue.
    /// This fee is set bc we don't want this to be used as an LP/arbitrage tool.
    /// -- Issue with this is that without arb usage it won't be able to sustain itself.
    /// -- But if we allow arbs, then CDT will track USDC's price. Is this bad?
    /// The fee creates a price floor though so if its set to 100% we'll just block any non-deployable venue usage.
    pub usage_fee: Decimal,
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
}


#[cw_serde]
pub struct VaultInfoResponse {
    pub total_deposit_value: Uint128,
    pub vault_token_supply: Uint128,
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
