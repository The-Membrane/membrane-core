use core::fmt;

use crate::math::{Decimal256, Uint256};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Decimal, Uint128};

//Stability Pool

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PositionUserInfo {
    pub basket_id: Uint128,
    pub position_id: Option<Uint128>,
    pub position_owner: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct LiqAsset {
    pub info: AssetInfo,
    pub amount: Decimal,
}

impl fmt::Display for LiqAsset {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.amount, self.info)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct UserRatio {
    pub user: Addr,
    pub ratio: Decimal,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Deposit {
    pub user: Addr,
    pub amount: Decimal,
    pub deposit_time: u64,
    pub last_accrued: u64,
    pub unstake_time: Option<u64>,
}

impl fmt::Display for Deposit {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.user, self.amount)
    }
}

impl Deposit {
    pub fn equal(&self, deposits: &Vec<Deposit>) -> bool {
        let mut check = false;
        for deposit in deposits.iter() {
            if self.amount == deposit.amount && self.user == deposit.user && self.deposit_time == deposit.deposit_time{
                check = true;
            }
        }

        check
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct AssetPool {
    pub credit_asset: Asset,
    pub liq_premium: Decimal,
    pub deposits: Vec<Deposit>,
}

impl fmt::Display for AssetPool {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.credit_asset)
    }
}

//Liq-queue
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Queue {
    pub bid_asset: Asset,
    pub max_premium: Uint128, //A slot for each premium is created when queue is created
    pub slots: Vec<PremiumSlot>,
    pub current_bid_id: Uint128,
    pub bid_threshold: Uint256,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct BidInput {
    pub bid_for: AssetInfo,
    pub liq_premium: u8, //Premium within range of Queue
}

impl fmt::Display for BidInput {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.bid_for, self.liq_premium)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Bid {
    pub user: Addr,
    pub id: Uint128,
    pub amount: Uint256,
    pub liq_premium: u8,
    pub product_snapshot: Decimal256,
    pub sum_snapshot: Decimal256,
    pub pending_liquidated_collateral: Uint256,
    pub wait_end: Option<u64>,
    pub epoch_snapshot: Uint128,
    pub scale_snapshot: Uint128,
}

impl fmt::Display for Bid {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.user, self.amount)
    }
}

