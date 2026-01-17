use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Uint128};
use crate::types::Locked;

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    /// Mars Params contract address for querying AssetParams
    pub mars_params_contract: String,
    /// LTV Disco contract address
    pub disco_contract: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    UpdateConfig {
        owner: Option<String>,
        mars_params_contract: Option<String>,
        disco_contract: Option<String>,
    },
    // Deposit MBRN into Disco via this contract
    // The contract queries Mars for LTVs and mirrors them in Disco
    // Deposit {
    //     /// User address (optional, defaults to sender)
    //     user: Option<String>,
    //     /// Asset to deposit into (must match Mars market)
    //     asset: String,
    //     /// Optional lock information
    //     lock: Option<Locked>,
    // },
    /// Process moves for managed deposits with pagination
    /// Queries Disco for managed deposit keys, processes up to limit, saves progress
    ProcessMoves {
        /// Limit of moves to process in this transaction
        limit: Option<u32>,
        /// Start after this deposit key (for pagination)
        start_after: Option<String>,
    },
    // Move a single deposit (can be called directly)
    // MoveSingleDeposit {
    //     deposit_key: String,
    //     new_ltv: Decimal,
    //     new_max_borrow_ltv: Decimal,
    // },
}

#[cw_serde]
pub enum QueryMsg {
    Config {},
    /// Get progress state for ProcessMoves
    MoveProgress {},
    /// Get Mars LTV info for an asset
    MarsLTVInfo {
        asset: String,
    },
}

#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub mars_params_contract: String,
    pub disco_contract: Addr,
}

#[cw_serde]
pub struct MoveProgress {
    /// Last deposit key processed
    pub last_processed_key: Option<String>,
    /// Total keys to process
    pub total_keys: u64,
    /// Keys processed so far
    pub processed_count: u64,
}

#[cw_serde]
pub struct MarsLTVInfoResponse {
    /// Max LTV from Mars market
    pub max_ltv: Decimal,
    /// Max borrow LTV from Mars market  
    pub max_borrow_ltv: Decimal,
}

