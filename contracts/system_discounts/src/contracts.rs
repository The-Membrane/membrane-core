use std::cmp::min;

use cosmwasm_std::{
    attr, entry_point, to_binary, Addr, Binary, Decimal, Deps, DepsMut, Env, MessageInfo, QueryRequest, Response, StdError, StdResult, Uint128, WasmQuery
};
use cw2::set_contract_version;

use membrane::math::{decimal_division, decimal_multiplication};
use membrane::system_discounts::{Config, ExecuteMsg, InstantiateMsg, QueryMsg, UpdateConfig, UserDiscountResponse, UserBoostResponse, IntentBoostsResponse, MigrateMsg};
use membrane::staking::{QueryMsg as Staking_QueryMsg, Config as Staking_Config};
use membrane::ltv_disco::{QueryMsg as LTVDisco_QueryMsg, AllUserDepositsResponse};
use membrane::osmosis_proxy::{QueryMsg as OsmosisProxy_QueryMsg, TokenInfoResponse};
use membrane::types::TimedDiscountPeriod;

use crate::error::ContractError;
use crate::state::{CONFIG, OWNERSHIP_TRANSFER, STATIC_DISCOUNTS, TIMED_DISCOUNT_PERIOD};

// Contract name and version used for migration.
const CONTRACT_NAME: &str = "system_discounts";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let owner = if let Some(owner) = msg.owner {
        deps.api.addr_validate(&owner)?
    } else {
        info.sender
    };

    // Query mbrn_denom from staking contract
    let staking_contract = deps.api.addr_validate(&msg.staking_contract)?;
    let mbrn_denom = deps.querier.query::<Staking_Config>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: staking_contract.to_string(),
        msg: to_binary(&Staking_QueryMsg::Config {})?,
    }))?
    .mbrn_denom;

    let max_discount = msg.max_discount.unwrap_or(Decimal::percent(30));
    let max_boost = msg.max_boost.unwrap_or(Decimal::percent(9));
    let ratio_of_mbrn_supply_at_max_discount = msg.ratio_of_mbrn_supply_at_max_discount.unwrap_or(Decimal::permille(10)); // 1%

    // Validate max_discount <= 1.0
    if max_discount > Decimal::one() {
        return Err(ContractError::CustomError {
            val: "max_discount cannot exceed 1.0 (100%)".to_string()
        });
    }

    let mut config = Config {
        owner,
        mbrn_denom,
        positions_contract: deps.api.addr_validate(&msg.positions_contract)?,
        oracle_contract: deps.api.addr_validate(&msg.oracle_contract)?,
        staking_contract,
        lockdrop_contract: None,
        discount_vault_contract: vec![],
        ltv_disco_contract: None,
        osmosis_proxy: deps.api.addr_validate(&msg.osmosis_proxy)?,
        minimum_time_in_network: msg.minimum_time_in_network,
        max_discount,
        max_boost,
        ratio_of_mbrn_supply_at_max_discount,
    };

    // Store optionals
    if let Some(lockdrop_contract) = msg.lockdrop_contract {
        config.lockdrop_contract = Some(deps.api.addr_validate(&lockdrop_contract)?);
    }
    if let Some(discount_vault_contract) = msg.discount_vault_contract {
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

    // Assert Authority
    if info.sender != config.owner {
        // Check if ownership transfer is in progress & transfer if so
        if info.sender == OWNERSHIP_TRANSFER.load(deps.storage)? {
            config.owner = info.sender;
        } else {
            return Err(ContractError::Unauthorized {});
        }
    }

    // Save optionals
    if let Some(addr) = update.owner {
        let valid_addr = deps.api.addr_validate(&addr)?;
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
        if add {
            config.discount_vault_contract.push(addr);
        } else {
            config.discount_vault_contract.retain(|x| x != &addr);
        }
    }
    if let Some(addr) = update.ltv_disco_contract {
        config.ltv_disco_contract = Some(deps.api.addr_validate(&addr)?);
    }
    if let Some(addr) = update.osmosis_proxy {
        config.osmosis_proxy = deps.api.addr_validate(&addr)?;
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
    if let Some(boost) = update.max_boost {
        config.max_boost = boost;
    }
    if let Some(mut new_discount) = update.static_discount {
        let mut static_discounts = STATIC_DISCOUNTS.load(deps.storage)?;
        if new_discount.discount > Decimal::one() {
            new_discount.discount = Decimal::one();
        }
        if let Some((index, _)) = static_discounts.clone().into_iter().enumerate().find(|(_, diss)| diss.user == new_discount.user) {
            static_discounts[index] = new_discount;
        } else {
            static_discounts.push(new_discount);
        }
        STATIC_DISCOUNTS.save(deps.storage, &static_discounts)?;
    }
    if let Some(ratio) = update.ratio_of_mbrn_supply_at_max_discount {
        if ratio > Decimal::one() {
            return Err(ContractError::CustomError {
                val: "ratio_of_mbrn_supply_at_max_discount cannot exceed 1.0 (100%)".to_string()
            });
        }
        config.ratio_of_mbrn_supply_at_max_discount = ratio;
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("new_config", format!("{:?}", config)))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::UserDiscount { user } => to_binary(&get_discount(deps, env, user)?),
        QueryMsg::UserBoost { user } => to_binary(&UserBoostResponse { user, boost: Decimal::zero() }),
        QueryMsg::IntentBoosts { intents: _ } => to_binary(&IntentBoostsResponse { boosts: vec![] }),
    }
}

