use prost::Message;
use core::fmt;
use std::{str::FromStr, convert::TryFrom};

use crate::{math::{Decimal256, Uint256}, liq_queue::QueueResponse, oracle::PriceResponse};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Uint128, StdError, Timestamp};
use cw_coins::Coins;

use osmosis_std::types::osmosis::poolmanager::v1beta1::SwapAmountInRoute;
use osmosis_std::types::cosmos::base::v1beta1::Coin;

/// Stability Pool
#[cw_serde]
pub struct PositionUserInfo {
    /// Position ID
    pub position_id: Option<Uint128>,
    /// User address
    pub position_owner: Option<String>,
}

#[cw_serde]
pub struct LiqAsset {
    /// Asset info
    pub info: AssetInfo,
    /// Asset amount
    pub amount: Decimal,
}

impl fmt::Display for LiqAsset {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.amount, self.info)
    }
}

#[cw_serde]
pub struct UserRatio {
    /// Address
    pub user: Addr,
    /// Ratio
    pub ratio: Decimal,
}

#[cw_serde]
pub struct Deposit {
    /// User address
    pub user: Addr,
    /// Deposit amount
    pub amount: Decimal,
    /// Deposit time in seconds
    pub deposit_time: u64,
    /// Last accrued time in seconds
    pub last_accrued: u64,
    /// Unstake time in seconds
    pub unstake_time: Option<u64>,
}

impl fmt::Display for Deposit {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.user, self.amount)
    }
}

impl Deposit {
    /// Equality check for Deposit's amount/time/user
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
    /// Credit asset
    pub credit_asset: Asset,
    /// Liquidation premium
    pub liq_premium: Decimal,
    /// Asset deposits
    pub deposits: Vec<Deposit>,
}

impl fmt::Display for AssetPool {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.credit_asset)
    }
}

/// Liquidation Queue
#[cw_serde]
pub struct Queue {
    /// Bid for asset
    pub bid_asset: Asset,
    /// Max premium.
    /// A slot for each premium is created when queue is created.
    pub max_premium: Uint128,
    /// Premium slots
    pub slots: Vec<PremiumSlot>,
    /// Current bid ID
    pub current_bid_id: Uint128,
    /// Minimum bid amount in the queue before waiting period is set to 0. Threshold should be larger than the largest single liquidation amount.
    pub bid_threshold: Uint256,
}

impl Queue {
    pub fn into_queue_response(self) -> QueueResponse {
        QueueResponse {
            bid_asset: self.bid_asset,
            max_premium: self.max_premium,
            current_bid_id: self.current_bid_id,
            bid_threshold: self.bid_threshold,
        }
    }
}

#[cw_serde]
pub struct BidInput {
    /// Bid for asset
    pub bid_for: AssetInfo,
    /// Liquidation premium within range of Queue's max_premium
    pub liq_premium: u8,
}

impl fmt::Display for BidInput {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.bid_for, self.liq_premium)
    }
}

#[cw_serde]
pub struct Bid {
    /// Bidder address
    pub user: Addr,
    /// Bid ID
    pub id: Uint128,
    /// Bid amount
    pub amount: Uint256,
    /// Liquidation premium
    pub liq_premium: u8,
    /// Product snapshot
    pub product_snapshot: Decimal256,
    /// Sum snapshot
    pub sum_snapshot: Decimal256,
    /// Pending liquidated collateral
    pub pending_liquidated_collateral: Uint256,
    /// End of waiting period in seconds
    pub wait_end: Option<u64>,
    /// Epoch snapshot
    pub epoch_snapshot: Uint128,
    /// Scale snapshot
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
    /// Claimable assets earned from liquidations
    pub claimable_assets: Coins,
}

#[cw_serde]
pub struct PremiumSlot {
    /// Bids in the slot
    pub bids: Vec<Bid>,    
    /// Waiting bids in the slot
    pub waiting_bids: Vec<Bid>,
    /// Liquidation premium
    pub liq_premium: Decimal256,
    /// Sum snapshot
    pub sum_snapshot: Decimal256,
    /// Product snapshot
    pub product_snapshot: Decimal256,
    /// Total bid amount
    pub total_bid_amount: Uint256,
    /// Last time the bids have been totaled, in seconds
    pub last_total: u64, 
    /// Current epoch
    pub current_epoch: Uint128,
    /// Current scale
    pub current_scale: Uint128,
    /// Residue collateral
    pub residue_collateral: Decimal256,
    /// Residue bid
    pub residue_bid: Decimal256,
}

/// Staking
#[cw_serde]
pub struct StakeDeposit {
    /// Staker address
    pub staker: Addr,
    /// Amount of stake
    pub amount: Uint128,
    /// Time of stake in seconds
    pub stake_time: u64,
    /// Time of unstake in seconds
    pub unstake_start_time: Option<u64>,
    /// last_accrued time in seconds
    pub last_accrued: Option<u64>,
}

impl fmt::Display for StakeDeposit {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.staker, self.amount)
    }
}

#[cw_serde]
pub struct OldStakeDeposit {
    /// Staker address
    pub staker: Addr,
    /// Amount of stake
    pub amount: Uint128,
    /// Time of stake in seconds
    pub stake_time: u64,
    /// Time of unstake in seconds
    pub unstake_start_time: Option<u64>,
}

#[cw_serde]
pub struct Delegation {
    /// Delegate address
    pub delegate: Addr,
    /// Amount of stake
    pub amount: Uint128,
    /// Fluidity toggle
    /// true: delegation can be redelegated by the delegate
    pub fluidity: bool,
    /// Delegate voting power as well as commission
    pub voting_power_delegation: bool,
    /// Time of delegation in seconds
    pub time_of_delegation: u64,
    /// last_accrued time in seconds
    pub last_accrued: Option<u64>,
}