impl Bid {
    pub fn equal(&self, bids: &Vec<Bid>) -> bool {
        let mut check = false;
        for bid in bids.iter() {
            if self.amount == bid.amount && self.user == bid.user {
                check = true;
            }
        }

        check
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct User {
    //pub user: Addr,
    pub claimable_assets: Vec<Asset>, //Collateral assets earned from liquidations
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct PremiumSlot {
    pub bids: Vec<Bid>,
    pub liq_premium: Decimal256, //
    pub sum_snapshot: Decimal256,
    pub product_snapshot: Decimal256,
    pub total_bid_amount: Uint256,
    pub last_total: u64, //last time the bids have been totaled
    pub current_epoch: Uint128,
    pub current_scale: Uint128,
    pub residue_collateral: Decimal256,
    pub residue_bid: Decimal256,
}

///Staking////
///
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct StakeDeposit {
    pub staker: Addr,
    pub amount: Uint128,
    pub stake_time: u64,
    pub unstake_start_time: Option<u64>,
}

impl fmt::Display for StakeDeposit {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.staker, self.amount)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct FeeEvent {
    pub time_of_event: u64,
    pub fee: LiqAsset,
}

///////Oracle////////
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct AssetOracleInfo {
    pub basket_id: Uint128,
    pub osmosis_pools_for_twap: Vec<TWAPPoolInfo>,
    pub static_price: Option<Decimal>,
}

impl fmt::Display for AssetOracleInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "osmosis_pool: {:?}", self.osmosis_pools_for_twap)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct TWAPPoolInfo {
    pub pool_id: u64,
    pub base_asset_denom: String,
    pub quote_asset_denom: String,
}

impl fmt::Display for TWAPPoolInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "pool_id: {}, base_asset_denom: {}, quote_asset_denom: {}",
            self.pool_id, self.base_asset_denom, self.quote_asset_denom
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct StoredPrice {
    pub price: Decimal,
    pub last_time_updated: u64,
    pub price_vol_limiter: PriceVolLimiter,//(Time since save, price)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct PriceVolLimiter {
    pub price: Decimal,
    pub last_time_updated: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct PriceInfo {
    pub source: String, //Chain name, Oracle Address or static
    pub price: Decimal,
}

////////////////CDP///////////
///
///
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct cAsset {
    pub asset: Asset, //amount is 0 when adding to basket_contract config or initiator
    pub max_borrow_LTV: Decimal, //aka what u can bprrpw up to
    pub max_LTV: Decimal, //ie liquidation point
    pub rate_index: Decimal, //Rate index to smooth rate accrual
    // //Osmosis Pool Info to pull TWAP from
    // pub pool_info_for_price: TWAPPoolInfo,
    // //NOTE: AssetInfo denom for an Osmo LP is the shares_denom
    pub pool_info: Option<PoolInfo>, //if its an Osmosis LP add PoolInfo.
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct PoolInfo {
    pub pool_id: u64,
    //AssetInfo, Asset Decimal Places
    pub asset_infos: Vec<LPAssetInfo>, //Asset decimals (https://api-osmosis.imperator.co/tokens/v2/all)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct LPAssetInfo {
    pub info: AssetInfo,
    pub decimals: u64,
    pub ratio: Decimal,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Position {
    pub position_id: Uint128,
    pub collateral_assets: Vec<cAsset>,
    pub credit_amount: Uint128,
    pub basket_id: Uint128, 
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Basket {
    pub owner: Addr,
    pub basket_id: Uint128,
    pub current_position_id: Uint128,
    pub collateral_types: Vec<cAsset>,
    pub collateral_supply_caps: Vec<SupplyCap>, //Order needs to correlate to collateral_types order
    pub credit_asset: Asset, //Depending on type of token we use for credit this.info will be an Addr or denom (Cw20 or Native token respectively)
    pub credit_price: Decimal, //This is credit_repayment_price, not market price
    pub base_interest_rate: Decimal, //Enter as percent, 0.02
    pub liquidity_multiplier: Decimal, //liquidity_multiplier for debt caps
    pub desired_debt_cap_util: Decimal, //Enter as percent, 0.90
    pub pending_revenue: Uint128,
    pub credit_last_accrued: u64, //credit redemption price last_accrued
    pub rates_last_accrued: u64, //rate_index last_accrued
    pub oracle_set: bool, //If the credit oracle was set. Can't update repayment price without.
    pub negative_rates: bool, //Allow negative repayment interest or not
    //% difference btwn credit TWAP and repayment price before the interest changes
    //Set to 100 if you want to turn off the PID
    pub cpc_margin_of_error: Decimal,
    //Contracts
    pub liq_queue: Option<Addr>, //Each basket holds its own liq_queue contract
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct SupplyCap {
    pub asset_info: AssetInfo,
    pub current_supply: Uint128,
    pub debt_total: Uint128,
    pub supply_cap_ratio: Decimal,    
    pub lp: bool,
    //Optional debt cap ratio based on Stability Pool Liquidity
    //If None, debt cap is based on proportion of TVL
    pub stability_pool_ratio_for_debt_cap: Option<Decimal>,     
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct SellWallDistribution {
    pub distributions: Vec<(AssetInfo, Decimal)>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct UserInfo {
    pub basket_id: Uint128,
    pub position_id: Uint128,
    pub position_owner: String,
}

impl fmt::Display for UserInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "owner: {}, basket: {}, position: {}",
            self.position_owner, self.basket_id, self.position_id
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct InsolventPosition {
    pub insolvent: bool,
    pub position_info: UserInfo,
    pub current_LTV: Decimal,
    pub available_fee: Uint128,
}

////////Builder Vesting////////
///
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct VestingPeriod {
    pub cliff: u64,  //In days
    pub linear: u64, //In days
}

impl VestingPeriod {
    pub fn equal(&self, vesting_period: &VestingPeriod) -> bool {
        vesting_period.cliff == self.cliff && vesting_period.linear == self.linear
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Allocation {
    pub amount: Uint128,
    pub amount_withdrawn: Uint128,
    pub start_time_of_allocation: u64, //block time of allocation in seconds
    pub vesting_period: VestingPeriod, //In days
}

/////Debt Auction

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct RepayPosition {
    pub repayment: Uint128,
    pub position_info: UserInfo,
}

/////////Liquidity Check
///
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct LiquidityInfo {
    pub asset: AssetInfo,
    pub pool_ids: Vec<u64>,
}


//////////Possibly switching to cw-asset//////

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssetInfo {
    Token { address: Addr },
    NativeToken { denom: String },
}

impl fmt::Display for AssetInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AssetInfo::NativeToken { denom } => write!(f, "{}", denom),
            AssetInfo::Token { address } => write!(f, "{}", address),
        }
    }
}

impl AssetInfo {
    pub fn is_native_token(&self) -> bool {
        match self {
            AssetInfo::NativeToken { .. } => true,
            AssetInfo::Token { .. } => false,
        }
    }

    pub fn equal(&self, asset: &AssetInfo) -> bool {
        match self {
            AssetInfo::Token { address, .. } => {
                let self_addr = address;
                match asset {
                    AssetInfo::Token { address, .. } => self_addr == address,
                    AssetInfo::NativeToken { .. } => false,
                }
            }
            AssetInfo::NativeToken { denom, .. } => {
                let self_denom = denom;
                match asset {
                    AssetInfo::Token { .. } => false,
                    AssetInfo::NativeToken { denom, .. } => self_denom == denom,
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Asset {
    pub info: AssetInfo,
    pub amount: Uint128,
}

impl fmt::Display for Asset {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.amount, self.info)
    }
}

////////////////////Osmosis binding types

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, JsonSchema, Debug)]
pub struct Swap {
    pub pool_id: u64,
    pub denom_in: String,
    pub denom_out: String,
}

impl Swap {
    pub fn new(pool_id: u64, denom_in: impl Into<String>, denom_out: impl Into<String>) -> Self {
        Swap {
            pool_id,
            denom_in: denom_in.into(),
            denom_out: denom_out.into(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, JsonSchema, Debug)]
pub struct Step {
    pub pool_id: u64,
    pub denom_out: String,
}

impl Step {
    pub fn new(pool_id: u64, denom_out: impl Into<String>) -> Self {
        Step {
            pool_id,
            denom_out: denom_out.into(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, JsonSchema, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SwapAmount {
    In(Uint128),
    Out(Uint128),
}

impl SwapAmount {
    pub fn as_in(&self) -> Uint128 {
        match self {
            SwapAmount::In(x) => *x,
            _ => panic!("was output"),
        }
    }

    pub fn as_out(&self) -> Uint128 {
        match self {
            SwapAmount::Out(x) => *x,
            _ => panic!("was input"),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, JsonSchema, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SwapAmountWithLimit {
    ExactIn { input: Uint128, min_output: Uint128 },
    ExactOut { output: Uint128, max_input: Uint128 },
}

impl SwapAmountWithLimit {
    pub fn discard_limit(self) -> SwapAmount {
        match self {
            SwapAmountWithLimit::ExactIn { input, .. } => SwapAmount::In(input),
            SwapAmountWithLimit::ExactOut { output, .. } => SwapAmount::Out(output),
        }
    }
}
