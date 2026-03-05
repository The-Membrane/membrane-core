use std::cmp::min;
use std::str::FromStr;

use cosmwasm_std::{
    attr, entry_point, to_binary, to_json_binary, Addr, Binary, Decimal, Deps, DepsMut, Env, MessageInfo, QuerierWrapper, QueryRequest, Response, StdError, StdResult, Uint128, WasmQuery
};
use cw2::set_contract_version;

use membrane::helpers::query_basket;
use osmosis_std::shim::Duration;
use osmosis_std::types::osmosis::lockup::{LockupQuerier, AccountLockedLongerDurationDenomResponse};

use membrane::math::{decimal_division, decimal_multiplication};
use membrane::system_discounts::{Config, ExecuteMsg, InstantiateMsg, QueryMsg, UpdateConfig, UserDiscountResponse, UserBoostResponse, IntentBoostsResponse, StableBackingDiscountsResponse, MigrateMsg};
use membrane::acquisition::MbrnIntentOption;
use membrane::transmuter::QueryMsg as Transmuter_QueryMsg;
use membrane::stability_pool::QueryMsg as SP_QueryMsg;
use membrane::staking::{QueryMsg as Staking_QueryMsg, Config as Staking_Config, StakerResponse, RewardsResponse};
use membrane::discount_vault::{QueryMsg as Discount_QueryMsg, UserResponse as Discount_UserResponse};
use membrane::cdp::{BasketPositionsResponse, QueryMsg as CDP_QueryMsg};
use membrane::oracle::{QueryMsg as Oracle_QueryMsg, PriceResponse};
use membrane::ltv_disco::{QueryMsg as LTVDisco_QueryMsg, AllUserDepositsResponse};
use membrane::types::Locked;
use membrane::types::{AssetInfo, AssetPool, Basket, Deposit, TimedDiscountPeriod};

use crate::error::ContractError;
use crate::state::{CONFIG, OWNERSHIP_TRANSFER, STATIC_DISCOUNTS, TIMED_DISCOUNT_PERIOD};

// Contract name and version used for migration.
const CONTRACT_NAME: &str = "system_discounts";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Constants
const SECONDS_PER_DAY: u64 = 86_400u64;

// Time deposited and locked time are additive so once the user gets to the max discount time, they'll have no reason to continue locking. This leaves room for incentive improvement.

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let mut config: Config;
    let owner = if let Some(owner) = msg.owner {
        deps.api.addr_validate(&owner)?
    } else {
        info.sender
    };

    ///Query mbrn_denom
    let staking_contract = deps.api.addr_validate(&msg.staking_contract)?;

    let mbrn_denom = deps.querier.query::<Staking_Config>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: staking_contract.to_string(),
        msg: to_binary(&Staking_QueryMsg::Config {})?,
    }))?
    .mbrn_denom;

    // Validate max_discount and max_boost if provided
    let max_discount = msg.max_discount.unwrap_or(Decimal::one());
    let max_boost = msg.max_boost.unwrap_or(Decimal::percent(9));
    let mbrn_at_max_discount = msg.mbrn_at_max_discount.unwrap_or(Uint128::new(100_000_000_000u128));
    
    // Validate max_discount <= 1.0
    if max_discount > Decimal::one() {
        return Err(ContractError::CustomError { 
            val: "max_discount cannot exceed 1.0 (100%)".to_string() 
        });
    }

    // Default stable backing discount constants
    let stable_backing_max_discount = msg.stable_backing_max_discount.unwrap_or(Decimal::percent(75));
    let stable_backing_first_month_discount = msg.stable_backing_first_month_discount.unwrap_or(Decimal::percent(45)); // 60% of 75%
    let stable_backing_remaining_discount = msg.stable_backing_remaining_discount.unwrap_or(Decimal::percent(30)); // 40% of 75%
    let stable_backing_curve_duration_days = msg.stable_backing_curve_duration_days.unwrap_or(90u64); // 3 months
    let stable_backing_first_month_days = msg.stable_backing_first_month_days.unwrap_or(30u64);
    let stable_backing_discountable_debt_multiplier = msg.stable_backing_discountable_debt_multiplier.unwrap_or(18u64);
    let stable_backing_transmuter_balance_multiplier = msg.stable_backing_transmuter_balance_multiplier.unwrap_or(Decimal::percent(200)); // 2x

    // Validate stable backing discount <= 1.0
    if stable_backing_max_discount > Decimal::one() {
        return Err(ContractError::CustomError { 
            val: "stable_backing_max_discount cannot exceed 1.0 (100%)".to_string() 
        });
    }

    config = Config {
        owner,
        mbrn_denom,
        positions_contract: deps.api.addr_validate(&msg.positions_contract)?,
        oracle_contract: deps.api.addr_validate(&msg.oracle_contract)?,
        staking_contract,
        lockdrop_contract: None,
        discount_vault_contract: vec![],
        ltv_disco_contract: None,
        transmuter_contract: None,
        minimum_time_in_network: msg.minimum_time_in_network,
        max_discount,
        mbrn_at_max_discount,
        max_boost,
        stable_backing_max_discount,
        stable_backing_first_month_discount,
        stable_backing_remaining_discount,
        stable_backing_curve_duration_days,
        stable_backing_first_month_days,
        stable_backing_discountable_debt_multiplier,
        stable_backing_transmuter_balance_multiplier,
    };
    //Store optionals
    if let Some(lockdrop_contract) = msg.lockdrop_contract{
        config.lockdrop_contract = Some(deps.api.addr_validate(&lockdrop_contract)?);
    }
    if let Some(discount_vault_contract) = msg.discount_vault_contract{
        config.discount_vault_contract.push(deps.api.addr_validate(&discount_vault_contract)?);
    }
    if let Some(ltv_disco_contract) = msg.ltv_disco_contract {
        config.ltv_disco_contract = Some(deps.api.addr_validate(&ltv_disco_contract)?);
    }
    if let Some(transmuter_contract) = msg.transmuter_contract {
        config.transmuter_contract = Some(deps.api.addr_validate(&transmuter_contract)?);
    }

    CONFIG.save(deps.storage, &config)?;
    STATIC_DISCOUNTS.save(deps.storage, &vec![])?;

    Ok(Response::new()
        .add_attribute("config", format!("{:?}", config))
        .add_attribute("contract_address", env.contract.address)
    )
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::UpdateConfig(update) => update_config(deps, info, update),
        ExecuteMsg::SetDiscountPeriod { start_time, duration, discount } => set_timed_discount_period(deps, env, info, start_time, duration, discount),
        ExecuteMsg::ClearDiscountPeriod {} => clear_timed_discount_period(deps, env),
    }
}

