#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, to_json_binary, BankMsg, Binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, QuerierWrapper, Reply, Response, StdError, StdResult, Storage, SubMsg, Uint128, WasmMsg
};
use std::cmp::max;
use cw2::set_contract_version;
use membrane::math::{decimal_multiplication, decimal_division};

use crate::error::TokenFactoryError;
use crate::state::{APRInstance, APRTracker, APR_TRACKER, TOKEN_RATE_ASSURANCE, TokenRateAssurance, CONFIG, OWNERSHIP_TRANSFER, VAULT_TOKEN, COST_ACCRUAL, CostAccrual};
use membrane::mars_vault_token::{Config, ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg, APRResponse};
use membrane::mars_redbank::{QueryMsg as Mars_QueryMsg, ExecuteMsg as Mars_ExecuteMsg, UserCollateralResponse, Market, MarketV2Response};
use membrane::cdp::{QueryMsg as CDPQueryMsg, BasketPositionsResponse, CollateralInterestResponse};
use membrane::types::{AssetInfo, Basket, LiqAsset, Asset, DistributionEntry};
use membrane::revenue_distributor::{ExecuteMsg as RevenueDistributorExecuteMsg, RevenuePromise};
use membrane::stability_pool_vault::{
    calculate_base_tokens, calculate_vault_tokens
};
use osmosis_std::types::osmosis::tokenfactory::v1beta1::{self as TokenFactory};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:mars-vault-token";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Timeframe constants
const HOURS_PER_YEAR: usize = 8784usize; //leap year
const SECONDS_PER_HOUR: u64 = 3_600u64;
const SECONDS_PER_DAY: u64 = 86_400u64;
const SECONDS_PER_WEEK: u64 = SECONDS_PER_DAY * 7;
const SECONDS_PER_MONTH: u64 = SECONDS_PER_DAY * 30;
const SECONDS_PER_THREE_MONTHS: u64 = SECONDS_PER_DAY * 90;
const SECONDS_PER_YEAR: u64 = SECONDS_PER_DAY * 365;

//Reply IDs for cost collection
const COLLECT_COST_EXIT_REPLY_ID: u64 = 1;
const COLLECT_COST_TRANSMUTE_REPLY_ID: u64 = 2;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, TokenFactoryError> {
    // Query CDP basket to find vault token index
    let cdp_contract_addr = deps.api.addr_validate(&msg.cdp_contract_addr)?;
    let basket_query = CDPQueryMsg::GetBasket {};
    let basket: Basket = deps.querier.query_wasm_smart(
        cdp_contract_addr.clone(),
        &basket_query,
    )?;
    
    // Find vault token index by matching with mars vault token denom
    let mars_vault_addr = deps.api.addr_validate(&msg.mars_redbank_addr)?;
    let vault_token_denom = String::from("factory/".to_owned() + env.contract.address.as_str() + "/" + msg.clone().vault_subdenom.as_str());
    
    let vault_cost_index = basket.collateral_types.iter()
        .position(|c_asset| {
            match &c_asset.asset.info {
                AssetInfo::Token { address } => address == &mars_vault_addr,
                AssetInfo::NativeToken { denom } => denom == &vault_token_denom,
            }
        })
        .unwrap_or(0); // Default to 0 if not found

    let config = Config {
        owner: info.sender.clone(),
        mars_redbank_addr: mars_vault_addr,
        vault_token: vault_token_denom,
        deposit_token: msg.clone().deposit_token,
        total_deposit_tokens: Uint128::zero(),
        vault_cost: membrane::mars_vault_token::VaultCost {
            static_cost: None,
            yield_ceiling: None,
        },
        transmuter_addr: deps.api.addr_validate(&msg.transmuter_addr)?,
        revenue_distributor_addr: deps.api.addr_validate(&msg.revenue_distributor_addr)?,
        cdt_denom: msg.cdt_denom.clone(),
        cdp_contract_addr,
        vault_cost_index,
        revenue_distributions: msg.revenue_distributions.clone(),
    };
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    //Save initial state
    CONFIG.save(deps.storage, &config)?;
    APR_TRACKER.save(deps.storage, &APRTracker {
        last_total_deposit: Uint128::zero(),
        aprs: vec![],
        last_updated: env.block.time.seconds(),
    })?;
    VAULT_TOKEN.save(deps.storage, &Uint128::zero())?;  
    COST_ACCRUAL.save(deps.storage, &CostAccrual {
        revenue_vault_tokens: Uint128::zero(),
        last_updated: 0u64,
        total_cost_collected: Uint128::zero(),
    })?;
    //Create Msg
    let denom_msg = TokenFactory::MsgCreateDenom { sender: env.contract.address.to_string(), subdenom: msg.vault_subdenom.clone() };
    
    //Create Response
    let res = Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("config", format!("{:?}", config))
        .add_attribute("contract_address", env.contract.address)
        .add_attribute("sub_denom", msg.clone().vault_subdenom)
        .add_attribute("vault_cost_index", vault_cost_index.to_string())
    //UNCOMMENT
    .add_message(denom_msg);
           Ok(res)
}


