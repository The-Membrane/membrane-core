use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Uint128};

use crate::types::DepositDenom;

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
    /// Minimum deposit amount
    pub minimum_deposit: Uint128,
    /// Waiting period for deposits (in seconds)
    pub waiting_period: u64,
    /// Maximum LTV
    pub max_ltv: Decimal,
    /// Percent of revenue to disperse linearly
    pub percent_to_disperse: Decimal,
    /// Default dispersal window in hours
    pub dispersal_window: u64,
    /// Window for liquidations to activate dispersals (in hours)
    pub activation_window: u64,
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
    },
    /// Submit a backing deposit
    SubmitDeposit {
        deposit_input: BackingDepositInput,
        deposit_owner: Option<String>,
    },
    /// Withdraw a backing deposit
    WithdrawDeposit {
        deposit_id: Uint128,
        asset: String,
        amount: Option<Uint128>,
    },
    /// Add bad debt to an LTV queue (CDP contract only)
    AddBadDebt {
        asset: String,
        amount: Uint128,
    },
    /// Retry failed bad debt fulfillments
    RetryFailedBadDebt {
        asset: String,
    },
    /// Add revenue to an asset's LTV queue
    AddRevenue {
        asset: String,
    },
    /// Activate a dispersal window for an asset
    ActivateDispersal {
        asset: String,
        dispersal_window: u64,
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
        minimum_deposit: Option<Uint128>,
        waiting_period: Option<u64>,
        percent_to_disperse: Option<Decimal>,
        dispersal_window: Option<u64>,
        activation_window: Option<u64>
    },
    /// Post a deposit tracker entry for base token tracking
    PostDepositTrackerEntry {
        asset: String,
        max_ltv: Decimal,
        max_borrow_ltv: Decimal,
    },
    /// Assures that for deposits & withdrawals the conversion rate is static
    /// Only callable by the contract
    RateAssurance {
        asset: String,
        max_ltv: Decimal,
        max_borrow_ltv: Decimal,
    },
}

/// Query messages
#[cw_serde]
pub enum QueryMsg {
    /// Get contract configuration
    Config {},
    /// Get LTV queue for an asset
    GetLTVQueue { asset: String },
    /// Get backing deposit by ID
    GetBackingDeposit { deposit_id: Uint128, asset: String },
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
    /// Deposit Growth queries
    GetDepositGrowth {
        asset: String,
        max_ltv: Decimal,
        max_borrow_ltv: Decimal,
    },
}

/// Response for LTV queue query
#[cw_serde]
pub struct LTVQueueResponse {
    pub queue: LTVQueue,
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

#[cw_serde]
pub struct VTGrowthResponse {
    pub timestamp: u64,
    pub amount: Uint128,
}

/// Migrate message
#[cw_serde]
pub struct MigrateMsg {}


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
    /// Minimum deposit amount
    pub minimum_deposit: Uint128,
    /// Waiting period for deposits (in seconds)
    pub waiting_period: u64,
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
    /// Backing deposits in this group
    pub backing_deposits: Vec<BackingDeposit>,
    /// Total deposit tokens for this group
    pub total_deposit_tokens: Uint128,
    /// Total vault tokens for this group
    pub total_vault_tokens: Uint128,
}

/// Backing deposit similar to Bid but for LTV slots
#[cw_serde]
pub struct BackingDeposit {
    /// User address
    pub user: Addr,
    /// Deposit ID
    pub id: Uint128,
    /// Deposit amount (vault tokens)
    pub vault_tokens: Uint128,
    /// Chosen max borrow LTV for sorting within slot
    pub max_borrow_ltv: Decimal,
    /// Wait end time (if any)
    pub wait_end: Option<u64>,
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
}

/// Base token tracking entry with timestamp
#[cw_serde]
pub struct BaseTokenTrackingEntry {
    /// Timestamp when this entry was created
    pub timestamp: u64,
    /// Base token amount for 1,000,000 vault tokens
    pub base_token_amount: Uint128,
}