#[cw_serde]
pub struct OldDelegation {
    /// Delegate address
    pub delegate: Addr,
    /// Amount of stake
    pub amount: Uint128,
    /// Fluidity toggle
    /// true: delegation can be redelegated by the delegate
    pub fluidity: bool,
    /// Delegate voting power as well as commission
    pub voting_power_delegation: bool,
    /// Time of delegation in seconds
    pub time_of_delegation: u64,
}

#[cw_serde]
pub struct DelegationInfo {    
    /// Delegated stake
    pub delegated: Vec<Delegation>,
    /// Stake delagated to staker
    pub delegated_to: Vec<Delegation>,
    /// Commission %
    pub commission: Decimal,
}
#[cw_serde]
pub struct OldDelegationInfo {    
    /// Delegated stake
    pub delegated: Vec<OldDelegation>,
    /// Stake delagated to staker
    pub delegated_to: Vec<OldDelegation>,
    /// Commission %
    pub commission: Decimal,
}

#[cw_serde]
pub struct Delegate {
    /// Delegate address
    pub delegate: Addr,
    /// Alias
    pub alias: Option<String>,
    /// Discord username
    pub discord_username: Option<String>,
    /// Twitter username
    pub twitter_username: Option<String>,
    /// Some URL
    pub url: Option<String>,
}

#[cw_serde]
pub struct FeeEvent {
    /// Time of event in seconds
    pub time_of_event: u64,
    /// Fee asset    
    pub fee: LiqAsset,
}

#[cw_serde]
pub struct StakeDistribution {
    /// Distribution rate
    pub rate: Decimal,
    /// Duration of distribution in days
    pub duration: u64,
}

#[cw_serde]
pub struct StakeDistributionLog {
    /// Distribution strategy
    pub ownership_distribution: StakeDistribution,
    /// Distribution start time in seconds
    pub start_time: u64,
}

#[cw_serde]
pub struct VaultTokenInfo {
    /// Vault contract address
    pub vault_contract: String,
    /// Underlying token 
    pub underlying_token: String,
}

/// Oracle
#[cw_serde]
pub struct AssetOracleInfo {
    /// Basket ID
    pub basket_id: Uint128,
    /// Pyth price feed ID
    pub pyth_price_feed_id: Option<String>,
    /// Osmosis pools for OSMO TWAP
    pub pools_for_osmo_twap: Vec<TWAPPoolInfo>,
    /// Bool to provide $1 static_price if the asset is USD-par
    pub is_usd_par: bool,
    /// LP pool info
    pub lp_pool_info: Option<PoolInfo>,
    /// Vault Info (for vault tokens only)
    pub vault_info: Option<VaultTokenInfo>,
    /// Asset decimals
    pub decimals: u64,
}

impl fmt::Display for AssetOracleInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "osmo_pools: {:?}, is_usd_par: {:?}", self.pools_for_osmo_twap, self.is_usd_par)
    }
}

#[cw_serde]
pub struct TWAPPoolInfo {
    /// Pool ID
    pub pool_id: u64,
    /// Base asset denom
    pub base_asset_denom: String,
    /// Quote asset denom
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
    /// Price
    pub price: PriceResponse,
    /// Time of price in seconds
    pub last_time_updated: u64,
}

#[cw_serde]
pub struct PriceInfo {
    /// Source of price,
    /// Chain name, Oracle Address or static
    pub source: String, 
    /// Price
    pub price: Decimal,
}

/// CDP
#[cw_serde]
pub struct cAsset {
    /// Asset data
    /// NOTE: AssetInfo denom for an Osmo LP is the shares_denom
    pub asset: Asset,
    /// Max borrow limit, aka what u can borrow up to
    pub max_borrow_LTV: Decimal, 
    /// Liquidation LTV
    pub max_LTV: Decimal,
    /// Rate index to smooth rate accrual
    pub rate_index: Decimal, 
    /// Pool Info for Osmosis LP
    pub pool_info: Option<PoolInfo>,
    /// Is this subject to rate hikes?
    pub hike_rates: Option<bool>,
}

//intent state
#[cw_serde]
pub struct AssetPrice {
    pub asset: String,
    pub price: Decimal,
}
#[cw_serde]
pub struct IntentInitiations {
    pub initiation_ltv: Decimal,
    pub initiation_cost: Decimal,
    pub initiation_price: Vec<AssetPrice>
}

#[cw_serde]
pub struct LoopIntent {
    pub loop_to_ltv: Decimal,
    pub initiations: IntentInitiations,
}

#[cw_serde]
pub struct CloseIntent { //Unloop/Close
    pub percent_to_close: Decimal,
    pub initiations: IntentInitiations,
}

#[cw_serde]
/// Enter RB LP Vault Intent
pub struct DeploymentIntent {
    pub user: String,
    pub position_id: Uint128,
    pub ltv_to_mint: Decimal,
    pub destination: String, 
}


#[cw_serde]
pub struct LeaveTokens {
    pub percent_to_leave: Decimal,
    pub intent_for_tokens: RangeBoundUserIntents,
}


#[cw_serde]
pub struct UserDeploymentIntents { 
    pub user: String,
    pub deployment_intents: Vec<DeploymentIntent>,
}

/// Osmosis PoolInfo
#[cw_serde]
pub struct PoolInfo {
    /// Pool ID
    pub pool_id: u64,
    /// Asset Infos
    /// Includes asset decimals (https://api-osmosis.imperator.co/tokens/v2/all)
    pub asset_infos: Vec<LPAssetInfo>, 
}

#[cw_serde]
pub struct LPAssetInfo {
    /// Pool asset denom
    pub info: AssetInfo,
    /// Asset decimals
    pub decimals: u64,
    /// Asset ratio in pool
    pub ratio: Decimal,
}