#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, TokenFactoryError> {
    match msg {
        ExecuteMsg::UpdateConfig { owner, mars_redbank_addr, transmuter_addr, revenue_distributor_addr, vault_cost, cdt_denom, cdp_contract_addr, revenue_distributions } => update_config(deps, info, owner, mars_redbank_addr, transmuter_addr, revenue_distributor_addr, vault_cost, cdt_denom, cdp_contract_addr, revenue_distributions),
        ExecuteMsg::EnterVault { } => enter_vault(deps, env, info),
        ExecuteMsg::ExitVault {  } => exit_vault(deps, env, info),
        ExecuteMsg::CrankAPR {  } => crank_apr(deps, env, info),
        ExecuteMsg::CollectCost { } => collect_cost(deps, env, info),
        ExecuteMsg::RateAssurance {  } => rate_assurance(deps, env, info),
        ExecuteMsg::UpdateCDPCosts { } => update_cdp_costs(deps, env, info),
    }
}

/// Get the current vault cost rate
pub fn get_vault_cost_rate(
    deps: Deps,
    config: &Config,
) -> StdResult<Decimal> {
    let mut base_cost = Decimal::zero();
    
    // If static_cost is set, use it
    if let Some(static_cost) = config.vault_cost.static_cost {
        base_cost = static_cost;
    }
    // If yield_ceiling is set, calculate mars_apr - yield_ceiling
    else if let Some(yield_ceiling) = config.vault_cost.yield_ceiling {
        // Query Mars market for liquidity_rate (APR)
        let market: Market = deps.querier.query_wasm_smart(
            config.mars_redbank_addr.to_string(),
            &Mars_QueryMsg::Market {
                denom: config.deposit_token.clone(),
            },
        )?;
        let mars_apr = market.liquidity_rate;
        
        // Calculate max(mars_apr - yield_ceiling, 0)
        if mars_apr > yield_ceiling {
            base_cost = mars_apr - yield_ceiling;
        }
    }
    
    Ok(base_cost)
}

/// Update CDP with calculated vault cost
pub fn update_cdp_costs(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, TokenFactoryError> {
    let config = CONFIG.load(deps.storage)?;
    
    // Only contract itself can call this
    // if info.sender != env.contract.address {
    //     return Err(TokenFactoryError::Unauthorized {});
    // }
    
    // Get the current vault cost rate
    let vault_cost_rate = get_vault_cost_rate(deps.as_ref(), &config)?;
    
    // Get vault token asset string
    let vault_token_asset_string = config.vault_token;
    
    // Create EditBasket message with individual_costs
    let edit_basket_msg = membrane::cdp::ExecuteMsg::EditBasket(
        membrane::cdp::EditBasket {
            added_cAsset: None,
            liq_queue: None,
            credit_pool_infos: None,
            collateral_supply_caps: None,
            multi_asset_supply_caps: None,
            base_interest_rate: None,
            credit_asset_twap_price_source: None,
            negative_rates: None,
            cpc_margin_of_error: None,
            frozen: None,
            distribute_revenue: None,
            take_revenue: None,
            individual_costs: Some(vec![(vault_token_asset_string, vault_cost_rate)]),
            individual_cost_updaters: None,
        }
    );
    
    // Send message to CDP
    let cdp_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.cdp_contract_addr.to_string(),
        msg: to_json_binary(&edit_basket_msg)?,
        funds: vec![],
    });
    
    Ok(Response::new()
        .add_message(cdp_msg)
        .add_attribute("method", "update_cdp_costs")
        .add_attribute("vault_cost_rate", vault_cost_rate.to_string()))
}

/// Query and save new info for the APRs of the contract
fn crank_apr(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
) -> Result<Response, TokenFactoryError> {
    let apr_tracker = APR_TRACKER.load(deps.storage)?;
    let config = CONFIG.load(deps.storage)?;

    //Get the current total deposit tokens
    let total_deposit_tokens = get_total_deposit_tokens(deps.as_ref(), env.clone(), config.clone())?;

    //Calc a new APRInstance
    let apr_instance = get_apr_instance(deps.querier, config.clone(), apr_tracker.clone(), total_deposit_tokens, env.block.time.seconds())?;

    //Save the new APRInstance
    save_apr_instance(deps.storage, apr_instance.clone(), env.block.time.seconds(), total_deposit_tokens)?;

    Ok(Response::new().add_attribute("new_apr_instance", format!("{:?}", apr_instance)))
}

/// Save a new APRInstance for the APRTracker
fn save_apr_instance(
    storage: &mut dyn Storage,
    apr_instance: APRInstance,
    block_time: u64,
    total_deposit_tokens: Uint128,
) -> StdResult<()> {
    let mut apr_tracker = APR_TRACKER.load(storage)?;
    //if it hasn't been at least 1 HOUR since the last update, don't update
    if block_time - apr_tracker.last_updated < SECONDS_PER_HOUR {
        return Ok(());
    }
    apr_tracker.aprs.push(apr_instance);
    apr_tracker.last_updated = block_time;
    apr_tracker.last_total_deposit = total_deposit_tokens;

    //If we have more than 1 year of APRs, remove the oldest
    if apr_tracker.aprs.len() > HOURS_PER_YEAR {
        apr_tracker.aprs.remove(0);
    }

    //Save the updated APRTracker
    APR_TRACKER.save(storage, &apr_tracker)?;

    Ok(())
}

