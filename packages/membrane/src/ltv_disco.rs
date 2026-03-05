use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Uint128};

use crate::types::DepositDenom;

/// Instantiate message
#[cw_serde]
pub struct InstantiateMsg {
    /// Contract owner
    pub owner: Option<String>,
    /// CDP contract address
    pub cdp_contract: String,
    /// Deposit denomination (CDT or Transmuter vault token)
    pub deposit_denom: DepositDenom,
    /// CDT token denomination for revenue distribution and bad debt fulfillment.
    /// CRITICAL: This MUST be the CDT token.
    pub cdt_denom: String,
    /// Minimum deposit amount
    pub minimum_deposit: Uint128,
    /// Unstaking period in seconds (default 172800 = 2 days)
    pub unstaking_period: Option<u64>,
    /// Oracle contract for querying asset prices
    pub oracle_contract: String,
    /// Chain proxy contract for executing swaps
    pub chain_proxy_contract: String,
    /// Emissions voting contract address
    pub emissions_voting_contract: Option<String>,
    /// Optional affiliate fee percentage (default 1%)
    pub affiliate_fee: Option<Decimal>,
    /// Optional maximum management fee percentage (default 5%)
    pub max_management_fee: Option<Decimal>,
    /// Points system contract address (optional)
    pub points_system_contract: Option<String>,
    /// Revenue distributor contract address (optional)
    pub revenue_distributor: Option<String>,
    /// Auction contract address (optional)
    pub auction_contract: Option<String>,
    /// MBRN denom (optional)
    pub mbrn_denom: Option<String>,
}