#[cw_serde]
pub struct DeploymentVenue {
    /// Address
    pub address: Addr,
    /// Amount of debt deployed to this venue
    pub deployed_debt_amount: Uint128,
    //Failed liquidation.
    //Only way to set this to false once its true is to reset the venue by setting the intent.ltv_to_mint to 0 in CDP::SetIntents
    pub failed_liquidation: bool, 
}

#[cw_serde]
pub struct Position {
    /// Position ID
    pub position_id: Uint128,
    /// Collateral assets
    pub collateral_assets: Vec<cAsset>,
    /// Loan size
    pub credit_amount: Uint128,
    /// Deployed to
    pub deployed_to: Vec<DeploymentVenue>,
    /// Interest waiting to be paid.
    /// This allows us to attribute interest payments to the position & therefore users.
    pub pending_interest: Uint128,
    /// Total interest paid.
    /// Helps track profits & losses when combined with the deploymeny vaults.
    pub total_interest_accrued: Uint128,
}


#[cw_serde]
pub struct AffiliateData {
    /// Affiliate Address
    pub affiliate_address: String,
    /// Affiliate fee %
    pub affiliate_fee: Decimal,
    /// Affiliate label.
    /// Can be used to track marketing campaigns.
    pub label: Option<String>,
    /// Time affiliation started in block time.
    /// Resets on position repayment.
    pub time_affiliated: u64,
}

#[cw_serde]
pub struct RedemptionInfo {
    /// Position owner 
    pub position_owner: Addr,
    /// Position redemption info of the positions to be redeemed from
    pub position_infos: Vec<PositionRedemption>,
}

#[cw_serde]
pub struct PositionRedemption {
    /// Position ID of the position to be redeemed from
    pub position_id: Uint128,
    /// Remaining available loan repayment in debt tokens
    pub remaining_loan_repayment: Uint128,
    /// Restricted collateral assets.
    /// These aren't used for redemptions.
    pub restricted_collateral_assets: Vec<String>,
}

#[cw_serde]
pub struct PremiumInfo {
    /// Premium
    pub premium: u128,
    /// IDs in the Premium
    pub users_of_premium: Vec<RedemptionInfo>,
}

#[cw_serde]
pub struct Rate {
    /// Rate
    pub rate: Decimal,
    /// Time of rate in seconds
    pub last_time_updated: u64,
}

#[cw_serde]
pub struct Basket {
    /// Basket ID
    pub basket_id: Uint128,
    /// Position ID for next position
    pub current_position_id: Uint128,
    /// Available collateral types
    pub collateral_types: Vec<cAsset>,
    /// Collateral supply caps
    pub collateral_supply_caps: Vec<SupplyCap>, 
    /// Lastest Collateral Rates
    pub lastest_collateral_rates: Vec<Rate>,
    /// Multi collateral supply caps
    pub multi_asset_supply_caps: Vec<MultiAssetSupplyCap>,
    /// Credit asset object
    pub credit_asset: Asset, 
    /// Credit redemption price, not market price
    pub credit_price: PriceResponse,
    /// Base collateral interest rate.
    /// Enter as percent, 0.02 = 2%.
    pub base_interest_rate: Decimal,
    /// Pending revenue available to mint
    pub pending_revenue: PendingRevenue,
    /// Pending bad debt
    pub pending_bad_debt: Uint128,
    /// Last time credit price was updated, in seconds
    pub credit_last_accrued: u64,
    /// Last time rate indices for collateral_types was updated, in seconds
    pub rates_last_accrued: u64,
    /// True if the credit oracle was set. Can't update redemption price without it.
    pub oracle_set: bool, 
    /// Toggle to allow negative redemption rates
    pub negative_rates: bool, 
    /// Freeze withdrawals and debt increases to provide time to fix vulnerabilities
    pub frozen: bool, 
    /// Toggle to allow revenue to be distributed to the revenue_destinations.
    /// If false, revenue is left in pending_revenue.
    pub distribute_revenue: bool,
    /// % difference btwn credit TWAP and redemption price before the controller is effected.
    /// Set to 100 if you want to turn off the controller.
    pub cpc_margin_of_error: Decimal,
    /// Liquidation queue contract address
    pub liq_queue: Option<Addr>,
}

#[cw_serde]
pub struct PendingRevenue {
    /// Total pending CDT revenue
    pub total_pending: Uint128,
    /// Per-asset revenue allocation used for LTV Disco ratios
    pub per_asset_rev: Vec<Asset>,
}

impl std::fmt::Display for PendingRevenue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PendingRevenue {{ total_pending: {}, per_asset_rev: {:?} }}", 
               self.total_pending, self.per_asset_rev)
    }
}

#[cw_serde]
pub struct RevenueDestination {
    /// Revenue destination
    pub destination: Addr,
    /// Distribution ratio
    pub distribution_ratio: Decimal,
}

#[cw_serde]
pub struct SupplyCap {
    /// Asset info
    pub asset_info: AssetInfo,
    /// Current amount of asset in Basket
    pub current_supply: Uint128,
    /// Total debt collateralized by asset
    pub debt_total: Uint128,
    /// Total supply cap ratio
    pub supply_cap_ratio: Decimal,    
    /// is LP?
    pub lp: bool,
    /// Toggle for a debt cap ratio based on Stability Pool Liquidity.
    /// If false, debt cap is based on proportion of TVL.
    pub stability_pool_ratio_for_debt_cap: Option<Decimal>,     
}

#[cw_serde]
pub struct MultiAssetSupplyCap {
    /// Asset infos
    pub assets: Vec<AssetInfo>,
    /// Target supply cap ratio
    pub supply_cap_ratio: Decimal,
}

