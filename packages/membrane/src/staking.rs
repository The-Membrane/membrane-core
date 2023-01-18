use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, Uint128, Addr};

use crate::types::{Asset, FeeEvent, StakeDeposit, StakeDistribution};

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub positions_contract: Option<String>,
    pub vesting_contract: Option<String>,
    pub governance_contract: Option<String>,
    pub osmosis_proxy: Option<String>,
    pub incentive_schedule: Option<StakeDistribution>,
    pub fee_wait_period: Option<u64>, //in days
    pub unstaking_period: Option<u64>,
    pub mbrn_denom: String,
    pub dex_router: Option<String>,
    pub max_spread: Option<Decimal>,
}

#[cw_serde]
pub enum ExecuteMsg {
    UpdateConfig {
        owner: Option<String>,
        positions_contract: Option<String>,
        vesting_contract: Option<String>,
        governance_contract: Option<String>,
        osmosis_proxy: Option<String>,
        mbrn_denom: Option<String>,
        incentive_schedule: Option<StakeDistribution>,
        unstaking_period: Option<u64>,
        fee_wait_period: Option<u64>,
        dex_router: Option<String>,
        max_spread: Option<Decimal>,
    },
    Stake {
        //Deposit MBRN tokens for a user
        user: Option<String>,
    },
    Unstake {
        //Withdraw and claim rewards
        mbrn_amount: Option<Uint128>,
    },
    Restake {
        //Restake unstak(ed/ing) MBRN
        mbrn_amount: Uint128,
    },
    ClaimRewards {
        //Claim ALL staking rewards
        //NOTE: Claim_As is for liq_fees, NOT MBRN tokens
        claim_as_native: Option<String>, //Native FullDenom
        send_to: Option<String>,
        restake: bool,
    },
    //Position's contract deposits protocol revenue
    DepositFee {},
    //Trim FeeEvent state object
    TrimFeeEvents {},

}

#[cw_serde]
pub enum QueryMsg {
    Config {},
    UserStake {
        staker: String,
    },
    //Fee claimables && Staking rewards
    StakerRewards {
        staker: String,
    },
    //List of all StakeDeposits
    Staked {
        limit: Option<u32>,
        start_after: Option<u64>, //Timestamp in seconds
        end_before: Option<u64>,  //Timestamp in seconds
        unstaking: bool,          //true if u want unstakers included
    },
    //List of all FeeEvents
    FeeEvents {
        limit: Option<u32>,
        start_after: Option<u64>, //Timestamp in seconds
    },
    //Total MBRN staked
    TotalStaked {},
    //Returns StakeDistribution log from STAKE_INCENTIVES state object
    IncentiveSchedule {},
}

#[cw_serde]
pub struct Config {
    pub owner: Addr, //MBRN Governance
    pub mbrn_denom: String,
    pub incentive_schedule: StakeDistribution,
    //Wait period between deposit & ability to earn fee events
    pub fee_wait_period: u64,  //in days
    pub unstaking_period: u64, //days
    pub positions_contract: Option<Addr>,
    pub vesting_contract: Option<Addr>,
    pub governance_contract: Option<Addr>,
    pub osmosis_proxy: Option<Addr>,
    pub dex_router: Option<Addr>,
    pub max_spread: Option<Decimal>, //max_spread for the router, mainly claim_as swaps
}

// We define a custom struct for each query response
#[cw_serde]
pub struct StakerResponse {
    pub staker: String,
    pub total_staked: Uint128,
    pub deposit_list: Vec<(String, String)>, //Amount and timestamp of each deposit
}

#[cw_serde]
pub struct RewardsResponse {
    pub claimables: Vec<Asset>,
    pub accrued_interest: Uint128,
}

#[cw_serde]
pub struct StakedResponse {
    pub stakers: Vec<StakeDeposit>,
}

#[cw_serde]
pub struct TotalStakedResponse {
    pub total_not_including_vested: Uint128,
    pub vested_total: Uint128,
}

#[cw_serde]
pub struct FeeEventsResponse {
    pub fee_events: Vec<FeeEvent>,
}