/// Calc a new APRInstance for the APRTracker
fn get_apr_instance(
    querier: QuerierWrapper,
    config: Config,
    apr_tracker: APRTracker,
    _total_deposit_tokens: Uint128,
    block_time: u64
) -> StdResult<APRInstance> {
    //Query APR from Mars
    let market: Market = querier.query_wasm_smart(
        config.mars_redbank_addr.to_string(),
        &Mars_QueryMsg::Market {
            denom: config.deposit_token.clone(),
        },
    )?;
    let apr = market.liquidity_rate;
    let time_since_last_update = max(block_time - apr_tracker.last_updated, 1u64);
    let apr_instance = APRInstance {
        apr_per_second: decimal_division(apr, Decimal::from_ratio(time_since_last_update.clone(), 1u64))?,
        time_since_last_update,
        apr_of_this_update: apr,
    };
    // println!("new_apr_instance: {:?}, {}, {}", apr_of_this_update, apr_tracker.last_total_deposit, total_deposit_tokens);

    Ok(apr_instance)
}

///Rate assurance
/// Ensures that the conversion rate is static for deposits & withdrawals
/// We are trusting that Mars deposits will only go up.
fn rate_assurance(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, TokenFactoryError> {
    //Load config    
    let config = CONFIG.load(deps.storage)?;

    //Error if not the contract calling
    if info.sender != env.contract.address {
        return Err(TokenFactoryError::Unauthorized {});
    }

    //Load State
    let token_rate_assurance = TOKEN_RATE_ASSURANCE.load(deps.storage)?;
    let total_vault_tokens = VAULT_TOKEN.load(deps.storage)?;

    //Get total deposit tokens
    let total_deposit_tokens = get_total_deposit_tokens(deps.as_ref(), env.clone(), config)?;

    //Calc the rate of vault tokens to deposit tokens
    let btokens_per_one = calculate_base_tokens(
        Uint128::new(1_000_000_000_000), 
        total_deposit_tokens, 
        total_vault_tokens
    )?;

    //Check that the rates are static 
    if btokens_per_one != token_rate_assurance.pre_btokens_per_one {
        return Err(TokenFactoryError::CustomError { val: format!("Deposit or withdraw rate assurance failed. Deposit tokens per 1 post-tx: {:?} --- pre-tx: {:?}", btokens_per_one, token_rate_assurance.pre_btokens_per_one) });
    }

    Ok(Response::new())
}


///Deposit the deposit_token to the vault & receive vault tokens in return
/// Send the deposit tokens to the yield strategy.
fn enter_vault(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, TokenFactoryError> {
    //Load State
    let apr_tracker = APR_TRACKER.load(deps.storage)?;
    let config = CONFIG.load(deps.storage)?;
    
    // Accrue costs first - REMOVED: Costs now tracked in CDP
    // let _cost_accrued = accrue_cost(&mut deps, env.clone(), &config)?;
 
    //Assert the only token sent is the deposit token
    if info.funds.len() != 1 {
        return Err(TokenFactoryError::CustomError { val: format!("More than 1 asset was sent, this function only accepts the deposit token: {:?}", config.clone().deposit_token) });
    }
    if info.funds[0].denom != config.deposit_token {
        return Err(TokenFactoryError::CustomError { val: format!("The wrong asset was sent ({:?}), this function only accepts the deposit token: {:?}", info.funds[0].denom, config.clone().deposit_token) });
    }
    
    //Get the amount of deposit token sent
    let deposit_amount = info.funds[0].amount;

    //////Calculate the amount of vault tokens to mint////
    //Get current total deposit tokens
    let total_deposit_tokens = get_total_deposit_tokens(deps.as_ref(), env.clone(), config.clone())?;
    
    //Calc & save new APRInstance  
    let new_apr_instance = get_apr_instance(deps.querier, config.clone(), apr_tracker.clone(), total_deposit_tokens, env.block.time.seconds())?;
    save_apr_instance(deps.storage, new_apr_instance.clone(), env.block.time.seconds(), total_deposit_tokens + deposit_amount)?;
    

    //Get the total amount of vault tokens circulating
    let total_vault_tokens: Uint128 = VAULT_TOKEN.load(deps.storage)?;
    //Calc & save base token rates
    let pre_btokens_per_one = calculate_base_tokens(
        Uint128::new(1_000_000_000_000), 
        total_deposit_tokens, 
        total_vault_tokens
    )?;
    TOKEN_RATE_ASSURANCE.save(deps.storage, &TokenRateAssurance {
        pre_btokens_per_one,
    })?;
    //Calculate the amount of vault tokens to mint
    let vault_tokens_to_distribute = calculate_vault_tokens(
        deposit_amount, 
        total_deposit_tokens, 
        total_vault_tokens
    )?;
    // println!("vault_tokens_to_distribute: {:?}, {}, {}, {}", vault_tokens_to_distribute, total_deposit_tokens, total_vault_tokens, deposit_amount);
    ////////////////////////////////////////////////////

    let mut msgs = vec![];
    //Mint vault tokens to the sender
    let mint_vault_tokens_msg: CosmosMsg = TokenFactory::MsgMint {
        sender: env.contract.address.to_string(), 
        amount: Some(osmosis_std::types::cosmos::base::v1beta1::Coin {
            denom: config.vault_token.clone(),
            amount: vault_tokens_to_distribute.to_string(),
        }), 
        mint_to_address: info.sender.to_string(),
    }.into();
    //UNCOMMENT FOR PRODUCTION
    msgs.push(mint_vault_tokens_msg);

    //Update the total vault tokens
    VAULT_TOKEN.save(deps.storage, &(total_vault_tokens + vault_tokens_to_distribute))?;

    //Save the updated config
    CONFIG.save(deps.storage, &config)?;

    //Send the deposit tokens to the yield strategy
    let send_deposit_to_yield_msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.mars_redbank_addr.to_string(),
        msg: to_json_binary(&Mars_ExecuteMsg::Deposit { 
            account_id: None,
            on_behalf_of: None,
         })?,
        funds: vec![Coin {
            denom: config.deposit_token.clone(),
            amount: deposit_amount,
        }],
    });
    msgs.push(send_deposit_to_yield_msg);
    

    //Add rate assurance callback msg
    if !total_deposit_tokens.is_zero() && !total_vault_tokens.is_zero() {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::RateAssurance { })?,
            funds: vec![],
        }));
    }

    //Create Response
    let res = Response::new()
        .add_attribute("method", "enter_vault")
        .add_attribute("deposit_amount", deposit_amount)
        .add_attribute("vault_tokens_distributed", vault_tokens_to_distribute)
        .add_attribute("deposit_sent_to_yield", deposit_amount)
        .add_messages(msgs);

    Ok(res)
}