//Used for Query Responses
#[cw_serde]
pub struct DebtCap {
    /// Asset info
    pub collateral: AssetInfo,
    /// Total debt collateralized by asset
    pub debt_total: Uint128,
    /// Debt ceiling
    pub cap: Uint128,
}

#[cw_serde]
pub struct UserInfo {
    /// Position ID
    pub position_id: Uint128,
    /// Position owner
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
    /// Is insolvent?
    pub insolvent: bool,
    /// Position info
    pub position_info: UserInfo,
    /// Current LTV
    pub current_LTV: Decimal,
    /// Available liquidation fee
    pub available_fee: Uint128,
}

/// Builder Vesting
#[cw_serde]
pub struct VestingPeriod {
    /// Cliff period in days
    pub cliff: u64,
    /// Linear period in days
    pub linear: u64,
}

impl VestingPeriod {
    pub fn equal(&self, vesting_period: &VestingPeriod) -> bool {
        vesting_period.cliff == self.cliff && vesting_period.linear == self.linear
    }
}

#[cw_serde]
pub struct Recipient {
    /// Recipient address
    pub recipient: Addr,
    /// Allocation
    pub allocation: Option<Allocation>,
    /// Claimable assets
    pub claimables: Vec<Asset>,
}

#[cw_serde]
pub struct Allocation {
    /// Remaining amount of allocation
    pub amount: Uint128,
    /// Amount of asset withdrawn
    pub amount_withdrawn: Uint128,
    /// Start time of allocation in seconds
    pub start_time_of_allocation: u64, 
    /// Vesting period
    pub vesting_period: VestingPeriod,
}

/// Debt Auction
#[cw_serde]
pub struct AuctionRecipient {
    /// Amount
    pub amount: Uint128,
    /// Recipient address
    pub recipient: Addr,
}

#[cw_serde]
pub struct DebtAuction {
    /// Remaining debt to repay
    pub remaining_recapitalization: Uint128,
    /// Positions to repay
    pub repayment_positions: Vec<RepayPosition>,
    /// Capital recipients
    pub send_to: Vec<AuctionRecipient>,
    /// Auction start time
    pub auction_start_time: u64,
}

#[cw_serde]
pub struct FeeAuction {
    /// Remaining debt to repay
    pub auction_asset: Asset,
    /// Auction start time
    pub auction_start_time: u64,
}

#[cw_serde]
pub struct RepayPosition {
    /// Repayment amount
    pub repayment: Uint128,
    /// Position info
    pub position_info: UserInfo,
}

/// Liquidity Check
#[cw_serde]
pub struct LiquidityInfo {
    /// Asset info
    pub asset: AssetInfo,
    /// Pool info
    pub pool_infos: Vec<PoolType>,
}

#[cw_serde]
pub enum PoolType {
    /// Balancer pool
    Balancer { pool_id: u64 },
    /// Stableswap pool
    StableSwap { pool_id: u64 },
}

/// Lockdrop
#[cw_serde]
pub struct LPPoolInfo {
    /// LP share token asset info
    pub share_token: AssetInfo,
    /// Pool ID
    pub pool_id: u64,
}

#[cw_serde]
pub struct DebtTokenAsset {
    /// Asset info
    pub info: AssetInfo,
    /// Amount
    pub amount: Uint128,
    /// Basket ID
    pub basket_id: Uint128,
}

/// Osmosis Proxy
#[cw_serde]
pub struct Owner {
    /// Owner address
    pub owner: Addr,
    /// Total CDT minted (Unused)
    pub total_minted: Uint128,
    /// Stability pool ratio allocated to CDT mint caps
    pub stability_pool_ratio: Option<Decimal>,
    /// Authority over non-token contract messages
    pub non_token_contract_auth: bool,
    /// Is a position's contract?
    pub is_position_contract: bool,
}

#[cw_serde]
pub struct SwapRoute {
    pub token_in: String,
    pub route_out: SwapAmountInRoute,
}
/// Launch
#[cw_serde]
#[serde(rename_all = "snake_case")]
pub struct Lockdrop {
    /// Total number of incentives to distribute
    pub num_of_incentives: Uint128,
    /// Asset needed to lock
    pub locked_asset: AssetInfo,    
    /// Lock up ceiling, in days
    pub lock_up_ceiling: u64,
    /// Start time, for queries
    pub start_time: u64,
    /// End of the Deposit period window, in seconds
    pub deposit_end: u64,
    /// End of the Withdrawal period window, in seconds
    pub withdrawal_end: u64,
    /// Has the protocol launched?
    pub launched: bool,
}

#[cw_serde]
#[serde(rename_all = "snake_case")]
pub struct LockedUser {
    /// User address
    pub user: Addr,
    /// List of deposits
    pub deposits: Vec<Lock>,
    /// Total number of tickets, i.e. share of incentives distributed
    pub total_tickets: Uint128,
    /// Total number of incentives withdrawn
    pub incentives_withdrawn: Uint128,
}

#[cw_serde]
#[serde(rename_all = "snake_case")]
pub struct Lock {
    /// Deposit amount
    pub deposit: Uint128,
    /// Lock up duration, in days
    pub lock_up_duration: u64,
}

/// Discount Vault
#[cw_serde]
pub struct VaultUser {
    /// User address
    pub user: Addr,
    /// List of vaulted LPs
    pub vaulted_lps: Vec<VaultedLP>,
}

#[cw_serde]
pub struct VaultedLP {
    /// LP share token asset info
    pub gamm: AssetInfo,
    /// Amount of LP share tokens
    pub amount: Uint128,
    /// Deposit time
    pub deposit_time: u64,
}

//////////Possibly switching to cw-asset//////

