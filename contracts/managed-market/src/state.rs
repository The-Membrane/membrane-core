use membrane::oracle::PriceResponse;

use cosmwasm_std::{Addr, Decimal, Uint128, Storage, QuerierWrapper, Env, StdResult, StdError};
use cosmwasm_schema::cw_serde;
use cw_storage_plus::{Item, Map};

use membrane::types::{ClaimTracker, UserPosition};
use membrane::managed_market::{Config, MarketParams};

use crate::ContractError;


#[cw_serde]
pub struct ContractVersion {
    /// contract is the crate name of the implementing contract, eg. `crate:cw20-base`
    /// we will use other prefixes for other languages, and their standard global namespacing
    pub contract: String,
    /// version is any string that this implementation knows. It may be simple counter "1", "2".
    /// or semantic version on release tags "v0.7.0", or some custom feature flag list.
    /// the only code that needs to understand the version parsing is code that knows how to
    /// migrate from the given contract (and is tied to it's implementation somehow)
    pub version: String,
}

#[cw_serde]
pub struct TokenRateAssurance {
    pub pre_btokens_per_one: Uint128,
}

#[cw_serde]
pub struct LiquidationPropagation {
    pub collateral_denom: String,
    pub position_owner: Addr,
    pub pre_liquidation_cdt_balance: Uint128,
}


pub const CONTRACT: Item<ContractVersion> = Item::new("contract_info");

pub const CONFIG: Item<Config> = Item::new("config");

pub const MARKET_PARAMS: Map<String, MarketParams> = Map::new("market_params");

pub const DEBT_VAULT_TOKEN: Item<Uint128> = Item::new("debt_vault_token");

pub const ACTIONS_PAUSED: Item<bool> = Item::new("actions_paused");
pub const LTV_RAMP_TIMER: Item<u64> = Item::new("ltv_ramp_timer");
pub const POSITIONS: Map<(Addr, String), UserPosition> = Map::new("user_position"); //(owner, collateral denom), position
pub const TOKEN_RATE_ASSURANCE: Item<TokenRateAssurance> = Item::new("token_rate_assurance");

pub const LIQUIDATION: Item<LiquidationPropagation> = Item::new("liquidation_propagation");
pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");
pub const CLAIM_TRACKER: Item<ClaimTracker> = Item::new("claim_tracker");




/////////
// pub const BASKET: Item<Basket> = Item::new("basket"); 
// pub const POSITIONS: Map<Addr, Vec<Position>> = Map::new("positions"); //owner, list of positions
// //Volatility Tracker
// pub const VOLATILITY: Map<String, CollateralVolatility> = Map::new("volatility");
// pub const STORED_PRICES: Map<String, StoredPrice> = Map::new("stored_prices");

// /// CDT redemption premium, opt-in mechanism.
// /// This is the premium that the user will pay to redeem their debt token.
// pub const REDEMPTION_OPT_IN: Map<u128, Vec<RedemptionInfo>> = Map::new("redemption_opt_in"); 

// /// Config ownership transfer

// //Reply State Propagations
// pub const WITHDRAW: Item<WithdrawPropagation> = Item::new("withdraw_propagation");
// pub const LIQUIDATION: Item<LiquidationPropagation> = Item::new("repay_propagation");
// pub const CLOSE_POSITION: Item<ClosePositionPropagation> = Item::new("close_position_propagation");
// //Intents
// pub const USER_INTENTS: Map<String, CDPUserIntents> = Map::new("user_intents");