/// Update contract configuration
fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    update: UpdateConfig,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    let mut attrs = vec![attr("method", "update_config")];

    //Assert Authority
    if info.sender != config.owner {
        //Check if ownership transfer is in progress & transfer if so
        if info.sender == OWNERSHIP_TRANSFER.load(deps.storage)? {
            config.owner = info.sender;
        } else {
            return Err(ContractError::Unauthorized {});
        }
    }

    //Save optionals
    if let Some(addr) = update.owner {
        let valid_addr = deps.api.addr_validate(&addr)?;

        //Set owner transfer state
        OWNERSHIP_TRANSFER.save(deps.storage, &valid_addr)?;
        attrs.push(attr("owner_transfer", valid_addr));    
    }
    if let Some(addr) = update.positions_contract {
        config.positions_contract = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = update.oracle_contract {
        config.oracle_contract = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = update.staking_contract {
        config.staking_contract = deps.api.addr_validate(&addr)?;
        
        let mbrn_denom = deps.querier.query::<Staking_Config>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: addr.to_string(),
            msg: to_binary(&Staking_QueryMsg::Config {})?,
        }))?
        .mbrn_denom;

        config.mbrn_denom = mbrn_denom;
    }
    if let Some(addr) = update.lockdrop_contract {
        config.lockdrop_contract = Some(deps.api.addr_validate(&addr)?);
    }
    if let Some((addr, add)) = update.discount_vault_contract {
        let addr = deps.api.addr_validate(&addr)?;
        //Add or remove address
        if add {
            config.discount_vault_contract.push(addr);
        } else {
            config.discount_vault_contract.retain(|x| x != &addr);
        }
    }
    if let Some(addr) = update.ltv_disco_contract {
        config.ltv_disco_contract = Some(deps.api.addr_validate(&addr)?);
    }
    if let Some(time) = update.minimum_time_in_network {
        config.minimum_time_in_network = time;
    }
    if let Some(discount) = update.max_discount {
        if discount > Decimal::one() {
            return Err(ContractError::CustomError { 
                val: "max_discount cannot exceed 1.0 (100%)".to_string() 
            });
        }
        config.max_discount = discount;
    }
    if let Some(mbrn_amount) = update.mbrn_at_max_discount {
        config.mbrn_at_max_discount = mbrn_amount;
    }
    if let Some(boost) = update.max_boost {
        config.max_boost = boost;
    }
    if let Some(mut new_discount) = update.static_discount {
        //Load static discounts
        let mut static_discounts = STATIC_DISCOUNTS.load(deps.storage)?;
        //Max discount is 1
        if new_discount.discount > Decimal::one() {
            new_discount.discount = Decimal::one();
        }
        //If its already in the list, update the discount.
        //Else, add it
        if let Some((index, _)) = static_discounts.clone().into_iter().enumerate().find(|(_, diss)| diss.user == new_discount.user){
            static_discounts[index] = new_discount;
        } else {
            static_discounts.push(new_discount);
        }
        //Save new state object 
        STATIC_DISCOUNTS.save(deps.storage, &static_discounts)?;
    }
    if let Some(discount) = update.stable_backing_max_discount {
        if discount > Decimal::one() {
            return Err(ContractError::CustomError { 
                val: "stable_backing_max_discount cannot exceed 1.0 (100%)".to_string() 
            });
        }
        config.stable_backing_max_discount = discount;
    }
    if let Some(discount) = update.stable_backing_first_month_discount {
        config.stable_backing_first_month_discount = discount;
    }
    if let Some(discount) = update.stable_backing_remaining_discount {
        config.stable_backing_remaining_discount = discount;
    }
    if let Some(days) = update.stable_backing_curve_duration_days {
        config.stable_backing_curve_duration_days = days;
    }
    if let Some(days) = update.stable_backing_first_month_days {
        config.stable_backing_first_month_days = days;
    }
    if let Some(multiplier) = update.stable_backing_discountable_debt_multiplier {
        config.stable_backing_discountable_debt_multiplier = multiplier;
    }
    if let Some(multiplier) = update.stable_backing_transmuter_balance_multiplier {
        config.stable_backing_transmuter_balance_multiplier = multiplier;
    }
    if let Some(addr) = update.transmuter_contract {
        config.transmuter_contract = Some(deps.api.addr_validate(&addr)?);
    }

    //Save Config
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("new_config", format!("{:?}", config)))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::UserDiscount { user } => to_binary(&get_discount(deps, env, user)?),
        QueryMsg::UserBoost { user } => to_binary(&get_boost(deps, env, user)?),
        QueryMsg::IntentBoosts { intents } => to_binary(&get_intent_boosts(deps, env, intents)?),
        QueryMsg::StableBackingDiscounts { user, debt_amount } => to_binary(&get_stable_backing_discounts(deps, env, user, debt_amount)?),
    }
}

