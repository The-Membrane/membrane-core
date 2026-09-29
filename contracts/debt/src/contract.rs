use cosmwasm_std::{
    entry_point, to_json_binary, Binary, Decimal, Deps, DepsMut, Env, MessageInfo, Response,
    StdResult, Uint128,
};

use membrane::debt::{
    BasketDebt, CallbackMsg, Config, EditBasketDebt, ExecuteMsg, InstantiateMsg,
    MigrateMsg, QueryMsg, UpdateConfig,
};
use membrane::oracle::PriceResponse;
use membrane::types::CreditAssetBreakdown;

use crate::error::ContractError;
use crate::state::{BASKET, CDT_SUPPLY, CONFIG, FROZEN, RATES};

// ============================================================================
// Instantiate
// ============================================================================

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let owner = if let Some(owner) = msg.owner {
        deps.api.addr_validate(&owner)?
    } else {
        info.sender.clone()
    };

    let config = Config {
        owner,
        collateral_contract: msg
            .collateral_contract
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        liquidation_contract: msg
            .liquidation_contract
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        staking_contract: msg
            .staking_contract
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        chain_proxy: msg
            .chain_proxy
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        debt_auction: msg
            .debt_auction
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        oracle_contract: msg
            .oracle_contract
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        liquidity_contract: msg
            .liquidity_contract
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        discounts_contract: msg
            .discounts_contract
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        revenue_distributor: msg
            .revenue_distributor
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        transmuter_addr: msg
            .transmuter_addr
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        points_contract: msg
            .points_contract
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        liq_fee: msg.liq_fee,
        credit_twap_timeframe: msg.credit_twap_timeframe,
        cpc_multiplier: msg.cpc_multiplier,
        debt_minimum: msg.debt_minimum,
        base_debt_cap_multiplier: msg.base_debt_cap_multiplier,
        rate_slope_multiplier: msg.rate_slope_multiplier,
        affiliate_fee_max: msg.affiliate_fee_max,
        skip_credit_price_accrual: false,
        liquidation_stat_limit: 1000,
        irm_config: msg.irm_config,
        acquisition_contract: msg
            .acquisition_contract
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
    };
    CONFIG.save(deps.storage, &config)?;

    // Create initial basket debt
    let basket = BasketDebt {
        basket_id: msg.create_basket.basket_id,
        credit_asset: CreditAssetBreakdown {
            info: msg.create_basket.credit_asset.info.clone(),
            variable_amount: Uint128::zero(),
            one_month_amount: Uint128::zero(),
            three_month_amount: Uint128::zero(),
            six_month_amount: Uint128::zero(),
            peg_variable_amount: Uint128::zero(),
            peg_one_month_amount: Uint128::zero(),
            peg_three_month_amount: Uint128::zero(),
            peg_six_month_amount: Uint128::zero(),
        },
        credit_price: PriceResponse {
            prices: vec![],
            price: msg.create_basket.credit_price,
            decimals: 6,
        },
        pending_revenue: membrane::types::PendingRevenue {
            total_pending: Uint128::zero(),
            per_asset_rev: vec![],
        },
        pending_bad_debt: Uint128::zero(),
        liq_queue: msg
            .create_basket
            .liq_queue
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        oracle_set: false,
        distribute_revenue: false,
    };
    BASKET.save(deps.storage, &basket)?;

    FROZEN.save(deps.storage, &false)?;
    CDT_SUPPLY.save(deps.storage, &Uint128::zero())?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("owner", config.owner.to_string()))
}