/// User sends vault_tokens to withdraw the deposit_token from the vault
/// We burn vault tokens & unstake whatever was withdrawn
fn exit_vault(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, TokenFactoryError> {
    let config = CONFIG.load(deps.storage)?;
    let apr_tracker = APR_TRACKER.load(deps.storage)?;
    
    // Accrue costs first - REMOVED: Costs now tracked in CDP
    // let _cost_accrued = accrue_cost(&mut deps, env.clone(), &config)?;
    
    let mut msgs: Vec<CosmosMsg> = vec![];
    
    //Assert the only token sent is the vault token
    if info.funds.len() != 1 {
        return Err(TokenFactoryError::CustomError { val: format!("More than 1 asset was sent, this function only accepts the vault token: {:?}", config.clone().vault_token) });
    }
    if info.funds[0].denom != config.vault_token {
        return Err(TokenFactoryError::CustomError { val: format!("The wrong asset was sent ({:?}), this function only accepts the vault token: {:?}", info.funds[0].denom, config.clone().vault_token) });
    }

    //Get the amount of vault tokens sent
    let vault_tokens = info.funds[0].amount;
    if vault_tokens.is_zero() {
        return Err(TokenFactoryError::CustomError { val: String::from("Need to send more than 0 vault tokens") });
    }

    //////Calculate the amount of deposit tokens to withdraw////
    //Get total deposit tokens
    let total_deposit_tokens = get_total_deposit_tokens(deps.as_ref(), env.clone(), config.clone())?;
    //Get the total amount of vault tokens circulating
    let total_vault_tokens = VAULT_TOKEN.load(deps.storage)?;
    //Calc & save token rate
    let pre_btokens_per_one = calculate_base_tokens(
        Uint128::new(1_000_000_000_000), 
        total_deposit_tokens, 
        total_vault_tokens
    )?;
    TOKEN_RATE_ASSURANCE.save(deps.storage, &TokenRateAssurance {
        pre_btokens_per_one,
    })?;
    //Calculate the amount of deposit tokens to withdraw
    let deposit_tokens_to_withdraw = calculate_base_tokens(
        vault_tokens, 
        total_deposit_tokens, 
        total_vault_tokens
    )?;
    //Calc & save new APRInstance
    let new_apr_instance = get_apr_instance(deps.querier, config.clone(), apr_tracker.clone(), total_deposit_tokens, env.block.time.seconds())?;
    save_apr_instance(deps.storage, new_apr_instance.clone(), env.block.time.seconds(), total_deposit_tokens - deposit_tokens_to_withdraw)?;
    ////////////////////////////////////////////////////
    
    //Burn vault tokens
    let burn_vault_tokens_msg: CosmosMsg = TokenFactory::MsgBurn {
        sender: env.contract.address.to_string(), 
        amount: Some(osmosis_std::types::cosmos::base::v1beta1::Coin {
            denom: config.vault_token.clone(),
            amount: vault_tokens.to_string(),
        }), 
        burn_from_address: env.contract.address.to_string(),
    }.into();
    //UNCOMMENT
    msgs.push(burn_vault_tokens_msg);

    //Update the total vault tokens
    let new_vault_token_supply = match total_vault_tokens.checked_sub(vault_tokens){
        Ok(v) => v,
        Err(_) => return Err(TokenFactoryError::CustomError { val: String::from("Failed to subtract vault tokens") }),
    };
    VAULT_TOKEN.save(deps.storage, &new_vault_token_supply)?;
    //Save the updated config
    CONFIG.save(deps.storage, &config)?;

    //Withdraw tokens from Mars
    let red_bank_withdrawal = Mars_ExecuteMsg::Withdraw {
        denom: config.deposit_token.clone(),
        amount: Some(deposit_tokens_to_withdraw),
        recipient: Some(info.sender.to_string()),
        account_id: None,
        liquidation_related: None,
    };
    let red_bank_withdrawal = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.mars_redbank_addr.to_string(),
        msg: to_json_binary(&red_bank_withdrawal)?,
        funds: vec![],
    });
    // println!("deposit_tokens_to_withdraw: {:?}", deposit_tokens_to_withdraw);
    msgs.push(red_bank_withdrawal);
    
    //Add rate assurance callback msg if this withdrawal leaves other depositors with tokens to withdraw
    if !new_vault_token_supply.is_zero() && total_deposit_tokens > deposit_tokens_to_withdraw {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::RateAssurance { })?,
            funds: vec![],
        }));
    }

    //Create Response 
    let res = Response::new()
        .add_attribute("method", "exit_vault")
        .add_attribute("vault_tokens", vault_tokens)
        .add_attribute("deposit_tokens_withdrawn", deposit_tokens_to_withdraw)
        .add_messages(msgs);

    Ok(res)
}
 