/// Calculate MBRN discount using time-based curve per deposit
/// Processes staking deposits and LTV Disco deposits individually
fn calculate_mbrn_discount(
    querier: QuerierWrapper,
    config: &Config,
    user: &String,
    current_time: u64,
) -> StdResult<Decimal> {
    const SECONDS_PER_DAY: u64 = 86_400;
    let max_discount = config.stable_backing_max_discount;
    let first_month_discount = config.stable_backing_first_month_discount;
    let remaining_discount = config.stable_backing_remaining_discount;
    let curve_duration_days = config.stable_backing_curve_duration_days;
    let first_month_days = config.stable_backing_first_month_days;

    let mut weighted_discount_sum = Decimal::zero();
    let mut total_weight = Decimal::zero();

    // Query staking contract for user's stake info
    // let staker_response = querier.query::<StakerResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
    //     contract_addr: config.staking_contract.to_string(),
    //     msg: to_binary(&Staking_QueryMsg::UserStake {
    //         staker: user.clone(),
    //     })?,
    // }))?;

    // // Process staking deposits
    // for deposit in &staker_response.deposit_list {
    //     let deposit_start_time = deposit.stake_time;
    //     let days_since_deposit = (current_time.saturating_sub(deposit_start_time)) / SECONDS_PER_DAY;

    //     // Calculate lock duration for acceleration (only for active locks)
    //     let lock_duration_days = if let Some(ref locked) = deposit.locked {
    //         if locked.locked_until > current_time {
    //             // Active lock: use remaining lock time
    //             Some((locked.locked_until.saturating_sub(current_time)) / SECONDS_PER_DAY)
    //         } else {
    //             // Expired lock: no acceleration
    //             None
    //         }
    //     } else {
    //         // No lock: no acceleration
    //         None
    //     };

    //     // Calculate discount using timed curve with lock acceleration
    //     let discount = calculate_time_curve_discount(
    //         days_since_deposit,
    //         lock_duration_days,
    //         max_discount,
    //         first_month_discount,
    //         remaining_discount,
    //         curve_duration_days,
    //         first_month_days,
    //     )?;

    //     // Weight by deposit amount
    //     let weight = Decimal::from_ratio(deposit.amount, Uint128::one());
    //     weighted_discount_sum = weighted_discount_sum
    //         .checked_add(decimal_multiplication(discount, weight)?)
    //         .map_err(|e| StdError::generic_err(format!("Error calculating weighted discount: {}", e)))?;
    //     total_weight = total_weight
    //         .checked_add(weight)
    //         .map_err(|e| StdError::generic_err(format!("Error calculating total weight: {}", e)))?;
    // }

    // Query LTV Disco contract if configured
    if let Some(ltv_disco_contract) = &config.ltv_disco_contract {
        // Query all user deposits (locked and unlocked) across all assets
        let all_deposits_response = querier.query::<AllUserDepositsResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: ltv_disco_contract.to_string(),
            msg: to_binary(&LTVDisco_QueryMsg::GetAllUserDeposits {
                user: user.clone(),
            })?,
        }))?;

        // Process all deposits
        for deposit_info in &all_deposits_response.deposits {
            let deposit = &deposit_info.deposit;
            let deposit_start_time = deposit.start_time;
            let days_since_deposit = (current_time.saturating_sub(deposit_start_time)) / SECONDS_PER_DAY;

            // Calculate lock duration for acceleration (only for active locks)
            let lock_duration_days = if let Some(ref locked) = deposit.locked {
                if locked.locked_until > current_time {
                    // Active lock: use remaining lock time
                    Some((locked.locked_until.saturating_sub(current_time)) / SECONDS_PER_DAY)
                } else {
                    // Expired lock: no acceleration
                    None
                }
            } else {
                // No lock: no acceleration
                None
            };

            // Use deposit_tokens from the response (already converted in ltv_disco contract)
            let deposit_tokens = deposit_info.deposit_tokens;

            // Calculate discount using timed curve with lock acceleration
            let discount = calculate_time_curve_discount(
                days_since_deposit,
                lock_duration_days,
                max_discount,
                first_month_discount,
                remaining_discount,
                curve_duration_days,
                first_month_days,
            )?;

            // Weight by deposit amount
            let weight = Decimal::from_ratio(deposit_tokens, Uint128::one());
            weighted_discount_sum = weighted_discount_sum
                .checked_add(decimal_multiplication(discount, weight)?)
                .map_err(|e| StdError::generic_err(format!("Error calculating weighted discount: {}", e)))?;
            total_weight = total_weight
                .checked_add(weight)
                .map_err(|e| StdError::generic_err(format!("Error calculating total weight: {}", e)))?;
        }
    }

    // Calculate weighted average discount
    let avg_discount = if total_weight.is_zero() {
        Decimal::zero()
    } else {
        decimal_division(weighted_discount_sum, total_weight)?
    };

    Ok(avg_discount)
}

