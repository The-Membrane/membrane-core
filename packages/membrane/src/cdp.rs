use cosmwasm_std::{Addr, Decimal, Uint128, StdResult, Api};
use cosmwasm_schema::cw_serde;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::{
    cAsset, Asset, AssetInfo, InsolventPosition, Position, PositionUserInfo,
    SupplyCap, MultiAssetSupplyCap, TWAPPoolInfo, UserInfo, PoolType, Basket, equal,
};

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: Option<String>,
    pub oracle_time_limit: u64, //in seconds until oracle failure is acceoted
    pub debt_minimum: Uint128,  //Debt minimum value per position
    pub liq_fee: Decimal,
    pub collateral_twap_timeframe: u64, //in minutes
    pub credit_twap_timeframe: u64,     //in minutes
    //Contracts
    pub stability_pool: Option<String>,
    pub dex_router: Option<String>,
    pub staking_contract: Option<String>,
    pub oracle_contract: Option<String>,
    pub osmosis_proxy: Option<String>,
    pub debt_auction: Option<String>,
    pub liquidity_contract: Option<String>,
    pub discounts_contract: Option<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    UpdateConfig(UpdateConfig),
    Deposit {
        position_id: Option<Uint128>, //If the user wants to create a new/separate position, no position id is passed
        position_owner: Option<String>,
    },
    //Increase debt by an amount or to a LTV
    IncreaseDebt {
        position_id: Uint128,
        amount: Option<Uint128>,
        LTV: Option<Decimal>,
        mint_to_addr: Option<String>,
    },
    Withdraw {
        position_id: Uint128,
        assets: Vec<Asset>,
        send_to: Option<String>, //If not the sender
    },
    Repay {
        position_id: Uint128,
        position_owner: Option<String>, //If not the sender
        send_excess_to: Option<String>, //If not the sender
    },
    LiqRepay {},
    Liquidate {
        position_id: Uint128,
        position_owner: String,
    },
    ClosePosition {
        position_id: Uint128,
        max_spread: Decimal,
        send_to: Option<String>,
    },
    Accrue { 
        position_owner: Option<String>, //Only Membrane contracts should be able to call for Positions they don't own
        position_id: Uint128
    },
    MintRevenue {
        send_to: Option<String>, 
        repay_for: Option<UserInfo>, //Repay for a position w/ the revenue
        amount: Option<Uint128>,
    },
    //Non-USD denominated baskets don't work due to the debt minimum
    CreateBasket {
        basket_id: Uint128,
        collateral_types: Vec<cAsset>,
        credit_asset: Asset, //Creates native denom for Asset
        credit_price: Decimal,
        base_interest_rate: Option<Decimal>,
        credit_pool_infos: Vec<PoolType>, //For liquidity measuring
        liquidity_multiplier_for_debt_caps: Option<Decimal>, //Ex: 5 = debt cap at 5x liquidity
        liq_queue: Option<String>,
    },
    EditBasket(EditBasket),
    EditcAsset {
        asset: AssetInfo,
        //Editables
        max_borrow_LTV: Option<Decimal>, //aka what u can borrow up to
        max_LTV: Option<Decimal>,        //ie liquidation point
    },
    //Callbacks; Only callable by the contract
    Callback(CallbackMsg),
}


// NOTE: Since CallbackMsg are always sent by the contract itself, we assume all types are already
// validated and don't do additional checks. E.g. user addresses are Addr instead of String
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CallbackMsg {
    BadDebtCheck {
        position_id: Uint128,
        position_owner: Addr,
    },
}


#[cw_serde]
pub enum QueryMsg {
    Config {},
    GetUserPositions {
        //All positions from a user
        user: String,
        limit: Option<u32>,
    },
    GetPosition {
        //Singular position
        position_id: Uint128,
        position_owner: String,
    },
    GetBasketPositions {
        //All positions in a basket
        start_after: Option<String>,
        limit: Option<u32>,
    },
    GetBasket { }, //Singular basket
    GetBasketDebtCaps { },
    GetBasketBadDebt { },
    GetPositionInsolvency {
        position_id: Uint128,
        position_owner: String,
    },
    GetCreditRate { },
    GetCollateralInterest { },
    //Used internally to test state propagation
    Propagation {},
}

