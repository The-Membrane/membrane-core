use core::panic;
use std::env;
use std::str::FromStr;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, to_json_binary, Addr, Binary, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, Order, QueryRequest, Reply, Response, StdError, StdResult, SubMsg, Uint128, WasmMsg, WasmQuery
};

use cw_storage_plus::Bound;
use membrane::auction::ExecuteMsg as AuctionExecuteMsg;
use membrane::helpers::{assert_sent_native_token_balance, get_contract_balances};
use membrane::liq_queue::ExecuteMsg as LQ_ExecuteMsg;
use membrane::managed_market::{BorrowCap, Config, ExecuteMsg, InstantiateMsg, LTVRamp, MarketParams, MigrateMsg, QueryMsg, RateIndex, RateParams, UserPositionResponse};
use membrane::types::{
    cAsset, Asset, AssetInfo, AssetOracleInfo, Basket, ClaimTracker, UserInfo, VTClaimCheckpoint
};

use crate::error::ContractError;
use crate::positions::{
    borrow_cdt, check_and_fulfill_bad_debt, check_debt_liquidatibility, crank_realized_apr, get_cdt_price, get_collateral_price, get_total_debt_tokens, liquidate, rate_assurance, repay_cdt, supply_collateral, supply_debt, withdraw_collateral, withdraw_debt, BAD_DEBT_REPLY_ID
};
use crate::rates::{external_accrue_call, get_interest_rate};
// use crate::query::{
//     query_basket_credit_interest, query_basket_positions, query_basket_redeemability, query_collateral_rates, simulate_LTV_mint, query_user_intent_state
// };
use crate::state::{ ContractVersion, ACTIONS_PAUSED, CLAIM_TRACKER, CONFIG, CONTRACT, DEBT_VAULT_TOKEN, MARKET_PARAMS, OWNERSHIP_TRANSFER, POSITIONS};

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
        osmosis_proxy_contract: deps.api.addr_validate(&msg.osmosis_proxy_contract)?,
        global_rate_index: RateIndex {
            rate_index: Decimal::one(),
            last_accrued: 0u64
        },
        total_debt_tokens: Uint128::zero(),
        debt_supply_cap: msg.debt_supply_cap,
        bad_debt: Uint128::zero(),
        whitelisted_debt_suppliers: msg.clone().whitelisted_debt_suppliers,
        debt_supply_vault_token: String::from("factory/".to_owned() + env.contract.address.as_str() + "/" + msg.clone().owner.as_str() + "/debt-suppliers"),
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
        max_slippage: Decimal::percent(20),
        per_user_debt_cap: None
    };
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
    let debt_vt_denom_msg = TokenFactory::MsgCreateDenom { sender: env.contract.address.to_string(), subdenom: (msg.clone().owner.as_str().to_owned() + "/debt-suppliers").clone() };

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
            osmosis_proxy_contract_addr,
            pause_actions,
            manager_fee,
            whitelisted_debt_suppliers,
            debt_supply_cap
        } => update_config(deps, info, owner, osmosis_proxy_contract_addr, pause_actions, manager_fee, whitelisted_debt_suppliers, debt_supply_cap),
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
            pool_for_oracle_and_liquidations
        } => update_market(deps, info, collateral_denom, max_borrow_LTV, liquidation_LTV, rate_params, borrow_fee, whitelisted_collateral_suppliers, borrow_cap, max_slippage, pool_for_oracle_and_liquidations, per_user_debt_cap),
        ExecuteMsg::SupplyCollateral { owner } => supply_collateral(deps, env, info, owner),
        ExecuteMsg::SupplyDebt { send_to } => supply_debt(deps, env, info, send_to),
        ExecuteMsg::Borrow { collateral_denom, send_to, borrow_amount } => borrow_cdt(deps, env, info, send_to, collateral_denom, borrow_amount),
        ExecuteMsg::Liquidate { collateral_denom, position_owner, take_fee, max_slippage } => liquidate(deps, env, info, collateral_denom, position_owner, take_fee, max_slippage),
        ExecuteMsg::WithdrawCollateral { collateral_denom, send_to, withdraw_amount } => withdraw_collateral(deps, env, info, send_to, collateral_denom, withdraw_amount),
        ExecuteMsg::WithdrawDebt { send_to } => withdraw_debt(deps, env, info, send_to),
        ExecuteMsg::Repay { collateral_denom} => repay_cdt(deps, env, info, collateral_denom ),
        ExecuteMsg::Accrue { position_owner, collateral_denom } => external_accrue_call(deps.storage, deps.api, deps.querier, info, env, position_owner, collateral_denom),
        ExecuteMsg::ClosePosition { position_owner, close_percentage, max_spread, send_to } => Err(ContractError::CustomError { val: String::from("ClosePosition not implemented") }),
        ExecuteMsg::CrankRealizedAPR {  } => crank_realized_apr(deps, env, info),
        /////Callbacks/////
        ExecuteMsg::RateAssurance {  } => rate_assurance(deps, env, info),
        ExecuteMsg::GetTotalDepositTokens {  } => panic!("{:?}", get_total_debt_tokens(CONFIG.load(deps.storage)?)?),
        ExecuteMsg::CheckBadDebt {  } => check_and_fulfill_bad_debt(deps, env),

    }
}

