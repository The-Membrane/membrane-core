use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, Uint128, Addr};

use crate::types::{Asset, FeeEvent, StakeDeposit, StakeDistribution};

#[cw_serde]
pub struct InstantiateMsg {
    /// Contract owner, defaults to info.sender
    pub owner: Option<String>,
    /// Positions contract address
    pub positions_contract: Option<String>,
    /// Vesting contract address
    pub vesting_contract: Option<String>,
    /// Governance contract address
    pub governance_contract: Option<String>,
    /// Osmosis Proxy contract address
    pub osmosis_proxy: Option<String>,
    /// Dex router contract address
    pub dex_router: Option<String>,
    /// Incentive scheduling
    pub incentive_schedule: Option<StakeDistribution>,
    /// Fee wait period in days
    pub fee_wait_period: Option<u64>,
    /// Unstaking period in days
    pub unstaking_period: Option<u64>,
    /// MBRN denom
    pub mbrn_denom: String,
    /// Max spread for dex swaps
    pub max_spread: Option<Decimal>,
}

#[cw_serde]
pub enum ExecuteMsg {
    UpdateConfig {
        /// Contract owner
        owner: Option<String>,
        /// Positions contract address
        positions_contract: Option<String>,
        /// Vesting contract address
        vesting_contract: Option<String>,
        /// Governance contract address
        governance_contract: Option<String>,
        /// Osmosis Proxy contract address
        osmosis_proxy: Option<String>,
        /// Dex router contract address
        dex_router: Option<String>,
        /// MBRN denom
        mbrn_denom: Option<String>,
        /// Incentive scheduling
        incentive_schedule: Option<StakeDistribution>,
        /// Unstaking period in days
        unstaking_period: Option<u64>,
        /// Fee wait period in days
        fee_wait_period: Option<u64>,
        /// Max spread for dex swaps
        max_spread: Option<Decimal>,
    },
    /// Stake MBRN tokens
    Stake {
        /// User address
        user: Option<String>,
    },
    /// Unstake/Withdraw MBRN tokens & claim claimables
    Unstake {
        /// MBRN amount 
        mbrn_amount: Option<Uint128>,
    },
    /// Restake unstak(ed/ing) MBRN
    Restake {
        /// MBRN amount
        mbrn_amount: Uint128,
    },
    /// Claim all claimables
    ClaimRewards {
        /// Claim rewards as a native token full denom
        ///NOTE: Claim_As is for liq_fees, not MBRN tokens.
        claim_as_native: Option<String>,
        /// Send rewards to address
        send_to: Option<String>,
        /// Toggle to restake MBRN rewards
        restake: bool,
    },
    /// Position's contract deposits protocol revenue
    DepositFee {},
    /// Clear FeeEvent state object
    TrimFeeEvents {},

}

#[cw_serde]
pub enum QueryMsg {
    /// Returns contract config
    Config {},
    /// Returns StakerResponse
    UserStake {
        /// Staker address
        staker: String,
    },
    /// Returns fee claimables && # of staking rewards
    StakerRewards {
        /// Staker address
        staker: String,
    },
    /// returns list of StakeDeposits
    Staked {
        /// Response limit
        limit: Option<u32>,
        /// Start after timestamp in seconds
        start_after: Option<u64>,
        /// End before timestamp in seconds
        end_before: Option<u64>,
        /// Include unstakers
        unstaking: bool,
    },
    /// Returns list of FeeEvents
    FeeEvents {
        /// Response limit
        limit: Option<u32>,
        /// Start after timestamp in seconds
        start_after: Option<u64>,
    },
    /// Returns total MBRN staked
    TotalStaked {},
    /// Returns progress of current incentive schedule
    IncentiveSchedule {},
}

#[cw_serde]
pub struct Config {
    /// Contract owner
    pub owner: Addr,
    /// MBRN denom
    pub mbrn_denom: String,
    /// Incentive schedule
    pub incentive_schedule: StakeDistribution,
    /// Wait period between deposit & ability to earn fee events, in days
    pub fee_wait_period: u64,
    /// Unstaking period, in days
    pub unstaking_period: u64,
    /// Positions contract address
    pub positions_contract: Option<Addr>,
    /// Vesting contract address
    pub vesting_contract: Option<Addr>,
    /// Governance contract address
    pub governance_contract: Option<Addr>,
    /// Osmosis Proxy contract address
    pub osmosis_proxy: Option<Addr>,
    /// Dex router contract address
    pub dex_router: Option<Addr>,
    /// Max spread for dex swaps
    pub max_spread: Option<Decimal>,
}

#[cw_serde]
pub struct StakerResponse {
    /// Staker address
    pub staker: String,
    /// Total MBRN staked
    pub total_staked: Uint128,
    /// Deposit list (amount, timestamp)
    pub deposit_list: Vec<(Uint128, u64)>,
}

#[cw_serde]
pub struct RewardsResponse {
    /// Claimable rewards
    pub claimables: Vec<Asset>,
    /// Number of staking rewards
    pub accrued_interest: Uint128,
}

#[cw_serde]
pub struct StakedResponse {
    /// List of StakeDeposits
    pub stakers: Vec<StakeDeposit>,
}

#[cw_serde]
pub struct TotalStakedResponse {
    /// Total MBRN staked not including vested
    pub total_not_including_vested: Uint128,
    /// Total vested stake
    pub vested_total: Uint128,
}

#[cw_serde]
pub struct FeeEventsResponse {
    /// List of FeeEvents
    pub fee_events: Vec<FeeEvent>,
}