/// Returns % of interest that is discounted,
/// i.e. 95% of 1% interest is .05% interest
fn get_discount(
    deps: Deps,
    env: Env,
    user: String, 
)-> StdResult<UserDiscountResponse>{
    
    //Load static discounts
    let static_discounts = STATIC_DISCOUNTS.load(deps.storage)?;

    //Load timed discount period (may not exist)
    let timed_discount_period: TimedDiscountPeriod = TIMED_DISCOUNT_PERIOD.may_load(deps.storage)?.unwrap_or(TimedDiscountPeriod {
        start_time: 0,
        end_time: 0,
        discount: Decimal::zero(),
    });

    //If the period is active, return the discount
    let user_static_discount = if env.block.time.seconds() >= timed_discount_period.start_time && env.block.time.seconds() <= timed_discount_period.end_time {
        timed_discount_period.discount
    } else if let Some(discount) = static_discounts.into_iter().find(|diss| diss.user == user){
        discount.discount
    } else {
        Decimal::zero()
    };
    if user_static_discount > Decimal::zero() {
        return Ok(UserDiscountResponse {
            user,
            discount: user_static_discount,
        })
    }

    //Load Config
    let config = CONFIG.load(deps.storage)?;

    // Calculate MBRN discount using time-based curve per deposit
    let mbrn_discount = calculate_mbrn_discount(
        deps.querier,
        &config,
        &user,
        env.block.time.seconds(),
    )?;

    // Add transmuter deposit discount (without debt multiplier)
    let transmuter_discount = get_transmuter_discount(
        deps,
        env,
        &config,
        &user,
    )?;

    // Combine discounts: use the maximum of MBRN discount and transmuter discount
    // This allows users to benefit from either MBRN staking or transmuter deposits
    // If there's no transmuter discount, MBRN discount can go up to stable_backing_max_discount
    // Otherwise, cap the combined discount at config.max_discount
    let discount = if transmuter_discount.is_zero() {
        // No transmuter discount, so MBRN discount can reach stable_backing_max_discount
        min(mbrn_discount, config.stable_backing_max_discount)
    } else {
        // Has transmuter discount, cap combined at config.max_discount
        min(transmuter_discount + mbrn_discount, config.max_discount)
    };

    Ok(UserDiscountResponse {
        user,
        discount,
    })
}

/// Returns % boost for user based on MBRN amount
fn get_boost(
    deps: Deps,
    env: Env,
    user: String, 
)-> StdResult<UserBoostResponse>{
    
    //Load Config
    let config = CONFIG.load(deps.storage)?;

    // Get user's total MBRN (staked + deposited in LTV Disco, with locked boosts)
    let user_total_mbrn = get_user_total_mbrn(deps.querier, config.clone(), user.clone(), env.block.time.seconds())?;

    // Calculate boost based on MBRN amount
    let boost = if config.mbrn_at_max_discount.is_zero() {
        // Avoid division by zero - if threshold is 0, return max boost
        config.max_boost
    } else {
        // Calculate ratio: min(user_total_mbrn / mbrn_at_max_discount, 1.0)
        let ratio = Decimal::from_ratio(user_total_mbrn, config.mbrn_at_max_discount);
        let capped_ratio = if ratio > Decimal::one() {
            Decimal::one()
        } else {
            ratio
        };
        
        // Calculate boost: ratio * max_boost
        decimal_multiplication(capped_ratio, config.max_boost)?
    };

    Ok(UserBoostResponse {
        user,
        boost,
    })
}

/// Returns % boost for each intent based on lock duration
fn get_intent_boosts(
    deps: Deps,
    env: Env,
    intents: Vec<MbrnIntentOption>,
) -> StdResult<IntentBoostsResponse> {
    let config = CONFIG.load(deps.storage)?;
    let current_time = env.block.time.seconds();
    let mut boosts = Vec::new();

    for intent in intents {
        let boost = if let Some(locked) = intent.lock {
            // Calculate lock duration in days
            let lock_duration_seconds = locked.locked_until.saturating_sub(current_time);
            let lock_duration_days = lock_duration_seconds / SECONDS_PER_DAY;

            // Get lock_ceiling based on intent type
            let lock_ceiling = match &intent.intent_type {
                membrane::acquisition::MbrnIntentType::Stake {} => {
                    // Query staking contract for lock_duration_ceiling
                    let staking_config = deps.querier.query::<Staking_Config>(&QueryRequest::Wasm(WasmQuery::Smart {
                        contract_addr: config.staking_contract.to_string(),
                        msg: to_binary(&Staking_QueryMsg::Config {})?,
                    }))?;
                    staking_config.lock_duration_ceiling
                }
                membrane::acquisition::MbrnIntentType::DepositViaMarsMirror { .. } => {
                    // Query disco contract for lock_duration_ceiling
                    if let Some(ltv_disco_contract) = config.ltv_disco_contract.clone() {
                        let ltv_disco_config = deps.querier.query::<membrane::ltv_disco::Config>(&QueryRequest::Wasm(WasmQuery::Smart {
                            contract_addr: ltv_disco_contract.to_string(),
                            msg: to_binary(&LTVDisco_QueryMsg::Config {})?,
                        }))?;

                        //Return lock ceiling from config
                        ltv_disco_config.lock_duration_ceiling

                    } else {
                        return Err(StdError::generic_err("LTV Disco contract not configured"));
                    }
                }
                membrane::acquisition::MbrnIntentType::SendToAddress { .. } => {
                    // SendToAddress intents don't support locks
                    // return Err(StdError::generic_err("SendToAddress intents cannot have locks"));
                    // SendToAddress intents don't support locks
                    0
                }
            };

            // Calculate ratio: min(lock_duration_days / lock_ceiling, 1.0)
            let ratio = if lock_ceiling == 0 {
                Decimal::zero()
            } else {
                let ratio = Decimal::from_ratio(lock_duration_days, lock_ceiling);
                if ratio > Decimal::one() {
                    Decimal::one()
                } else {
                    ratio
                }
            };

            // Calculate boost: ratio * max_boost
            decimal_multiplication(ratio, config.max_boost)?
        } else {
            // No lock, return 0% boost
            Decimal::zero()
        };

        boosts.push(boost);
    }

    Ok(IntentBoostsResponse { boosts })
}

