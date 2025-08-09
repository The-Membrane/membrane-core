use core::panic;
use std::env;
use std::str::FromStr;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, to_json_binary, Addr, Binary, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, Order, QueryRequest, Reply, Response, StdError, StdResult, Storage, SubMsg, Uint128, WasmMsg, WasmQuery
};

use cw_storage_plus::Bound;
use membrane::auction::ExecuteMsg as AuctionExecuteMsg;
use membrane::helpers::{assert_sent_native_token_balance, get_contract_balances};
use membrane::liq_queue::ExecuteMsg as LQ_ExecuteMsg;
use membrane::managed_market::{BorrowCap, Config, DebtInfo, ExecuteMsg, InstantiateMsg, LTVRamp, MarketParams, MigrateMsg, QueryMsg, RateIndex, RateParams, UserPositionResponse};
use membrane::stability_pool_vault::calculate_base_tokens;
use membrane::tokenfactory::{ExecuteMsg as TokenFactory, create_denom_msg};
use membrane::mm_oracle::ExecuteMsg as OracleExecuteMsg;
use membrane::mm_swap::ExecuteMsg as SwapExecuteMsg;
use membrane::types::{
    cAsset, Asset, AssetInfo, AssetOracleInfo, Basket, BorrowOptions, ClaimTracker, OsmosisOracleInfo, OsmosisRouteInfo, TWAPPoolInfo, UXBoosts, UserHistory, UserInfo, UserPosition, VTClaimCheckpoint
};

use crate::error::ContractError;
use crate::positions::{
    borrow_cdt, calc_borrowable_amount, check_and_fulfill_bad_debt, close_position, collateral_rate_assurance, crank_realized_apr, edit_ux_boosts, get_total_debt_tokens, get_total_vault_tokens, liquidate, loop_position, rate_assurance, repay_cdt, supply_collateral, supply_debt, withdraw_collateral, withdraw_debt, BAD_DEBT_REPLY_ID, CDT_DENOM, CLOSE_POSITION_REPLY_ID, LIQUIDATE_REPLY_ID, LOOP_POSITION_REPLY_ID, LTV_CHECK_REPLY_ID, NOBLE_USDC_DENOM
};
use crate::rates::{external_accrue_call, get_interest_rate, get_market_collateral_types};
use crate::reply::{handle_close_position_reply, handle_liquidation_reply, handle_loop_position_reply, handle_ltv_check_reply};
use crate::oracle::{get_asset_prices};
// use crate::query::{
//     query_basket_credit_interest, query_basket_positions, query_basket_redeemability, query_collateral_rates, simulate_LTV_mint, query_user_intent_state
// };
use crate::state::{ ContractVersion, LTVRampTimer, ACTIONS_PAUSED, CLAIM_TRACKER, CONFIG, CONTRACT, DEBT_VAULT_TOKEN, JUNIOR_CLAIM_TRACKER, JUNIOR_DEBT_VAULT_TOKEN, LTV_RAMP_TIMER, MARKET_PARAMS, OWNERSHIP_TRANSFER, POSITIONS, POSITION_UX_BOOSTS, USER_HISTORY, COLLATERAL_STATE_TOTAL};

use cosmwasm_std::Empty;

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:managed_market";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

const MAX_LIMIT: u32 = 30;

//NOTE:
// - Bc risk tranches were added later, anything debt names that are not specified are senior. Junior is explicitly specified.


