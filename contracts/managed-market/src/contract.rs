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
use membrane::managed_market::{BorrowCap, Config, ExecuteMsg, InstantiateMsg, LTVRamp, MarketParams, MigrateMsg, QueryMsg, RateIndex, RateParams, UserPositionResponse};
use membrane::types::{
    cAsset, Asset, AssetInfo, AssetOracleInfo, Basket, ClaimTracker, UserHistory, UserInfo, VTClaimCheckpoint, UserPosition
};

use crate::error::ContractError;
use crate::positions::{
    borrow_cdt, check_and_fulfill_bad_debt, check_debt_liquidatibility, close_position, crank_realized_apr, edit_ux_boosts, get_cdt_price, get_collateral_price, get_total_debt_tokens, liquidate, loop_position, rate_assurance, repay_cdt, supply_collateral, supply_debt, withdraw_collateral, withdraw_debt, BAD_DEBT_REPLY_ID, CLOSE_POSITION_REPLY_ID, LIQUIDATE_REPLY_ID, LOOP_POSITION_REPLY_ID
};
use crate::rates::{external_accrue_call, get_interest_rate};
use crate::reply::{handle_close_position_reply, handle_liquidation_reply, handle_loop_position_reply};
// use crate::query::{
//     query_basket_credit_interest, query_basket_positions, query_basket_redeemability, query_collateral_rates, simulate_LTV_mint, query_user_intent_state
// };
use crate::state::{ ContractVersion, LTVRampTimer, ACTIONS_PAUSED, CLAIM_TRACKER, CONFIG, CONTRACT, DEBT_VAULT_TOKEN, LTV_RAMP_TIMER, MARKET_PARAMS, OWNERSHIP_TRANSFER, POSITIONS, POSITION_UX_BOOSTS, USER_HISTORY};

use osmosis_std::types::osmosis::tokenfactory::v1beta1::{self as TokenFactory};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:managed_market";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

