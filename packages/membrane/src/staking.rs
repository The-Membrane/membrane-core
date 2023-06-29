use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Uint128, Addr, Decimal};

use crate::types::{Asset, FeeEvent, StakeDeposit, StakeDistribution, DelegationInfo};

#[cw_serde]
pub struct InstantiateMsg {
    /// Contract owner, defaults to info.sender
    pub owner: Option<String>,
    /// Positions contract address
    pub positions_contract: Option<String>,
    /// Auction contract address
    pub auction_contract: Option<String>,
    /// Vesting contract address
    pub vesting_contract: Option<String>,
    /// Governance contract address
    pub governance_contract: Option<String>,
    /// Osmosis Proxy contract address
    pub osmosis_proxy: Option<String>,
    /// Incentive scheduling
    pub incentive_schedule: Option<StakeDistribution>,
    /// Fee wait period in days
    pub fee_wait_period: Option<u64>,
    /// Unstaking period in days
    pub unstaking_period: Option<u64>,
    /// MBRN denom
    pub mbrn_denom: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    UpdateConfig {
        /// Contract owner
        owner: Option<String>,
        /// Positions contract address
        positions_contract: Option<String>,
        /// Auction contract address
        auction_contract: Option<String>,
        /// Vesting contract address
        vesting_contract: Option<String>,
        /// Governance contract address
        governance_contract: Option<String>,
        /// Osmosis Proxy contract address
        osmosis_proxy: Option<String>,
        /// MBRN denom
        mbrn_denom: Option<String>,
        /// Incentive scheduling
        incentive_schedule: Option<StakeDistribution>,
        /// Unstaking period in days
        unstaking_period: Option<u64>,
        /// Fee wait period in days
        fee_wait_period: Option<u64>,
        /// Max commission rate
        max_commission_rate: Option<Decimal>,
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
        /// Send rewards to address
        send_to: Option<String>,
        /// Toggle to restake MBRN rewards
        restake: bool,
    },
    /// Delegate MBRN to a Governator
    UpdateDelegations {
        /// Governator address
        governator_addr: Option<String>,
        /// MBRN amount
        /// If None, act on total delegatible MBRN
        mbrn_amount: Option<Uint128>,
        /// Delegate or Undelegate
        delegate: Option<bool>,
        /// Set fluidity
        /// To change fluidity, you must undelegate & redelegate because your delegate may have delegated your MBRN
        fluid: Option<bool>,
        /// Update commission rate
        commission: Option<Decimal>,
        /// Toggle voting power delegation
        voting_power_delegation: Option<bool>,
    },
    /// Delegate delegated MBRN
    /// i.e. MBRN that is fluid delegated to a governator
    /// Once delegated, the MBRN can't be undelegated by the governator, only the initial staker
    DelegateFluidDelegations {
        /// Governator address
        governator_addr: String,
        /// MBRN amount
        /// If None, act on total delegatible MBRN
        mbrn_amount: Option<Uint128>,
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
    UserRewards {
        /// User address
        user: String,
    },
    /// Returns list of StakeDeposits
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
    /// Returns list of DelegationInfo
    Delegations {
        /// Response limit
        limit: Option<u32>,
        /// Start after governator address
        start_after: Option<String>,
        /// Query a specific user
        user: Option<String>,
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
    /// Max commission rate
    pub max_commission_rate: Decimal,
    /// Positions contract address
    pub positions_contract: Option<Addr>,
    /// Auction contract address
    pub auction_contract: Option<Addr>,
    /// Vesting contract address
    pub vesting_contract: Option<Addr>,
    /// Governance contract address
    pub governance_contract: Option<Addr>,
    /// Osmosis Proxy contract address
    pub osmosis_proxy: Option<Addr>,
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

#[cw_serde]
pub struct DelegationResponse {
    /// User
    pub user: Addr,
    /// DelegationInfo
    pub delegation_info: DelegationInfo,
}