#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    //Set token creation address
    let token_creation_address = if msg.token_factory_contract.is_some() {
        msg.token_factory_contract.clone().unwrap().to_string()
    } else {
        env.contract.address.to_string()
    };
    

    //Validate token factory contract
    let token_factory_contract = if msg.token_factory_contract.is_some() {
        Some(deps.api.addr_validate(&msg.token_factory_contract.clone().unwrap())?)
    } else {
        None
    };
    let protocol_revenue_collector = if msg.protocol_revenue_collector.is_some() {
        Some(deps.api.addr_validate(&msg.protocol_revenue_collector.clone().unwrap())?)
    } else {
        None
    };
    //Instantiate config
    let config = Config {
        owner: deps.api.addr_validate(&msg.owner)?,
        markets_manager_contract: info.sender.clone(),
        osmosis_proxy_contract: None,
        token_factory_contract,
        protocol_revenue_collector,
        global_rate_index: RateIndex {
            rate_index: Decimal::one(),
            last_accrued: 0u64
        },
        total_debt_tokens: Uint128::zero(),
        debt_supply_cap: msg.debt_supply_cap,
        bad_debt: Uint128::zero(),
        whitelisted_debt_suppliers: msg.clone().whitelisted_debt_suppliers,
        debt_supply_vault_token: String::from("factory/".to_owned() + token_creation_address.as_str() + "/debt-suppliers"),
        junior_debt_supply_vault_token: Some(String::from("factory/".to_owned() + token_creation_address.as_str() + "/junior-debt-suppliers")),
        junior_debt_info: Some(DebtInfo {
            total_debt: Uint128::zero(),
            bad_debt: Uint128::zero(),
        }),
        senior_debt_fixed_yield_target: Some(Decimal::percent(6)),
        manager_fee: msg.manager_fee.unwrap_or_else(|| Decimal::percent(5)),
        total_borrowed: Some(Uint128::zero()),
        debt_token: Some(msg.clone().debt_token),
        oracle_contract: Some(deps.api.addr_validate(&msg.clone().oracle_contract)?),
        swap_contract: Some(deps.api.addr_validate(&msg.clone().swap_contract)?),
    };
    CONFIG.save(deps.storage, &config)?;



    //Create the first market 
    let market = MarketParams {
        collateral_params: msg.clone().collateral_params,
        rate_params: msg.clone().rate_params,
        market_rate_index: RateIndex {
            rate_index: Decimal::one(),
            last_accrued: 0u64 //unused
        },
        total_borrowed: Uint128::zero(),
        pool_for_oracle_and_liquidations: None,
        borrow_fee: msg.clone().borrow_fee,
        whitelisted_collateral_suppliers: msg.clone().whitelisted_collateral_suppliers,
        borrow_cap: msg.clone().borrow_cap,
        max_slippage: msg.clone().max_slippage,
        per_user_debt_cap: msg.clone().per_user_debt_cap,
        debt_minimum: msg.clone().debt_minimum.unwrap_or_else(|| Uint128::one()),
    };

    //Param checks

    if market.borrow_fee > Decimal::percent(10) {
        return Err(ContractError::CustomError { val: String::from("Borrow fee cannot be greater than 10%") });
    }
    if market.collateral_params.max_borrow_LTV > Decimal::percent(100) || market.collateral_params.max_borrow_LTV >= market.collateral_params.liquidation_LTV {
        return Err(ContractError::CustomError { val: String::from("Max borrow LTV cannot be greater than 100% or greater than/equal to liquidation LTV") });
    }
    
    if market.collateral_params.liquidation_LTV.is_zero() || market.collateral_params.liquidation_LTV > Decimal::percent(100) || market.collateral_params.max_borrow_LTV >= market.collateral_params.liquidation_LTV {
        return Err(ContractError::CustomError { val: String::from("Liquidation LTV cannot be 0, greater than 100% or less than / equal to max_borrow_LTV") });
    }
    //Save market 
    MARKET_PARAMS.save(deps.storage, market.clone().collateral_params.collateral_asset, &market)?;

    //Enable the pause state object
    if msg.pause_option {
        ACTIONS_PAUSED.save(deps.storage, &false)?;
    }


    //Set contract version
    CONTRACT.save(deps.storage, &ContractVersion {
        contract: String::from(CONTRACT_NAME),
        version: String::from(CONTRACT_VERSION),
    })?;

    DEBT_VAULT_TOKEN.save(deps.storage, &Uint128::zero())?;
    JUNIOR_DEBT_VAULT_TOKEN.save(deps.storage, &Uint128::zero())?;
    // Initialize collateral state total entry for the first market created
    COLLATERAL_STATE_TOTAL.save(deps.storage, market.clone().collateral_params.collateral_asset.clone(), &Uint128::zero())?;
    CLAIM_TRACKER.save(deps.storage, &ClaimTracker {
        vt_claim_checkpoints: vec![
            VTClaimCheckpoint {
                vt_claim_of_checkpoint: Uint128::new(1_000_000), //Assumes the decimal of the deposit token is 6
                time_since_last_checkpoint: 0u64,
            }
        ],
        last_updated: env.block.time.seconds(),
    })?;
    JUNIOR_CLAIM_TRACKER.save(deps.storage, &ClaimTracker {
        vt_claim_checkpoints: vec![
            VTClaimCheckpoint {
                vt_claim_of_checkpoint: Uint128::new(1_000_000), //Assumes the decimal of the deposit token is 6
                time_since_last_checkpoint: 0u64,
            }
        ],
        last_updated: env.block.time.seconds(),
    })?;
    
    //Create Debt VT Msg

    let debt_vt_denom_msg = create_denom_msg(
        config.token_factory_contract.clone(),
        env.contract.address.as_str(),
        "debt-suppliers",
    );
    
    let junior_debt_vt_denom_msg = create_denom_msg(
        config.token_factory_contract.clone(),
        env.contract.address.as_str(),
        "junior-debt-suppliers",
    );
    

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("config", format!("{:?}", config))
        .add_attribute("contract_address", env.contract.address)
        .add_message(debt_vt_denom_msg)
        .add_message(junior_debt_vt_denom_msg)
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
        ExecuteMsg::UpdateConfig {
            owner,
            markets_manager_contract,
            oracle_contract,
            swap_contract   ,
            token_factory_contract,
            protocol_revenue_collector,
            pause_actions,
            manager_fee,
            whitelisted_debt_suppliers,
            debt_supply_cap,
            senior_debt_fixed_yield_target,
        } => update_config(deps, info, owner, markets_manager_contract, oracle_contract, swap_contract, token_factory_contract, protocol_revenue_collector, pause_actions, manager_fee, whitelisted_debt_suppliers, debt_supply_cap, senior_debt_fixed_yield_target),
        ExecuteMsg::UpdateMarket {
            collateral_denom,
            max_borrow_LTV,
            liquidation_LTV,
            rate_params,
            borrow_fee,
            whitelisted_collateral_suppliers,
            borrow_cap,
            max_slippage,
            per_user_debt_cap,
            debt_minimum
        } => update_market(deps, info, env, collateral_denom, max_borrow_LTV, liquidation_LTV, rate_params, borrow_fee, whitelisted_collateral_suppliers, borrow_cap, max_slippage, per_user_debt_cap, debt_minimum),
        ExecuteMsg::EditUXBoosts { collateral_denom, loop_ltv, take_profit_params, stop_loss_params, 
            arb_price, collateral_value_fee_to_executor } => edit_ux_boosts(deps, env, info, collateral_denom, loop_ltv, take_profit_params, stop_loss_params, arb_price, collateral_value_fee_to_executor), 
        ExecuteMsg::SupplyCollateral { owner } => supply_collateral(deps, env, info, owner),
        ExecuteMsg::SupplyDebt { send_to, is_junior } => supply_debt(deps, env, info, send_to, is_junior),
        ExecuteMsg::Borrow { collateral_denom, send_to, borrow_amount } => borrow_cdt(deps, env, info, send_to, collateral_denom, borrow_amount),
        ExecuteMsg::Liquidate { collateral_denom, position_owner, take_fee, max_slippage } => liquidate(deps, env, info, collateral_denom, position_owner, take_fee, max_slippage),
        ExecuteMsg::WithdrawCollateral { collateral_denom, send_to, withdraw_amount } => withdraw_collateral(deps, env, info, send_to, collateral_denom, withdraw_amount),
        ExecuteMsg::WithdrawDebt { send_to } => withdraw_debt(deps, env, info, send_to),
        ExecuteMsg::Repay { collateral_denom, send_excess_to } => repay_cdt(deps, env, info, collateral_denom, send_excess_to ),
        ExecuteMsg::Accrue { position_owner, collateral_denom } => external_accrue_call(deps.storage, deps.api, deps.querier, info, env, position_owner, collateral_denom),
        ExecuteMsg::ClosePosition { collateral_denom, position_owner, close_percentage, max_spread, send_to } => close_position(deps, env, info, collateral_denom, close_percentage, max_spread, send_to, position_owner),
        ExecuteMsg::LoopPosition { collateral_denom, position_owner, max_slippage } => loop_position(deps, env, info, collateral_denom, position_owner, max_slippage),
        ExecuteMsg::CrankRealizedAPR { is_junior } => crank_realized_apr(deps, env, info, is_junior),
        ExecuteMsg::ChangeAlias { collateral_denom, alias } => change_alias(deps, env, info, collateral_denom, alias),
        /////Callbacks/////
        ExecuteMsg::RateAssurance { is_junior } => rate_assurance(deps, env, info, is_junior),
        ExecuteMsg::CollateralRateAssurance {  } => collateral_rate_assurance(deps, env, info),
        ExecuteMsg::GetTotalDepositTokens { is_junior } => panic!("{:?}", get_total_debt_tokens(CONFIG.load(deps.storage)?, Some(is_junior))?),
        ExecuteMsg::CheckBadDebt {  } => check_and_fulfill_bad_debt(deps, env),

    }
}