#[cw_serde]
pub enum AssetInfo {
    /// Cw20 token
    Token { address: Addr },
    /// Native token
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
    /// Asset info
    pub info: AssetInfo,
    /// Amount
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
    /// Pool id
    pub pool_id: u64,
    /// Denom in
    pub denom_in: String,
    /// Denom out
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
    /// Pool id
    pub pool_id: u64,
    /// Denom out
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

///////////////VAULTS////////////////////
#[cw_serde]
pub struct VTClaimCheckpoint {
    pub vt_claim_of_checkpoint: Uint128,
    pub time_since_last_checkpoint: u64,
}

#[cw_serde]
pub struct ClaimTracker {
    pub vt_claim_checkpoints: Vec<VTClaimCheckpoint>,
    pub last_updated: u64,
}

#[cw_serde]
pub struct APR {
    pub apr: Decimal,
    pub negative: bool,
}

#[cw_serde]
pub struct DepositToken {
    pub deposit_token: String,
    pub decimal: u32,
}

/// Earn Vault
#[cw_serde]
pub struct VaultInfo {
    pub vault_addr: Addr,
    pub deposit_token: String,
    pub vault_token: String
}

/// Range Bound LP Vault
#[cw_serde]
pub struct RangeTokens {
    pub ceiling_deposit_token: String,
    pub floor_deposit_token: String
}

#[cw_serde]
pub struct RangeBounds {
    pub ceiling: RangeTicks,
    pub floor: RangeTicks,
}

#[cw_serde]
pub struct RangeTicks {
    pub lower_tick: i64,
    pub upper_tick: i64,
}

#[cw_serde]
pub struct RangePositions {
    pub ceiling: u64,
    pub floor: u64,
}
#[cw_serde]
pub struct IntentRoutes {
    pub cdt_route: Vec<SwapAmountInRoute>,
    pub usdc_route: Vec<SwapAmountInRoute>,
}

#[cw_serde]
pub struct PurchaseIntent {
    pub desired_asset: String,
    /// We don't limit the min amount for misc. asset routes (i.e. max slippage at 100%)
    pub route: Option<IntentRoutes>, //We don't use Osmosis Proxy for this
    /// Yield ditribution percent
    pub yield_percent: Decimal,
    /// If some we deposit into the position    
    pub position_id: Option<u64>,
    /// Slippage tolerance
    pub slippage: Option<Decimal>,
}

// #[cw_serde]
// pub struct RepayIntent { //Repay using VT tokens
//     pub position_id: u64,
//     pub percent_of_vt_to_repay: Decimal,
//     pub initiation_ltv: Decimal,
//     ///flat fee of deposit token (CDT)
//     pub fee: Uint128, 
// }

#[cw_serde]
pub struct RangeBoundUserIntents {
    pub user: String,
    pub last_conversion_rate: Uint128,
    pub purchase_intents: Vec<PurchaseIntent>,
}


#[cw_serde]
pub struct UserIntentState {
    pub vault_tokens: Uint128,
    pub intents: RangeBoundUserIntents,
    /// Unused until withdrawal period is added
    pub unstake_time: u64,
    ///Fee as a % of the yield
    pub fee_to_caller: Decimal, 
}

////////Managed Markets///////
 #[cw_serde]
pub struct BorrowOptions {
    pub amount: Option<Uint128>,
    pub ltv: Option<Decimal>,
}

#[cw_serde]
pub struct AutoCloseParams {
    pub ltv: Decimal,
    pub percent_to_close: Decimal,
    pub send_to: Option<String>,
    pub perpetual: bool,
}

#[cw_serde]
pub struct LoopLTVParams {
    pub loop_ltv: Decimal,
    pub perpetual: bool,
}

#[cw_serde]
pub struct PurchaseData {
    pub post_purchase_price: Decimal,
    pub amount_purchased: Uint128,
}


#[cw_serde]
pub struct UserHistory {
    pub collateral_denom: String,
    pub user: String,
    pub alias: Option<String>,
    pub profits: Decimal,
    pub losses: Decimal,
    pub volume: Decimal,
}

#[cw_serde] 
pub struct UXBoosts {
    pub collateral_value_fee_to_executor: Decimal, 
    pub loop_ltv: Option<LoopLTVParams>,
    pub take_profit_params: Option<AutoCloseParams>,
    pub stop_loss_params: Option<AutoCloseParams>,
    pub arb_price: Option<Decimal>,
    pub collateral_bought_from_loops: Vec<PurchaseData>
}

#[cw_serde]
pub struct UserPosition {
    pub collateral_denom: String, 
    pub collateral_amount: Uint128,
    pub debt_amount: Uint128,
    pub rate_index: Decimal,
}

//////////Points///////
/// 

#[cw_serde]
pub struct VaultMultiplier {
    ///Vault Address
    pub vault_address: String,
    //multiplier
    pub multiplier: Decimal,
}

///Multiplier for the points system
#[cw_serde]
pub struct PointsMultipliers {
    pub interest_rate: Decimal,
    pub vault_yields: Vec<VaultMultiplier>,
    pub liquidation_execution: Decimal,
    pub liquidation_claims: Decimal,
    pub governance_votes: Decimal,
} 

//Managed Market
#[cw_serde]
pub struct OsmosisOracleInfo {
    /// Pyth price feed ID
    pub pyth_price_feed_id: Option<String>,
    /// Osmosis pools for OSMO TWAP
    pub pools_for_osmo_twap: Vec<TWAPPoolInfo>,
    /// LP pool info
    pub lp_pool_info: Option<PoolInfo>,
    /// Vault Info (for vault tokens only)
    pub vault_info: Option<VaultTokenInfo>,
    /// Asset decimals
    pub decimals: u64,
}

impl fmt::Display for OsmosisOracleInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "osmo_pools: {:?}", self.pools_for_osmo_twap)
    }
}

#[cw_serde]
pub struct OsmosisRouteInfo {
    /// Osmosis pools for swap routing
    pub pools_for_osmo_twap: Vec<TWAPPoolInfo>,
}