// ============================================================================
// Execute
// ============================================================================

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::UpdateConfig(update) => execute_update_config(deps, info, update),

        // === User operations ===
        ExecuteMsg::IncreaseDebt {
            position_id,
            amount,
            LTV,
            mint_to_addr,
            deployment_intent,
            debt_split,
            rollover_updates,
            peg_debt,
        } => crate::positions::increase_debt(
            deps,
            env,
            info,
            position_id,
            amount,
            LTV,
            mint_to_addr,
            deployment_intent,
            debt_split,
            rollover_updates,
            peg_debt,
        ),
        ExecuteMsg::Repay {
            position_id,
            position_owner,
            send_excess_to,
            debt_split,
        } => crate::positions::repay(
            deps,
            env,
            info,
            position_id,
            position_owner,
            send_excess_to,
            debt_split,
        ),
        ExecuteMsg::Accrue {
            position_owner,
            position_ids,
        } => crate::rates::accrue(deps, env, info, position_owner, position_ids),
        ExecuteMsg::ClosePosition {
            position_id,
            close_percentage,
            max_spread,
            send_to,
        } => crate::positions::close_position(
            deps,
            env,
            info,
            position_id,
            close_percentage,
            max_spread,
            send_to,
        ),
        ExecuteMsg::SetUserIntents { deployment_intent } => {
            crate::positions::set_user_intents(deps, info, deployment_intent)
        }
        ExecuteMsg::FulfillIntents { users } => {
            crate::positions::fulfill_intents(deps, env, info, users)
        }
        ExecuteMsg::SetDeploymentVenue {
            position_id,
            venue_address,
            initial_deployed_debt_amount,
        } => crate::positions::set_deployment_venue(
            deps,
            env,
            info,
            position_id,
            venue_address,
            initial_deployed_debt_amount,
        ),

        // === Admin ===
        ExecuteMsg::EditBasket(edit) => execute_edit_basket(deps, info, edit),
        ExecuteMsg::SetAffiliate {
            position_id,
            affiliate_address,
            affiliate_fee,
            label,
        } => crate::positions::set_affiliate(
            deps,
            info,
            position_id,
            affiliate_address,
            affiliate_fee,
            label,
        ),
        ExecuteMsg::FulfillBadDebt {} => crate::positions::fulfill_bad_debt(deps, env, info),
        ExecuteMsg::TakeRevenue {} => crate::positions::take_revenue(deps, env, info),
        ExecuteMsg::CheckAndClearDebtDelta {
            position_owner,
            position_id,
        } => crate::positions::check_and_clear_debt_delta(deps, info, position_owner, position_id),
        ExecuteMsg::SetFrozen { frozen } => execute_set_frozen(deps, info, frozen),

        // === Authorized calls from Collateral ===
        ExecuteMsg::NotifyDeposit {
            position_id,
            position_owner,
            affiliate_address,
        } => crate::positions::notify_deposit(
            deps,
            env,
            info,
            position_id,
            position_owner,
            affiliate_address,
        ),
        ExecuteMsg::AccrueAndValidate {
            position_owner,
            position_id,
            remaining_collateral_value,
            avg_max_borrow_ltv,
            avg_max_ltv,
        } => crate::positions::accrue_and_validate(
            deps,
            env,
            info,
            position_owner,
            position_id,
            remaining_collateral_value,
            avg_max_borrow_ltv,
            avg_max_ltv,
        ),

        // === Authorized calls from Liquidation ===
        ExecuteMsg::AccrueAndReturnDebt {
            position_owner,
            position_id,
        } => crate::positions::accrue_and_return_debt(deps, env, info, position_owner, position_id),
        ExecuteMsg::LiquidationDebtUpdate {
            position_id,
            position_owner,
            repay_amount,
        } => crate::positions::liquidation_debt_update(
            deps,
            env,
            info,
            position_id,
            position_owner,
            repay_amount,
        ),
        ExecuteMsg::RecordBadDebt {
            position_id,
            position_owner,
            bad_debt_amount,
        } => crate::positions::record_bad_debt(
            deps,
            info,
            position_id,
            position_owner,
            bad_debt_amount,
        ),

        // === Acquisition contract ===
        ExecuteMsg::SetAcquisitionBumpRate { rate } => {
            set_acquisition_bump_rate(deps, info, rate)
        }

        // === Callbacks ===
        ExecuteMsg::Callback(callback_msg) => {
            // Only callable by self
            if info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }
            match callback_msg {
                CallbackMsg::ClosePositionCallback {
                    position_id,
                    position_owner,
                } => crate::positions::close_position_callback(deps, env, position_id, position_owner),
            }
        }
    }
}