/// Change user alias
fn change_alias(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    collateral_denom: String,
    alias: String,
) -> Result<Response, ContractError> {
    ///Get user history
    let mut user_history = match USER_HISTORY.load(deps.storage, info.sender.clone().to_string()){
        Ok(history) => history,
        Err(_) => {
            vec![UserHistory {
                collateral_denom: collateral_denom.clone(),
                alias: Some(alias.clone()),
                user: info.sender.clone().to_string(),
                volume: Decimal::zero(),
                profits: Decimal::zero(),
                losses: Decimal::zero(),
            }]
        }
    };
    //Find the collateral denom in the user history & update alias
    user_history.iter_mut().for_each(|history| {
        if history.collateral_denom == collateral_denom {
            history.alias = Some(alias.clone());
        }
    });
    USER_HISTORY.save(deps.storage, info.sender.to_string(), &user_history)?;

    Ok(Response::new().add_attributes(vec![
        attr("method", "change_alias"),
        attr("alias", alias),
    ]))
}

/// Update contract config
fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    markets_manager_contract: Option<String>,
    oracle_contract: Option<String>,
    swap_contract: Option<String>,
    token_factory_contract: Option<String>,
    protocol_revenue_collector: Option<Option<String>>,
    pause_actions: Option<bool>,
    manager_fee: Option<Decimal>,
    whitelisted_debt_suppliers: Option<Option<Vec<String>>>,
    debt_supply_cap: Option<Option<Uint128>>,
    senior_debt_fixed_yield_target: Option<Decimal>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    let mut attrs = vec![
        attr("method", "update_config"),
    ];

    //Assert Authority
    if info.sender != config.owner {
        //Check if ownership transfer is in progress & transfer if so
        let new_owner = match OWNERSHIP_TRANSFER.load(deps.storage) {
            Ok(val) => val,
            Err(_) => {
                return Err(ContractError::CustomError { val: "No ownership transfer in progress".to_string() });
            }
        };
        if info.sender == new_owner {
            config.owner = info.sender;
        } else {
            return Err(ContractError::Unauthorized { owner: new_owner.to_string() });
        }
    }
    
    if let Some(owner) = owner {
        let valid_addr = deps.api.addr_validate(&owner)?;

        //Set owner transfer state
        OWNERSHIP_TRANSFER.save(deps.storage, &valid_addr)?; 
        attrs.push(attr("owner_transfer", valid_addr));
    }
    if let Some(markets_manager_contract) = markets_manager_contract {

        let valid_addr = deps.api.addr_validate(&markets_manager_contract)?;
        config.markets_manager_contract = valid_addr.clone();
        attrs.push(attr("markets_manager_contract", valid_addr));
    }

    if pause_actions.is_some() {
        //If pauses weren't enabled at creation, you can't pause_actions
        let _ = ACTIONS_PAUSED.load(deps.storage)?; 
        ACTIONS_PAUSED.save(deps.storage, &pause_actions.unwrap())?;
        attrs.push(attr("pause_actions", format!("{:?}", pause_actions)));
    }
    
    if let Some(oracle_contract) = oracle_contract {
        let valid_addr = deps.api.addr_validate(&oracle_contract)?;
        config.oracle_contract = Some(valid_addr.clone());
        attrs.push(attr("oracle_contract", valid_addr));
    }
    if let Some(swap_contract) = swap_contract {
        let valid_addr = deps.api.addr_validate(&swap_contract)?;
        config.swap_contract = Some(valid_addr.clone());
        attrs.push(attr("swap_contract", valid_addr));
    }
    if let Some(token_factory_contract) = token_factory_contract {
        let valid_addr = deps.api.addr_validate(&token_factory_contract)?;
        config.token_factory_contract = Some(valid_addr.clone());
        attrs.push(attr("token_factory_contract", valid_addr));
    }
    if let Some(manager_fee) = manager_fee {
        config.manager_fee = manager_fee;
        attrs.push(attr("manager_fee", format!("{:?}", manager_fee)));
    }
    if let Some(whitelisted_debt_suppliers) = whitelisted_debt_suppliers {
        config.whitelisted_debt_suppliers = whitelisted_debt_suppliers.clone();
        attrs.push(attr("whitelisted_debt_suppliers", format!("{:?}", whitelisted_debt_suppliers)));
    }
    if let Some(debt_supply_cap) = debt_supply_cap {
        config.debt_supply_cap = debt_supply_cap;
        attrs.push(attr("debt_supply_cap", format!("{:?}", debt_supply_cap)));
    }
    if let Some(senior_debt_fixed_yield_target) = senior_debt_fixed_yield_target {
        config.senior_debt_fixed_yield_target = Some(senior_debt_fixed_yield_target);
        attrs.push(attr("senior_debt_fixed_yield_target", format!("{:?}", senior_debt_fixed_yield_target)));
    }
    if let Some(protocol_revenue_collector) = protocol_revenue_collector {
        if let Some(valid_addr) = protocol_revenue_collector {
            let valid_addr = deps.api.addr_validate(&valid_addr)?;
            config.protocol_revenue_collector = Some(valid_addr.clone());
            attrs.push(attr("protocol_revenue_collector", valid_addr));
        } else {
            config.protocol_revenue_collector = None;
            attrs.push(attr("protocol_revenue_collector", "None"));
        }
    }

    //Save new Config
    CONFIG.save(deps.storage, &config)?;
    
    attrs.push(
        attr("updated_config", format!("{:?}", config)));
    Ok(Response::new().add_attributes(attrs))
}

