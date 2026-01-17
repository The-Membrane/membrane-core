#![allow(unused_variables)]
use cosmwasm_std::{entry_point, to_json_binary, Addr, Binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdResult, SubMsg, Uint128, WasmMsg};
use cw2::set_contract_version;

use membrane::yield_arb::{InstantiateMsg, ExecuteMsg, QueryMsg, Config, UserPosition, MarketConditions};
use membrane::mars_vault_token::{QueryMsg as MarsQueryMsg, APRResponse, Config as MarsConfig};
use membrane::cdp::{QueryMsg as CDPQueryMsg, CollateralInterestResponse, BasketPositionsResponse, PositionResponse};
use membrane::transmuter::{ExecuteMsg as TransmuterExecuteMsg, QueryMsg as TransmuterQueryMsg, Config as TransmuterConfig};
use membrane::cdp::ExecuteMsg as CDPExecuteMsg;
use membrane::types::{AssetInfo, Basket, UserInfo};

use crate::error::ContractError;
use crate::state::{CONFIG, USER_POSITIONS, MARKET_CONDITIONS, TVL_TRACKER, LOOP_USER, DEPLOYMENT_SNAPSHOTS, LOOP_CDT_AMOUNT};
use crate::helpers::{append_user_position, append_market_conditions, try_update_tvl, get_current_tvl, update_deployment_snapshot};

const CONTRACT_NAME: &str = "crates.io:yield-arb";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

// Reply IDs for future async flows
const SWAP_CDT_REPLY_ID: u64 = 1u64;
const DEPOSIT_MARS_REPLY_ID: u64 = 2u64;
const DEPOSIT_CDP_REPLY_ID: u64 = 3u64;

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> StdResult<Response> {
    let owner = if let Some(o) = msg.owner { 
        deps.api.addr_validate(&o)? 
    } else { 
        info.sender.clone() 
    };
    
    // Query CDP basket to find vault token index
    let cdp_contract_addr = deps.api.addr_validate(&msg.cdp_contract_addr)?;
    let basket_query = CDPQueryMsg::GetBasket {};
    let basket: Basket = deps.querier.query_wasm_smart(
        cdp_contract_addr.clone(),
        &basket_query,
    )?;
    
    // Find vault token index by matching with mars vault token denom
    let mars_vault_addr = deps.api.addr_validate(&msg.mars_vault_addr)?;
    let mars_cfg: MarsConfig = deps.querier.query_wasm_smart(
        mars_vault_addr.clone(),
        &MarsQueryMsg::Config {},
    )?;
    
    let vault_cost_index = basket.collateral_types.iter()
        .position(|c_asset| {
            match &c_asset.asset.info {
                AssetInfo::Token { address } => address == &mars_vault_addr,
                AssetInfo::NativeToken { denom } => denom == &mars_cfg.vault_token,
            }
        })
        .unwrap_or(0); // Default to 0 if not found

    //Get CDT and USDC denom from Transmuter Config
    let transmuter_addr = deps.api.addr_validate(&msg.transmuter_addr)?;
    let transmuter_cfg: TransmuterConfig = deps.querier.query_wasm_smart(
        transmuter_addr.clone(),
        &TransmuterQueryMsg::Config { },
    )?;
    let cdt_denom = transmuter_cfg.deposit_pair.clone().cdt;
    let usdc_denom = transmuter_cfg.deposit_pair.paired_asset.clone();
    
    let config = Config {
        owner,
        cdt_denom,
        usdc_denom,
        vault_token_denom: mars_cfg.vault_token.clone(),
        mars_vault_addr,
        cdp_contract_addr,
        transmuter_addr,
        vault_cost_index,
    };
    CONFIG.save(deps.storage, &config)?;
    MARKET_CONDITIONS.save(deps.storage, &vec![])?;
    TVL_TRACKER.save(deps.storage, &vec![])?;

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("vault_cost_index", vault_cost_index.to_string()))
}

