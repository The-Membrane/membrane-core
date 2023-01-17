use prost::Message;
use core::fmt;
use std::{str::FromStr, convert::TryFrom};

use crate::math::{Decimal256, Uint256};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Uint128, StdError};

use osmosis_std::types::cosmos::base::v1beta1::Coin;

//Stability Pool

#[cw_serde]
pub struct PositionUserInfo {
    pub position_id: Option<Uint128>,
    pub position_owner: Option<String>,
}

#[cw_serde]
pub struct LiqAsset {
    pub info: AssetInfo,
    pub amount: Decimal,
}

impl fmt::Display for LiqAsset {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.amount, self.info)
    }
}

#[cw_serde]
pub struct UserRatio {
    pub user: Addr,
    pub ratio: Decimal,
}

#[cw_serde]
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
    pub fn equal(&self, deposits: &[Deposit]) -> bool {
        let mut check = false;
        for deposit in deposits.iter() {
            if self.amount == deposit.amount && self.user == deposit.user && self.deposit_time == deposit.deposit_time{
                check = true;
            }
        }

        check
    }
}

#[cw_serde]
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
#[cw_serde]
pub struct Queue {
    pub bid_asset: Asset,
    pub max_premium: Uint128, //A slot for each premium is created when queue is created
    pub slots: Vec<PremiumSlot>,
    pub current_bid_id: Uint128,
    pub bid_threshold: Uint256,
}

#[cw_serde]
pub struct BidInput {
    pub bid_for: AssetInfo,
    pub liq_premium: u8, //Premium within range of Queue
}

impl fmt::Display for BidInput {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.bid_for, self.liq_premium)
    }
}

#[cw_serde]
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
    pub fn equal(&self, bids: &[Bid]) -> bool {
        let mut check = false;
        for bid in bids.iter() {
            if self.amount == bid.amount && self.user == bid.user {
                check = true;
            }
        }

        check
    }
}

#[cw_serde]
pub struct User {
    //pub user: Addr,
    pub claimable_assets: Vec<Asset>, //Collateral assets earned from liquidations
}

#[cw_serde]
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
#[cw_serde]
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

#[cw_serde]
pub struct FeeEvent {
    pub time_of_event: u64,
    pub fee: LiqAsset,
}

#[cw_serde]
pub struct StakeDistribution {
    pub rate: Decimal,
    pub duration: u64, //in days
}

#[cw_serde]
pub struct StakeDistributionLog {
    pub ownership_distribution: StakeDistribution,
    pub start_time: u64,
}

///////Oracle////////
#[cw_serde]
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

#[cw_serde]
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

#[cw_serde]
pub struct StoredPrice {
    pub price: Decimal,
    pub last_time_updated: u64,
    pub price_vol_limiter: PriceVolLimiter,//(Time since save, price)
}

#[cw_serde]
pub struct PriceVolLimiter {
    pub price: Decimal,
    pub last_time_updated: u64,
}

#[cw_serde]
pub struct PriceInfo {
    pub source: String, //Chain name, Oracle Address or static
    pub price: Decimal,
}

////////////////CDP///////////
///
///
#[cw_serde]
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

#[cw_serde]
pub struct PoolInfo {
    pub pool_id: u64,
    //AssetInfo, Asset Decimal Places
    pub asset_infos: Vec<LPAssetInfo>, //Asset decimals (https://api-osmosis.imperator.co/tokens/v2/all)
}

#[cw_serde]
pub struct LPAssetInfo {
    pub info: AssetInfo,
    pub decimals: u64,
    pub ratio: Decimal,
}

#[cw_serde]
pub struct Position {
    pub position_id: Uint128,
    pub collateral_assets: Vec<cAsset>,
    pub credit_amount: Uint128,
}