/// Update contract config
fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
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
        let new_owner = OWNERSHIP_TRANSFER.load(deps.storage)?;
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


/// Update market config
fn update_market(
    deps: DepsMut,
    info: MessageInfo,
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
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    let mut attrs = vec![
        attr("method", "update_market"),
    ];

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
    if let Some(max_borrow_LTV) = max_borrow_LTV {
        market.collateral_params.max_borrow_LTV = max_borrow_LTV;
        attrs.push(attr("max_borrow_LTV", format!("{:?}", max_borrow_LTV)));
    }
    // if let Some(liquidation_LTV) = liquidation_LTV {
    //     market.collateral_params.liquidation_LTV = liquidation_LTV;
    //     attrs.push(attr("liquidation_LTV", format!("{:?}", liquidation_LTV)));
    // }
    if let Some(rate_params) = rate_params {
        market.rate_params = rate_params.clone();
        attrs.push(attr("rate_params", format!("{:?}", rate_params)));
    }
    if let Some(borrow_fee) = borrow_fee {
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
    //Save new market
    MARKET_PARAMS.save(deps.storage, collateral_denom.clone(), &market)?;
    
    attrs.push(
        attr("updated_market", format!("{:?}", market)));
    Ok(Response::new().add_attributes(attrs))
}



#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.id {
        // 99u64 => handle_rblp_query(deps, env, msg),
        BAD_DEBT_REPLY_ID => Ok(Response::new()),
        id => Err(StdError::generic_err(format!("invalid reply id: {}", id))),
    }
}

/// Handle RBLP query
// fn handle_rblp_query(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response>{
//     //Query target position
//     let target_position = match get_target_position(deps.storage, Addr::unchecked("osmo1988s5h45qwkaqch8km4ceagw2e08vdw28mwk4n"), Uint128::new(1u128)){
//         Ok((_i, pos)) => pos,
//         Err(_) => panic!("No target position found"),
//     };


//     //Query RBLP's UserIntentState to see if the user has funds sitting in the vault
//     let user_intents: Vec<UserIntentResponse> = match deps.querier
//         .query::<Vec<UserIntentResponse>>(&QueryRequest::Wasm(WasmQuery::Smart {
//             contract_addr: "osmo17rvvd6jc9javy3ytr0cjcypxs20ru22kkhrpwx7j3ym02znuz0vqa37ffx".to_string(),
//             msg: to_json_binary(&RBLP_QueryMsg::GetUserIntent { 
//                 start_after: None, 
//                 limit: None, 
//                 users: vec!["osmo1988s5h45qwkaqch8km4ceagw2e08vdw28mwk4n".to_string()],
//             })?,
//         })){
//             Ok(res) => res,
//             Err(_) => vec![],
//         };
//     let user_intent: UserIntentResponse = if user_intents.len() > 0 {user_intents[0].clone()} else {
//         panic!("UserIntent: {:?}, Target Position: {:?}", Vec::<UserIntentResponse>::new(), target_position);
//     };

//     panic!("UserIntent: {:?}, Target Position: {:?}", user_intent, target_position);

    
//     //Return response
//     Ok(Response::new())
// }

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::MarketParams { collateral_denom } => to_json_binary(&match MARKET_PARAMS.load(deps.storage, collateral_denom.clone()){
            Ok(market) => market,
            Err(err) => return Err(StdError::generic_err(format!("Error getting market params: {:?}", err))),
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
        QueryMsg::ClaimTracker {} => to_json_binary(&CLAIM_TRACKER.load(deps.storage)?),
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
    
    //Return response
    Ok(Response::default())
}