/////RACING/////

// ===== SHARED TYPES =====

#[cw_serde]
pub struct CarMetadata {
    /// Name of the car
    pub name: String,
    /// Optional svg image data
    pub image_data: Option<String>,
    /// Optional list of car attributes/traits
    pub attributes: Option<Vec<CarAttribute>>,
    /// Optional on-chain car id (stringified), auto-populated by the contract
    pub car_id: Option<String>,
}

#[cw_serde]
pub struct CarAttribute {
    /// Type of the attribute (e.g., "Speed", "Handling", "Durability")
    pub trait_type: String,
    /// Value of the attribute (e.g., "High", "Medium", "Low")
    pub value: String,
}

#[cw_serde]
pub struct QTableEntry {
    /// Hash representing the state of the car
    pub state_hash:  [u8; 32],
    /// Q-values for all 4 actions [Up, Down, Left, Right]
    pub action_values: [i32; 4],
}

/// NEW: Integer-based Q-table entry for compressed state representation
#[cw_serde]
pub struct IntegerQTableEntry {
    /// Integer hash representing the state of the car (compressed from 32 bytes to 4 bytes)
    pub state_hash: u32,
    /// Q-values for all 4 actions [Up, Down, Left, Right] (compressed to i8)
    pub action_values: [i8; 4],
}



/// Conversion mapping from legacy byte array hashes to new integer hashes
#[cw_serde]
pub struct StateHashConversion {
    /// Legacy byte array hash
    pub legacy_hash: [u8; 32],
    /// New integer hash
    pub integer_hash: u32,
}

#[cw_serde]
pub enum RewardType {
    /// Distance-based reward with specific value
    Distance(i32),
    /// Penalty for getting stuck (negative reward)
    Stuck,
    /// Penalty for hitting a wall (negative reward)
    Wall,
    /// Penalty for no movement (negative reward)
    NoMove,
    /// Bonus for exploration (positive reward)
    Explore,
    /// Rank-based reward (0=1st place, 1=2nd place, etc.)
    Rank(u8),
}

#[cw_serde]
pub struct GoingBackward {
    pub penalty: i32,
    pub include_progress_towards_finish: bool,
}

#[cw_serde]
pub struct RewardNumbers {
    /// Distance-based reward with specific value
    pub distance: i32,
    /// Penalty for going backward (negative reward)
    pub going_backward: GoingBackward,
    /// Penalty for getting stuck (negative reward)
    pub stuck: i32,
    /// Penalty for hitting a wall (negative reward)
    pub wall: i32,
    /// Penalty for no movement (negative reward)
    pub no_move: i32,
    /// Bonus for exploration (positive reward)
    pub explore: i32,
    /// Rank-based reward (0=1st place, 1=2nd place, etc.)
    pub rank: RankReward,
}

#[cw_serde]
pub struct RankReward {
    pub first: i32,
    pub second: i32,
    pub third: i32,
    pub other: i32,
}

#[cw_serde]
pub struct TrackTrainingStats {
    /// Solo training statistics
    pub solo: TrainingStats,
    /// PvP training statistics
    pub pvp: TrainingStats,
}

#[cw_serde]
pub struct TrainingStats {
    /// Total number of training runs
    pub tally: u32,
    /// Win rate as a percentage (0-100)
    pub win_rate: u32,
    /// Fastest completion time in ticks
    pub fastest: u32,
    /// First time completion in ticks
    pub first_time: u32,
}

#[cw_serde]
pub struct QUpdate {
    /// Unique identifier for the car being trained
    pub car_id: String,
    /// Hash representing the current state of the car
    pub state_hash:  [u8; 32],
    /// Action taken (0=Up, 1=Down, 2=Left, 3=Right)
    pub action: u8,
    /// Type and value of reward received for this action
    pub reward_type: RewardType,
    /// Hash of the next state (None if terminal state)
    pub next_state_hash: Option< [u8; 32]>,
}

#[cw_serde]
pub struct TileProperties {
    /// Speed modifier (2 = normal, 1 = slow, 3 = boost, etc.)
    pub speed_modifier: u32,
    /// Whether this tile blocks movement
    pub blocks_movement: bool,
    /// Whether this tile causes the car to skip the next turn
    pub skip_next_turn: bool,
    /// Damage dealt to car when entering this tile (negative for healing)
    pub damage: i32,
    /// Whether this tile is a finish line
    pub is_finish: bool,
    /// Whether this tile is a start line
    pub is_start: bool,
}

impl Default for TileProperties {
    fn default() -> Self {
        Self {
            speed_modifier: 1, 
            blocks_movement: false,
            skip_next_turn: false,
            damage: 0,
            is_finish: false,
            is_start: false,
        }
    }
}

impl TileProperties {
    /// Create a normal tile
    pub fn normal() -> Self {
        Self {
            ..Default::default()
        }
    }

    /// Check if this tile is empty (default properties)
    pub fn is_empty(&self) -> bool {
        self.speed_modifier == 1 &&
        !self.blocks_movement &&
        !self.skip_next_turn &&
        self.damage == 0 &&
        !self.is_finish &&
        !self.is_start
    }

    /// Create a compressed representation for empty tiles
    /// Returns None for empty tiles, Some(self) for non-empty tiles
    pub fn compress(self) -> Option<Self> {
        if self.is_empty() {
            None
        } else {
            Some(self)
        }
    }

    /// Decompress a tile, returning default empty tile if None
    pub fn decompress(compressed: Option<Self>) -> Self {
        compressed.unwrap_or_default()
    }

    /// Create a boost tile
    pub fn boost(speed_modifier: u32) -> Self {
        Self {
            speed_modifier,
            ..Default::default()
        }
    }