/// Sets up the LTV ramp timer in state
fn setup_ltv_ramp_timer(
    storage: &mut dyn Storage,
    env: Env,
    ltv_ramp: &LTVRamp,
    collateral_denom: String,
) -> Result<(), ContractError> {
    let start_time = env.block.time.seconds();
    let end_time = start_time + (ltv_ramp.duration_in_hours * 3600);
    let timer = LTVRampTimer {
        start_time,
        end_time,
        new_LTV: ltv_ramp.new_LTV,
    };
    LTV_RAMP_TIMER.save(storage, collateral_denom.clone(), &timer)?;
    Ok(())
}

/// Update market config
fn update_market(
    deps: DepsMut,
    info: MessageInfo,
    env: Env,
    collateral_denom: String,
    max_borrow_LTV: Option<Decimal>,
    liquidation_LTV: Option<LTVRamp>,
    rate_params: Option<RateParams>,
    borrow_fee: Option<Decimal>,
    whitelisted_collateral_suppliers: Option<Option<Vec<String>>>,
    borrow_cap: Option<BorrowCap>,
    max_slippage: Option<Decimal>,
    per_user_debt_cap: Option<Option<Uint128>>,
    debt_minimum: Option<Uint128>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    let mut attrs = vec![
        attr("method", "update_market"),
    ];

    //Check if the ltv timer is complete
    if let Ok(ltv_timer) = LTV_RAMP_TIMER.load(deps.storage, collateral_denom.clone()) {
        
        if ltv_timer.end_time <= env.block.time.seconds() {
            //If the timer is complete, set the LTV to the new LTV
        let mut market = MARKET_PARAMS.load(deps.storage, collateral_denom.clone())?;
        market.collateral_params.liquidation_LTV = ltv_timer.new_LTV;
        MARKET_PARAMS.save(deps.storage, collateral_denom.clone(), &market)?;
        LTV_RAMP_TIMER.remove(deps.storage, collateral_denom.clone());
            attrs.push(attr("ltv_ramp_completed", format!("{:?}", ltv_timer.new_LTV)));
        }
    }

    //Assert Authority
    if info.sender != config.owner {
        //Check if ownership transfer is in progress & transfer if so
        let new_owner = OWNERSHIP_TRANSFER.load(deps.storage)?;
        if info.sender == new_owner {
            config.owner = info.sender;
        } else {
            return Err(ContractError::Unauthorized { owner: new_owner.to_string() });
        }
    }
    
    //Load market
    let mut market = MARKET_PARAMS.load(deps.storage, collateral_denom.clone())?;
    //Update market
    if let Some(max_borrow_LTV) = max_borrow_LTV {
        if max_borrow_LTV > Decimal::percent(100) || max_borrow_LTV >= market.collateral_params.liquidation_LTV {
            return Err(ContractError::CustomError { val: String::from("Max borrow LTV cannot be greater than 100% or greater than/equal to liquidation LTV") });
        }
        market.collateral_params.max_borrow_LTV = max_borrow_LTV;
        attrs.push(attr("max_borrow_LTV", format!("{:?}", max_borrow_LTV)));
    }
    if let Some(liquidation_LTV) = liquidation_LTV {
        if liquidation_LTV.new_LTV.is_zero() || liquidation_LTV.new_LTV > Decimal::percent(100) || market.collateral_params.max_borrow_LTV >= liquidation_LTV.new_LTV {
            return Err(ContractError::CustomError { val: String::from("Liquidation LTV cannot be 0, greater than 100% or less than / equal to max_borrow_LTV") });
        }
        //Set up the LTV ramping timer
        setup_ltv_ramp_timer(deps.storage, env.clone(), &liquidation_LTV, collateral_denom.clone())?;
        attrs.push(attr("liquidation_LTV", format!("{:?}", liquidation_LTV)));
    }
    if let Some(rate_params) = rate_params {
        market.rate_params = rate_params.clone();
        attrs.push(attr("rate_params", format!("{:?}", rate_params)));
    }
    if let Some(borrow_fee) = borrow_fee {
        if borrow_fee > Decimal::percent(10) {
            return Err(ContractError::CustomError { val: String::from("Borrow fee cannot be greater than 10%") });
        }
        market.borrow_fee = borrow_fee;
        attrs.push(attr("borrow_fee", format!("{:?}", borrow_fee)));
    }
    if let Some(whitelisted_collateral_suppliers) = whitelisted_collateral_suppliers {
        market.whitelisted_collateral_suppliers = whitelisted_collateral_suppliers.clone();
        attrs.push(attr("whitelisted_collateral_suppliers", format!("{:?}", whitelisted_collateral_suppliers)));
    }
    if let Some(borrow_cap) = borrow_cap {
        market.borrow_cap = borrow_cap.clone();
        attrs.push(attr("borrow_cap", format!("{:?}", borrow_cap)));
    }
    if let Some(max_slippage) = max_slippage {
        market.max_slippage = max_slippage;
        attrs.push(attr("max_slippage", format!("{:?}", max_slippage)));
    }
    if let Some(per_user_debt_cap) = per_user_debt_cap {
        market.per_user_debt_cap = per_user_debt_cap;
        attrs.push(attr("per_user_debt_cap", format!("{:?}", per_user_debt_cap)));
    }
    if let Some(debt_minimum) = debt_minimum {
        market.debt_minimum = debt_minimum;
        attrs.push(attr("debt_minimum", format!("{:?}", debt_minimum)));
    }
    //Save new market
    MARKET_PARAMS.save(deps.storage, collateral_denom.clone(), &market)?;
    
    attrs.push(
        attr("updated_market", format!("{:?}", market)));
    Ok(Response::new().add_attributes(attrs))
}