const MAX_LIMIT: u32 = 30;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {

    
    let config = Config {
        owner: deps.api.addr_validate(&msg.owner)?,
        markets_manager_contract: info.sender.clone(),
        osmosis_proxy_contract: deps.api.addr_validate(&msg.osmosis_proxy_contract)?,
        global_rate_index: RateIndex {
            rate_index: Decimal::one(),
            last_accrued: 0u64
        },
        total_debt_tokens: Uint128::zero(),
        debt_supply_cap: msg.debt_supply_cap,
        bad_debt: Uint128::zero(),
        whitelisted_debt_suppliers: msg.clone().whitelisted_debt_suppliers,
        debt_supply_vault_token: String::from("factory/".to_owned() + env.contract.address.as_str() + "/debt-suppliers"),
        manager_fee: Decimal::percent(10),
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
        pool_for_oracle_and_liquidations: msg.clone().pool_for_oracle_and_liquidations,
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
    CLAIM_TRACKER.save(deps.storage, &ClaimTracker {
        vt_claim_checkpoints: vec![
            VTClaimCheckpoint {
                vt_claim_of_checkpoint: Uint128::new(1_000_000), //Assumes the decimal of the deposit token is 6
                time_since_last_checkpoint: 0u64,
            }
        ],
        last_updated: env.block.time.seconds(),
    })?;

    //ORACLE INFO NEEDS TO END WITH USDC
    if market.pool_for_oracle_and_liquidations.pools_for_osmo_twap[market.pool_for_oracle_and_liquidations.pools_for_osmo_twap.len()-1].quote_asset_denom 
    != String::from("ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4") {
        return Err(ContractError::CustomError { val: String::from("The last pool in the oracle pool list must be USDC") });
    }
    //ORACLE POOL ROUTE WILL BE APPENDED WITH CDT/USDC POOL 1268 FOR LIQUIDATIONS
    
    //Create Debt VT Msg
    let debt_vt_denom_msg = TokenFactory::MsgCreateDenom { sender: env.contract.address.to_string(), subdenom: String::from("debt-suppliers")};

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("config", format!("{:?}", config))
        .add_attribute("contract_address", env.contract.address)
        .add_message(debt_vt_denom_msg)
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
            osmosis_proxy_contract_addr,
            pause_actions,
            manager_fee,
            whitelisted_debt_suppliers,
            debt_supply_cap
        } => update_config(deps, info, owner, markets_manager_contract, osmosis_proxy_contract_addr, pause_actions, manager_fee, whitelisted_debt_suppliers, debt_supply_cap),
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
            pool_for_oracle_and_liquidations,
            debt_minimum
        } => update_market(deps, info, env, collateral_denom, max_borrow_LTV, liquidation_LTV, rate_params, borrow_fee, whitelisted_collateral_suppliers, borrow_cap, max_slippage, pool_for_oracle_and_liquidations, per_user_debt_cap, debt_minimum),
        ExecuteMsg::EditUXBoosts { collateral_denom, loop_ltv, take_profit_params, stop_loss_params, collateral_value_fee_to_executor } => edit_ux_boosts(deps, env, info, collateral_denom, loop_ltv, take_profit_params, stop_loss_params, collateral_value_fee_to_executor), 
        ExecuteMsg::SupplyCollateral { owner } => supply_collateral(deps, env, info, owner),
        ExecuteMsg::SupplyDebt { send_to } => supply_debt(deps, env, info, send_to),
        ExecuteMsg::Borrow { collateral_denom, send_to, borrow_amount } => borrow_cdt(deps, env, info, send_to, collateral_denom, borrow_amount),
        ExecuteMsg::Liquidate { collateral_denom, position_owner, take_fee, max_slippage } => liquidate(deps, env, info, collateral_denom, position_owner, take_fee, max_slippage),
        ExecuteMsg::WithdrawCollateral { collateral_denom, send_to, withdraw_amount } => withdraw_collateral(deps, env, info, send_to, collateral_denom, withdraw_amount),
        ExecuteMsg::WithdrawDebt { send_to } => withdraw_debt(deps, env, info, send_to),
        ExecuteMsg::Repay { collateral_denom, send_excess_to } => repay_cdt(deps, env, info, collateral_denom, send_excess_to ),
        ExecuteMsg::Accrue { position_owner, collateral_denom } => external_accrue_call(deps.storage, deps.api, deps.querier, info, env, position_owner, collateral_denom),
        ExecuteMsg::ClosePosition { collateral_denom, position_owner, close_percentage, max_spread, send_to } => close_position(deps, env, info, collateral_denom, close_percentage, max_spread, send_to, position_owner),
        ExecuteMsg::LoopPosition { collateral_denom, position_owner, max_slippage } => loop_position(deps, env, info, collateral_denom, position_owner, max_slippage),
        ExecuteMsg::CrankRealizedAPR {  } => crank_realized_apr(deps, env, info),
        ExecuteMsg::ChangeAlias { collateral_denom, alias } => change_alias(deps, env, info, collateral_denom, alias),
        /////Callbacks/////
        ExecuteMsg::RateAssurance {  } => rate_assurance(deps, env, info),
        ExecuteMsg::GetTotalDepositTokens {  } => panic!("{:?}", get_total_debt_tokens(CONFIG.load(deps.storage)?)?),
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
    osmosis_proxy_contract_addr: Option<String>,
    pause_actions: Option<bool>,
    manager_fee: Option<Decimal>,
    whitelisted_debt_suppliers: Option<Option<Vec<String>>>,
    debt_supply_cap: Option<Option<Uint128>>,
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
    
    if let Some(osmosis_proxy_contract_addr) = osmosis_proxy_contract_addr {
        let valid_addr = deps.api.addr_validate(&osmosis_proxy_contract_addr)?;
        config.osmosis_proxy_contract = valid_addr.clone();
        attrs.push(attr("osmosis_proxy_contract", valid_addr));
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
    pool_for_oracle_and_liquidations: Option<AssetOracleInfo>,
    per_user_debt_cap: Option<Option<Uint128>>,
    debt_minimum: Option<Uint128>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    let mut attrs = vec![
        attr("method", "update_market"),
    ];

    //Check if the ltv timer is complete
    if let Ok(ltv_timer) = LTV_RAMP_TIMER.load(deps.storage, collateral_denom.clone()) {
        if ltv_timer.end_time < env.block.time.seconds() {
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
    if let Some(pool_for_oracle_and_liquidations) = pool_for_oracle_and_liquidations {
        market.pool_for_oracle_and_liquidations = pool_for_oracle_and_liquidations.clone();
        attrs.push(attr("pool_for_oracle_and_liquidations", format!("{:?}", pool_for_oracle_and_liquidations)));
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
        BAD_DEBT_REPLY_ID => Ok(Response::new()),
        id => Err(StdError::generic_err(format!("invalid reply id: {}", id))),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::TotalVaultTokens {  } => to_json_binary(&DEBT_VAULT_TOKEN.load(deps.storage)?),
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
        QueryMsg::GetCollateralPrice { asset } => to_json_binary(&match get_collateral_price(deps.storage, deps.querier, env, MARKET_PARAMS.load(deps.storage, asset)?){
            Ok(price) => price,
            Err(err) => return Err(StdError::generic_err(format!("Error getting collateral price: {:?}", err))),
        }),
        QueryMsg::GetDebtPrice { } => to_json_binary(&match get_cdt_price(deps.querier, env){
            Ok(price) => price,
            Err(err) => return Err(StdError::generic_err(format!("Error getting CDT price: {:?}", err))),
        }),
        QueryMsg::GetCurrentInterestRate { collateral_denom } => to_json_binary(&match get_interest_rate(MARKET_PARAMS.load(deps.storage, collateral_denom)?, CONFIG.load(deps.storage)?){
            Ok(rate) => rate,
            Err(err) => return Err(StdError::generic_err(format!("Error getting interest rate: {:?}", err))),
        }),
        QueryMsg::TestDebtAllowance { collateral_denom, potential_total_debt } => {
            let market = MARKET_PARAMS.load(deps.storage, collateral_denom.clone())?;
            
            to_json_binary(&match check_debt_liquidatibility(
                deps.querier,
                market.clone(),
                potential_total_debt.unwrap_or_else(||  market.total_borrowed),
                get_contract_balances(
                    deps.querier,
                    env.clone(),
                    vec![AssetInfo::NativeToken { denom: market.clone().collateral_params.collateral_asset }],
                )?[0]

            ){
                Ok(rate) => rate,
                Err(err) => return Err(StdError::generic_err(format!("Error testing debt allowance: {:?}", err))),
            })},
        QueryMsg::GetUserPositions { collateral_denom, user, start_after, limit } => to_json_binary(&match get_user_positions(deps, env, collateral_denom, user, start_after, limit){
            Ok(positions) => positions,
            Err(err) => return Err(StdError::generic_err(format!("Error getting user positions: {:?}", err))),
        }),
        QueryMsg::GetUserHistory { collateral_denom, user, start_after, limit } => to_json_binary(&match get_user_history(deps, env, collateral_denom, user, start_after, limit){
            Ok(history) => history,
            Err(err) => return Err(StdError::generic_err(format!("Error getting user history: {:?}", err))),
        }),
        QueryMsg::GetUserUXBoosts { collateral_denom, user } => to_json_binary(&match POSITION_UX_BOOSTS.load(deps.storage, (deps.api.addr_validate(&user)?, collateral_denom.clone())){
            Ok(ux_boosts) => ux_boosts,
            Err(err) => return Err(StdError::generic_err(format!("Error getting user ux boosts: {:?}", err))),
        }),
        QueryMsg::ClaimTracker {} => to_json_binary(&CLAIM_TRACKER.load(deps.storage)?),
    }
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
    // use membrane::types::UserPosition;
    // use cosmwasm_std::Decimal;
    // // Save the provided user position
    // let user_addr = deps.api.addr_validate("osmo1hfv5gzmpjpgc2ml0qf87j9lrwu9dayq24m33r0")?;
    // let collateral_denom = "factory/osmo1s794h9rxggytja3a4pmwul53u98k06zy2qtrdvjnfuxruh7s8yjs6cyxgd/umbrn".to_string();
    // let user_position = UserPosition {
    //     collateral_denom: collateral_denom.clone(),
    //     collateral_amount: Uint128::new(409_547_601),
    //     debt_amount: Uint128::new(66_727),
    //     rate_index: Decimal::one(),
    // };
    // POSITIONS.save(deps.storage, (user_addr, collateral_denom), &user_position)?;
    //Return response
    Ok(Response::default())
}