#[entry_point]
pub fn execute(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::EnterVault { leave_vault_tokens_in_vault: _ } => enter_vault(deps, env, info),
        ExecuteMsg::RepayUserDebt { user_info: _, repayment: _ } => repay_user_debt(deps, env, info),
        ExecuteMsg::UpdateMarketConditions { } => update_market_conditions(&mut deps, env, info),
        ExecuteMsg::UpdateConfig { owner, cdt_denom, usdc_denom, mars_vault_addr, cdp_contract_addr, transmuter_addr, vault_cost_index } => update_config(deps, env, info, owner, cdt_denom, usdc_denom, mars_vault_addr, cdp_contract_addr, transmuter_addr, vault_cost_index),
    }
}

fn enter_vault(deps: DepsMut, env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    let cfg = CONFIG.load(deps.storage)?;
    
    // Amount of CDT provided
    let cdt_amount: Uint128 = info.funds
        .iter()
        .find(|c| c.denom == cfg.cdt_denom)
        .map(|c| c.amount)
        .unwrap_or_else(|| Uint128::zero());
    
    let response = Response::new()
        .add_attribute("action", "enter_vault")
        .add_attribute("user", info.sender.to_string());
    
    // Check if user already has positions in our Map
    let user_key = info.sender.to_string();
    let existing_user_positions = USER_POSITIONS.may_load(deps.storage, user_key.clone())?;
    
    // Check if this is the first loop (no deployment snapshot exists)
    let is_first_loop = DEPLOYMENT_SNAPSHOTS.may_load(deps.storage, user_key.clone())?.is_none();
    
    let position_id = if let Some(positions) = existing_user_positions {
        // User has existing positions in our Map, use the latest position_id
        if let Some(latest_position) = positions.last() {
            latest_position.position_id
        } else {
            Uint128::zero()
        }
    } else {
        // User not found in our Map, query CDP for existing positions
        let position_query = CDPQueryMsg::GetBasketPositions {
            start_after: None,
            limit: None,
            user_info: None,
            user: Some(info.sender.to_string()),
        };
        let existing_positions: Result<BasketPositionsResponse, _> = deps.querier.query_wasm_smart(
            cfg.cdp_contract_addr.clone(),
            &position_query,
        );
        
        // If no existing positions, return error
        if existing_positions.is_err() || existing_positions.as_ref().unwrap_or(&BasketPositionsResponse { user: "".to_string(), positions: vec![] }).positions.is_empty() {
            return Err(ContractError::CustomError {
                msg: "No existing position".to_string(),
            });
        }
        
        // Get the position whose collateral is only the vault token
        let positions = existing_positions.unwrap();
        let position = positions.positions.iter().find(|p| 
            p.collateral_assets.len() == 1 
            && 
            p.collateral_assets[0].asset.info == AssetInfo::NativeToken { denom: cfg.vault_token_denom.clone() });
        if position.is_none() {
            return Err(ContractError::CustomError {
                msg: "No position with only the vault token".to_string(),
            });
        }
        
        position.unwrap().position_id
    };
    
    // If first loop, save initial collateral assets and time
    if is_first_loop {
        let position_query = CDPQueryMsg::GetBasketPositions {
            start_after: None,
            limit: None,
            user_info: Some(UserInfo {
                position_owner: info.sender.to_string(),
                position_id,
            }),
            user: None,
        };
        let position_resp: BasketPositionsResponse = deps.querier.query_wasm_smart(
            cfg.cdp_contract_addr.clone(),
            &position_query,
        ).unwrap_or(BasketPositionsResponse { 
            user: info.sender.to_string(),  
            positions: vec![] 
        });
        
        if let Some(position) = position_resp.positions.first() {
            update_deployment_snapshot(
                deps.storage,
                user_key.clone(),
                Some(position.collateral_assets.clone()), // Save initial collateral
                Some(env.block.time.seconds()), // Save initial time
                Uint128::zero(), // Will be updated in reply
                Uint128::zero(), // Will be updated in reply
            )?;
        }
    }
    
    // Save user for multi-reply flow
    let user_info = UserInfo {
        position_id,
        position_owner: info.sender.to_string(),
    };
    LOOP_USER.save(deps.storage, &user_info)?;
    // Initialize first snapshot for user
    let snapshot = UserPosition {
        user: info.sender.clone(),
        collateral_amount: Uint128::zero(),
        debt_amount: Uint128::zero(),
        position_id: Uint128::zero(),
        timestamp: env.block.time.seconds(),
    };
    let mut deps_mut = deps;
    append_user_position(&mut deps_mut, snapshot)?;
    
    // Optional TVL snapshot if a day passed
    let tvl = get_current_tvl(deps_mut.as_ref());
    let _ = try_update_tvl(&mut deps_mut, env.clone(), tvl);
    
    // If no CDT funds provided, just return the response without transmutation
    if cdt_amount.is_zero() {
        return Ok(response.add_attribute("cdt_amount", "0"));
    }

    // Check transmuter balance and adjust CDT amount to available balance if needed
    let transmuter_balance = deps_mut.querier.query_balance(&cfg.transmuter_addr, &cfg.usdc_denom)?;
    // Cap CDT amount to available transmuter balance (1:1 with USDC)
    let cdt_amount = cdt_amount.min(transmuter_balance.amount);
    
    // If no CDT amount available after adjustment, return early
    if cdt_amount.is_zero() {
        return Ok(response.add_attribute("cdt_amount", "0").add_attribute("reason", "insufficient_transmuter_balance"));
    }
    
    // Save adjusted CDT amount for later use in reply
    LOOP_CDT_AMOUNT.save(deps_mut.storage, &cdt_amount)?;

    // Transmute CDT -> USDC (paired asset) back to this contract
    let transmute_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: cfg.transmuter_addr.to_string(),
        msg: to_json_binary(&TransmuterExecuteMsg::Transmute { 
            recipient: Some(env.contract.address.to_string()) 
        })?,
        funds: vec![
            Coin { 
                denom: cfg.cdt_denom.clone(), 
                amount: cdt_amount 
            }],
    });
    let sub = SubMsg::reply_on_success(transmute_msg, SWAP_CDT_REPLY_ID);

    Ok(response
        .add_attribute("cdt_amount", cdt_amount)
        .add_submessage(sub))
}