/// Collect accrued vault costs by burning revenue vault tokens and swapping to CDT
fn collect_cost(
    mut deps: DepsMut,
    env: Env,
    _info: MessageInfo,
) -> Result<Response, TokenFactoryError> {
    let config = CONFIG.load(deps.storage)?;
    
    // Accrue costs first to update state - REMOVED: Costs now tracked in CDP
    // let _cost_accrued = accrue_cost(&mut deps, env.clone(), &config)?;
    
    // Get revenue vault tokens amount
    let cost_accrual = COST_ACCRUAL.load(deps.storage)?;
    let revenue_vault_tokens = cost_accrual.revenue_vault_tokens;
    
    // If zero or below minimum threshold, return early
    if revenue_vault_tokens.is_zero() {
        return Ok(Response::new()
            .add_attribute("method", "collect_cost")
            .add_attribute("revenue_vault_tokens", "0")
            .add_attribute("message", "No revenue to collect"));
    }
    
    // Calculate expected USDC using current exchange rate
    let total_deposit_tokens = get_total_deposit_tokens(deps.as_ref(), env.clone(), config.clone())?;
    let total_vault_tokens = VAULT_TOKEN.load(deps.storage)?;
    let expected_usdc = calculate_base_tokens(
        revenue_vault_tokens,
        total_deposit_tokens,
        total_vault_tokens
    )?;
    
    // Update total cost collected tracking
    let mut cost_accrual = COST_ACCRUAL.load(deps.storage)?;
    cost_accrual.total_cost_collected += expected_usdc;
    cost_accrual.revenue_vault_tokens = Uint128::zero(); // Reset to zero
    COST_ACCRUAL.save(deps.storage, &cost_accrual)?;
    
    // Create messages
    let mut msgs: Vec<CosmosMsg> = vec![];
    
    // 1. Mint revenue vault tokens to contract address
    let mint_msg: CosmosMsg = TokenFactory::MsgMint {
        sender: env.contract.address.to_string(),
        amount: Some(osmosis_std::types::cosmos::base::v1beta1::Coin {
            denom: config.vault_token.clone(),
            amount: revenue_vault_tokens.to_string(),
        }),
        mint_to_address: env.contract.address.to_string(),
    }.into();
    //UNCOMMENT FOR PRODUCTION
    msgs.push(mint_msg);
    
    // 2. Execute ExitVault on self with revenue vault tokens as funds (SubMsg)
    let exit_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&ExecuteMsg::ExitVault {})?,
        funds: vec![Coin {
            denom: config.vault_token.clone(),
            amount: revenue_vault_tokens,
        }],
    });
    
    Ok(Response::new()
        .add_attribute("method", "collect_cost")
        .add_attribute("revenue_vault_tokens", revenue_vault_tokens)
        .add_attribute("expected_usdc", expected_usdc)
        .add_messages(msgs)
        .add_submessage(SubMsg::reply_on_success(exit_msg, COLLECT_COST_EXIT_REPLY_ID)))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> Result<Response, TokenFactoryError> {
    match msg.id {
        COLLECT_COST_EXIT_REPLY_ID => handle_collect_cost_exit_reply(deps, env),
        COLLECT_COST_TRANSMUTE_REPLY_ID => handle_collect_cost_transmute_reply(deps, env),
        _ => Err(TokenFactoryError::CustomError { val: format!("Unknown reply ID: {}", msg.id) }),
    }
}

