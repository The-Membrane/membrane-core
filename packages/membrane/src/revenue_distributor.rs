use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Uint128, Addr};
use crate::types::{Asset, VaultInfo};

/// Revenue promise for distribution
#[cw_serde]
pub struct RevenuePromise {
    /// Address to receive the revenue
    pub address: String,
    /// Amount to distribute (in canonical asset)
    pub amount: Uint128,
}

/// Configuration for the revenue distributor
#[cw_serde]
pub struct Config {
    /// Contract owner/admin
    pub owner: Addr,
    /// Canonical asset for all distributions
    pub canonical_asset: Asset,
    /// Revenue destinations for automatic distribution
    pub revenue_destinations: Vec<RevenueDestination>,
    /// LTV Disco contract address.
    /// WARNING: DO NOT SET THE LTV DISCO AS A PROMISE, ONLY AS A DESTINATION.
    pub ltv_disco: Addr,
    /// Transmuter vault info used for EnterVault and VT denom
    pub transmuter_vault: VaultInfo,
    /// Points system contract address (optional, for awarding points to affiliates)
    pub points_system_contract: Option<Addr>,
    /// CDP contract address (for TakeRevenueFromBasket)
    pub cdp_contract: Option<Addr>,
    /// Revenue dispersal window in days (optional, if set, distributions only occur within this window)
    pub revenue_dispersal_window: Option<u64>,
    /// Transmuter lockdrop contract address (optional, for synchronized parameter updates)
    pub transmuter_lockdrop_contract: Option<Addr>,
    /// LTV Disco contract address (optional, for synchronized parameter updates)
    pub ltv_disco_contract: Option<Addr>,
    /// Auction contract address (for routing non-CDT revenue)
    pub auction_contract: Option<Addr>,
}

/// Revenue destination configuration
#[cw_serde]
pub struct RevenueDestination {
    /// Destination address (typically staking contract)
    pub destination: Addr,
    /// Distribution ratio (0.0 to 1.0)
    pub distribution_ratio: cosmwasm_std::Decimal,
}

/// Vault info message type for instantiate/update (string address)
#[cw_serde]
pub struct RDVaultInfoMessage {
    /// Transmuter contract address used for EnterVault
    pub vault_addr: String,
    /// Deposit token denom (canonical)
    pub deposit_token: String,
    /// Vault token denom (VT)
    pub vault_token: String,
}

/// Execute messages for the revenue distributor contract
#[cw_serde]
pub enum ExecuteMsg {
    /// Set revenue promises for distribution.
    /// Validates that total promised amount <= sent amount.
    /// Any excess amount will be distributed to revenue_destinations.
    /// WARNING: DO NOT SET THE LTV DISCO AS A PROMISE, ONLY AS A DESTINATION.
    SetPromises {
        /// Array of revenue promises
        promises: Vec<RevenuePromise>,
        /// Optional asset distribution for LTV Disco rev distribution
        ltv_disco_distribution: Option<Vec<Asset>>,
    },
    /// Distribute all current promises and clear them
    /// Uses reply_on_error for failed distributions to continue with others
    DistributePromises {
        /// Optional limit on number of promises to distribute
        limit: Option<u32>,
    },
    /// Take revenue from CDP Basket's pending_revenue
    /// Always takes ALL available revenue and maintains per-asset attribution
    /// Uses CDP contract address from config
    TakeRevenueFromBasket {},
    /// Update contract configuration (admin only)
    UpdateConfig {
        /// New revenue destinations
        revenue_destinations: Option<Vec<RevenueDestination>>,
        /// Update LTV Disco address
        ltv_disco: Option<String>,
        /// Update transmuter vault info
        transmuter_vault: Option<RDVaultInfoMessage>,
        /// Update points system contract address
        points_system_contract: Option<String>,
        /// Update CDP contract address
        cdp_contract: Option<String>,
        /// Update revenue dispersal window (in days)
        revenue_dispersal_window: Option<u64>,
        /// Update transmuter lockdrop contract address
        transmuter_lockdrop_contract: Option<String>,
        /// Update LTV Disco contract address (for synchronized updates)
        ltv_disco_contract: Option<String>,
        /// Update auction contract address (for routing non-CDT revenue)
        auction_contract: Option<String>,
    },
    /// Permissionless execution to pull revenue and distribute within window
    /// Calls TakeRevenueFromBasket then DistributePromises if window has passed
    ExecuteRevenueDistribution {},
    /// Clear failed distributions (admin only)
    ClearFailedDistributions {},
    /// Clear pending distributions (admin only)
    ClearPendingDistributions {},
    /// Retry failed distributions (admin only)
    RetryFailedDistribute {
        /// Optional limit on number of failed distributions to retry
        limit: Option<u32>,
    },
    /// Route non-CDT revenue (collateral fees) to auction
    /// Accepts any non-CDT asset and sends it to the auction contract via StartAuction
    AddNonCdtRevenue {
        /// Which collateral assets earned this revenue
        per_asset_distribution: Vec<crate::types::Asset>,
    },
}

/// Query messages for the revenue distributor contract
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    /// Get current configuration
    #[returns(Config)]
    Config {},
    /// Get current promises
    #[returns(Vec<RevenuePromise>)]
    Promises {},
    /// Get failed distributions
    #[returns(Vec<(String, Uint128)>)]
    FailedDistributions {},
    /// Get pending distribution promises
    #[returns(Vec<RevenuePromise>)]
    PendingDistributions {},
    /// Get current epoch revenue accumulation per asset
    #[returns(CurrentEpochRevenueResponse)]
    CurrentEpochRevenue {},
    /// Get countdown until next epoch/distribution (in seconds)
    #[returns(EpochCountdownResponse)]
    EpochCountdown {},
}

/// Response for current epoch revenue accumulation
#[cw_serde]
pub struct CurrentEpochRevenueResponse {
    /// Per-asset revenue accumulation: (asset_denom, accumulated_amount)
    pub revenue: Vec<(String, Uint128)>,
}

/// Response for epoch countdown
#[cw_serde]
pub struct EpochCountdownResponse {
    /// Seconds until next distribution/epoch end
    pub seconds_remaining: u64,
    /// Epoch start timestamp
    pub epoch_start: u64,
    /// Epoch end timestamp
    pub epoch_end: u64,
    /// Current timestamp
    pub current_time: u64,
}

/// Instantiate message for the revenue distributor contract
#[cw_serde]
pub struct InstantiateMsg {
    /// Initial owner/admin
    pub owner: String,
    /// Canonical asset for all distributions
    pub canonical_asset: Asset,
    /// Initial revenue destinations
    pub revenue_destinations: Vec<RevenueDestination>,
    /// LTV Disco contract address
    pub ltv_disco: String,
    /// Transmuter vault info used for EnterVault and VT denom
    pub transmuter_vault: RDVaultInfoMessage,
    /// Points system contract address (optional)
    pub points_system_contract: Option<String>,
    /// CDP contract address (optional, for TakeRevenueFromBasket)
    pub cdp_contract: Option<String>,
    /// Revenue dispersal window in days (optional)
    pub revenue_dispersal_window: Option<u64>,
    /// Transmuter lockdrop contract address (optional, for synchronized parameter updates)
    pub transmuter_lockdrop_contract: Option<String>,
    /// LTV Disco contract address (optional, for synchronized parameter updates)
    pub ltv_disco_contract: Option<String>,
    /// Auction contract address (optional, for routing non-CDT revenue)
    pub auction_contract: Option<String>,
}