fn execute_update_config(
    deps: DepsMut,
    info: MessageInfo,
    update: UpdateConfig,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    if let Some(owner) = update.owner {
        config.owner = deps.api.addr_validate(&owner)?;
    }
    if let Some(collateral_contract) = update.collateral_contract {
        config.collateral_contract = Some(deps.api.addr_validate(&collateral_contract)?);
    }
    if let Some(liquidation_contract) = update.liquidation_contract {
        config.liquidation_contract = Some(deps.api.addr_validate(&liquidation_contract)?);
    }
    if let Some(staking_contract) = update.staking_contract {
        config.staking_contract = Some(deps.api.addr_validate(&staking_contract)?);
    }
    if let Some(chain_proxy) = update.chain_proxy {
        config.chain_proxy = Some(deps.api.addr_validate(&chain_proxy)?);
    }
    if let Some(debt_auction) = update.debt_auction {
        config.debt_auction = Some(deps.api.addr_validate(&debt_auction)?);
    }
    if let Some(oracle_contract) = update.oracle_contract {
        config.oracle_contract = Some(deps.api.addr_validate(&oracle_contract)?);
    }
    if let Some(liquidity_contract) = update.liquidity_contract {
        config.liquidity_contract = Some(deps.api.addr_validate(&liquidity_contract)?);
    }
    if let Some(discounts_contract) = update.discounts_contract {
        config.discounts_contract = Some(deps.api.addr_validate(&discounts_contract)?);
    }
    if let Some(revenue_distributor) = update.revenue_distributor {
        config.revenue_distributor = Some(deps.api.addr_validate(&revenue_distributor)?);
    }
    if let Some(transmuter_addr) = update.transmuter_addr {
        config.transmuter_addr = Some(deps.api.addr_validate(&transmuter_addr)?);
    }
    if let Some(points_contract) = update.points_contract {
        config.points_contract = Some(deps.api.addr_validate(&points_contract)?);
    }
    if let Some(liq_fee) = update.liq_fee {
        config.liq_fee = liq_fee;
    }
    if let Some(credit_twap_timeframe) = update.credit_twap_timeframe {
        config.credit_twap_timeframe = credit_twap_timeframe;
    }
    if let Some(cpc_multiplier) = update.cpc_multiplier {
        config.cpc_multiplier = cpc_multiplier;
    }
    if let Some(debt_minimum) = update.debt_minimum {
        config.debt_minimum = debt_minimum;
    }
    if let Some(base_debt_cap_multiplier) = update.base_debt_cap_multiplier {
        config.base_debt_cap_multiplier = base_debt_cap_multiplier;
    }
    if let Some(rate_slope_multiplier) = update.rate_slope_multiplier {
        config.rate_slope_multiplier = rate_slope_multiplier;
    }
    if let Some(affiliate_fee_max) = update.affiliate_fee_max {
        config.affiliate_fee_max = affiliate_fee_max;
    }
    if let Some(skip_credit_price_accrual) = update.skip_credit_price_accrual {
        config.skip_credit_price_accrual = skip_credit_price_accrual;
    }
    if let Some(liquidation_stat_limit) = update.liquidation_stat_limit {
        config.liquidation_stat_limit = liquidation_stat_limit;
    }
    if let Some(irm_config) = update.irm_config {
        config.irm_config = irm_config;
    }
    if let Some(acquisition_contract) = update.acquisition_contract {
        config.acquisition_contract = Some(deps.api.addr_validate(&acquisition_contract)?);
    }

    CONFIG.save(deps.storage, &config)?;
    Ok(Response::new().add_attribute("method", "update_config"))
}