/// Handle reply from exit_vault during cost collection
fn handle_collect_cost_exit_reply(
    deps: DepsMut,
    env: Env,
) -> Result<Response, TokenFactoryError> {
    let config = CONFIG.load(deps.storage)?;
    
    // Contract now has USDC from exit_vault
    // Query contract's USDC balance
    let usdc_balance = deps.querier.query_balance(&env.contract.address, &config.deposit_token)?;
    
    if usdc_balance.amount.is_zero() {
        return Ok(Response::new()
            .add_attribute("method", "handle_collect_cost_exit_reply")
            .add_attribute("message", "No USDC received from exit"));
    }
    
    // Create submessage to transmuter
    let transmute_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.transmuter_addr.to_string(),
        msg: to_json_binary(&membrane::transmuter::ExecuteMsg::Transmute {
            recipient: Some(env.contract.address.to_string()),
        })?,
        funds: vec![Coin {
            denom: config.deposit_token.clone(),
            amount: usdc_balance.amount,
        }],
    });
    
    Ok(Response::new()
        .add_attribute("method", "handle_collect_cost_exit_reply")
        .add_attribute("usdc_amount", usdc_balance.amount)
        .add_submessage(SubMsg::reply_on_success(transmute_msg, COLLECT_COST_TRANSMUTE_REPLY_ID)))
}

/// Handle reply from transmuter during cost collection
fn handle_collect_cost_transmute_reply(
    deps: DepsMut,
    env: Env,
) -> Result<Response, TokenFactoryError> {
    let config = CONFIG.load(deps.storage)?;
    
    let cdt_balance = deps.querier.query_balance(&env.contract.address, config.cdt_denom.clone())?;
    
    if cdt_balance.amount.is_zero() {
        return Ok(Response::new()
            .add_attribute("method", "handle_collect_cost_transmute_reply")
            .add_attribute("message", "No CDT received from transmute"));
    }
    
    // Convert LiqAsset to Asset for ltv_disco_distribution
    let ltv_disco_distributions: Vec<Asset> = config.revenue_distributions.iter()
        .map(|liq_asset| {
            // Calculate amount based on ratio: cdt_amount * ratio
            let amount = cdt_balance.amount * liq_asset.amount;
            Asset {
                info: liq_asset.info.clone(),
                amount,
            }
        })
        .collect();
    
    // Create empty promises (we're only setting ltv_disco_distribution)
    let promises: Vec<RevenuePromise> = vec![];
    
    // Call SetPromises on revenue distributor
    let set_promises_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.revenue_distributor_addr.to_string(),
        msg: to_json_binary(&RevenueDistributorExecuteMsg::SetPromises {
            promises,
            ltv_disco_distribution: Some(ltv_disco_distributions),
        })?,
        funds: vec![Coin {
            denom: config.cdt_denom.clone(),
            amount: cdt_balance.amount,
        }],
    });
    
    Ok(Response::new()
        .add_attribute("method", "handle_collect_cost_transmute_reply")
        .add_attribute("cdt_amount", cdt_balance.amount)
        .add_attribute("revenue_distributor", config.revenue_distributor_addr)
        .add_message(set_promises_msg))
}