/// Returns % of interest that is discounted.
/// Uses a simple linear curve: discount = min(total_mbrn / mbrn_needed, 1.0) * max_discount
/// where total_mbrn = wallet balance + sum of Disco deposit tokens
/// and mbrn_needed = total_supply * ratio_of_mbrn_supply_at_max_discount
fn get_discount(
    deps: Deps,
    env: Env,
    user: String,
) -> StdResult<UserDiscountResponse> {
    // Load static discounts
    let static_discounts = STATIC_DISCOUNTS.load(deps.storage)?;

    // Load timed discount period (may not exist)
    let timed_discount_period: TimedDiscountPeriod = TIMED_DISCOUNT_PERIOD.may_load(deps.storage)?.unwrap_or(TimedDiscountPeriod {
        start_time: 0,
        end_time: 0,
        discount: Decimal::zero(),
    });

    // If a timed discount period is active, return it
    let user_static_discount = if env.block.time.seconds() >= timed_discount_period.start_time && env.block.time.seconds() <= timed_discount_period.end_time {
        timed_discount_period.discount
    } else if let Some(discount) = static_discounts.into_iter().find(|diss| diss.user == user) {
        discount.discount
    } else {
        Decimal::zero()
    };
    if user_static_discount > Decimal::zero() {
        return Ok(UserDiscountResponse {
            user,
            discount: user_static_discount,
        });
    }

    // Load Config
    let config = CONFIG.load(deps.storage)?;

    // 1. Query user's MBRN wallet balance
    let wallet_balance = deps.querier.query_balance(&user, &config.mbrn_denom)?.amount;

    // 2. Query Disco deposits and sum deposit_tokens
    let disco_tokens = if let Some(ltv_disco_contract) = &config.ltv_disco_contract {
        let all_deposits_response = deps.querier.query::<AllUserDepositsResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: ltv_disco_contract.to_string(),
            msg: to_binary(&LTVDisco_QueryMsg::GetAllUserDeposits {
                user: user.clone(),
            })?,
        }))?;

        all_deposits_response.deposits.iter().fold(Uint128::zero(), |acc, d| acc + d.deposit_tokens)
    } else {
        Uint128::zero()
    };

    let total_mbrn = wallet_balance + disco_tokens;

    // 3. Query MBRN total supply from osmosis-proxy
    let token_info = deps.querier.query::<TokenInfoResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.osmosis_proxy.to_string(),
        msg: to_binary(&OsmosisProxy_QueryMsg::GetTokenInfo {
            denom: config.mbrn_denom.clone(),
        })?,
    }))?;

    let total_supply = token_info.current_supply;

    // 4. Calculate discount: min(total_mbrn / mbrn_needed, 1.0) * max_discount
    if total_supply.is_zero() || config.ratio_of_mbrn_supply_at_max_discount.is_zero() {
        return Ok(UserDiscountResponse {
            user,
            discount: Decimal::zero(),
        });
    }

    let mbrn_needed = decimal_multiplication(
        Decimal::from_ratio(total_supply, Uint128::one()),
        config.ratio_of_mbrn_supply_at_max_discount,
    )?;

    if mbrn_needed.is_zero() {
        return Ok(UserDiscountResponse {
            user,
            discount: Decimal::zero(),
        });
    }

    let ratio = decimal_division(
        Decimal::from_ratio(total_mbrn, Uint128::one()),
        mbrn_needed,
    )?;

    let capped_ratio = min(ratio, Decimal::one());
    let discount = decimal_multiplication(capped_ratio, config.max_discount)?;

    Ok(UserDiscountResponse {
        user,
        discount,
    })
}