/// Calculate discount using time curve with optional lock duration acceleration
/// Lock duration accelerates progress through the curve (only for active locks)
/// For active locks: effective_days = days_since_deposit + lock_duration_days
/// For expired locks or no lock: effective_days = days_since_deposit
fn calculate_time_curve_discount(
    days_since_deposit: u64,
    lock_duration_days: Option<u64>,
    max_discount: Decimal,
    first_month_discount: Decimal,
    remaining_discount: Decimal,
    curve_duration_days: u64,
    first_month_days: u64,
) -> StdResult<Decimal> {
    // Calculate effective days: add lock duration if active lock exists
    let effective_days = if let Some(lock_days) = lock_duration_days {
        days_since_deposit + lock_days
    } else {
        days_since_deposit
    };

    // Calculate discount using timed curve
    let discount = if effective_days >= curve_duration_days {
        max_discount
    } else {
        let first_month_progress = Decimal::from_ratio(
            effective_days.min(first_month_days),
            first_month_days,
        );
        let first_month_discount_amount = decimal_multiplication(first_month_discount, first_month_progress)?;

        let remaining_days = if effective_days > first_month_days {
            effective_days - first_month_days
        } else {
            0
        };
        // Calculate remaining duration in days (not months)
        let remaining_duration_days = curve_duration_days - first_month_days;
        let remaining_progress = if remaining_duration_days > 0 {
            Decimal::from_ratio(
                remaining_days.min(remaining_duration_days),
                remaining_duration_days,
            )
        } else {
            Decimal::zero()
        };
        let remaining_discount_amount = decimal_multiplication(remaining_discount, remaining_progress)?;

        let total_discount = first_month_discount_amount + remaining_discount_amount;
        if total_discount > max_discount {
            max_discount
        } else {
            total_discount
        }
    };

    Ok(discount)
}

/// Calculate transmuter deposit discount (without debt multiplier)
/// Uses the same timed curve as stable backing discounts but without the 18x debt capacity multiplier
fn get_transmuter_discount(
    deps: Deps,
    env: Env,
    config: &Config,
    user: &String,
) -> StdResult<Decimal> {
    // Check if transmuter contract is configured
    let transmuter_contract = match config.transmuter_contract {
        Some(ref addr) => addr.clone(),
        None => {
            return Ok(Decimal::zero());
        }
    };

    // Query user deposits from transmuter
    let deposits_response: membrane::transmuter::UserDepositsResponse = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: transmuter_contract.to_string(),
        msg: to_json_binary(&Transmuter_QueryMsg::UserDeposits {
            user: user.clone(),
        })?,
    }))?;

    if deposits_response.deposits.is_empty() {
        return Ok(Decimal::zero());
    }

    // Get constants from config
    const SECONDS_PER_DAY: u64 = 86_400;
    let max_discount = config.stable_backing_max_discount;
    let first_month_discount = config.stable_backing_first_month_discount;
    let remaining_discount = config.stable_backing_remaining_discount;
    let curve_duration_days = config.stable_backing_curve_duration_days;
    let first_month_days = config.stable_backing_first_month_days;
    let transmuter_balance_multiplier = config.stable_backing_transmuter_balance_multiplier;

    let current_time = env.block.time.seconds();
    let mut weighted_discount_sum = Decimal::zero();
    let mut total_weight = Decimal::zero();

    // Process each deposit
    for deposit in &deposits_response.deposits {
        // Use start_time for discount curve calculation
        let deposit_start_time = deposit.start_time;
        let days_since_deposit = (current_time.saturating_sub(deposit_start_time)) / SECONDS_PER_DAY;

        // Calculate lock duration for acceleration (only for active locks)
        let lock_duration_days = if let Some(ref locked) = deposit.locked {
            if locked.locked_until > current_time {
                // Active lock: use remaining lock time
                Some((locked.locked_until.saturating_sub(current_time)) / SECONDS_PER_DAY)
            } else {
                // Expired lock: no acceleration
                None
            }
        } else {
            // No lock: no acceleration
            None
        };

        // Calculate discount using timed curve with lock acceleration
        let discount = calculate_time_curve_discount(
            days_since_deposit,
            lock_duration_days,
            max_discount,
            first_month_discount,
            remaining_discount,
            curve_duration_days,
            first_month_days,
        )?;

        // Calculate weight based on boosted deposit amount (with 2x multiplier, but no debt multiplier)
        let boosted_amount = decimal_multiplication(
            Decimal::from_ratio(deposit.amount, Uint128::one()),
            transmuter_balance_multiplier,
        )?.to_uint_floor();

        // Weight by boosted amount for weighted average
        let weight = Decimal::from_ratio(boosted_amount, Uint128::one());
        weighted_discount_sum = weighted_discount_sum
            .checked_add(decimal_multiplication(discount, weight)?)
            .map_err(|e| StdError::generic_err(format!("Error calculating weighted discount: {}", e)))?;
        total_weight = total_weight
            .checked_add(weight)
            .map_err(|e| StdError::generic_err(format!("Error calculating total weight: {}", e)))?;
    }

    // Calculate weighted average discount
    let avg_discount = if total_weight.is_zero() {
        Decimal::zero()
    } else {
        decimal_division(weighted_discount_sum, total_weight)?
    };

    Ok(avg_discount)
}

