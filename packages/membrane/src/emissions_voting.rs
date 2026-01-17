use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Uint128};

/// Instantiate message for the emissions voting contract
#[cw_serde]
pub struct InstantiateMsg {
    /// Contract owner (defaults to sender if not provided)
    pub owner: Option<String>,
    /// LTV Disco contract address for querying user deposits
    pub ltv_disco_contract: String,
    /// Staking contract address for querying user stakes
    pub staking_contract: String,
}

/// Execute messages for emissions voting
#[cw_serde]
pub enum ExecuteMsg {
    /// Create a new voting graph (owner only)
    CreateGraph {
        /// Unique label/identifier for this graph
        label: String,
        /// Type of graph: "uint128" or "decimal"
        graph_type: GraphType,
        /// Minimum value of the range
        range_min: String,
        /// Maximum value of the range
        range_max: String,
        /// Duration of each voting period in days
        period_days: u64,
        /// Contract address to receive voting results
        callback_contract: String,
    },
    /// Cast a vote on a graph for any value within the range
    Vote {
        /// Label of the graph to vote on
        graph_label: String,
        /// Value to vote for (must be within graph's range)
        /// For Uint128 graphs: integer string like "1000000"
        /// For Decimal graphs: decimal string like "0.5" or "1.25"
        vote_value: String,
    },
    /// End the current voting period and send results (permissionless)
    EndVoting {
        /// Label of the graph to finalize
        graph_label: String,
    },
    /// Update contract configuration (owner only)
    UpdateConfig {
        /// New owner address
        owner: Option<String>,
        /// New LTV Disco contract address
        ltv_disco_contract: Option<String>,
        /// New staking contract address
        staking_contract: Option<String>,
    },
    /// Update graph parameters (owner only)
    UpdateGraph {
        /// Label of the graph to update
        label: String,
        /// New period duration in days
        period_days: Option<u64>,
        /// New callback contract address
        callback_contract: Option<String>,
    },
    /// Remove a graph (owner only)
    RemoveGraph {
        /// Label of the graph to remove
        label: String,
    },
    /// Remove a vote from a graph (allows user to unstake/withdraw after)
    RemoveVote {
        /// Label of the graph to remove vote from
        graph_label: String,
    },
}

/// Query messages for emissions voting
#[cw_serde]
pub enum QueryMsg {
    /// Get contract configuration
    Config {},
    /// Get a single graph's details
    Graph {
        label: String,
    },
    /// List all graphs with pagination
    Graphs {
        start_after: Option<String>,
        limit: Option<u32>,
    },
    /// Get the current weighted average result for a graph (live calculation)
    CurrentResult {
        label: String,
    },
    /// Get a user's current vote on a graph
    UserVote {
        user: String,
        graph_label: String,
    },
    /// Get historical results for a graph
    PeriodHistory {
        label: String,
        limit: Option<u32>,
    },
    /// Get all votes on a graph with pagination
    AllVotes {
        graph_label: String,
        start_after: Option<String>,
        limit: Option<u32>,
    },
    /// Check if user has any active votes across all graphs
    HasAnyVotes {
        user: String,
    },
}

/// Graph type enum
#[cw_serde]
pub enum GraphType {
    /// Uint128 graph for emissions/integer values
    Uint128,
    /// Decimal graph for multipliers/percentage values
    Decimal,
}

/// Contract configuration
#[cw_serde]
pub struct Config {
    /// Contract owner
    pub owner: Addr,
    /// LTV Disco contract address
    pub ltv_disco_contract: Addr,
    /// Staking contract address
    pub staking_contract: Addr,
}

/// Uint128 voting graph
#[cw_serde]
pub struct Uint128Graph {
    /// Unique label/identifier
    pub label: String,
    /// Minimum value of the range
    pub range_min: Uint128,
    /// Maximum value of the range
    pub range_max: Uint128,
    /// Duration of each voting period in days
    pub period_days: u64,
    /// Timestamp when the current period started
    pub current_period_start: u64,
    /// Contract to receive results
    pub callback_contract: Addr,
}

/// Decimal voting graph
#[cw_serde]
pub struct DecimalGraph {
    /// Unique label/identifier
    pub label: String,
    /// Minimum value of the range
    pub range_min: Decimal,
    /// Maximum value of the range
    pub range_max: Decimal,
    /// Duration of each voting period in days
    pub period_days: u64,
    /// Timestamp when the current period started
    pub current_period_start: u64,
    /// Contract to receive results
    pub callback_contract: Addr,
}