/// Set the acquisition bump rate. Only callable by the configured acquisition contract.
/// Stores the bump rate in the Rates struct. The bump is applied ONLY to peg (USDC) debt
/// during rate index accumulation as: peg_rate = smoothed_rate + (acquisition_bump_rate * 1/max_LTV).
/// Regular (CDT) debt is NOT affected by this bump.
fn set_acquisition_bump_rate(
    deps: DepsMut,
    info: MessageInfo,
    rate: Decimal,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only the configured acquisition contract can call this
    match &config.acquisition_contract {
        Some(acq_addr) if info.sender == *acq_addr => {},
        _ => return Err(ContractError::Unauthorized {}),
    }

    let mut rates = RATES.load(deps.storage)?;
    rates.acquisition_bump_rate = rate;
    RATES.save(deps.storage, &rates)?;

    Ok(Response::new()
        .add_attribute("action", "set_acquisition_bump_rate")
        .add_attribute("rate", rate.to_string()))
}

fn execute_edit_basket(
    deps: DepsMut,
    info: MessageInfo,
    edit: EditBasketDebt,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let mut basket = BASKET.load(deps.storage)?;

    if let Some(liq_queue) = edit.liq_queue {
        basket.liq_queue = Some(deps.api.addr_validate(&liq_queue)?);
    }
    if let Some(distribute_revenue) = edit.distribute_revenue {
        basket.distribute_revenue = distribute_revenue;
    }

    // TODO: Handle credit_pool_infos, credit_asset_twap_price_source,
    // base_interest_rate, negative_rates, cpc_margin_of_error, take_revenue

    BASKET.save(deps.storage, &basket)?;
    Ok(Response::new().add_attribute("method", "edit_basket"))
}

fn execute_set_frozen(
    deps: DepsMut,
    info: MessageInfo,
    frozen: bool,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    // Owner or Collateral contract can set frozen
    if info.sender != config.owner
        && Some(info.sender.clone()) != config.collateral_contract
    {
        return Err(ContractError::Unauthorized {});
    }

    FROZEN.save(deps.storage, &frozen)?;

    Ok(Response::new()
        .add_attribute("method", "set_frozen")
        .add_attribute("frozen", frozen.to_string()))
}

// ============================================================================
// Query
// ============================================================================

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::GetDebtPositions {
            start_after,
            limit,
            user_info,
            user,
        } => crate::query::query_debt_positions(deps, start_after, limit, user_info, user),
        QueryMsg::GetFullPositions {
            start_after,
            limit,
            user_info,
            user,
        } => crate::query::query_full_positions(deps, start_after, limit, user_info, user),
        QueryMsg::GetBasket {} => to_json_binary(&BASKET.load(deps.storage)?),
        QueryMsg::GetCreditRate {} => crate::query::query_credit_rate(deps),
        QueryMsg::GetCollateralInterest {} => crate::query::query_collateral_interest(deps),
        QueryMsg::GetRates {} => crate::query::query_rates(deps),
        QueryMsg::SimulateMint { position_info, LTV } => {
            crate::query::query_simulate_mint(deps, position_info, LTV)
        }
        QueryMsg::GetUserIntent {
            start_after,
            limit,
            users,
        } => crate::query::query_user_intents(deps, start_after, limit, users),
        QueryMsg::GetAffiliates { position_id } => {
            crate::query::query_affiliates(deps, position_id)
        }
        QueryMsg::GetActiveDeploymentVenues {
            venue,
            start_after,
            limit,
        } => crate::query::query_active_deployment_venues(deps, venue, start_after, limit),
        QueryMsg::GetLiquidationStats { start_after, limit } => {
            crate::query::query_liquidation_stats(deps, start_after, limit)
        }
        QueryMsg::GetHistoricalInterestRates { asset } => {
            crate::query::query_historical_interest_rates(deps, asset)
        }
        QueryMsg::GetFrozen {} => to_json_binary(&FROZEN.load(deps.storage)?),
    }
}

// ============================================================================
// Reply
// ============================================================================

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(
    deps: DepsMut,
    env: Env,
    msg: cosmwasm_std::Reply,
) -> Result<Response, ContractError> {
    crate::reply::handle_reply(deps, env, msg)
}

// ============================================================================
// Migrate
// ============================================================================

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    Ok(Response::default())
}