#[cw_serde]
pub struct Basket {
    pub basket_id: Uint128,
    pub current_position_id: Uint128,
    pub collateral_types: Vec<cAsset>,
    pub collateral_supply_caps: Vec<SupplyCap>, 
    pub multi_asset_supply_caps: Vec<MultiAssetSupplyCap>,
    pub credit_asset: Asset, 
    pub credit_price: Decimal, //This is credit_repayment_price, not market price
    pub base_interest_rate: Decimal, //Enter as percent, 0.02
    pub liquidity_multiplier: Decimal, //liquidity_multiplier for debt caps
    pub pending_revenue: Uint128,
    pub credit_last_accrued: u64, //credit redemption price last_accrued
    pub rates_last_accrued: u64, //rate_index last_accrued
    pub oracle_set: bool, //If the credit oracle was set. Can't update repayment price without.
    pub negative_rates: bool, //Allow negative repayment interest or not
    pub frozen: bool, //Freeze withdrawals and debt increases to provide time to fix vulnerabilities
    pub rev_to_stakers: bool,
    //% difference btwn credit TWAP and repayment price before the interest changes
    //Set to 100 if you want to turn off the PID
    pub cpc_margin_of_error: Decimal,
    //Contracts
    pub liq_queue: Option<Addr>, //Each basket holds its own liq_queue contract
}

#[cw_serde]
pub struct SupplyCap {
    pub asset_info: AssetInfo,
    pub current_supply: Uint128,
    pub debt_total: Uint128,
    pub supply_cap_ratio: Decimal,    
    pub lp: bool,
    //Toggle for a debt cap ratio based on Stability Pool Liquidity
    //If false, debt cap is based on proportion of TVL
    pub stability_pool_ratio_for_debt_cap: Option<Decimal>,     
}

#[cw_serde]
pub struct MultiAssetSupplyCap {
    pub assets: Vec<AssetInfo>,
    pub supply_cap_ratio: Decimal,
}

#[cw_serde]
pub struct DebtCap {
    pub collateral: AssetInfo,
    pub debt_total: Uint128,
    pub cap: Uint128,
}

#[cw_serde]
pub struct UserInfo {
    pub position_id: Uint128,
    pub position_owner: String,
}

impl fmt::Display for UserInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "owner: {}, position: {}",
            self.position_owner, self.position_id
        )
    }
}

#[cw_serde]
pub struct InsolventPosition {
    pub insolvent: bool,
    pub position_info: UserInfo,
    pub current_LTV: Decimal,
    pub available_fee: Uint128,
}

////////Builder Vesting////////
///
#[cw_serde]
pub struct VestingPeriod {
    pub cliff: u64,  //In days
    pub linear: u64, //In days
}

impl VestingPeriod {
    pub fn equal(&self, vesting_period: &VestingPeriod) -> bool {
        vesting_period.cliff == self.cliff && vesting_period.linear == self.linear
    }
}

#[cw_serde]
pub struct Recipient {
    pub recipient: Addr,
    pub allocation: Option<Allocation>,
    pub claimables: Vec<Asset>,
}

#[cw_serde]
pub struct Allocation {
    pub amount: Uint128,
    pub amount_withdrawn: Uint128,
    pub start_time_of_allocation: u64, //block time of allocation in seconds
    pub vesting_period: VestingPeriod, //In days
}

/////Debt Auction

#[cw_serde]
pub struct RepayPosition {
    pub repayment: Uint128,
    pub position_info: UserInfo,
}

#[cw_serde]
pub struct AuctionRecipient {
    pub amount: Uint128,
    pub recipient: Addr,
}

#[cw_serde]
pub struct Auction {
    pub remaining_recapitalization: Uint128,
    pub repayment_positions: Vec<RepayPosition>, //Repayment amount, Positions info
    pub send_to: Vec<AuctionRecipient>,
    pub auction_start_time: u64,
}

/////////Liquidity Check
///
#[cw_serde]
pub struct LiquidityInfo {
    pub asset: AssetInfo,
    pub pool_infos: Vec<PoolType>,
}

#[cw_serde]
pub enum PoolType {
    Balancer { pool_id: u64 },
    StableSwap { pool_id: u64 },
}

/////////Lockdrop
///
#[cw_serde]
pub struct LPPoolInfo {
    pub share_token: AssetInfo,
    pub pool_id: u64,
}

#[cw_serde]
pub struct DebtTokenAsset {
    pub info: AssetInfo,
    pub amount: Uint128,
    pub basket_id: Uint128,
}

///////Osmosis Proxy
#[cw_serde]
pub struct Owner {
    pub owner: Addr,
    pub total_minted: Uint128, //for CDP mints
    pub liquidity_multiplier: Option<Decimal>, //for CDP mints
    pub non_token_contract_auth: bool,
}
////////Launch/////////
#[cw_serde]
#[serde(rename_all = "snake_case")]
pub struct Lockdrop {
    pub locked_users: Vec<LockedUser>,
    pub num_of_incentives: Uint128,
    pub locked_asset: AssetInfo,    
    pub lock_up_ceiling: u64, //in days
    pub deposit_end: u64, //5 days
    pub withdrawal_end: u64, //2 days
}

