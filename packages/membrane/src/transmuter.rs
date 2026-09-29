use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Timestamp, Uint128};
use crate::types::LiqAsset;

// ================= Tranche Types =================

#[cw_serde]
pub struct TrancheSplit {
    /// CDP collateral denom this split covers bad debt for
    pub asset: String,
    /// true = junior (higher yield, first-loss in bad debt waterfall)
    pub junior: bool,
    /// Virtual vault token amount tracking yield/losses via conversion rate
    pub vault_token_amount: Uint128,
}

#[cw_serde]
pub struct TrancheState {
    /// Total virtual vault token supply for this tranche
    pub vault_token_supply: Uint128,
    /// Total base value assigned (moves with yield/losses)
    pub total_staked: Uint128,
}

#[cw_serde]
pub struct TrancheRateAssurance {
    pub pre_vault_token_supply: Uint128,
    pub pre_total_staked: Uint128,
}

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
/// LPs accept this depeg risk in exchange for yield. The allowlist controls which addresses
/// are exempt from usage fees (CDP system contracts, deployment venues).
///
/// We also have no answer for if TVL gets pulled and deployed capital can't be transmuted back to CDT.
/// Solution: If TVL gets pulled while its used, there will be higher APRs for users.

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub discounts_contract: String,
    pub cdp_contract: String,
    /// Collateral contract address for validating tranche asset denoms
    pub collateral_contract: String,
    pub deposit_pair: AssetPair,
    pub composition_leeway: Decimal,
    pub cdt_target_ratio: Decimal,
    pub usage_fee: Option<Decimal>,
    /// Paired asset utilization threshold at which usage_fee activates (default 80%)
    pub usage_fee_utilization_threshold: Option<Decimal>,
    pub swap_history_cap: u32,
    pub volume_history_cap: u32,
    /// Optional revenue distributor contract address
    pub revenue_distributor_addr: Option<String>,
    /// Optional revenue distribution ratios (Vec<LiqAsset>)
    pub revenue_distributions: Option<Vec<LiqAsset>>,
    /// Optional allowlist set at instantiate
    pub allowlist: Option<Vec<String>>,
    /// Affiliate fee percentage (required, default 1%)
    pub affiliate_fee: Decimal,
    /// Whether to send swap fees to revenue distributor (true) or keep them in contract (false)
    pub send_swap_fee: Option<bool>,
    /// Percentage of swap fees to send to revenue distributor (0% = none, 100% = all). Default 20%
    pub revenue_distributor_fee_percentage: Option<Decimal>,
    /// Optional acquisition (acquisition) contract address for utilization notifications
    pub acquisition_contract: Option<String>,
    /// Optional points system contract address for awarding points
    pub points_system_contract: Option<String>,
    /// Multiplier for rate calculations
    pub retention_multiplier: Option<Decimal>,
}

