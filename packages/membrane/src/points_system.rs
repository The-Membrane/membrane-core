use cosmwasm_std::{Addr, Coin, Decimal, Uint128};
use cosmwasm_schema::cw_serde;

use crate::types::{UserInfo, PointsMultipliers};
use crate::liq_queue::ClaimsResponse;

#[cw_serde]
pub struct InstantiateMsg {
    pub cdt_denom: String,
    pub mbrn_denom: String,
    pub oracle_contract: String,
    pub positions_contract: String,
    pub stability_pool_contract: String,
    pub liq_queue_contract: String,
    pub governance_contract: String,
    pub osmosis_proxy_contract: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    UpdateConfig {
        owner: Option<String>,
        cdt_denom: Option<String>,
        mbrn_denom: Option<String>,
        oracle_contract: Option<String>,
        positions_contract: Option<String>,
        stability_pool_contract: Option<String>,
        liq_queue_contract: Option<String>,
        governance_contract: Option<String>,
        osmosis_proxy_contract: Option<String>,
        transmuter_contract: Option<String>,
        ltv_disco_contract: Option<String>,
        system_discounts_contract: Option<String>,
        mbrn_per_point: Option<Decimal>,
        max_mbrn_distribution: Option<Uint128>,
        points_per_dollar: Option<Decimal>,
        points_multipliers: Option<PointsMultipliers>
    },
    /// Queries contracts to confirm & save current claims for the user
    CheckClaims {
        sp_claims: bool,
        lq_claims: bool,
        /// Proposal ID
        vote: Option<Vec<u64>>,
    }, 
    /// Execute CDP repay and allocate points based on revenue attribute
    RepayAndGivePoints {
        position_id: Uint128,
        position_owner: Option<String>,
        send_excess_to: Option<String>,
    },
    /// Execute disco revenue claim and allocate points
    ClaimDiscoRevenueAndGivePoints {
        user: String,
        asset: String,
        limit: Option<u32>,
        compound_action: Option<crate::ltv_disco::CompoundAction>,
    },
    /// Execute transmuter operation and allocate points
    TransmuteAndGivePoints {
        recipient: Option<String>,
    },
    /// Give points for operations that don't need reply handlers
    /// (SP claims, LQ claims, votes, rangebound vault yields)
    GivePoints {
        sp_claims: bool,
        lq_claims: bool,
        /// Proposal ID
        vote: Option<Vec<u64>>,
        // User address
        // rangebound_user: Option<String>,
    },
    /// Liquidate & send fees to caller (Points for liquidator and liquidatee)
    Liquidate {
        /// Position ID
        position_id: Uint128,
        /// Position owner
        position_owner: String,
    },
    /// Claim MBRN from level ups
    ClaimMBRN {},
}
//Position Repayments can be done on the the base Positions contract

#[cw_serde]
pub enum QueryMsg {
    /// Return contract config
    Config {},
    // Return current claim check
    ClaimCheck {},
    // Return user's stats
    UserStats { 
        user: Option<String>,
        limit: Option<u64>,
        start_after: Option<String>,
     },
     // Return user's vault conversion rates
     UserConversionRates { 
         user: Option<String>,
         limit: Option<u64>,
         start_after: Option<String>,
      },
}


#[cw_serde]
pub struct Config {
    /// Contract owner
    pub owner: Addr,
    /// CDT Denom
    pub cdt_denom: String,
    /// MBRN Denom
    pub mbrn_denom: String,
    /// Oracle contract address
    pub oracle_contract: Addr,
    /// Positions contract address
    pub positions_contract: Addr,
    /// Stability Pool contract address
    pub stability_pool_contract: Addr,
    /// Liq Queue contract address
    pub liq_queue_contract: Addr,
    /// Gov contract address
    pub governance_contract: Addr,
    /// Osmosis Proxy contract address
    pub osmosis_proxy_contract: Addr,
    /// Transmuter contract address (optional)
    pub transmuter_contract: Option<Addr>,
    /// LTV Disco contract address (optional)
    pub ltv_disco_contract: Option<Addr>,
    /// System discounts contract address (optional, for boost queries)
    pub system_discounts_contract: Option<Addr>,
    ///MBRN distribution per point
    pub mbrn_per_point: Decimal,
    ///Total MBRN distributon from the contract
    pub total_mbrn_distribution: Uint128,
    ///Maximum MBRN distributon for the contract
    pub max_mbrn_distribution: Uint128,
    ///Points per $1
    pub points_per_dollar: Decimal,
}

#[cw_serde]
pub struct ClaimCheck {
    /// Block time of the last check
    pub check_time: u64,
    /// User address
    pub user: Addr,
    ///Basket's last pending_revenue value
    pub cdp_pending_revenue: Uint128,
    ///LQ's Pending Claims
    pub lq_pending_claims: Vec<ClaimsResponse>,
    ///SP's Pending Claims
    pub sp_pending_claims: Vec<Coin>,
    ///Proposal IDs that the user hadn't voted in during the check
    pub vote_pending: Vec<u64>,
}

#[cw_serde]
pub struct UserStats {
    /// Total points
    pub total_points: Decimal,
    /// Claimable points
    pub claimable_points: Decimal,
}


#[cw_serde]
pub struct UserStatsResponse {
    /// User address
    pub user: Addr,
    ///Stats
    pub stats: UserStats,
}


#[cw_serde]
pub struct VaultConversionRate {
    ///Vault Address
    pub vault_address: String,
    ///Deposit Token Conversion Rate for 1 vault token
    pub last_conversion_rate: Uint128,
    /// Total Vault Tokens
    pub last_vt_balance: Uint128,
}

#[cw_serde]
pub struct UserConversionResponse {
    /// User address
    pub user: Addr,
    ///Stats
    pub conversion_rates: Vec<VaultConversionRate>,
}