#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.id {
        CLOSE_POSITION_REPLY_ID => handle_close_position_reply(deps, env, msg),
        LIQUIDATE_REPLY_ID => handle_liquidation_reply(deps, env, msg),
        LOOP_POSITION_REPLY_ID => handle_loop_position_reply(deps, env, msg),
        LTV_CHECK_REPLY_ID => handle_ltv_check_reply(deps, env, msg),
        BAD_DEBT_REPLY_ID => Ok(Response::new()),
        id => Err(StdError::generic_err(format!("invalid reply id: {}", id))),
    }
}

fn get_underlying_debt_amount(
    deps: Deps,
    _env: Env,
    vault_token_amount: Uint128,
    is_junior: bool,
) -> StdResult<Uint128> {
    let config = CONFIG.load(deps.storage)?;

    let total_debt_tokens = get_total_debt_tokens(config, Some(is_junior))?;

    Ok(calculate_base_tokens(
        vault_token_amount,
        total_debt_tokens,
        get_total_vault_tokens(deps.storage, is_junior)?
    )?)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::TotalVaultTokens { is_junior } => to_json_binary(&get_total_vault_tokens(deps.storage, is_junior)?),
        QueryMsg::GetUnderlyingDebtAmount { vault_token_amount, is_junior } => to_json_binary(&get_underlying_debt_amount(deps, env, vault_token_amount, is_junior)?),
        QueryMsg::SimulateBorrowAmount { user, collateral_denom, borrow_amount } => to_json_binary(&match simulate_borrow_amount(deps, env, user, collateral_denom, borrow_amount){
            Ok(borrowable_amount) => borrowable_amount,
            Err(err) => return Err(StdError::generic_err(format!("Error simulating borrow amount: {:?}", err))),
        }),
        QueryMsg::MarketParams { 
            start_after,
            limit,
            collateral_denom
         } => to_json_binary(&match get_market_params(deps, collateral_denom, start_after, limit){
            Ok(market) => market,
            Err(err) => return Err(StdError::generic_err(format!("Error getting market params: {:?}", err))),
        }),
        QueryMsg::GetCollateralAssets { start_after, limit } => to_json_binary(&match get_collateral_assets(deps, start_after, limit){
            Ok(assets) => assets,
            Err(err) => return Err(StdError::generic_err(format!("Error getting collateral assets: {:?}", err))),
        }),
        QueryMsg::ActionsPaused {  } => to_json_binary(&match ACTIONS_PAUSED.load(deps.storage){
            Ok(paused) => paused,
            Err(err) => return Err(StdError::generic_err(format!("Error getting actions paused state: {:?}", err))),
        }),
        QueryMsg::GetCollateralPrice { asset } => to_json_binary(&match get_asset_prices(
            deps.querier,
            CONFIG.load(deps.storage)?,
            env.contract.address.to_string(),
            true,
            vec![asset]
        ){
            Ok(prices) => prices[0].clone(),
            Err(err) => return Err(StdError::generic_err(format!("Error getting collateral price: {:?}", err))),
        }),
        QueryMsg::GetDebtPrice { } => to_json_binary(&match get_asset_prices(
            deps.querier, 
            CONFIG.load(deps.storage)?, 
            env.contract.address.to_string(),
            true, 
            vec![CONFIG.load(deps.storage)?.debt_token.clone().unwrap()]
        ){
            Ok(prices) => prices[0].clone(),
            Err(err) => return Err(StdError::generic_err(format!("Error getting debt price: {:?}", err))),
        }),
        QueryMsg::GetCurrentInterestRate { collateral_denom } => to_json_binary(&match get_interest_rate(MARKET_PARAMS.load(deps.storage, collateral_denom)?, CONFIG.load(deps.storage)?){
            Ok(rate) => rate,
            Err(err) => return Err(StdError::generic_err(format!("Error getting interest rate: {:?}", err))),
        }),
        // QueryMsg::TestDebtAllowance { collateral_denom, potential_total_debt } => {
        //     let market = MARKET_PARAMS.load(deps.storage, collateral_denom.clone())?;
            
        //     to_json_binary(&match check_debt_liquidatibility(
        //         deps.querier,
        //         market.clone(),
        //         potential_total_debt.unwrap_or_else(||  market.total_borrowed),
        //         get_contract_balances(
        //             deps.querier,
        //             env.clone(),
        //             vec![AssetInfo::NativeToken { denom: market.clone().collateral_params.collateral_asset }],
        //         )?[0]

        //     ){
        //         Ok(rate) => rate,
        //         Err(err) => return Err(StdError::generic_err(format!("Error testing debt allowance: {:?}", err))),
        //     })},
        QueryMsg::GetUserPositions { collateral_denom, user, start_after, limit } => to_json_binary(&match get_user_positions(deps, env, collateral_denom, user, start_after, limit){
            Ok(positions) => positions,
            Err(err) => return Err(StdError::generic_err(format!("Error getting user positions: {:?}", err))),
        }),
        QueryMsg::GetUserHistory { collateral_denom, user, start_after, limit } => to_json_binary(&match get_user_history(deps, env, collateral_denom, user, start_after, limit){
            Ok(history) => history,
            Err(err) => return Err(StdError::generic_err(format!("Error getting user history: {:?}", err))),
        }),
        QueryMsg::GetUserUXBoosts { collateral_denom, user, start_after, limit } => to_json_binary(&match get_user_ux_boosts(deps, env, collateral_denom, user, start_after, limit){
            Ok(ux_boosts) => ux_boosts,
            Err(err) => return Err(StdError::generic_err(format!("Error getting user ux boosts: {:?}", err))),
        }),
        QueryMsg::GetTotalBorrowed {} => to_json_binary(&get_total_borrowed(deps)?),
        QueryMsg::ClaimTracker { is_junior } => to_json_binary(&match is_junior {
            true => JUNIOR_CLAIM_TRACKER.load(deps.storage)?,
            false => CLAIM_TRACKER.load(deps.storage)?,
        }),
    }
}

