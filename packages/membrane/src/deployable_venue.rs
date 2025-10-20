use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Uint128;

use crate::types::{UserInfo, LeaveTokens};


/// Execute messages for the deployable venue contract
/// NOTE: Any additional params have to be optional.

#[cw_serde]
pub enum ExecuteMsg {
    /// Enter the vault.
    EnterVault {
        leave_vault_tokens_in_vault: Option<LeaveTokens>,
    },
    /// Repay user debt.
    // RepayUserDebt needs repay whatever is possible.
    RepayUserDebt {
        /// User info
        user_info: UserInfo,
        /// Repayment amount
        repayment: Uint128,
    },
    /// Fulfill intents.
    /// We need the intent contract to be able to call this, withdraw underlying tokens & act on them.
    FulfillIntents {
        user: String
    }
}

/// Query messages for the deployable venue contract
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    /// Get user's retrievable CDT.
    /// NOTE: Needs to take into account the withdraw & conversion path to CDT.
    /// Ex: If it needs to go thru the Transmuter, it needs to check how much CDT the Transmuter has in it.
    #[returns(Uint128)]
    RetrievableCDT {
        /// User to query
        user: String,
    },
    /// Get the underlying token amount for a given vault token amount
    #[returns(Uint128)]
    VaultTokenUnderlying { vault_token_amount: Uint128 },
    /// Get the vault token conversion for a given deposit token amount
    #[returns(Uint128)]
    DepositTokenConversion { deposit_token_amount: Uint128 },
    // VT Growth queries
    // REQUIRED. Commented bc there is no standardized return type.
    // #[returns(Vec<T>)]
    // GetVTGrowth { 
    //     limit: Option<u32>,
    //     start_after: Option<u64>,
    // },
}