/// Execute messages
#[cw_serde]
pub enum ExecuteMsg {
    /// Create a new asset queue with LTV-designated slots.
    /// Slots are created at 1% intervals from max_ltv down to min_ltv.
    CreateQueue {
        asset: String,
        /// Minimum LTV percentage (e.g., Decimal::percent(50) for 50%)
        min_ltv: Decimal,
        /// Maximum LTV percentage (e.g., Decimal::percent(90) for 90%)
        max_ltv: Decimal,
    },
    /// Update an existing asset queue's LTV range (admin only).
    /// Expansion creates new slots; contraction deactivates out-of-range slots.
    UpdateQueue {
        asset: String,
        /// New minimum LTV (optional)
        min_ltv: Option<Decimal>,
        /// New maximum LTV (optional)
        max_ltv: Option<Decimal>,
    },
    /// Submit a backing deposit into a slot
    SubmitDeposit {
        deposit_input: BackingDepositInput,
        deposit_owner: Option<String>,
        /// Optional deposit_id to top up an existing deposit (auto-claims first)
        deposit_id: Option<Uint128>,
        /// Optional manager address (can move deposits but cannot withdraw)
        manager: Option<String>,
        /// Optional affiliate address
        affiliate_address: Option<String>,
        /// Optional revenue destination address
        revenue_destination: Option<String>,
    },
    /// Request unstaking (starts cooldown, deposit keeps earning)
    RequestUnstake {
        asset: String,
        /// LTV percentage identifying the slot (e.g., 80 for 80%)
        slot: u8,
        deposit_id: Uint128,
        /// Vault tokens to unstake (None = full withdrawal)
        amount: Option<Uint128>,
    },
    /// Complete unstaking after cooldown period has passed
    CompleteUnstake {
        asset: String,
        /// LTV percentage identifying the slot (e.g., 80 for 80%)
        slot: u8,
        deposit_id: Uint128,
    },
    /// Cancel a pending unstake request
    CancelUnstake {
        asset: String,
        /// LTV percentage identifying the slot (e.g., 80 for 80%)
        slot: u8,
        deposit_id: Uint128,
    },
    /// Move a deposit between slots (auto-claims first)
    MoveDeposit {
        asset: String,
        /// LTV percentage identifying the source slot (e.g., 80 for 80%)
        slot: u8,
        deposit_id: Uint128,
        destination: BackingDepositInput,
        amount: Option<Uint128>,
        /// User address (optional, defaults to sender; if provided, sender must be manager)
        user: Option<String>,
    },
    /// Update deposit settings (owner, manager, revenue_destination)
    UpdateDeposit {
        asset: String,
        /// LTV percentage identifying the slot (e.g., 80 for 80%)
        slot: u8,
        deposit_id: Uint128,
        deposit_owner: Option<String>,
        manager: Option<String>,
        revenue_destination: Option<String>,
    },
    /// Add bad debt to an asset queue (CDP contract only)
    /// Slashes deposits from slot 1 (riskiest) first
    AddBadDebt {
        asset: String,
        amount: Uint128,
    },
    /// Add CDT revenue to an asset queue (distributes immediately)
    AddRevenue {
        asset: String,
    },
    /// Claim accumulated revenue for a user
    ClaimRevenueForUser {
        user: String,
        asset: String,
        limit: Option<u32>,
        compound_action: Option<CompoundAction>,
    },
    /// Update contract configuration
    UpdateConfig {
        owner: Option<String>,
        cdp_contract: Option<String>,
        deposit_denom: Option<DepositDenom>,
        cdt_denom: Option<String>,
        minimum_deposit: Option<Uint128>,
        unstaking_period: Option<u64>,
        oracle_contract: Option<String>,
        chain_proxy_contract: Option<String>,
        emissions_voting_contract: Option<String>,
        affiliate_fee: Option<Decimal>,
        max_management_fee: Option<Decimal>,
        points_system_contract: Option<String>,
        revenue_distributor: Option<String>,
        auction_contract: Option<String>,
        mbrn_denom: Option<String>,
    },
    /// Rate assurance check per slot (only callable by the contract)
    RateAssurance {
        asset: String,
        /// LTV percentage identifying the slot (e.g., 80 for 80%)
        slot: u8,
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
    AddDepositTokenRevenue {
        per_asset_distribution: Vec<crate::types::Asset>,
    },
    /// Send MBRN for bad debt auction sale (callable by auction contract only).
    /// Auction pulls MBRN from disco to send to buyers.
    SendMBRNForSale {
        amount: Uint128,
        recipient: String,
    },
}

/// Query messages
#[cw_serde]
pub enum QueryMsg {
    /// Get contract configuration
    Config {},
    /// Get asset queue(s)
    GetAssetQueue {
        assets: Vec<String>,
        limit: Option<u32>,
        start_after: Option<String>,
    },
    /// Get a specific backing deposit
    GetBackingDeposit {
        user: String,
        asset: String,
        /// LTV percentage identifying the slot (e.g., 80 for 80%)
        slot: u8,
        deposit_id: Uint128,
    },
    /// Get all backing deposits for a user on an asset
    GetBackingDepositsByUser {
        user: String,
        asset: String,
        limit: Option<u32>,
        start_after: Option<Uint128>,
    },
    /// Check if the contract can handle bad debt for an asset
    CanHandleBadDebt {
        asset: String,
        amount: Uint128,
    },
    /// Get cumulative revenue (optionally per slot)
    GetCumulativeRevenue {
        asset: String,
        slot: Option<u8>,
    },
    /// Get pending claims for a user
    PendingClaims { user: String, asset: String },
    /// Get user lifetime revenue summary
    GetUserLifetimeRevenue { user: String, asset: String },
    /// Get revenue events for a slot
    GetRevenueEvents { asset: String, slot: u8 },
    /// Get all assets that have queues
    GetAssets {},
    /// Get daily TVL tracker history
    GetDailyTVL {},
    /// Get daily deposit tracker history for an asset
    GetDailyDeposits { asset: String },
    /// Get user's total deposits
    UserTotalDeposits { user: String },
    /// Get all user deposits across all assets
    GetAllUserDeposits { user: String },
    /// Get affiliates for a user
    GetAffiliates { user: String },
    /// Convert vault tokens to deposit tokens for a slot
    VaultTokenConversion {
        asset: String,
        slot: u8,
        vault_tokens: Uint128,
    },
    /// Convert deposit tokens to vault tokens for a slot
    DepositTokenConversion {
        asset: String,
        slot: u8,
        deposit_tokens: Uint128,
    },
    /// Get managed deposit keys for a manager (paginated)
    GetManagedDepositKeys {
        manager: String,
        limit: Option<u32>,
        start_after: Option<String>,
    },
    /// Get total insurance (deposit totals)
    GetTotalInsurance {},
    /// Get manager fee
    GetManagerFee { manager: String },
    /// Get pending unstake requests for a user
    GetUnstakeRequests { user: String, asset: String },
    /// Get computed revenue weights for all active slots
    GetSlotWeights { asset: String },
    /// Get weighted average max LTV for assets.
    /// Queried by the Collateral contract during dynamic LTV updates.
    GetAverageLTVs { assets: Vec<String> },
}

// ============== Responses ==============

/// Response for asset queue query
#[cw_serde]
pub struct AssetQueueResponse {
    pub queues: Vec<(String, AssetQueue)>,
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
    WithOracle {
        total_insurance: Uint128,
    },
    WithoutOracle {
        deposit_totals: Vec<(String, Uint128)>,
    },
}

/// Response for manager fee query
#[cw_serde]
pub struct ManagerFeeResponse {
    pub manager: String,
    pub fee: Decimal,
}

/// Pending claims aggregate response
#[cw_serde]
pub struct PendingClaimsResponse {
    pub user: String,
    pub asset: String,
    pub claims: Vec<DepositPendingClaim>,
}

#[cw_serde]
pub struct DepositPendingClaim {
    pub slot: u8,
    pub deposit_id: Uint128,
    pub pending_amount: Uint128,
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

/// Response for daily deposit tracker query
#[cw_serde]
pub struct DailyDepositResponse {
    pub entries: Vec<DepositEntry>,
}

/// Response for user total deposits query
#[cw_serde]
pub struct UserTotalDepositsResponse {
    pub total_deposits: Uint128,
}

/// Response for all user deposits query
#[cw_serde]
pub struct AllUserDepositsResponse {
    pub deposits: Vec<UserDepositInfo>,
}

/// Response for unstake requests query
#[cw_serde]
pub struct UnstakeRequestsResponse {
    pub requests: Vec<UnstakeRequest>,
}

/// Response for slot weights query
#[cw_serde]
pub struct SlotWeightsResponse {
    pub weights: Vec<(u8, Decimal)>,
}

/// Response for GetAverageLTVs query.
/// Returns the weighted average max_LTV across Disco deposits for the queried assets.
/// Borrow LTV is derived by the Collateral contract as max_LTV - ltv_borrow_distance.
#[cw_serde]
pub struct AverageLTVsResponse {
    /// Weighted average max_LTV from Disco deposits
    pub average_max_ltv: Decimal,
}

/// Migrate message
#[cw_serde]
pub struct MigrateMsg {}

// ============== Core Types ==============

/// Compound action for revenue claims
#[cw_serde]
pub struct CompoundAction {
    /// Whether to compound this claim
    pub compound_now: bool,
    /// Whether to set compound_claims as ongoing intent on deposits
    pub set_ongoing: bool,
    /// Optional recipient address for the claim
    pub recipient_address: Option<String>,
}

/// Configuration for the Disco contract
#[cw_serde]
pub struct Config {
    /// Contract owner
    pub owner: Addr,
    /// CDP contract address
    pub cdp_contract: Addr,
    /// Deposit denomination (CDT or Transmuter vault token)
    pub deposit_denom: DepositDenom,
    /// CDT token denomination for revenue distribution and bad debt fulfillment
    pub cdt_denom: String,
    /// Minimum deposit amount
    pub minimum_deposit: Uint128,
    /// Unstaking period in seconds (default 172800 = 2 days)
    pub unstaking_period: u64,
    /// Oracle contract for querying asset prices
    pub oracle_contract: Addr,
    /// Chain proxy contract for executing swaps
    pub chain_proxy_contract: Addr,
    /// Emissions Voting contract address
    pub emissions_voting_contract: Option<Addr>,
    /// Affiliate fee percentage (default 1%)
    pub affiliate_fee: Decimal,
    /// Maximum management fee percentage (default 5%)
    pub max_management_fee: Decimal,
    /// Points system contract address
    pub points_system_contract: Option<Addr>,
    /// Revenue distributor contract address
    pub revenue_distributor: Option<Addr>,
    /// Auction contract address
    pub auction_contract: Option<Addr>,
    /// MBRN denom
    pub mbrn_denom: Option<String>,
}

/// A single LTV-designated slot in the risk tranche system.
/// Each slot represents a specific max_LTV percentage.
/// Higher LTV = riskier (first to absorb bad debt, earns most revenue).
#[cw_serde]
pub struct Slot {
    /// The max LTV this slot represents (e.g., Decimal::percent(80) = 0.80 for 80%)
    pub max_ltv: Decimal,
    /// Total deposit tokens in this slot
    pub total_deposit_tokens: Uint128,
    /// Total vault tokens in this slot (for share accounting)
    pub total_vault_tokens: Uint128,
    /// Cumulative bad debt absorbed by this slot
    pub bad_debt: Uint128,
}

/// Asset queue containing LTV-designated slots.
/// Slots are sorted descending by max_ltv (highest LTV / riskiest first).
/// Slot count is determined by the per-asset min_ltv/max_ltv range at 1% intervals.
#[cw_serde]
pub struct AssetQueue {
    /// Slots sorted descending by max_ltv (highest LTV = riskiest first)
    pub slots: Vec<Slot>,
    /// Auto-incrementing deposit ID counter
    pub current_deposit_id: Uint128,
    /// Current active minimum LTV for this asset
    pub min_ltv: Decimal,
    /// Current active maximum LTV for this asset
    pub max_ltv: Decimal,
}

/// Pending unstake request
#[cw_serde]
pub struct UnstakeRequest {
    /// User address
    pub user: Addr,
    /// Asset
    pub asset: String,
    /// LTV percentage identifying the slot (e.g., 80 for 80%)
    pub slot: u8,
    /// Deposit ID
    pub deposit_id: Uint128,
    /// Vault tokens to unstake
    pub vault_tokens: Uint128,
    /// Timestamp when unstake was requested
    pub request_time: u64,
    /// Timestamp when unstake can be completed
    pub unlock_time: u64,
}

/// Individual backing deposit within a slot
#[cw_serde]
pub struct BackingDeposit {
    /// User address (deposit owner)
    pub user: Addr,
    /// Deposit amount in vault tokens
    pub vault_tokens: Uint128,
    /// Last timestamp the user claimed revenue
    pub last_claimed: u64,
    /// Timestamp when deposit was created
    pub start_time: u64,
    /// Timestamp when deposit was made (for tracking)
    pub deposit_time: Option<u64>,
    /// Auto-compound claimed revenue
    pub compound_claims: bool,
    /// Optional manager address (can move deposits but cannot withdraw)
    pub manager: Option<Addr>,
    /// Address that made the deposit (for deposits on behalf of others)
    pub depositor: Option<Addr>,
    /// Whether withdrawals are enabled (toggled by depositor)
    pub withdrawals_enabled: bool,
    /// Optional revenue destination address
    pub revenue_destination: Option<Addr>,
}

/// Input for creating/identifying a backing deposit
#[cw_serde]
pub struct BackingDepositInput {
    /// Asset to deposit for
    pub asset: String,
    /// LTV percentage identifying the slot (e.g., 80 for 80%)
    pub slot: u8,
}

/// Revenue tracking entry with timestamp
#[cw_serde]
pub struct RevenueTrackingEntry {
    pub timestamp: u64,
    pub total_revenue: Uint128,
}

/// Revenue event for a slot (one per AddRevenue call)
#[cw_serde]
pub struct RevenueEvent {
    /// Timestamp when this event was created
    pub timestamp: u64,
    /// Revenue per vault token for this event
    pub amount_per_vt: Decimal,
    /// Remaining total to be claimed (safety net against draining)
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

/// Daily deposit tracking entry per asset
#[cw_serde]
pub struct DepositEntry {
    pub timestamp: u64,
    pub deposit_tokens: Uint128,
}

/// User deposit information with identifying details
#[cw_serde]
pub struct UserDepositInfo {
    /// Asset identifier
    pub asset: String,
    /// LTV percentage identifying the slot (e.g., 80 for 80%)
    pub slot: u8,
    /// Deposit ID
    pub deposit_id: Uint128,
    /// The backing deposit
    pub deposit: BackingDeposit,
    /// Deposit tokens (converted from vault tokens)
    pub deposit_tokens: Uint128,
}