#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub stability_pool: Option<Addr>,
    pub dex_router: Option<Addr>, //Apollo's router, will need to change msg types if the router changes
    pub staking_contract: Option<Addr>,
    pub osmosis_proxy: Option<Addr>,
    pub debt_auction: Option<Addr>,
    pub oracle_contract: Option<Addr>,
    pub liquidity_contract: Option<Addr>,
    pub discounts_contract: Option<Addr>,
    pub liq_fee: Decimal,               //Enter as percent, 0.01
    pub collateral_twap_timeframe: u64, //in minutes
    pub credit_twap_timeframe: u64,     //in minutes
    pub oracle_time_limit: u64, //in seconds until oracle failure is accepted. Think of it as how many blocks you allow the oracle to fail for.
    //Augment the rate of increase per % difference for the redemption rate
    pub cpc_multiplier: Decimal,
    //This needs to be large enough so that USDC positions are profitable to liquidate,
    //1-2% of liquidated debt (max -> borrow_LTV) needs to be more than gas fees assuming ~98% LTV.
    pub debt_minimum: Uint128, //Debt minimum value per position.
    //Debt Minimum multiplier for base debt cap
    //ie; How many users do we want at 0 credit liquidity?
    pub base_debt_cap_multiplier: Uint128,
    //Interest rate 2nd Slope multiplier
    pub rate_slope_multiplier: Decimal,
}

#[cw_serde]
pub struct UpdateConfig {
    pub owner: Option<String>,
    pub stability_pool: Option<String>,
    pub dex_router: Option<String>,
    pub osmosis_proxy: Option<String>,
    pub debt_auction: Option<String>,
    pub staking_contract: Option<String>,
    pub oracle_contract: Option<String>,
    pub liquidity_contract: Option<String>,
    pub discounts_contract: Option<String>,
    pub liq_fee: Option<Decimal>,
    pub debt_minimum: Option<Uint128>,
    pub base_debt_cap_multiplier: Option<Uint128>,
    pub oracle_time_limit: Option<u64>,
    pub credit_twap_timeframe: Option<u64>,
    pub collateral_twap_timeframe: Option<u64>,
    pub cpc_multiplier: Option<Decimal>,
    pub rate_slope_multiplier: Option<Decimal>,
}

impl UpdateConfig {
    pub fn update_config(
        self,
        api: &dyn Api,
        config: &mut Config,
    ) -> StdResult<()>{
        //Set Optionals
        if let Some(owner) = self.owner {
            config.owner = api.addr_validate(&owner)?;
        }
        if let Some(stability_pool) = self.stability_pool {
            config.stability_pool = Some(api.addr_validate(&stability_pool)?);
        }
        if let Some(dex_router) = self.dex_router {
            config.dex_router = Some(api.addr_validate(&dex_router)?);
        }
        if let Some(osmosis_proxy) = self.osmosis_proxy {
            config.osmosis_proxy = Some(api.addr_validate(&osmosis_proxy)?);
        }
        if let Some(debt_auction) = self.debt_auction {
            config.debt_auction = Some(api.addr_validate(&debt_auction)?);
        }
        if let Some(staking_contract) = self.staking_contract {
            config.staking_contract = Some(api.addr_validate(&staking_contract)?);
        }
        if let Some(oracle_contract) = self.oracle_contract {
            config.oracle_contract = Some(api.addr_validate(&oracle_contract)?);
        }
        if let Some(liquidity_contract) = self.liquidity_contract {
            config.liquidity_contract = Some(api.addr_validate(&liquidity_contract)?);
        }
        if let Some(discounts_contract) = self.discounts_contract {
            config.discounts_contract = Some(api.addr_validate(&discounts_contract)?);
        }
        if let Some(liq_fee) = self.liq_fee {
            config.liq_fee = liq_fee.clone();
        }
        if let Some(debt_minimum) = self.debt_minimum {
            config.debt_minimum = debt_minimum.clone();
        }
        if let Some(base_debt_cap_multiplier) = self.base_debt_cap_multiplier {
            config.base_debt_cap_multiplier = base_debt_cap_multiplier.clone();
        }
        if let Some(oracle_time_limit) = self.oracle_time_limit {
            config.oracle_time_limit = oracle_time_limit.clone();
        }
        if let Some(collateral_twap_timeframe) = self.collateral_twap_timeframe {
            config.collateral_twap_timeframe = collateral_twap_timeframe.clone();
        }
        if let Some(credit_twap_timeframe) = self.credit_twap_timeframe {
            config.credit_twap_timeframe = credit_twap_timeframe.clone();
        }
        if let Some(cpc_multiplier) = self.cpc_multiplier {
            config.cpc_multiplier = cpc_multiplier.clone();
        }
        if let Some(rate_slope_multiplier) = self.rate_slope_multiplier {
            config.rate_slope_multiplier = rate_slope_multiplier.clone();
        }
        Ok(())
    }
}