#[cw_serde]
pub enum ExecuteMsg {
    UpdateConfig {
        owner: Option<String>,
        deposit_pair: Option<AssetPair>,
        composition_leeway: Option<Decimal>,
        cdt_target_ratio: Option<Decimal>,
        discounts_contract: Option<String>,
        cdp_contract: Option<String>,
        collateral_contract: Option<String>,
        usage_fee: Option<Decimal>,
        /// Paired asset utilization threshold at which usage_fee activates
        usage_fee_utilization_threshold: Option<Decimal>,
        swap_history_cap: Option<u32>,
        volume_history_cap: Option<u32>,
        /// Update allowlist entries (add/remove)
        allowlist: Option<Vec<crate::types::StringEntry>>,
        /// Optional revenue distributor contract address
        revenue_distributor_addr: Option<String>,
        /// Optional revenue distribution ratios (Vec<LiqAsset>)
        revenue_distributions: Option<Vec<crate::types::DistributionEntry>>,
        /// Affiliate fee percentage (required)
        affiliate_fee: Decimal,
        /// Whether to send swap fees to revenue distributor (true) or keep them in contract (false)
        send_swap_fee: Option<bool>,
        /// Percentage of swap fees to send to revenue distributor (0% = none, 100% = all). Default 20%
        revenue_distributor_fee_percentage: Option<Decimal>,
        /// Optional acquisition (acquisition) contract address for utilization notifications
        acquisition_contract: Option<String>,
        /// Optional points system contract address for awarding points
        points_system_contract: Option<String>,
        /// Multiplier applied to acquisition rate for retention emissions
        retention_multiplier: Option<Decimal>,
    },
    EnterVault {
        recipient: Option<String>,
        /// Optional affiliate address to set when depositing
        affiliate_address: Option<String>,
        /// Optional label for affiliate campaign tracking
        affiliate_label: Option<String>,
        /// Required: tranche allocations (collateral_denom, junior).
        /// No duplicate (denom, junior) pairs. Deposit split equally across entries.
        tranches: Vec<(String, bool)>,
    },
    ExitVault {
        recipient: Option<String>,
        withdraw_as: Option<String>,
        /// Optional user address to exit for (only contract can use this)
        user: Option<String>,
        /// Optional amount to withdraw (only used for acquisition path)
        amount: Option<Uint128>,
        /// Which tranches to withdraw from. No duplicate (denom, junior) pairs.
        /// If None, proportional withdrawal from all user tranches.
        tranche_withdrawals: Option<Vec<(String, bool, Uint128)>>,
    },
    Transmute {
        recipient: Option<String>,
    },
    UpdateVolumeWindow {},
    /// Assures that for deposits & withdrawals the conversion rate is static per tranche
    /// Only callable by the contract
    RateAssurance {},
    /// Set affiliate for a user
    SetAffiliate {
        user: String,
        affiliate_address: String,
        label: Option<String>,
    },
    /// Add current conversion rate to history
    AddToRateHistory {},
    /// Add yield revenue to a specific tranche (called by revenue distributor)
    AddRevenue {
        collateral_denom: String,
        junior: bool,
    },
    /// Distribute accumulated swap fees proportionally across all tranches (permissionless keeper)
    DistributeSwapFees {},
    /// Assign bad debt to a collateral's tranches (junior-first waterfall).
    /// Sends CDT to CDP.FulfillBadDebt after assignment.
    AssignBadDebt {
        collateral_denom: String,
        amount: Uint128,
    },
    /// Repay user debt (for deployable venue interface)
    RepayUserDebt {
        /// User info
        user_info: crate::types::UserInfo,
        /// Repayment amount
        repayment: Uint128,
    },
    /// Transfer ownership of a deposit from one user to another
    TransferDepositOwnership {
        user: String,
        amount: Uint128,
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
    /// Get affiliates for a user
    GetAffiliates { user: String },
    /// Get conversion rate history
    RateHistory { start_after: Option<u64>, limit: Option<u32> },
    /// Get user's retrievable CDT (for deployable venue interface)
    RetrievableCDT { user: String },
    /// Get total deposits from the acquisition contract (tracked separately)
    AcquisitionDepositTotal {},
    /// Get user's tranche positions
    UserTranches { user: String },
    /// Get global state for a specific tranche
    TrancheInfo { collateral_denom: String, junior: bool },
    /// Get all tranche states
    AllTranches {},
    /// Get conversion rate for a specific tranche
    TrancheConversionRate { collateral_denom: String, junior: bool },
    /// Max bad debt assignable for a collateral (junior.total_staked + senior.total_staked)
    MaxAssignableBadDebt { collateral_denom: String },
}

#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub discounts_contract: String,
    pub cdp_contract: String,
    /// Collateral contract address for validating tranche asset denoms
    pub collateral_contract: String,
    pub deposit_pair: AssetPair,
    pub composition_leeway: Decimal,
    /// Target ratio for CDT of total deposits
    pub cdt_target_ratio: Decimal,
    /// Usage fee applied to swaps for non-allowlisted senders.
    pub usage_fee: Decimal,
    /// Paired asset utilization threshold (0-1) at which usage_fee activates.
    pub usage_fee_utilization_threshold: Decimal,
    pub swap_history_cap: u32,
    pub volume_history_cap: u32,
    /// Allowlist of addresses exempt from usage fees
    pub allowlist: Vec<String>,
    /// Revenue distributor contract address (optional)
    pub revenue_distributor_addr: Option<Addr>,
    /// Revenue distribution ratios (Vec<LiqAsset>)
    pub revenue_distributions: Vec<LiqAsset>,
    /// Optional affiliate fee percentage (default 1%)
    pub affiliate_fee: Decimal,
    /// Whether to send swap fees to revenue distributor (true) or keep them in contract (false)
    pub send_swap_fee: bool,
    /// Percentage of swap fees to send to revenue distributor (0% = none, 100% = all). Default 20%
    pub revenue_distributor_fee_percentage: Decimal,
    /// Optional acquisition (acquisition) contract address for utilization notifications
    pub acquisition_contract: Option<Addr>,
    /// Optional points system contract address for awarding points
    pub points_system_contract: Option<Addr>,
    /// Multiplier for rate calculations
    pub retention_multiplier: Decimal,
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
    /// Sum of all tranche total_staked
    pub total_tranche_staked: Uint128,
    pub cdt_balance: Uint128,
    pub paired_asset_balance: Uint128,
}

#[cw_serde]
pub struct UserTranchesResponse {
    pub tranches: Vec<TrancheSplit>,
}

#[cw_serde]
pub struct TrancheInfoResponse {
    pub collateral_denom: String,
    pub junior: bool,
    pub state: TrancheState,
}

#[cw_serde]
pub struct AllTranchesResponse {
    pub tranches: Vec<TrancheInfoResponse>,
}

#[cw_serde]
pub struct TrancheConversionRateResponse {
    pub rate: Decimal,
}

#[cw_serde]
pub struct MaxAssignableBadDebtResponse {
    pub max_amount: Uint128,
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
pub struct DeployedPairedAssetResponse {
    pub amount: Uint128,
}

#[cw_serde]
pub struct EffectiveTargetResponse {
    pub target: Decimal,
}

#[cw_serde]
pub struct AcquisitionDepositTotalResponse {
    pub total: Uint128,
}