/// Calculate stable backing discounts based on transmuter deposits
/// Uses timed curve: 60% of max (45%) in first month, remaining 30% over next 2 months, max 75%
fn get_stable_backing_discounts(
    deps: Deps,
    env: Env,
    user: String,
    debt_amount: Uint128,
) -> StdResult<StableBackingDiscountsResponse> {
    let config = CONFIG.load(deps.storage)?;
    
    // Check if transmuter contract is configured
    let transmuter_contract = match config.transmuter_contract {
        Some(addr) => addr,
        None => {
            return Ok(StableBackingDiscountsResponse {
                user,
                discount: Decimal::zero(),
            });
        }
    };

    // Query user deposits from transmuter
    let deposits_response: membrane::transmuter::UserDepositsResponse = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: transmuter_contract.to_string(),
        msg: to_json_binary(&membrane::transmuter::QueryMsg::UserDeposits {
            user: user.clone(),
        })?,
    }))?;

    if deposits_response.deposits.is_empty() {
        return Ok(StableBackingDiscountsResponse {
            user,
            discount: Decimal::zero(),
        });
    }

    // Get constants from config
    const SECONDS_PER_DAY: u64 = 86_400;
    let max_discount = config.stable_backing_max_discount;
    let first_month_discount = config.stable_backing_first_month_discount;
    let remaining_discount = config.stable_backing_remaining_discount;
    let curve_duration_days = config.stable_backing_curve_duration_days;
    let first_month_days = config.stable_backing_first_month_days;
    let discountable_debt_multiplier = config.stable_backing_discountable_debt_multiplier;
    let transmuter_balance_multiplier = config.stable_backing_transmuter_balance_multiplier;

    let current_time = env.block.time.seconds();
    let mut total_discountable_capacity = Uint128::zero();
    let mut weighted_discount_sum = Decimal::zero();
    let mut total_weight = Decimal::zero();

    // Process each deposit
    for deposit in &deposits_response.deposits {
        // Use start_time for discount curve calculation
        let deposit_start_time = deposit.start_time;
        let days_since_deposit = (current_time.saturating_sub(deposit_start_time)) / SECONDS_PER_DAY;

        // Calculate discount using timed curve
        let discount = if days_since_deposit >= curve_duration_days {
            max_discount
        } else {
            let first_month_progress = Decimal::from_ratio(
                days_since_deposit.min(first_month_days),
                first_month_days,
            );
            let first_month_discount_amount = decimal_multiplication(first_month_discount, first_month_progress)?;

            let remaining_days = if days_since_deposit > first_month_days {
                days_since_deposit - first_month_days
            } else {
                0
            };
            // Calculate remaining duration in days (not months)
            let remaining_duration_days = curve_duration_days - first_month_days;
            let remaining_progress = if remaining_duration_days > 0 {
                Decimal::from_ratio(
                    remaining_days.min(remaining_duration_days),
                    remaining_duration_days,
                )
            } else {
                Decimal::zero()
            };
            let remaining_discount_amount = decimal_multiplication(remaining_discount, remaining_progress)?;

            let total_discount = first_month_discount_amount + remaining_discount_amount;
            if total_discount > max_discount {
                max_discount
            } else {
                total_discount
            }
        };

        // Calculate discountable debt capacity: multiplier * (deposit_amount * balance_multiplier)
        let boosted_amount = decimal_multiplication(
            Decimal::from_ratio(deposit.amount, Uint128::one()),
            transmuter_balance_multiplier,
        )?.to_uint_floor();
        
        let capacity = boosted_amount
            .checked_mul(Uint128::from(discountable_debt_multiplier))
            .map_err(|e| StdError::overflow(e))?;

        total_discountable_capacity = total_discountable_capacity
            .checked_add(capacity)
            .map_err(|e| StdError::overflow(e))?;

        // Weight by capacity for weighted average
        let weight = Decimal::from_ratio(capacity, Uint128::one());
        weighted_discount_sum = weighted_discount_sum
            .checked_add(decimal_multiplication(discount, weight)?)
            .map_err(|e| StdError::generic_err(format!("Error calculating weighted discount: {}", e)))?;
        total_weight = total_weight
            .checked_add(weight)
            .map_err(|e| StdError::generic_err(format!("Error calculating total weight: {}", e)))?;
    }

    // Calculate weighted average discount
    let avg_discount = if total_weight.is_zero() {
        Decimal::zero()
    } else {
        decimal_division(weighted_discount_sum, total_weight)?
    };

    // Apply proportional discount if debt exceeds capacity
    let final_discount = if debt_amount > total_discountable_capacity {
        // Apply discount proportionally: (capacity / debt) * discount
        let coverage_ratio = Decimal::from_ratio(total_discountable_capacity, debt_amount);
        decimal_multiplication(avg_discount, coverage_ratio)?
    } else {
        avg_discount
    };

    Ok(StableBackingDiscountsResponse {
        user,
        discount: final_discount,
    })
}