#[cw_serde]
pub struct EditBasket {
    pub added_cAsset: Option<cAsset>,
    pub liq_queue: Option<String>,
    pub credit_pool_infos: Option<Vec<PoolType>>, //For liquidity measuring
    pub liquidity_multiplier: Option<Decimal>,
    pub collateral_supply_caps: Option<Vec<SupplyCap>>,
    pub multi_asset_supply_caps: Option<Vec<MultiAssetSupplyCap>>,
    pub base_interest_rate: Option<Decimal>,
    pub credit_asset_twap_price_source: Option<TWAPPoolInfo>,
    pub negative_rates: Option<bool>, //Allow negative repayment interest or not
    pub cpc_margin_of_error: Option<Decimal>,
    pub frozen: Option<bool>,
    pub rev_to_stakers: Option<bool>,
}

impl EditBasket {    
    /// Use EditBasket to edit a Basket
    pub fn edit_basket(
        self,
        basket: &mut Basket,
        new_cAsset: cAsset,
        new_queue: Option<Addr>,
        oracle_set: bool,
    ) -> StdResult<()> {
        if self.clone().added_cAsset.is_some() {
            basket.collateral_types.push(new_cAsset.clone());
        }
        if self.clone().liq_queue.is_some() {
            basket.liq_queue = new_queue.clone();
        }
        if let Some(collateral_supply_caps) = self.clone().collateral_supply_caps {
            //Set new cap parameters
            for new_cap in collateral_supply_caps {
                if let Some((index, _cap)) = basket.clone().collateral_supply_caps
                    .into_iter()
                    .enumerate()
                    .find(|(_x, cap)| cap.asset_info.equal(&new_cap.asset_info))
                {
                    //Set supply cap ratio
                    basket.collateral_supply_caps[index].supply_cap_ratio = new_cap.supply_cap_ratio;
                    //Set stability pool based ratio
                    basket.collateral_supply_caps[index].stability_pool_ratio_for_debt_cap = new_cap.stability_pool_ratio_for_debt_cap;
                }
            }
        }
        if let Some(multi_asset_supply_caps) = self.clone().multi_asset_supply_caps {
            //Set new cap parameters
            for new_cap in multi_asset_supply_caps {
                if let Some((index, _cap)) = basket.clone().multi_asset_supply_caps
                    .into_iter()
                    .enumerate()
                    .find(|(_x, cap)| equal(&cap.assets, &new_cap.assets))
                {
                    //Set supply cap ratio
                    basket.multi_asset_supply_caps[index].supply_cap_ratio = new_cap.supply_cap_ratio;
                } else {
                    basket.multi_asset_supply_caps.push(new_cap);
                }
            }
        }
        if let Some(base_interest_rate) = self.clone().base_interest_rate {
            basket.base_interest_rate = base_interest_rate.clone();
        }
        if let Some(toggle) = self.clone().negative_rates {
            basket.negative_rates = toggle.clone();
        }
        if let Some(toggle) = self.clone().frozen {
            basket.frozen = toggle.clone();
        }
        if let Some(toggle) = self.clone().rev_to_stakers {
            basket.rev_to_stakers = toggle.clone();
        }
        if let Some(error_margin) = self.clone().cpc_margin_of_error {
            basket.cpc_margin_of_error = error_margin.clone();
        }
        //Set basket specific multiplier
        if let Some(multiplier) = self.clone().liquidity_multiplier {
            basket.liquidity_multiplier = multiplier.clone();
        }
        basket.oracle_set = oracle_set;

        Ok(())
    }
} 

// We define a custom struct for each query response
#[cw_serde]
pub struct PositionResponse {
    pub position_id: Uint128,
    pub collateral_assets: Vec<cAsset>,
    //Allows front ends to get ratios using the same oracles
    //Useful for users who want to deposit or withdraw at the current ratio
    pub cAsset_ratios: Vec<Decimal>,
    pub credit_amount: Uint128,
    pub basket_id: Uint128,
    pub avg_borrow_LTV: Decimal,
    pub avg_max_LTV: Decimal,
}

#[cw_serde]
pub struct PositionsResponse {
    pub user: String,
    pub positions: Vec<Position>,
}

#[cw_serde]
pub struct BadDebtResponse {
    pub has_bad_debt: Vec<(PositionUserInfo, Uint128)>,
}

#[cw_serde]
pub struct InsolvencyResponse {
    pub insolvent_positions: Vec<InsolventPosition>,
}

#[cw_serde]
pub struct InterestResponse {
    pub credit_interest: Decimal,
    pub negative_rate: bool,
}

#[cw_serde]
pub struct CollateralInterestResponse {
    pub rates: Vec<Decimal>,
}
