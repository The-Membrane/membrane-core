use std::str::FromStr;

use cosmwasm_std::{
    attr, entry_point, to_binary, to_json_binary, Addr, Binary, Decimal, Deps, DepsMut, Env, MessageInfo, QuerierWrapper, QueryRequest, Response, StdResult, Uint128, WasmQuery
};
use cw2::set_contract_version;

use membrane::helpers::query_basket;
use osmosis_std::shim::Duration;
use osmosis_std::types::osmosis::lockup::{LockupQuerier, AccountLockedLongerDurationDenomResponse};

use membrane::math::{decimal_division, decimal_multiplication};
use membrane::system_discounts::{Config, ExecuteMsg, InstantiateMsg, QueryMsg, UpdateConfig, UserDiscountResponse, UserBoostResponse, MigrateMsg};
use membrane::stability_pool::QueryMsg as SP_QueryMsg;
use membrane::staking::{QueryMsg as Staking_QueryMsg, Config as Staking_Config, StakerResponse, RewardsResponse};
use membrane::discount_vault::{QueryMsg as Discount_QueryMsg, UserResponse as Discount_UserResponse};
use membrane::cdp::{BasketPositionsResponse, QueryMsg as CDP_QueryMsg};
use membrane::oracle::{QueryMsg as Oracle_QueryMsg, PriceResponse};
use membrane::ltv_disco::{QueryMsg as LTVDisco_QueryMsg, UserTotalDepositsResponse};
use membrane::types::{AssetInfo, AssetPool, Basket, Deposit, TimedDiscountPeriod};

use crate::error::ContractError;
use crate::state::{CONFIG, OWNERSHIP_TRANSFER, STATIC_DISCOUNTS, TIMED_DISCOUNT_PERIOD};

// Contract name and version used for migration.
const CONTRACT_NAME: &str = "system_discounts";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Constants
const SECONDS_PER_DAY: u64 = 86_400u64;

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

    config = Config {
        owner,
        mbrn_denom,
        positions_contract: deps.api.addr_validate(&msg.positions_contract)?,
        oracle_contract: deps.api.addr_validate(&msg.oracle_contract)?,
        staking_contract,
        stability_pool_contract: deps.api.addr_validate(&msg.stability_pool_contract)?,
        lockdrop_contract: None,
        discount_vault_contract: vec![],
        ltv_disco_contract: None,
        minimum_time_in_network: msg.minimum_time_in_network,
        max_discount,
        mbrn_at_max_discount,
        max_boost,
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
    if let Some(addr) = update.stability_pool_contract {
        config.stability_pool_contract = deps.api.addr_validate(&addr)?;
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

    //Save Config
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("new_config", format!("{:?}", config)))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::UserDiscount { user } => to_binary(&get_discount(deps, env, user)?),
        QueryMsg::UserBoost { user } => to_binary(&get_boost(deps, user)?),
    }
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

    //Load timed discount period
    let timed_discount_period: TimedDiscountPeriod = TIMED_DISCOUNT_PERIOD.load(deps.storage)?;

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

    // Get user's total MBRN (staked + deposited in LTV Disco)
    let user_total_mbrn = get_user_total_mbrn(deps.querier, config.clone(), user.clone())?;

    // Calculate discount based on MBRN amount
    let discount = if config.mbrn_at_max_discount.is_zero() {
        // Avoid division by zero - if threshold is 0, return max discount
        config.max_discount
    } else {
        // Calculate ratio: min(user_total_mbrn / mbrn_at_max_discount, 1.0)
        let ratio = Decimal::from_ratio(user_total_mbrn, config.mbrn_at_max_discount);
        let capped_ratio = if ratio > Decimal::one() {
            Decimal::one()
        } else {
            ratio
        };
        
        // Calculate discount: ratio * max_discount
        decimal_multiplication(capped_ratio, config.max_discount)?
    };

    Ok(UserDiscountResponse {
        user,
        discount,
    })
}

/// Returns % boost for user based on MBRN amount
fn get_boost(
    deps: Deps,
    user: String, 
)-> StdResult<UserBoostResponse>{
    
    //Load Config
    let config = CONFIG.load(deps.storage)?;

    // Get user's total MBRN (staked + deposited in LTV Disco)
    let user_total_mbrn = get_user_total_mbrn(deps.querier, config.clone(), user.clone())?;

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

/// Get user's total MBRN: staked in staking contract + deposited in LTV Disco
fn get_user_total_mbrn(
    querier: QuerierWrapper,
    config: Config,
    user: String,
) -> StdResult<Uint128> {
    // Query staking contract for user's staked MBRN
    let user_stake = querier.query::<StakerResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.staking_contract.to_string(),
        msg: to_binary(&Staking_QueryMsg::UserStake {
            staker: user.clone(),
        })?,
    }))?
    .total_staked;

    // Query rewards and add accrued interest
    let rewards = querier.query::<RewardsResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.staking_contract.to_string(),
        msg: to_binary(&Staking_QueryMsg::UserRewards {
            user: user.clone(),
        })?,
    }))?;

    let mut total_mbrn = user_stake + rewards.accrued_interest;

    // Query LTV Disco contract if configured
    if let Some(ltv_disco_contract) = config.ltv_disco_contract {
        let ltv_deposits = querier.query::<UserTotalDepositsResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: ltv_disco_contract.to_string(),
            msg: to_binary(&LTVDisco_QueryMsg::UserTotalDeposits {
                user: user.clone(),
            })?,
        }))?;
        
        total_mbrn += ltv_deposits.total_deposits;
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
    //Load state
    let timed_discount_period: TimedDiscountPeriod = TIMED_DISCOUNT_PERIOD.load(deps.storage)?;

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