fn repay_user_debt(_deps: DepsMut, _env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    Ok(Response::new().add_attribute("action", "repay_user_debt").add_attribute("user", info.sender))
}

fn update_market_conditions(deps: &mut DepsMut, env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    let cfg = CONFIG.load(deps.storage)?;
    // CDT mint cost from CDP
    let cdp_resp: CollateralInterestResponse = deps.querier.query_wasm_smart(
        cfg.cdp_contract_addr.clone(),
        &CDPQueryMsg::GetCollateralInterest { },
    ).unwrap_or(CollateralInterestResponse { rates: vec![] });
    let cdt_mint_cost = cdp_resp.rates.get(cfg.vault_cost_index).cloned().unwrap_or_else(Decimal::zero);
    
    // APR from mars vault
    let apr_resp: APRResponse = deps.querier.query_wasm_smart(
        cfg.mars_vault_addr.clone(),
        &MarsQueryMsg::APR { },
    ).unwrap_or(APRResponse { week_apr: None, month_apr: None, three_month_apr: None, year_apr: None });
    let vault_apr = apr_resp.year_apr.unwrap_or_else(Decimal::zero);

    // Vault cost via Cost query on mars vault token; default to zero if unavailable
    let vault_cost: Decimal = deps.querier.query_wasm_smart(
        cfg.mars_vault_addr.clone(),
        &MarsQueryMsg::Cost { },
    ).unwrap_or_else(|_| Decimal::zero());
    
    //Append market data
    let mc = MarketConditions { cdt_mint_cost, vault_apr, vault_cost, timestamp: env.block.time.seconds() };
    let mut deps_mut = deps;
    append_market_conditions(&mut deps_mut, mc)?;

    Ok(Response::new().add_attribute("action", "update_market_conditions"))
}

