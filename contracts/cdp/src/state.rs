use membrane::oracle::PriceResponse;

use cosmwasm_std::{to_json_binary, Addr, Coin, CosmosMsg, Decimal, Env, QuerierWrapper, StdError, StdResult, Storage, Uint128, WasmMsg};
use cosmwasm_schema::cw_serde;
use cw_storage_plus::{Item, Map};
use membrane::helpers::get_contract_balances;
use membrane::stability_pool_vault::calculate_base_tokens;

use membrane::types::{AffiliateData, cAsset, Asset, AssetInfo, Basket, IndividualCost, UserDeploymentIntents, Position, RedemptionInfo, StoredPrice, UserInfo};
use membrane::cdp::{Config, ExecuteMsg};

use crate::ContractError;
use crate::risk_engine::update_basket_tally;

const MAX_CDT_SUPPLY_ENTRIES: usize = 500;
const MAX_ORACLE_ENTRIES: usize = 100;

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

//This propogates liquidation info && state to reduce gas
#[cw_serde]
pub struct LiquidationPropagation {
    pub per_asset_repayment: Vec<Decimal>,//List of repayments
    pub liq_queue_repayment: Decimal, //LQ repayment
    pub stability_pool: Decimal, //SP repayment
    pub user_repay_amount: Decimal,
    pub positions_contract: Addr,
    pub sp_liq_fee: Decimal,
    pub cAsset_ratios: Vec<Decimal>, //these don't change during liquidation bc we liquidate based on the ratios
    pub cAsset_prices: Vec<PriceResponse>,
    pub target_position: Position,
    pub liquidated_assets: Vec<cAsset>, //List of assets liquidated for supply caps
    pub caller_fee_value_paid: Decimal,
    pub total_repaid: Decimal,
    pub position_owner: Addr,
    pub basket: Basket,
    pub config: Config,
}

#[cw_serde]
pub struct WithdrawPropagation {
    pub positions_prev_collateral: Vec<Asset>, //Amount of collateral in the position before the withdrawal
    pub withdraw_amounts: Vec<Uint128>,
    pub contracts_prev_collateral_amount: Vec<Uint128>,
    pub position_info: UserInfo,
}
#[cw_serde]
pub struct ClosePositionPropagation {
    pub withdrawn_assets: Vec<Asset>,
    pub position_info: UserInfo,
    pub send_to: Option<String>,
}

#[cw_serde]
pub struct SellCollateralPropagation {
    pub collateral_sold: Vec<Coin>,
    pub position_info: UserInfo,
}

#[cw_serde]
pub struct DeployableVenuePropagation {
    pub user: UserInfo,
    pub venues: Vec<String>,
}

#[cw_serde]
pub struct LiquidationStat {
    pub block_time: u64,
    pub position_id: Uint128,
    pub collateral_assets: Vec<Asset>,
    pub amount_liquidated: Uint128,
}
#[cw_serde]
pub struct Timer {
    pub start_time: u64,
    pub end_time: u64,
}
#[cw_serde]
pub struct CollateralVolatility {
    pub index: Decimal,
    /// Speed of volatility (price_change / time_elapsed) - used for index calculation
    pub volatility_list: Vec<Decimal>,
    /// Raw price change percentages - used for comparative rate calculation
    pub raw_volatility_list: Vec<Decimal>,
}

#[cw_serde]
pub struct CollateralRateAssurance {
    pub collateral_denom: String,
    pub pre_collateral_per_one: Uint128,
}

#[cw_serde]
pub struct SupplyTimestamp {
    pub supply: u64,
    pub timestamp: u64,
}

#[cw_serde]
pub struct PriceTimestamp {
    pub price: String,
    pub timestamp: u64,
}