fn get_user_ux_boosts(
    deps: Deps,
    env: Env,
    collateral_denom: String,
    user: Option<String>,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<Vec<UXBoosts>> {
    //If user return early
    if let Some(user) = user {
        return Ok(vec![POSITION_UX_BOOSTS.load(deps.storage, (deps.api.addr_validate(&user)?, collateral_denom.clone()))?]);
    }

    //Get limit
    let limit = limit.unwrap_or(MAX_LIMIT) as usize;

    //Get start
    let start = if let Some(start) = start_after {
        let start_after_addr = deps.api.addr_validate(&start)?;
        Some(Bound::exclusive((start_after_addr, collateral_denom)))
    } else {
        None
    };

    POSITION_UX_BOOSTS
        .range(deps.storage, start, None, Order::Ascending)
        .take(limit)
        .map(|item| {
            let (k, v) = item?;
            Ok(v)
        })
        .collect()
}

fn simulate_borrow_amount(
    deps: Deps,
    env: Env,
    user: String,
    collateral_denom: String,
    borrow_amount: BorrowOptions,
) -> StdResult<Uint128> {
    let market = MARKET_PARAMS.load(deps.storage, collateral_denom.clone())?;
    let config = CONFIG.load(deps.storage)?;
    let prices = match get_asset_prices(
        deps.querier,
        config.clone(),
        env.contract.address.to_string(),
        true,
        vec![market.clone().collateral_params.collateral_asset, config.debt_token.clone().unwrap()]
    ){
        Ok(prices) => prices,
        Err(err) => return Err(StdError::generic_err(format!("Error getting asset prices in simulate borrow amount: {:?}", err))),
    };
    let collateral_price = prices[0].clone();
    let debt_price = prices[1].clone();
    let user_position = POSITIONS.load(deps.storage, (deps.api.addr_validate(&user)?, collateral_denom.clone()))?;
    let (borrowable_amount, _) = match calc_borrowable_amount(
        deps.querier,
        env,
        borrow_amount,
        market,
        collateral_price,
        debt_price,
        user_position,
        config.debt_token.clone().unwrap()
    ){
        Ok((borrowable_amount, borrow_fee)) => (borrowable_amount, borrow_fee),
        Err(err) => return Err(StdError::generic_err(format!("Error simulating borrow amount: {:?}", err))),
    };
    Ok(borrowable_amount)
}

//Get total borrowed
fn get_total_borrowed(
    deps: Deps,
) -> StdResult<Uint128> {
    let mut total_borrowed = Uint128::zero();
    for item in MARKET_PARAMS.range(deps.storage, None, None, Order::Ascending) {
        let (_k, market) = item?;
        total_borrowed += market.total_borrowed;
    }
    Ok(total_borrowed)
}

//Get user history
fn get_user_history(
    deps: Deps,
    env: Env,
    collateral_denom: String,
    user: Option<String>,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<Vec<UserHistory>> {

    //if user is Some, return single user's history
    if let Some(user) = user {
        return Ok(USER_HISTORY.load(deps.storage, user)?);
    }

    //Get limit
    let limit = limit.unwrap_or(MAX_LIMIT) as usize;

    //Get start
    let start = if let Some(start) = start_after {
        let start_after_addr = deps.api.addr_validate(&start)?;
        Some(Bound::exclusive(start_after_addr))
    } else {
        None
    };

    USER_HISTORY
        .range(deps.storage, start, None, Order::Ascending)
        .take(limit)
        .flat_map(|item| match item {
            Ok((_k, v)) => v.into_iter().map(Ok).collect::<Vec<_>>(),
            Err(e) => vec![Err(e)],
        })
        .collect()
}   

fn get_collateral_assets(
    deps: Deps,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<Vec<String>> {
    let limit = limit.unwrap_or(MAX_LIMIT) as usize;

    let start = if let Some(start) = start_after {
        let start_after_addr = deps.api.addr_validate(&start)?;
        Some(Bound::exclusive(start_after_addr))
    } else {
        None
    };

    MARKET_PARAMS
        .keys(deps.storage, start, None, Order::Ascending)
        .take(limit)
        .map(|item| {
            let k = item?;
            Ok(k)
        })
        .collect()
}

fn get_market_params(
    deps: Deps,
    collateral_denom: Option<String>,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<Vec<MarketParams>> {
    //If some collateral denom, load single market
    if let Some(collateral_denom) = collateral_denom {
        let market = MARKET_PARAMS.load(deps.storage, collateral_denom.clone())?;
        return Ok(vec![market]);
    } 
    //If no collateral denom, return all markets
    else {
        let limit = limit.unwrap_or(MAX_LIMIT) as usize;

        let start = if let Some(start) = start_after {
            let start_after_addr = deps.api.addr_validate(&start)?;
            Some(Bound::exclusive(start_after_addr))
        } else {
            None
        };

        MARKET_PARAMS
            .range(deps.storage, start, None, Order::Ascending)
            .take(limit)
            .map(|item| {
                let (k, v) = item?;
                Ok(v)
            })
            .collect()
    }
}

fn get_user_positions(
    deps: Deps,
    env: Env,
    collateral_denom: String,
    user: Option<String>,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<Vec<UserPositionResponse>> {
    //Load market
    let market = MARKET_PARAMS.load(deps.storage, collateral_denom.clone())?;

    //If user, load single position
    if let Some(user) = user {
        let user_position = POSITIONS.load(deps.storage, (deps.api.addr_validate(&user)?, collateral_denom.clone()))?;
        let user_position_response = UserPositionResponse {
            user: user.clone(),
            position: user_position,
        };
        return Ok(vec![user_position_response]);
    } else {

    //User Positions
    let limit = limit.unwrap_or(MAX_LIMIT) as usize;

    let start = if let Some(start) = start_after {
        let start_after_addr = deps.api.addr_validate(&start)?;
        Some(Bound::exclusive((start_after_addr, collateral_denom)))
    } else {
        None
    };

    POSITIONS
        .range(deps.storage, start, None, Order::Ascending)
        .take(limit)
        .map(|item| {
            let (k, v) = item?;
            let user = k.0;
            Ok(UserPositionResponse {
                user: user.to_string(),
                position: v
            })
        })
        .collect()
    }
    
}


#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    // Get the collateral params of the first market
    let market_params = match MARKET_PARAMS
        .range(deps.storage, None, None, Order::Ascending)
        .take(1)
        .next()
    {
        Some(Ok((_k, v))) => v,
        _ => {
            return Err(ContractError::CustomError { val: "No collateral markets found".to_string() });
        }
    };
    //Load Config
    let mut config = CONFIG.load(deps.storage)?;
    // todo!();
    //Set oracle contract
    config.oracle_contract = Some(Addr::unchecked("osmo1a0k36dskvskmghhkmwtkgt2qmxpkwzfnspupl09fnsezljhxxryqu2wyxe"));
    //Set swap contract
    config.swap_contract = Some(Addr::unchecked("osmo1zwfha9a73a7wsug3vvn2mmvhp3x53v886eskrmsusyy2mgx8faxqw7fjjw"));
    //Set protocol revenue collector
    config.protocol_revenue_collector = Some(Addr::unchecked("osmo1wk0zlag50ufu5wrsfyelrylykfe3cw68fgv9s8xqj20qznhfm44qgdnq86"));
    //Set osmosis proxy contract
    config.osmosis_proxy_contract = None;
    //Set debt token
    config.debt_token = Some(CDT_DENOM.to_string());
    //Save config
    CONFIG.save(deps.storage, &config)?;

    let mut msgs: Vec<CosmosMsg<Empty>> = vec![];
    let oracle_info = market_params.clone().pool_for_oracle_and_liquidations.unwrap();
    //Add current collateral & debt tokens to oracle
    //Set debt pools
    let debt_pools = vec![
        TWAPPoolInfo {
            pool_id: 1268,
            base_asset_denom: CDT_DENOM.to_string(), 
            quote_asset_denom: NOBLE_USDC_DENOM.to_string(), 
        }
    ];
    //Add debt oracle info
    let debt_oracle_info = OsmosisOracleInfo {
        pyth_price_feed_id: None, 
        pools_for_osmo_twap: debt_pools.clone(), 
        lp_pool_info: None, 
        vault_info: None, 
        decimals: 6u64
    };

    //Add collateral asset to oracle
    let oracle_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.oracle_contract.clone().unwrap().to_string(),
        msg: to_json_binary(&OracleExecuteMsg::AddAsset {
            asset_info: market_params.clone().collateral_params.collateral_asset,
            oracle_info: OsmosisOracleInfo {
                pyth_price_feed_id: oracle_info.clone().pyth_price_feed_id,
                pools_for_osmo_twap: oracle_info.clone().pools_for_osmo_twap,
                lp_pool_info: oracle_info.clone().lp_pool_info,
                vault_info: oracle_info.clone().vault_info,
                decimals: oracle_info.clone().decimals
            },
            caller: env.contract.address.to_string(),
        })?,
        funds: vec![],
    });
    msgs.push(oracle_msg);

    //Add collateral asset to swap contract
    let swap_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.swap_contract.clone().unwrap().to_string(),
        msg: to_json_binary(&SwapExecuteMsg::AddRoute {
            caller: env.contract.address.to_string(),
            denom: market_params.clone().collateral_params.collateral_asset,
            route_info: OsmosisRouteInfo {
                pools_for_osmo_twap: oracle_info.pools_for_osmo_twap
            },
        })?,
        funds: vec![],
    });
    msgs.push(swap_msg);
    ////// Do the same for the debt contracts
    /// 
    //Add debt asset to swap contract
    let debt_swap_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.swap_contract.clone().unwrap().to_string(),
        msg: to_json_binary(&SwapExecuteMsg::AddRoute {
            caller: env.contract.address.to_string(),
            denom: config.debt_token.clone().unwrap(),
            route_info: OsmosisRouteInfo {
                pools_for_osmo_twap: debt_pools.clone()
            },
        })?,
        funds: vec![],
    });
    msgs.push(debt_swap_msg);
    //Add debt asset to oracle
    let debt_oracle_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.oracle_contract.clone().unwrap().to_string(),
        msg: to_json_binary(&OracleExecuteMsg::AddAsset {
            asset_info: config.debt_token.clone().unwrap(),
            oracle_info: debt_oracle_info,
            caller: env.contract.address.to_string(),
        })?,
        funds: vec![],
    });
    msgs.push(debt_oracle_msg);

    // //Create junior debt token 
    // let junior_debt_vt_denom_msg = create_denom_msg(
    //     config.token_factory_contract.clone(),
    //     &env.contract.address.to_string(),
    //     &String::from("junior-debt-suppliers"),
    // );
    // msgs.push(junior_debt_vt_denom_msg);
    //Create senior debt token 
    // let senior_debt_vt_denom_msg = create_denom_msg(
    //     config.token_factory_contract.clone().unwrap().to_string(),
    //     String::from("debt-suppliers"),
    // );
    // msgs.push(senior_debt_vt_denom_msg);


    Ok(Response::new()
        .add_attribute("migrate", "abstracted")
        .add_messages(msgs)
    )
}