fn update_config(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    owner: Option<String>,
    cdt_denom: Option<String>,
    usdc_denom: Option<String>,
    mars_vault_addr: Option<String>,
    cdp_contract_addr: Option<String>,
    transmuter_addr: Option<String>,
    vault_cost_index: Option<usize>,
) -> Result<Response, ContractError> {
    let mut cfg = CONFIG.load(deps.storage)?;
    if info.sender != cfg.owner { return Err(ContractError::Unauthorized); }
    if let Some(o) = owner { cfg.owner = deps.api.addr_validate(&o)?; }
    if let Some(d) = cdt_denom { cfg.cdt_denom = d; }
    if let Some(d) = usdc_denom { cfg.usdc_denom = d; }
    if let Some(a) = mars_vault_addr { cfg.mars_vault_addr = deps.api.addr_validate(&a)?; }
    if let Some(a) = cdp_contract_addr { cfg.cdp_contract_addr = deps.api.addr_validate(&a)?; }
    if let Some(a) = transmuter_addr { cfg.transmuter_addr = deps.api.addr_validate(&a)?; }
    if let Some(i) = vault_cost_index { cfg.vault_cost_index = i; }
    CONFIG.save(deps.storage, &cfg)?;
    Ok(Response::new().add_attribute("action", "update_config"))
}

// Reply handler functions
fn handle_swap_cdt_reply(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
    // After transmute, deposit USDC into mars vault
    let cfg = CONFIG.load(deps.storage)?;
    // Query mars vault config for deposit token denom
    let mars_cfg: MarsConfig = deps.querier.query_wasm_smart(
        cfg.mars_vault_addr.to_string(), 
        &MarsQueryMsg::Config { }
    )?;
    println!("mars_cfg.env.contract.address: {}", env.contract.address);
    let deposit_balance = deps.querier.query_balance(&env.contract.address, &mars_cfg.deposit_token)?.amount;
    println!("deposit_balance: {}", deposit_balance);
    if deposit_balance.is_zero() {
        return Ok(Response::new().add_attribute("reply", "swap_cdt_no_deposit_balance"));
    }

    let deposit_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: cfg.mars_vault_addr.to_string(),
        msg: to_json_binary(&membrane::mars_vault_token::ExecuteMsg::EnterVault { })?,
        funds: vec![Coin { denom: mars_cfg.deposit_token.clone(), amount: deposit_balance }],
    });
    let sub = SubMsg::reply_on_success(deposit_msg, DEPOSIT_MARS_REPLY_ID);
    Ok(Response::new()
        .add_attribute("reply", "swap_cdt")
        .add_attribute("deposit_amount", deposit_balance)
        .add_submessage(sub))
}

fn handle_deposit_mars_reply(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
    // After deposit into mars vault, send vault tokens to user's CDP
    let cfg = CONFIG.load(deps.storage)?;
    let mars_cfg: MarsConfig = deps.querier.query_wasm_smart(
        cfg.mars_vault_addr.to_string(), 
        &MarsQueryMsg::Config { }
    )?;
    // mars_cfg.vault_token is string denom for vault token
    let vt_balance = deps.querier.query_balance(&env.contract.address, &mars_cfg.vault_token)?.amount;
    if vt_balance.is_zero() {
        return Ok(Response::new().add_attribute("reply", "deposit_mars_no_vt_balance"));
    }
    //Get the user address from the loop user state
    let user = LOOP_USER.load(deps.storage)?;
    //Deposit the vault tokens into the CDP
    let cdp_deposit = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: cfg.cdp_contract_addr.to_string(),
        msg: to_json_binary(&CDPExecuteMsg::Deposit { 
            position_id: Some(user.position_id),
            position_owner: Some(user.position_owner.clone()),
            affiliate_address: Some(env.contract.address.to_string()),
        })?,
        funds: vec![Coin { denom: mars_cfg.vault_token.clone(), amount: vt_balance }],
    });
    let sub = SubMsg::reply_on_success(cdp_deposit, DEPOSIT_CDP_REPLY_ID);
    Ok(Response::new()
        .add_attribute("reply", "deposit_mars")
        .add_attribute("vt_amount", vt_balance)
        .add_submessage(sub))
}