/// Per-asset circuit breaker configuration and state
#[cw_serde]
pub struct AssetCircuitBreaker {
    /// Is the asset currently frozen due to abnormal price moves?
    pub frozen: bool,
    /// Timestamp (in seconds) when the asset was frozen
    pub frozen_at: u64,
    /// Allowed price deviation from the reference price before freezing
    /// Example: 0.10 = 10% deviation
    pub price_deviation_threshold: Decimal,
    /// Reference price used to measure deviation (typically a TWAP or recent average)
    pub reference_price: Option<Decimal>,
}

#[cw_serde]
pub struct LTVUpdateTracker {
    /// Timestamp of last upward accrual
    pub last_upward_update: u64,
    /// Staged downward max_LTV (waiting to be applied)
    pub staged_max_ltv: Option<Decimal>,
    /// Staged downward max_borrow_LTV (waiting to be applied)
    pub staged_max_borrow_ltv: Option<Decimal>,
    /// Timestamp when downward values were staged (timer starts here)
    pub staged_timestamp: Option<u64>,
}

pub const CONTRACT: Item<ContractVersion> = Item::new("contract_info");

pub const CONFIG: Item<Config> = Item::new("config");
pub const BASKET: Item<Basket> = Item::new("basket"); 
pub const POSITIONS: Map<Addr, Vec<Position>> = Map::new("positions"); //owner, list of positions
/// Affiliates are not handled during redemption. If this becomes a large sum of loss revenue, we will find a solution.
pub const AFFILIATES: Map<String, Vec<AffiliateData>> = Map::new("affiliations"); //position ID, list of affiliations
//Volatility Tracker
pub const VOLATILITY: Map<String, CollateralVolatility> = Map::new("volatility");
pub const STORED_PRICES: Map<String, StoredPrice> = Map::new("stored_prices");

/// CDT redemption premium, opt-in mechanism.
/// This is the premium that the user will pay to redeem their debt token.
pub const REDEMPTION_OPT_IN: Map<u128, Vec<RedemptionInfo>> = Map::new("redemption_opt_in"); 

/// Config ownership transfer
pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");

//Reply State Propagations
pub const WITHDRAW: Item<WithdrawPropagation> = Item::new("withdraw_propagation");
pub const LIQUIDATION: Item<LiquidationPropagation> = Item::new("repay_propagation");
pub const CLOSE_POSITION: Item<ClosePositionPropagation> = Item::new("close_position_propagation");
pub const SELL_COLLATERAL: Item<SellCollateralPropagation> = Item::new("sell_collateral_propagation");
pub const DEPLOYABLE_VENUE: Item<DeployableVenuePropagation> = Item::new("deployable_venue_propagation");
//////
pub const ACTIVE_DEPLOYMENT_VENUES: Item<Vec<String>> = Item::new("active_deployment_venues");
pub const LIQUIDATION_STATS: Item<Vec<LiquidationStat>> = Item::new("liquidation_stats");
//Freeze Timer
pub const FREEZE_TIMER: Item<Timer> = Item::new("freeze_timer");
//Intents
pub const USER_INTENTS: Map<String, UserDeploymentIntents> = Map::new("user_intents");

//Collateral Rate Assurance
pub const COLLATERAL_RATE_ASSURANCE: Map<String, CollateralRateAssurance> = Map::new("collateral_rate_assurance");

// CDT Supply Growth Tracker
pub const CDT_SUPPLY: Item<Vec<SupplyTimestamp>> = Item::new("cdt_supply");
/// Historical Oracle Price tracker
pub const HISTORICAL_ORACLE_PRICES: Map<String, Vec<PriceTimestamp>> = Map::new("historical_oracle"); //asset, price
/// LTV Update Trackers for dynamic LTV mechanism
pub const LTV_UPDATE_TRACKERS: Map<String, LTVUpdateTracker> = Map::new("ltv_update_trackers"); //asset_denom, tracker
/// Per-asset circuit breaker state
pub const ASSET_CIRCUIT_BREAKERS: Map<String, AssetCircuitBreaker> = Map::new("asset_circuit_breakers");

