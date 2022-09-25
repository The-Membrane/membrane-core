use cw20::Cw20ReceiveMsg;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Decimal, Uint128};

use crate::types::{Asset, FeeEvent, StakeDeposit};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub positions_contract: Option<String>,
    pub builders_contract: Option<String>,
    pub osmosis_proxy: Option<String>,
    pub staking_rate: Option<Decimal>,
    pub fee_wait_period: Option<u64>, //in days
    pub unstaking_period: Option<u64>,
    pub mbrn_denom: String,
    pub dex_router: Option<String>,
    pub max_spread: Option<Decimal>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Receive(Cw20ReceiveMsg),
    UpdateConfig {
        owner: Option<String>,
        positions_contract: Option<String>,
        builders_contract: Option<String>,
        osmosis_proxy: Option<String>,
        mbrn_denom: Option<String>,
        staking_rate: Option<Decimal>,
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
        claim_as_cw20: Option<String>,   //Contract Address
        send_to: Option<String>,
        restake: bool,
    },
    //Position's contract deposits liq_fees
    DepositFee {},
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cw20HookMsg {
    //Deposit Liquidation fee
    DepositFee {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {},
    UserStake {
        staker: String,
    },
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
}

// We define a custom struct for each query response
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct ConfigResponse {
    pub owner: String,
    pub positions_contract: String,
    pub builders_contract: String,
    pub osmosis_proxy: String,
    pub staking_rate: String,
    pub unstaking_period: String,
    pub fee_wait_period: String,
    pub mbrn_denom: String,
    pub dex_router: String,
    pub max_spread: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct StakerResponse {
    pub staker: String,
    pub total_staked: Uint128,
    pub deposit_list: Vec<(String, String)>, //Amount and timestamp of each deposit
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct RewardsResponse {
    pub claimables: Vec<Asset>,
    pub accrued_interest: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct StakedResponse {
    pub stakers: Vec<StakeDeposit>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct TotalStakedResponse {
    pub total_not_including_builders: String,
    pub builders_total: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct FeeEventsResponse {
    pub fee_events: Vec<FeeEvent>,
}
