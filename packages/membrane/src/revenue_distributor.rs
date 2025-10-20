use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Uint128, Addr, Decimal};
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
    /// Update contract configuration (admin only)
    UpdateConfig {
        /// New revenue destinations
        revenue_destinations: Option<Vec<RevenueDestination>>,
        /// Update LTV Disco address
        ltv_disco: Option<String>,
        /// Update transmuter vault info
        transmuter_vault: Option<RDVaultInfoMessage>,
    },
    /// Clear failed distributions (admin only)
    ClearFailedDistributions {},
    /// Clear pending distributions (admin only)
    ClearPendingDistributions {},
    /// Retry failed distributions (admin only)
    RetryFailedDistribute {
        /// Optional limit on number of failed distributions to retry
        limit: Option<u32>,
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
}