//Helper functions

/// Update CDT Supply Growth Tracker
pub fn update_historical_oracle(
    storage: &mut dyn Storage,
    env: Env,
    asset: String,
    price: String,
) -> StdResult<()> {
    let mut historicale = HISTORICAL_ORACLE_PRICES.may_load(storage, asset.clone())?.unwrap_or_else(|| vec![]);

    //If the price is the same as the last price, don't add it
    if historicale.len() > 0 && historicale.last().unwrap().price == price {
        return Ok(());
    } else {
        //Add new price
        historicale.push(PriceTimestamp {
            timestamp: env.block.time.seconds(),
            price,
        });
    }

    //Prune up to 100 .
    //Basic remove bc we polish per addition.
    if historicale.len() > MAX_ORACLE_ENTRIES {
        historicale.remove(0);
    }
    println!("historicale: {:?}", historicale);
    //Save new CDT Supply Growth Tracker
    HISTORICAL_ORACLE_PRICES.save(storage, asset, &historicale)?;
    Ok(())
}
/// Update CDT Supply Growth Tracker
pub fn update_cdt_supply(
    storage: &mut dyn Storage,
    env: Env,
    current_cdt_supply: Uint128,
) -> StdResult<()> {
    let mut cdt_supply = CDT_SUPPLY.load(storage)?;
    cdt_supply.push(SupplyTimestamp {
        timestamp: env.block.time.seconds(),
        supply: current_cdt_supply.u128() as u64,
    });

    //Prune up to 500 .
    //Basic remove bc we polish per addition.
    if cdt_supply.len() > MAX_CDT_SUPPLY_ENTRIES {
        cdt_supply.remove(0);
    }
    //Save new CDT Supply Growth Tracker
    CDT_SUPPLY.save(storage, &cdt_supply)?;
    Ok(())
}
/// Update asset claims a Position has
pub fn update_position_claims(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: Config,
    position_id: Uint128,
    position_owner: Addr,
    liquidated_asset: AssetInfo,
    liquidated_amount: Uint128,
) -> StdResult<()> {
    let mut credit_amount: Uint128 = Uint128::zero();

    let mut target_position = None;

    POSITIONS.update(
        storage,
        position_owner,
        |old_positions| -> StdResult<Vec<Position>> {
            if let Some(old_positions) = old_positions {
                let new_positions = old_positions
                    .into_iter()
                    .map(|mut position| {
                        //Find position
                        if position.position_id == position_id {
                            //Set target_position
                            target_position = Some(position.clone());
                            //Set credit_amount
                            credit_amount = position.credit_amount;

                            //Find asset in position
                            position.collateral_assets = position
                                .collateral_assets
                                .into_iter()
                                .map(|mut c_asset| {
                                    //Subtract amount liquidated from claims
                                    if c_asset.asset.info.equal(&liquidated_asset) {                                    
                                        c_asset.asset.amount -= liquidated_amount;
                                    }

                                    c_asset
                                })
                                .collect::<Vec<cAsset>>();
                        }
                        position
                    })
                    .collect::<Vec<Position>>();

                Ok(new_positions)
            } else {
                Err(StdError::GenericErr {
                    msg: String::from("Invalid position owner"),
                })
            }
        },
    )?;

    //Subtract liquidated amount from total asset tally
    let collateral_assets = vec![cAsset {
        asset: Asset {
            info: liquidated_asset,
            amount: liquidated_amount,
        },
        max_borrow_LTV: Decimal::zero(),
        max_LTV: Decimal::zero(),
        pool_info: None,
        rate_index: Decimal::one(),
        individual_cost: Some(IndividualCost {
            rate: Decimal::zero(),
            updater_address: None,
        }),
    }];

    //If there is no credit, basket tallies were updated in the repay function
    if credit_amount.is_zero() {
        return Ok(());
    }

    let mut basket = BASKET.load(storage)?;
    match update_basket_tally(storage, querier, env, &mut basket, collateral_assets, target_position.unwrap().collateral_assets, false, config, false) {
        Ok(_res) => {
            BASKET.save(storage, &basket)?;
        }
        Err(err) => {
            return Err(StdError::GenericErr {
                msg: err.to_string(),
            })
        }
    };

    Ok(())
}