#[cw_serde]
#[serde(rename_all = "snake_case")]
pub struct LockedUser {
    pub user: String,
    pub deposits: Vec<Lock>,
    pub total_tickets: Uint128,
    pub incentives_withdrawn: Uint128,
}

#[cw_serde]
#[serde(rename_all = "snake_case")]
pub struct Lock {
    pub deposit: Uint128,
    pub lock_up_duration: u64, //in days
}

////Discount Vault
#[cw_serde]
pub struct VaultUser {
    pub user: Addr,
    pub vaulted_lps: Vec<VaultedLP>,
}

#[cw_serde]
pub struct VaultedLP {
    pub gamm: AssetInfo,
    pub amount: Uint128,
    pub deposit_time: u64,
}

//////////Possibly switching to cw-asset//////

#[cw_serde]
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

pub fn equal(assets_1: &Vec<AssetInfo>, assets_2: &Vec<AssetInfo>) -> bool {

    if assets_1.len() != assets_2.len() {
        return false
    }

    for asset in assets_2{
        if assets_1.iter().find(|self_asset| asset.equal(self_asset)).is_none(){
           return false
        }
    }

    true
}

#[cw_serde]
pub struct Asset {
    pub info: AssetInfo,
    pub amount: Uint128,
}

impl fmt::Display for Asset {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.amount, self.info)
    }
}

////////////////////Osmosis-std types////////////////////
pub enum Pool {
    Balancer(osmosis_std::types::osmosis::gamm::v1beta1::Pool),
    StableSwap(osmosis_std::types::osmosis::gamm::poolmodels::stableswap::v1beta1::Pool),
}

impl Pool {
    pub fn into_pool_state_response(&self) -> PoolStateResponse {
        
        match self {
            Pool::Balancer(pool) => {
                PoolStateResponse { 
                    assets: pool.clone().pool_assets.into_iter().map(|pool_asset| pool_asset.token.unwrap_or_default()).collect::<Vec<Coin>>(), 
                    shares: pool.clone().total_shares.unwrap_or_default(),
                }
            },
            Pool::StableSwap(pool) => {
                PoolStateResponse { 
                    assets: pool.clone().pool_liquidity, 
                    shares: pool.clone().total_shares.unwrap_or_default(),
                }
            },
        }
    }
}

impl TryFrom<osmosis_std::shim::Any> for Pool {
    type Error = StdError;

    fn try_from(value: osmosis_std::shim::Any) -> Result<Self, Self::Error> {
        if let Ok(pool) = osmosis_std::types::osmosis::gamm::v1beta1::Pool::decode(value.value.as_slice()) {
            return Ok(Pool::Balancer(pool));
        }
        if let Ok(pool) = osmosis_std::types::osmosis::gamm::poolmodels::stableswap::v1beta1::Pool::decode(value.value.as_slice()) {
            return Ok(Pool::StableSwap(pool));
        }
        
        Err(StdError::ParseErr {
            target_type: "Pool".to_string(),
            msg: "Unmatched pool: must be either `Balancer` or `StableSwap`.".to_string(),
        })
    }
}

#[cw_serde]
#[serde(rename_all = "snake_case")]
pub struct PoolStateResponse {
    /// The various assets that be swapped. Including current liquidity.
    pub assets: Vec<Coin>,
    /// The number of lp shares and their amount
    pub shares: Coin,
}

impl PoolStateResponse {
    pub fn has_denom(&self, denom: &str) -> bool {
        self.assets.iter().any(|c| c.denom == denom)
    }

    pub fn lp_denom(&self) -> &str {
        &self.shares.denom
    }

    /// If I hold num_shares of the lp_denom, how many assets does that equate to?
    pub fn shares_value(&self, num_shares: impl Into<Uint128>) -> Vec<Coin> {
        let num_shares = num_shares.into();
        self.assets
            .iter()
            .map(|c| Coin {
                denom: c.denom.clone(),
                amount: (Uint128::from_str(&c.amount).unwrap() * num_shares / Uint128::from_str(&self.shares.amount).unwrap()).to_string(),
            })
            .collect()
    }
}

#[cw_serde]
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

#[cw_serde]
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

#[cw_serde]
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

#[cw_serde]
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