fn handle_deposit_cdp_reply(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
    // Finalize: query CDP position and append snapshot with real data
    let cfg = CONFIG.load(deps.storage)?;
    let user = LOOP_USER.load(deps.storage)?;
    
    // Get the CDT amount that was looped
    let cdt_amount_looped = LOOP_CDT_AMOUNT.load(deps.storage)?;
    
    // Query user's CDP position for real collateral/debt/position_id
    let position_query = CDPQueryMsg::GetBasketPositions {
        start_after: None,
        limit: None,
        user_info: Some(
            UserInfo {
                position_owner: user.position_owner.clone(), 
                position_id: user.position_id 
            }),
        user: None,
    };
    let position_resp: BasketPositionsResponse = deps.querier.query_wasm_smart(
        cfg.cdp_contract_addr.clone(),
        &position_query,
    ).unwrap_or(BasketPositionsResponse { 
        user: user.position_owner.clone(),  
        positions: vec![] 
    });
    
    // Get the most recent position or default values
    let default_position = PositionResponse {
        position_id: Uint128::zero(),
        collateral_assets: vec![],
        cAsset_ratios: vec![],
        credit_amount: Uint128::zero(),
        avg_borrow_LTV: Decimal::zero(),
        avg_max_LTV: Decimal::zero(),
        deployed_to: vec![],
        pending_interest: Uint128::zero(),
        total_interest_accrued: Uint128::zero(),
    };
    let position = position_resp.positions.first().unwrap_or(&default_position);
    
    // Calculate total collateral from collateral assets
    let total_collateral = position.collateral_assets.iter()
        .map(|asset| asset.asset.amount)
        .sum::<Uint128>();
    
    let snapshot = UserPosition {
        user: Addr::unchecked(user.position_owner.clone()),
        collateral_amount: total_collateral,
        debt_amount: position.credit_amount,
        position_id: user.position_id,
        timestamp: env.block.time.seconds(),
    };
    
    // Append user position snapshot
    let mut deps_mut = deps;
    append_user_position(&mut deps_mut, snapshot)?;
    
    // Update deployment snapshot with loop amount and debt (collateral_assets and time only set on first loop)
    // Get existing snapshot to calculate cumulative amount_looped
    let existing_snapshot = DEPLOYMENT_SNAPSHOTS.may_load(deps_mut.storage, user.position_owner.clone())?;
    let cumulative_amount_looped = if let Some(existing) = existing_snapshot {
        existing.amount_looped + cdt_amount_looped
    } else {
        cdt_amount_looped
    };
    
    update_deployment_snapshot(
        deps_mut.storage,
        user.position_owner.clone(),
        None, // Don't update collateral_assets (only set on first loop)
        None, // Don't update block_time (only set on first loop)
        cumulative_amount_looped, // Update cumulative amount looped
        position.credit_amount, // Update current debt taken
    )?;
    
    // Update market conditions if needed
    update_market_conditions(&mut deps_mut, env.clone(), MessageInfo { sender: env.contract.address.clone(), funds: vec![] })?;
    
    // Try to update TVL if a day has passed
    let current_tvl = get_current_tvl(deps_mut.as_ref());
    try_update_tvl(&mut deps_mut, env.clone(), current_tvl)?;
    
    Ok(Response::new()
        .add_attribute("reply", "deposit_cdp")
        .add_attribute("position_id", position.position_id)
        .add_attribute("collateral", total_collateral)
        .add_attribute("debt", position.credit_amount))
}