/// Update contract configuration
/// This function is only callable by an owner with non_token_contract_auth set to true
fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    mars_redbank_addr: Option<String>,
    transmuter_addr: Option<String>,
    revenue_distributor_addr: Option<String>,
    vault_cost: Option<membrane::mars_vault_token::VaultCost>,
    cdt_denom: Option<String>,
    cdp_contract_addr: Option<String>,
    revenue_distributions: Option<Vec<DistributionEntry>>,
) -> Result<Response, TokenFactoryError> {
    let mut config = CONFIG.load(deps.storage)?;

    //Assert Authority
    if info.sender != config.owner {
        //Check if ownership transfer is in progress & transfer if so
        if info.sender == OWNERSHIP_TRANSFER.load(deps.storage)? {
            config.owner = info.sender;
        } else {
            return Err(TokenFactoryError::Unauthorized {});
        }
    }

    let mut attrs = vec![attr("method", "update_config")];
    //Save optionals
    if let Some(addr) = owner {
        let valid_addr = deps.api.addr_validate(&addr)?;

        //Set owner transfer state
        OWNERSHIP_TRANSFER.save(deps.storage, &valid_addr)?;
        attrs.push(attr("owner_transfer", valid_addr));  
    }
    if let Some(addr) = mars_redbank_addr {
        config.mars_redbank_addr = deps.api.addr_validate(&addr)?;
        attrs.push(attr("updated_mars_redbank_addr", addr));
    }
    if let Some(addr) = transmuter_addr {
        config.transmuter_addr = deps.api.addr_validate(&addr)?;
        attrs.push(attr("updated_transmuter_addr", addr));
    }
    if let Some(addr) = revenue_distributor_addr {
        config.revenue_distributor_addr = deps.api.addr_validate(&addr)?;
        attrs.push(attr("updated_revenue_distributor_addr", addr));
    }
    if let Some(cost) = vault_cost {
        config.vault_cost = cost;
        attrs.push(attr("updated_vault_cost", "true"));
    }
    if let Some(denom) = cdt_denom {
        config.cdt_denom = denom.clone();
        attrs.push(attr("updated_cdt_denom", denom));
    }
    if let Some(addr) = cdp_contract_addr {
        config.cdp_contract_addr = deps.api.addr_validate(&addr)?;
        attrs.push(attr("updated_cdp_contract_addr", addr));
    }
    if let Some(distributions) = revenue_distributions {
        for entry in distributions {
            if entry.remove {
                // Remove distribution entry that matches the asset info
                config.revenue_distributions.retain(|liq_asset| liq_asset.info != entry.asset.info);
            } else {
                // Add or update distribution entry
                // First remove any existing entry with the same asset info
                config.revenue_distributions.retain(|liq_asset| liq_asset.info != entry.asset.info);
                // Then add the new entry
                config.revenue_distributions.push(entry.asset);
            }
        }
        attrs.push(attr("updated_revenue_distributions", "true"));
    }
    CONFIG.save(deps.storage, &config)?;
    attrs.push(attr("updated_config", format!("{:?}", config)));

    Ok(Response::new().add_attributes(attrs))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::VaultTokenUnderlying { vault_token_amount } => to_json_binary(&query_vault_token_underlying(deps, env, vault_token_amount)?),
        QueryMsg::DepositTokenConversion { deposit_token_amount } => to_json_binary(&query_deposit_token_conversion(deps, env, deposit_token_amount)?),
        QueryMsg::APR {} => to_json_binary(&query_apr(deps, env)?),
        QueryMsg::Cost {} => to_json_binary(&query_cost(deps)?),
    }
}

/// Return current vault cost rate
fn query_cost(
    deps: Deps,
) -> StdResult<Decimal> {
    let config = CONFIG.load(deps.storage)?;
    get_vault_cost_rate(deps, &config)
}

/// Return APR for the valid durations 7, 30, 90, 365 days
fn query_apr(
    deps: Deps,
    env: Env,
) -> StdResult<APRResponse> {
    let apr_tracker = APR_TRACKER.load(deps.storage)?;
    let config = CONFIG.load(deps.storage)?;
    let mut aprs = APRResponse {
        week_apr: None,
        month_apr: None,
        three_month_apr: None,
        year_apr: None,        
    };
    let mut running_duration = 0;
    let mut running_aprs = vec![];
    //Get total_deposit_tokens
    let total_deposit_tokens = get_total_deposit_tokens(deps, env.clone(), CONFIG.load(deps.storage)?)?;
    //Calc & add new APRInstance
    let new_apr_instance = get_apr_instance(deps.querier, config.clone(), apr_tracker.clone(), total_deposit_tokens, env.block.time.seconds())?;
    let mut apr_instances = apr_tracker.clone().aprs;
    apr_instances.push(new_apr_instance);
    //We reverse to get the most recent instances first
    apr_instances.reverse();
    
    //Parse instances to allocate APRs to the correct duration
    for apr_instance in apr_instances.into_iter() {
        running_duration += apr_instance.time_since_last_update;

        //We add the instance to calc pro-rata APRs later
        running_aprs.push(apr_instance);

        if running_duration >= SECONDS_PER_WEEK && aprs.week_apr.is_none() {
            //Calc & Set the APR for the duration
            aprs.week_apr = calc_duration_apr(running_aprs.clone(), running_duration)?;

        } else if running_duration >= SECONDS_PER_MONTH && aprs.month_apr.is_none() {            
            //Calc & Set the APR for the duration
            aprs.month_apr = calc_duration_apr(running_aprs.clone(), running_duration)?;

        } else if running_duration >= SECONDS_PER_THREE_MONTHS && aprs.three_month_apr.is_none() {
            //Calc & Set the APR for the duration
            aprs.three_month_apr = calc_duration_apr(running_aprs.clone(), running_duration)?;

        } else if running_duration >= SECONDS_PER_YEAR && aprs.year_apr.is_none() {
            //Calc & Set the APR for the duration
            aprs.year_apr = calc_duration_apr(running_aprs.clone(), running_duration)?;     

        }        
    }

    Ok(aprs)
}

fn calc_duration_apr(
    apr_instances: Vec<APRInstance>,
    duration: u64,
) -> StdResult<Option<Decimal>>{
    let mut running_apr = Decimal::zero();
    /////Find the ratio of each apr duration to the total duration////
    //Use the next time_since_last_update to calc the ratio for the previous apr
    //NOTE: we do this so if someone manipulates APR they are taking an opportunity cost to hold the rate for longer
    let mut previous_apr = Decimal::zero();
    for (index, apr_instance) in apr_instances.iter().enumerate() {
        if index == 0 {
            previous_apr = apr_instance.apr_of_this_update;
            continue;
        }
        //Calc the ratio of the previous_APR's duration to the total duration
        let ratio = Decimal::from_ratio(apr_instance.time_since_last_update, duration);
        //Add the ratio of the APR to the running APR
        running_apr += decimal_multiplication(previous_apr, ratio)?;
        
        previous_apr = apr_instance.apr_of_this_update;
    }


    Ok(Some(running_apr))
}