    //No more slow tiles bc normal speed is 1
    /// Create a slow tile
    // pub fn slow(speed_modifier: u32) -> Self {
    //     Self {
    //         speed_modifier,
    //         ..Default::default()
    //     }
    // }

    /// Create a sticky tile
    pub fn sticky() -> Self {
        Self {
            skip_next_turn: true,
            ..Default::default()
        }
    }

    /// Create a wall tile
    pub fn wall() -> Self {
        Self {
            blocks_movement: true,
            ..Default::default()
        }
    }

    /// Create a finish tile
    pub fn finish() -> Self {
        Self {
            is_finish: true,
            ..Default::default()
        }
    }

    /// Create a start tile
    pub fn start() -> Self {
        Self {
            is_start: true,
            ..Default::default()
        }
    }

    /// Create a damage tile (e.g., spikes)
    pub fn damage(damage_amount: i32) -> Self {
        Self {
            damage: damage_amount,
            ..Default::default()
        }
    }

    /// Create a healing tile
    pub fn healing() -> Self {
        Self {
            damage: -1,
            ..Default::default()
        }
    }
}

#[cw_serde]
pub struct TrackTile {
    /// Properties of the track tile
    pub properties: TileProperties,
    /// Progress towards the finish line in positions
    pub progress_towards_finish: u16,
    /// For starting tiles only: minimum steps from this start to any finish
    /// Used for PvP fairness checks. None for non-start tiles.
    pub min_steps_to_finish_from_start: Option<u16>,
    /// x position of the tile
    pub x: u8,
    /// y position of the tile
    pub y: u8,
}

/// Compressed track layout for efficient storage
/// Uses Option<TileProperties> where None represents empty tiles
#[cw_serde]
pub struct CompressedTrackLayout {
    /// Width of the track
    pub width: u8,
    /// Height of the track  
    pub height: u8,
    /// Compressed layout: None = empty tile, Some = actual tile properties
    pub compressed_layout: Vec<Vec<Option<TileProperties>>>,
}

/// Compressed track with metadata for efficient storage
#[cw_serde]
pub struct CompressedTrack {
    /// Creator of the track
    pub creator: String,
    /// Name of the track
    pub name: String,
    /// Compressed layout
    pub layout: CompressedTrackLayout,
}

impl CompressedTrackLayout {
    /// Create compressed layout from full layout
    pub fn from_full_layout(layout: &Vec<Vec<TileProperties>>) -> Self {
        let height = layout.len() as u8;
        let width = if height > 0 { layout[0].len() as u8 } else { 0 };
        
        let compressed_layout = layout.iter()
            .map(|row| row.iter().map(|tile| tile.clone().compress()).collect())
            .collect();
            
        Self {
            width,
            height,
            compressed_layout,
        }
    }
    
    /// Expand compressed layout to full layout for race simulation
    pub fn to_full_layout(&self) -> Vec<Vec<TileProperties>> {
        self.compressed_layout.iter()
            .map(|row| row.iter().map(|compressed| TileProperties::decompress(compressed.clone())).collect())
            .collect()
    }
    
    /// Get tile properties at specific coordinates
    pub fn get_tile(&self, x: usize, y: usize) -> TileProperties {
        if y < self.compressed_layout.len() && x < self.compressed_layout[y].len() {
            TileProperties::decompress(self.compressed_layout[y][x].clone())
        } else {
            TileProperties::default()
        }
    }
}

impl CompressedTrack {
    /// Create compressed track from full track data
    pub fn from_track_data(creator: String, name: String, layout: &Vec<Vec<TileProperties>>) -> Self {
        Self {
            creator,
            name,
            layout: CompressedTrackLayout::from_full_layout(layout),
        }
    }
    
    /// Expand to full Track for race engine compatibility
    pub fn to_track(&self, track_id: u128, fastest_tick_time: u64, starting_tiles: Vec<TrackTile>) -> Track {
        let full_layout = self.layout.to_full_layout();
        
        // Convert to TrackTile format with coordinates
        let mut track_layout = vec![];
        for (y, row) in full_layout.iter().enumerate() {
            let mut track_row = vec![];
            for (x, properties) in row.iter().enumerate() {
                track_row.push(TrackTile {
                    properties: properties.clone(),
                    progress_towards_finish: 0, // Will be calculated by race engine
                    min_steps_to_finish_from_start: None,
                    x: x as u8,
                    y: y as u8,
                });
            }
            track_layout.push(track_row);
        }
        
        Track {
            creator: self.creator.clone(),
            id: track_id,
            name: self.name.clone(),
            width: self.layout.width,
            height: self.layout.height,
            layout: track_layout,
            fastest_tick_time,
            starting_tiles,
        }
    }
}

#[cw_serde]
pub struct Track {
    /// Creator of the track
    pub creator: String,    
    /// Unique identifier for the track
    pub id: u128,
    /// Name of the track
    pub name: String,
    /// Width of the track in tiles
    pub width: u8,
    /// Height of the track in tiles
    pub height: u8,
    /// 2D layout of the track with tile information (expanded from compressed storage)
    pub layout: Vec<Vec<TrackTile>>,
    /// Fastest possible tick time 
    pub fastest_tick_time: u64,
    /// Number of starting tiles
    pub starting_tiles: Vec<TrackTile>,
}


#[cw_serde]
pub enum TournamentCriteria {
    /// Random selection of cars
    Random,
    /// Top trained cars with minimum training updates
    TopTrained { 
        /// Minimum number of training updates required
        min_training_updates: u32 
    },
    /// All cars participate
    AllCars,
}

#[cw_serde]
pub enum TournamentStatus {
    /// Tournament has not started yet
    NotStarted,
    /// Tournament is currently in progress
    InProgress,
    /// Tournament has completed
    Completed,
}