/// Calculate boosted amount for a locked deposit
/// Time deposited and locked time are additive so once the user gets to the max discount time, they'll have no reason to continue locking. This leaves room for incentive improvement.
/// NOTES: We use this to boost the MBRN count used for discount queries & global boosts. Intent boosts don't incorporate existing deposits.
fn calculate_locked_boost(
    deposit_amount: Uint128,
    locked: &Locked,
    start_time: u64,
    current_time: u64,
    lock_ceiling: u64,
    perpetual_lock: Option<u64>,
) -> StdResult<Uint128> {
    // Virtually refresh the lock with the perp duration for calculation
    let virtual_locked_until = if let Some(perpetual_days) = perpetual_lock {
        // Extend locked_until by perpetual_lock days from current time
        let new_locked_until = current_time + perpetual_days * SECONDS_PER_DAY;
        let max_lock_time = start_time + (lock_ceiling * SECONDS_PER_DAY);
        std::cmp::min(new_locked_until, max_lock_time)
    } else {
        locked.locked_until
    };
    
    // Calculate lock duration (how long the deposit is locked)
    let lock_duration = virtual_locked_until.saturating_sub(start_time);
    
    // Calculate time since deposit
    let time_since_deposit = current_time.saturating_sub(start_time);
    
    // Convert lock ceiling to seconds
    let lock_ceiling_seconds = lock_ceiling * SECONDS_PER_DAY;
    
    // Calculate ratios
    let lock_ratio = if lock_ceiling_seconds == 0 {
        Decimal::zero()
    } else {
        Decimal::from_ratio(lock_duration, lock_ceiling_seconds)
    };
    
    let time_ratio = if lock_ceiling_seconds == 0 {
        Decimal::zero()
    } else {
        Decimal::from_ratio(time_since_deposit, lock_ceiling_seconds)
    };
    
    // Add the ratios together instead of taking the max
    let combined_ratio = lock_ratio + time_ratio;
    
    // Cap ratio at 1.0 (100%)
    let capped_ratio = if combined_ratio > Decimal::one() {
        Decimal::one()
    } else {
        combined_ratio
    };
    
    // Apply boost: boosted_amount = deposit_amount * (1 + ratio)
    let boost_multiplier = Decimal::one() + capped_ratio;
    let boosted_amount = decimal_multiplication(
        Decimal::from_ratio(deposit_amount, Uint128::one()),
        boost_multiplier,
    )?.to_uint_floor();
    
    Ok(boosted_amount)
}

/// Get user's total MBRN: staked in staking contract + deposited in LTV Disco
/// Includes boosted amounts from locked deposits
fn get_user_total_mbrn(
    querier: QuerierWrapper,
    config: Config,
    user: String,
    current_time: u64,
) -> StdResult<Uint128> {
    // Query staking contract for user's stake info
    let staker_response = querier.query::<StakerResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.staking_contract.to_string(),
        msg: to_binary(&Staking_QueryMsg::UserStake {
            staker: user.clone(),
        })?,
    }))?;
    
    // Query staking config for lock_ceiling
    let staking_config = querier.query::<Staking_Config>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.staking_contract.to_string(),
        msg: to_binary(&Staking_QueryMsg::Config {})?,
    }))?;

    // Query rewards and add accrued interest
    let rewards = querier.query::<RewardsResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.staking_contract.to_string(),
        msg: to_binary(&Staking_QueryMsg::UserRewards {
            user: user.clone(),
        })?,
    }))?;

    // Base amount: total_staked + accrued_interest
    let mut total_mbrn = staker_response.total_staked + rewards.accrued_interest;

    // Boost locked stake deposits
    for deposit in &staker_response.deposit_list {
        if let Some(ref locked) = deposit.locked {
            // Only boost if still locked
            if locked.locked_until > current_time {
                let boosted = calculate_locked_boost(
                    deposit.amount,
                    locked,
                    deposit.stake_time,
                    current_time,
                    staking_config.lock_duration_ceiling,
                    locked.perpetual_lock,
                )?;
                // Add the boost (boosted - original)
                total_mbrn = total_mbrn.checked_add(boosted.checked_sub(deposit.amount)?)?;
            }
        }
    }

    // Query LTV Disco contract if configured
    if let Some(ltv_disco_contract) = config.ltv_disco_contract {
        // Query all user deposits (locked and unlocked) across all assets
        let all_deposits_response = querier.query::<AllUserDepositsResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: ltv_disco_contract.to_string(),
            msg: to_binary(&LTVDisco_QueryMsg::GetAllUserDeposits {
                user: user.clone(),
            })?,
        }))?;
        
        // Query ltv_disco config for lock_ceiling
        let ltv_disco_config = querier.query::<membrane::ltv_disco::Config>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: ltv_disco_contract.to_string(),
            msg: to_binary(&LTVDisco_QueryMsg::Config {})?,
        }))?;
        
        for deposit_info in &all_deposits_response.deposits {
            let deposit = &deposit_info.deposit;
            
            // Use deposit_tokens from the response (already converted in ltv_disco contract)
            let deposit_tokens = deposit_info.deposit_tokens;
            
            // Add base deposit amount
            total_mbrn += deposit_tokens;
            
            // Boost locked deposits
            if let Some(ref locked) = deposit.locked {
                // Only boost if still locked
                if locked.locked_until > current_time {
                    let lock_ceiling = ltv_disco_config.lock_duration_ceiling;
                    let boosted = calculate_locked_boost(
                        deposit_tokens,
                        locked,
                        deposit.start_time,
                        current_time,
                        lock_ceiling,
                        locked.perpetual_lock,
                    )?;
                    // Add the boost (boosted - original deposit tokens)
                    total_mbrn = total_mbrn.checked_add(boosted.checked_sub(deposit_tokens)?)?;
                }
            }
        }
    }

    Ok(total_mbrn)
}