/// Set a timed discount period (owner only)
fn set_timed_discount_period(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    start_time: Option<u64>,
    duration: u64,
    discount: Decimal,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let duration_seconds = duration * 60;
    let start_time = start_time.unwrap_or(env.block.time.seconds());
    let end_time = start_time + duration_seconds;

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
    let timed_discount_period: TimedDiscountPeriod = match TIMED_DISCOUNT_PERIOD.may_load(deps.storage)? {
        Some(period) => period,
        None => {
            return Err(ContractError::CustomError {
                val: "No timed discount period to clear".to_string()
            });
        }
    };

    if env.block.time.seconds() > timed_discount_period.clone().end_time {
        TIMED_DISCOUNT_PERIOD.remove(deps.storage);
        Ok(Response::new())
    } else {
        Err(ContractError::CustomError { val: format!("Period is not expired, ends in {} seconds", timed_discount_period.clone().end_time - env.block.time.seconds()) })
    }
}

/// Old config struct for migration (includes removed fields)
/// Permissive deserialization: ignores unknown fields from old config
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct OldConfig {
    pub owner: Addr,
    pub mbrn_denom: String,
    pub oracle_contract: Addr,
    pub positions_contract: Addr,
    pub staking_contract: Addr,
    pub lockdrop_contract: Option<Addr>,
    pub discount_vault_contract: Vec<Addr>,
    pub ltv_disco_contract: Option<Addr>,
    pub minimum_time_in_network: u64,
    pub max_discount: Decimal,
    pub max_boost: Decimal,
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, msg: MigrateMsg) -> Result<Response, ContractError> {
    let osmosis_proxy = deps.api.addr_validate(&msg.osmosis_proxy)?;

    // Try loading the new config first; if that fails, load old config and convert
    let config = match CONFIG.load(deps.storage) {
        Ok(mut config) => {
            config.osmosis_proxy = osmosis_proxy;
            config.ratio_of_mbrn_supply_at_max_discount = msg.ratio_of_mbrn_supply_at_max_discount.unwrap_or(Decimal::permille(10));
            if let Some(max_discount) = msg.max_discount {
                config.max_discount = max_discount;
            }
            config
        }
        Err(_) => {
            // Load old config via raw storage read
            let old_data = deps.storage.get(b"config").ok_or(ContractError::CustomError {
                val: "Config not found in storage".to_string(),
            })?;
            let old_config: OldConfig = cosmwasm_std::from_json(&old_data).map_err(|e| {
                ContractError::CustomError {
                    val: format!("Failed to deserialize old config: {}", e),
                }
            })?;

            Config {
                owner: old_config.owner,
                mbrn_denom: old_config.mbrn_denom,
                oracle_contract: old_config.oracle_contract,
                positions_contract: old_config.positions_contract,
                staking_contract: old_config.staking_contract,
                lockdrop_contract: old_config.lockdrop_contract,
                discount_vault_contract: old_config.discount_vault_contract,
                ltv_disco_contract: old_config.ltv_disco_contract,
                osmosis_proxy,
                minimum_time_in_network: old_config.minimum_time_in_network,
                max_discount: msg.max_discount.unwrap_or(Decimal::percent(30)),
                max_boost: old_config.max_boost,
                ratio_of_mbrn_supply_at_max_discount: msg.ratio_of_mbrn_supply_at_max_discount.unwrap_or(Decimal::permille(10)),
            }
        }
    };

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::default())
}