/// Returns Position & index of Position in User's list
pub fn get_target_position(
    storage: &dyn Storage,
    valid_position_owner: Addr,
    position_id: Uint128,
) -> Result<(usize, Position), ContractError> {
    let positions: Vec<Position> = match POSITIONS.load(
        storage, valid_position_owner
    ){
        Err(_) => return Err(ContractError::NoUserPositions {}),
        Ok(positions) => positions,
    };

    match positions.into_iter().enumerate().find(|(_i, x)| x.position_id == position_id) {
        Some(position) => Ok(position),
        None => Err(ContractError::NonExistentPosition { id: position_id }),
    }
}


/// Replace Position data in state
pub fn update_position(
    storage: &mut dyn Storage,
    valid_position_owner: Addr,
    new_position: Position,
) -> StdResult<()>{

    POSITIONS.update(
        storage,
        valid_position_owner,
        |old_positions| -> StdResult<Vec<Position>> {
            match old_positions {
                Some(old_positions) => {
                    let new_positions = old_positions
                        .into_iter()
                        .map(|stored_position| {
                            //Find position
                            if stored_position.position_id == new_position.position_id {
                                //Swap to target_position 
                                new_position.clone()
                            } else {
                                //Don't override
                                stored_position
                            }
                        })
                        .collect::<Vec<Position>>();

                    Ok(new_positions)
                },
                None => {
                    Err(StdError::GenericErr {
                        msg: String::from("Invalid position owner"),
                    })
                }
            }
        },
    )?;

    Ok(())
}

/// Helper function to create collateral rate assurance for operations that modify collateral
pub fn create_collateral_rate_assurance(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    collateral_denoms: Vec<String>,
    basket: &Basket,
) -> StdResult<Vec<CosmosMsg>> {
    let mut valid_denoms = Vec::new();
    
    for denom in collateral_denoms {
        // Get current collateral balance
        let current_collateral = get_contract_balances(
            querier,
            env.clone(),
            vec![AssetInfo::NativeToken { denom: denom.clone() }]
        )?[0];
        
        // Get current collateral state total from basket
        let collateral_state_total = basket.collateral_supply_caps.iter()
            .find(|cap| cap.asset_info.to_string() == denom)
            .map(|cap| cap.current_supply)
            .unwrap_or(Uint128::zero());
        
        // Calculate current rate (collateral_per_state)
        let collateral_per_one = calculate_base_tokens(
            Uint128::new(1_000_000),
            current_collateral,
            collateral_state_total
        )?;
        
        // Check if rate assurance already exists for this denom
        if COLLATERAL_RATE_ASSURANCE.load(storage, denom.clone()).is_err() {
            // Create new rate assurance state
            COLLATERAL_RATE_ASSURANCE.save(storage, denom.clone(), &CollateralRateAssurance {
                collateral_denom: denom.clone(),
                pre_collateral_per_one: collateral_per_one,
            })?;
        } else {
            // Update existing rate assurance state
            COLLATERAL_RATE_ASSURANCE.save(storage, denom.clone(), &CollateralRateAssurance {
                collateral_denom: denom.clone(),
                pre_collateral_per_one: collateral_per_one,
            })?;
        }
        
        // Always add to valid denoms list for rate checking
        valid_denoms.push(denom);
    }
    
    // Create a single rate assurance callback message for all denoms
    let mut msgs = Vec::new();
    if !valid_denoms.is_empty() {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::CollateralRateAssurance {
                collateral_denoms: Some(valid_denoms),
            })?,
            funds: vec![],
        }));
    }
    
    Ok(msgs)
}