fn query_user_positions(
    deps: Deps,
    user: Option<String>,
    limit: Option<u32>,
    start_after: Option<u64>,
) -> StdResult<Binary> {
    let limit = limit.unwrap_or(50) as usize;
    let start_after = start_after.unwrap_or(0);
    
    if let Some(user_addr) = user {
        // Query specific user's positions
        let user_key = user_addr.clone();
        let positions = USER_POSITIONS.may_load(deps.storage, user_key.clone())?.unwrap_or_default();
        let filtered: Vec<_> = positions.into_iter()
            .filter(|p| p.timestamp > start_after)
            .take(limit)
            .collect();
        to_json_binary(&filtered)
    } else {
        // Query all users' positions (flattened)
        let mut all_positions = Vec::new();
        let range = USER_POSITIONS.range(deps.storage, None, None, cosmwasm_std::Order::Ascending);
        for item in range {
            let (_, positions): (String, Vec<UserPosition>) = item?;
            all_positions.extend(positions);
        }
        let filtered: Vec<_> = all_positions.into_iter()
            .filter(|p| p.timestamp > start_after)
            .take(limit)
            .collect();
        to_json_binary(&filtered)
    }
}

fn query_market_conditions(
    deps: Deps,
    limit: Option<u32>,
    start_after: Option<u64>,
) -> StdResult<Binary> {
    let limit = limit.unwrap_or(50) as usize;
    let start_after = start_after.unwrap_or(0);
    let vec = MARKET_CONDITIONS.may_load(deps.storage)?.unwrap_or_default();
    let filtered: Vec<_> = vec.into_iter()
        .filter(|mc| mc.timestamp > start_after)
        .take(limit)
        .collect();
    to_json_binary(&filtered)
}

fn query_tvl_history(
    deps: Deps,
    limit: Option<u32>,
    start_after: Option<u64>,
) -> StdResult<Binary> {
    let limit = limit.unwrap_or(50) as usize;
    let start_after = start_after.unwrap_or(0);
    let vec = TVL_TRACKER.may_load(deps.storage)?.unwrap_or_default();
    let filtered: Vec<_> = vec.into_iter()
        .filter(|s| s.timestamp > start_after)
        .take(limit)
        .collect();
    to_json_binary(&filtered)
}

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::RetrievableCDT { user: _ } => to_json_binary(&Uint128::zero()),
        QueryMsg::VaultTokenUnderlying { vault_token_amount: _ } => {
            // let cfg = CONFIG.load(deps.storage)?;
            // let resp: Uint128 = deps.querier.query_wasm_smart(
            //     cfg.mars_vault_addr,
            //     &MarsQueryMsg::VaultTokenUnderlying { vault_token_amount },
            // )?;
            to_json_binary(&Uint128::zero())
        },
        QueryMsg::DepositTokenConversion { deposit_token_amount: _ } => {
            // let cfg = CONFIG.load(deps.storage)?;
            // let resp: Uint128 = deps.querier.query_wasm_smart(
            //     cfg.mars_vault_addr,
            //     &MarsQueryMsg::DepositTokenConversion { deposit_token_amount },
            // )?;
            to_json_binary(&Uint128::zero())

        },
        QueryMsg::GetUserPositions { user, limit, start_after } => {
            query_user_positions(deps, user, limit, start_after)
        },
        QueryMsg::GetMarketConditions { limit, start_after } => {
            query_market_conditions(deps, limit, start_after)
        },
        QueryMsg::GetTVLHistory { limit, start_after } => {
            query_tvl_history(deps, limit, start_after)
        },
        QueryMsg::Config { } => {
            to_json_binary(&CONFIG.load(deps.storage)?)
        },
        QueryMsg::GetDeploymentSnapshot { user } => {
            let snapshot = DEPLOYMENT_SNAPSHOTS
                .may_load(deps.storage, user)?;
            to_json_binary(&snapshot)
        },
    }
}

#[entry_point]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> Result<Response, ContractError> {
    match msg.id as u64 {
        SWAP_CDT_REPLY_ID => handle_swap_cdt_reply(deps, env),
        DEPOSIT_MARS_REPLY_ID => handle_deposit_mars_reply(deps, env),
        DEPOSIT_CDP_REPLY_ID => handle_deposit_cdp_reply(deps, env),
        _ => Ok(Response::new().add_attribute("reply", "unknown")),
    }
}