/// Return underlying deposit token amount for an amount of vault tokens
fn query_vault_token_underlying(
    deps: Deps,
    env: Env,
    vault_token_amount: Uint128,
) -> StdResult<Uint128> {
    let config = CONFIG.load(deps.storage)?;
    let total_vault_tokens = VAULT_TOKEN.load(deps.storage)?;

    
    //Get total deposit tokens
    let total_deposit_tokens = get_total_deposit_tokens(deps, env.clone(), config.clone())?;
    //Calc the amount of deposit tokens the user owns pre-discount
    let users_base_tokens = calculate_base_tokens(
        vault_token_amount, 
        total_deposit_tokens, 
        total_vault_tokens
    )?;
    // println!("{:?}, {}, {}, {}", users_base_tokens, total_deposit_tokens, total_vault_tokens, vault_token_amount);

    //Return the discounted amount
    Ok(users_base_tokens)
}

/// Return vault token amount for an amount of newly deposited tokens
fn query_deposit_token_conversion(
    deps: Deps,
    env: Env,
    deposit_token_amount: Uint128,
) -> StdResult<Uint128> {
    let config = CONFIG.load(deps.storage)?;
    let total_vault_tokens = VAULT_TOKEN.load(deps.storage)?;

    
    //Get total deposit tokens
    let total_deposit_tokens = get_total_deposit_tokens(deps, env.clone(), config.clone())?;
    //Calc the amount of vault tokens the user would receive for depositing
    let vault_tokens = calculate_vault_tokens(
        deposit_token_amount, 
        total_deposit_tokens, 
        total_vault_tokens
    )?;

    //Return the discounted amount
    Ok(vault_tokens)
}

/// This checks the Red Bank to make sure its solvent & if not it discounts the total deposit tokens so that...
/// ..all users take the risk of a Red Bank insolvency instead of it being a race to withdraw

/// Querying the Mars Redbank Marketv2 for total collateral and total debt allows us to calculate the expected contract balance of tokens.
/// If the expected balance is greater than the actual balance, we discount the total deposit tokens to account for this assumed loss/exploit.
/// This is hack insurance & guarantees that underlying queries return less if the Red Bank has been exploited.
/// This also allows us to price it with its actual current value and not an assumed value.
fn get_total_deposit_tokens(
    deps: Deps,
    env: Env,
    config: Config,
) -> StdResult<Uint128> {
    //Query the underlying deposit token amount from the Mars deposits
    let vault_user_info: UserCollateralResponse = match deps.querier.query_wasm_smart::<UserCollateralResponse>(
        config.mars_redbank_addr.to_string(),
        &Mars_QueryMsg::UserCollateral {
            user: env.contract.address.to_string(),
            account_id: None,
            denom: config.deposit_token.clone(),
        },
    ){
        Ok(vault_info) => vault_info,
        Err(_) => return Err(StdError::GenericErr { msg: format!("Failed to query the Mars Redbank for the vault's collateral info") }),
    };

    //Query the market v2
    let market_v2: MarketV2Response = match deps.querier.query_wasm_smart::<MarketV2Response>(
        config.mars_redbank_addr.to_string(),
        &Mars_QueryMsg::MarketV2 {
            denom: config.deposit_token.clone(),
        },
    ){
        Ok(market_v2) => market_v2,
        Err(_) => return Err(StdError::GenericErr { msg: format!("Failed to query the Mars Redbank for the vault's market v2") }),
    };
    //Query the balance of the deposit token
    let mars_deposit_token_balance: Uint128 = match deps.querier.query_balance(config.mars_redbank_addr.clone(), config.deposit_token.clone()){
        Ok(balance) => balance.amount,
        Err(_) => return Err(StdError::GenericErr { msg: format!("Failed to query the Mars Redbank for the vault's deposit token balance") }),
    };

    //Calc expected deposit token balance
    let expected_deposit_token_balance = market_v2.collateral_total_amount.checked_sub(market_v2.debt_total_amount)?;
    
    //Calc total deposit token discount based on the expected balance & actual balance
    let total_deposit_token_discount = Decimal::from_ratio(mars_deposit_token_balance, expected_deposit_token_balance);

    //Calc the total deposit tokens
    let total_deposit_tokens = decimal_multiplication(
        Decimal::from_ratio(vault_user_info.amount, Uint128::one()),
         total_deposit_token_discount
        )?;

    //Return the total deposit tokens
    Ok(total_deposit_tokens.to_uint_floor())
    
}


#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, TokenFactoryError> {
    // // Load APR tracker
    // let mut apr_tracker = APR_TRACKER.load(deps.storage)?;
    // apr_tracker.aprs = vec![];
    // //Reset APR tracker
    // APR_TRACKER.save(deps.storage, &apr_tracker)?;

    Ok(Response::default())
}