/// Set current timed discount period
fn set_timed_discount_period(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    start_time: Option<u64>, //Now or specified time
    duration: u64, //hours
    discount: Decimal,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    let mut attrs = vec![attr("method", "set_timed_discount_period")];

    //Assert Authority
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    //Set duration seconds
    let duration_seconds = duration * 60;
    //Set start time
    let start_time = start_time.unwrap_or(env.block.time.seconds());
    //Set end time
    let end_time = start_time + duration_seconds;

    //Set timed discount period
    TIMED_DISCOUNT_PERIOD.save(deps.storage, &TimedDiscountPeriod {
        start_time,
        end_time,
        discount,
    })?;

    Ok(Response::new())
}

/// Clear current timed discount period
fn clear_timed_discount_period(
    deps: DepsMut,
    env: Env,
) -> Result<Response, ContractError> {
    //Load state (may not exist)
    let timed_discount_period: TimedDiscountPeriod = match TIMED_DISCOUNT_PERIOD.may_load(deps.storage)? {
        Some(period) => period,
        None => {
            return Err(ContractError::CustomError { 
                val: "No timed discount period to clear".to_string() 
            });
        }
    };

    //Anyone can clear if the period is expired
    if env.block.time.seconds() > timed_discount_period.clone().end_time {
        TIMED_DISCOUNT_PERIOD.remove(deps.storage);
        Ok(Response::new())
    } else {
        Err(ContractError::CustomError { val: format!("Period is not expired, ends in {} seconds", timed_discount_period.clone().end_time - env.block.time.seconds()) })
    }
}

/// Return value of LPs in Osmosis Incentive Lockups
// fn get_incentive_gauge_value(
//     querier: QuerierWrapper,
//     config: Config,
//     valid_denoms: Vec<AssetInfo>,
//     user: String,
//     minimum_time_in_network: u64,
// ) -> StdResult<Decimal>{
//     //Initialize user_locked_value
//     let mut user_locked_value = Decimal::zero();

//     //Parse through all valid denoms
//     for denom in valid_denoms {
//         let res: AccountLockedLongerDurationDenomResponse = LockupQuerier::account_locked_longer_duration_denom(
//             &LockupQuerier::new(&querier),
//             user.clone(),
//             Some(Duration { 
//                 seconds: ((minimum_time_in_network * SECONDS_PER_DAY) - 1) as i64, 
//                 nanos: 0 }),
//             denom.to_string(),
//         )?;


//         //Parse through all locks, price, & value
//         for user_lock_period in res.locks.clone().into_iter(){
//             //Parse thru locked coins in the lock
//             for coin in user_lock_period.coins {
//                 let coin_price = match querier.query::<Vec<PriceResponse>>(&QueryRequest::Wasm(WasmQuery::Smart {
//                     contract_addr: config.clone().oracle_contract.to_string(),
//                     msg: to_binary(&Oracle_QueryMsg::Price {
//                         asset_info: AssetInfo::NativeToken { denom: coin.clone().denom },
//                         twap_timeframe: 60,
//                         oracle_time_limit: 600,
//                         basket_id: None,
//                     })?,
//                 })){
//                     Ok(price) => price[0].clone(),
//                     Err(_) => continue,
//                 };
    
//                 //If price is found, add its value
//                 user_locked_value += coin_price.get_value(Uint128::from_str(&coin.clone().amount).unwrap())?;
//             }
//         }

//     }
    
//     Ok(user_locked_value)
// }

/// Return value of LPs in the discount vault
// fn get_discounts_vault_value(
//     querier: QuerierWrapper,
//     discount_vault: Addr,
//     user: String,
//     minimum_time_in_network: u64,
// ) -> StdResult<Decimal>{

//     //Get user capital from the Gauge Vault
//     let user = querier.query::<Discount_UserResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
//         contract_addr: discount_vault.to_string(),
//         msg: to_binary(&Discount_QueryMsg::User {
//             user,
//             minimum_deposit_time: Some(minimum_time_in_network),
//         })?,
//     }))?;

//     Ok( Decimal::from_ratio(user.discount_value, Uint128::one()) )

// }


/// Return user's total Stability Pool value from credit & MBRN incentives 
// fn get_sp_value(
//     querier: QuerierWrapper,
//     config: Config,
//     current_block_time: u64,
//     user: String,
// ) -> StdResult<Decimal>{

//     //Query Stability Pool to see if the user has funds
//     let user_deposits = querier.query::<AssetPool>(&QueryRequest::Wasm(WasmQuery::Smart {
//         contract_addr: config.clone().stability_pool_contract.to_string(),
//         msg: to_binary(&SP_QueryMsg::AssetPool { 
//             user: Some(user.clone()), 
//             start_after: None,
//             deposit_limit: None 
//         })?,
//     }))?
//     .deposits
//         .into_iter()
//         //Filter for user deposits deposited for a minimum_time_in_network
//         .filter(|deposit| current_block_time - deposit.deposit_time > (config.clone().minimum_time_in_network * SECONDS_PER_DAY))
//         .collect::<Vec<Deposit>>();

//     let total_user_deposit: Decimal = user_deposits
//         .iter()
//         .map(|user_deposit| user_deposit.amount)
//         .collect::<Vec<Decimal>>()
//         .into_iter()
//         .sum();

//     //Return total_value
//     Ok( total_user_deposit)
// }

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    Ok(Response::default())
}