/// Combined graph enum for storage
#[cw_serde]
pub enum Graph {
    Uint128(Uint128Graph),
    Decimal(DecimalGraph),
}

impl Graph {
    pub fn label(&self) -> &str {
        match self {
            Graph::Uint128(g) => &g.label,
            Graph::Decimal(g) => &g.label,
        }
    }

    pub fn period_days(&self) -> u64 {
        match self {
            Graph::Uint128(g) => g.period_days,
            Graph::Decimal(g) => g.period_days,
        }
    }

    pub fn current_period_start(&self) -> u64 {
        match self {
            Graph::Uint128(g) => g.current_period_start,
            Graph::Decimal(g) => g.current_period_start,
        }
    }

    pub fn callback_contract(&self) -> &Addr {
        match self {
            Graph::Uint128(g) => &g.callback_contract,
            Graph::Decimal(g) => &g.callback_contract,
        }
    }

    pub fn set_period_start(&mut self, start: u64) {
        match self {
            Graph::Uint128(g) => g.current_period_start = start,
            Graph::Decimal(g) => g.current_period_start = start,
        }
    }

    pub fn graph_type(&self) -> GraphType {
        match self {
            Graph::Uint128(_) => GraphType::Uint128,
            Graph::Decimal(_) => GraphType::Decimal,
        }
    }
}

/// User vote record for Uint128 graphs
#[cw_serde]
pub struct UserVoteUint128 {
    /// Value the user voted for
    pub voted_value: Uint128,
    /// User's voting power at time of vote
    pub voting_power: Uint128,
    /// Period when the vote was cast
    pub period_start: u64,
}

/// User vote record for Decimal graphs
#[cw_serde]
pub struct UserVoteDecimal {
    /// Value the user voted for
    pub voted_value: Decimal,
    /// User's voting power at time of vote
    pub voting_power: Uint128,
    /// Period when the vote was cast
    pub period_start: u64,
}

/// Combined user vote enum
#[cw_serde]
pub enum UserVote {
    Uint128(UserVoteUint128),
    Decimal(UserVoteDecimal),
}

impl UserVote {
    pub fn voting_power(&self) -> Uint128 {
        match self {
            UserVote::Uint128(v) => v.voting_power,
            UserVote::Decimal(v) => v.voting_power,
        }
    }

    pub fn period_start(&self) -> u64 {
        match self {
            UserVote::Uint128(v) => v.period_start,
            UserVote::Decimal(v) => v.period_start,
        }
    }
}

/// Historical result for a completed period
#[cw_serde]
pub struct PeriodResult {
    /// Period start timestamp
    pub period_start: u64,
    /// Period end timestamp
    pub period_end: u64,
    /// Result as Uint128 (for Uint128 graphs)
    pub result_uint128: Option<Uint128>,
    /// Result as Decimal (for Decimal graphs)
    pub result_decimal: Option<Decimal>,
    /// Total voting power that participated
    pub total_voting_power: Uint128,
}

// Query responses

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

#[cw_serde]
pub struct GraphResponse {
    pub graph: Graph,
    /// Current period end timestamp
    pub period_end: u64,
    /// Total voting power currently in this graph
    pub total_voting_power: Uint128,
}

#[cw_serde]
pub struct GraphsResponse {
    pub graphs: Vec<GraphResponse>,
}

#[cw_serde]
pub struct CurrentResultResponse {
    /// Result as Uint128 (for Uint128 graphs)
    pub result_uint128: Option<Uint128>,
    /// Result as Decimal (for Decimal graphs)
    pub result_decimal: Option<Decimal>,
    /// Total voting power participating
    pub total_voting_power: Uint128,
    /// Whether the period has ended and can be finalized
    pub period_ended: bool,
}

#[cw_serde]
pub struct UserVoteResponse {
    pub vote: Option<UserVote>,
    /// User's current voting power
    pub current_voting_power: Uint128,
}

#[cw_serde]
pub struct PeriodHistoryResponse {
    pub history: Vec<PeriodResult>,
}

#[cw_serde]
pub struct AllVotesResponse {
    pub votes: Vec<VoteInfo>,
    pub total_power: Uint128,
}

#[cw_serde]
pub struct VoteInfo {
    pub user: Addr,
    pub vote: UserVote,
}

#[cw_serde]
pub struct HasAnyVotesResponse {
    pub has_votes: bool,
}

/// Migrate message
#[cw_serde]
pub struct MigrateMsg {}
