use cosmwasm_std::{Addr, Uint128, Decimal};
use cosmwasm_schema::cw_serde;


#[cw_serde]
pub struct InstantiateMsg {
    /// Contract owner, defaults to info.sender
    pub owner: Option<String>,
    /// Positions contract address
    pub positions_contract: String,
    /// Apollo Router contract address
    pub apollo_router_contract: String,
    /// Max slippage for swaps
    pub max_slippage: Decimal,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Deposits asset into a new Position in the Positions contract
    Deposit {
        /// Position ID to deposit into. 
        /// If the user wants to create a new/separate position, no position id is passed.
        position_id: Option<Uint128>, 
    }, 
    /// Looped loans to create leverage
    Loop {
        /// Position ID
        position_id: Uint128,
        /// Number of loops to run
        num_loops: Option<u64>,
        /// Target LTV
        target_LTV: Decimal,
    },
    /// Closes position
    ClosePosition {
        /// Position ID
        position_id: Uint128,
        /// Max slippage for swaps. 
        /// ClosePosition uses the spread as a multiplier on the collateral amount to ensure success.
        max_spread: Decimal,
    },
    /// Update contract config
    UpdateConfig {
        /// Contract owner
        owner: Option<String>,
        /// Positions contract address
        positions_contract: Option<String>,
        /// Apollo Router contract address
        apollo_router_contract: Option<String>,
        /// Max slippage for swaps
        max_slippage: Option<Decimal>,
    },
}
//Position Repayments can be done on the the base Positions contract

#[cw_serde]
pub enum QueryMsg {
    /// Return contract config
    Config {},
    /// Returns IDs of user Positions in this contract.
    /// For full position responses query the Positions contract.
    GetUserPositions { user: String },
    /// Returns IDs of user Positions in this contract.
    /// For full position responses query the Positions contract.
    GetPositionIDs {
        /// User limit
        limit: Option<u64>,
        /// User to start after
        start_after: Option<String>,
    },
}


#[cw_serde]
pub struct Config {
    /// Contract owner
    pub owner: Addr,
    /// Positions contract address
    pub positions_contract: Addr,
    /// Apollo Router contract address
    pub apollo_router_contract: Addr,
    /// Max slippage for swaps
    pub max_slippage: Decimal,
}