#[cw_serde]
pub struct TournamentMatch {
    /// Unique identifier for the match
    pub match_id: String,
    /// First car in the match
    pub car1: u128,
    /// Second car in the match
    pub car2: u128,
    /// Winner of the match (None if not completed)
    pub winner: Option<u128>,
    /// Whether the match has been completed
    pub completed: bool,
}

#[cw_serde]
pub struct TournamentResult {
    /// Unique identifier for the car
    pub car_id: u128,
    /// Final rank in the tournament
    pub rank: u32,
    /// Number of wins in the tournament
    pub wins: u32,
    /// Number of losses in the tournament
    pub losses: u32,
}

#[cw_serde]
pub struct TournamentRanking {
    /// Unique identifier for the car
    pub car_id: u128,
    /// Final rank in the tournament
    pub rank: u32,
    /// Number of wins in the tournament
    pub wins: u32,
    /// Number of losses in the tournament
    pub losses: u32,
}


/// Strategies for selecting actions during training or racing
#[cw_serde]
pub enum ActionSelectionStrategy {
    Best,                       // Exploit: highest Q-value
    Random,                     // Pure exploration
    EpsilonGreedy(f32),         // Exploration with ε chance
    Softmax(f32),               // Probabilistic based on Q-values
    EpsilonDecay {              // Epsilon that decays over training progress
        initial_epsilon: f32,   // Starting epsilon value
        final_epsilon: f32,     // Final epsilon value
        current_tick: u32,      // Current training tick
        total_ticks: u32,       // Total training ticks
    },
}

// ===== RPS TYPES =====
#[cw_serde]
pub struct RpsRewardConfig {
    pub win_points: i64,
    pub lose_penalty: i64,
    pub draw_points: i64,
    pub series_win_points: i64,
}

#[cw_serde]
// #[serde(rename_all = "snake_case")]
pub enum SeriesMode {
    FixedTicks { ticks: u32 },
    BestOf { wins_target: u32 },
}


#[cw_serde]
pub struct WinLossDraw {
    pub win: u8,
    pub loss: u8,
    pub draw: u8,
}
// Top times per track
#[cw_serde]
pub struct TopTimeEntry {
    pub car_id: u128,
    pub time: u16,
}

#[cw_serde]
pub struct TopTimes {
    pub entries: Vec<TopTimeEntry>,
    pub highest: Option<TopTimeEntry>,
    pub highest_index: Option<u16>,
    pub car_index: std::collections::BTreeMap<u128, u16>,
}

// Brain Progress Tracking Types
#[cw_serde]
pub struct BrainProgressEntry {
    /// Block timestamp when this entry was recorded
    pub timestamp: Timestamp,
    /// Number of unique states seen (out of 625 possible)
    pub states_seen: u16,
    /// Average confidence across all seen states (0-100)
    pub avg_confidence: u8,
    /// Number of preferred actions that lead to walls during this training session
    pub wall_collisions: u16,
}

// Legacy struct for migration
#[cw_serde]
pub struct BrainProgressLegacy {
    /// Historical progress entries (limited to keep storage low)
    pub entries: Vec<BrainProgressEntry>,
    /// Total unique states ever seen across all training
    pub total_states_seen: u16,
    /// Current average confidence across all states
    pub current_avg_confidence: u8,
    /// Total preferred actions into walls across all training
    pub total_wall_collisions: u32,
}

#[cw_serde]
pub struct BrainProgress {
    /// Historical progress entries (limited to keep storage low)
    pub entries: Vec<BrainProgressEntry>,
    /// Total unique states ever seen across all training (deprecated - use latest entry)
    pub total_states_seen: Option<u16>,
    /// Current average confidence across all states (deprecated - use latest entry)
    pub current_avg_confidence: Option<u8>,
    /// Total preferred actions into walls across all training (deprecated - use latest entry)
    pub total_wall_collisions: Option<u32>,
}

impl Default for BrainProgress {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            total_states_seen: None,
            current_avg_confidence: None,
            total_wall_collisions: None,
        }
    }
}

impl From<BrainProgressLegacy> for BrainProgress {
    fn from(legacy: BrainProgressLegacy) -> Self {
        Self {
            entries: legacy.entries,
            total_states_seen: None,
            current_avg_confidence: None,
            total_wall_collisions: None,
        }
    }
}


// A single tick (round) record for a car within a series
#[cw_serde]
pub struct TickRecord {
    pub my_action: u8,
    pub opp_action: u8,
    pub outcome: u8, // 0 = lose, 1 = draw, 2 = win
}

#[cw_serde]
pub struct StringEntry {
    pub entry: String,
    pub remove: bool,
}

/// Neutron Proxy
#[cw_serde]
pub struct NeutronOwner {
    /// Owner address
    pub owner: Addr,
    /// Authority over non-token contract messages
    pub non_token_contract_auth: bool,
}

// Neutron Oracle

#[cw_serde]
pub struct NeutronOracleInfo {
    /// Bool to provide $1 static_price if the asset is USD-par
    pub is_usd_par: bool,
    /// Asset decimals
    pub decimals: u64,
    /// Slinky base symbol for price queries
    pub slinky_base_symbol: Option<String>,
    /// Maximum number of blocks that Slinky price can be old
    pub slinky_max_blocks_old: Option<u8>,
}

//LTV Disco
#[cw_serde]
pub struct DepositDenom {
    /// Denom 
    pub denom: String,
    /// Vault Info (for vault tokens only)
    pub vault_info: Option<VaultTokenInfo>,
}

//System Discounts
#[cw_serde]
pub struct TimedDiscountPeriod {
    /// Start time of the discount period (Unix timestamp)
    pub start_time: u64,
    /// End time of the discount period (Unix timestamp)
    pub end_time: u64,
    /// Discount
    pub discount: Decimal,
}
