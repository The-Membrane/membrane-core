#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, to_json_binary, BankMsg, Binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, QuerierWrapper, Reply, Response, StdError, StdResult, Storage, SubMsg, Uint128, WasmMsg
};
use core::time;
use std::cmp::max;
use cw2::set_contract_version;
use membrane::math::{decimal_multiplication, decimal_division};

use crate::error::TokenFactoryError;
use crate::state::{APRInstance, APRTracker, APR_TRACKER, TOKEN_RATE_ASSURANCE, TokenRateAssurance, CONFIG, OWNERSHIP_TRANSFER, VAULT_TOKEN};
use membrane::mars_vault_token::{Config, ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg, APRResponse};
use membrane::mars_redbank::{QueryMsg as Mars_QueryMsg, ExecuteMsg as Mars_ExecuteMsg, UserCollateralResponse, Market};
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

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, TokenFactoryError> {
    let config = Config {
        owner: info.sender.clone(),
        mars_redbank_addr: deps.api.addr_validate(&msg.mars_redbank_addr)?,
        vault_token: String::from("factory/".to_owned() + env.contract.address.as_str() + "/" + msg.clone().vault_subdenom.as_str()),
        deposit_token: msg.clone().deposit_token,
        total_deposit_tokens: Uint128::zero(),
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
    //Create Msg
    let denom_msg = TokenFactory::MsgCreateDenom { sender: env.contract.address.to_string(), subdenom: msg.vault_subdenom.clone() };
    
    //Create Response
    let res = Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("config", format!("{:?}", config))
        .add_attribute("contract_address", env.contract.address)
        .add_attribute("sub_denom", msg.clone().vault_subdenom)
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
        ExecuteMsg::UpdateConfig { owner, mars_redbank_addr } => update_config(deps, info, owner, mars_redbank_addr),
        ExecuteMsg::EnterVault { } => enter_vault(deps, env, info),
        ExecuteMsg::ExitVault {  } => exit_vault(deps, env, info),
        ExecuteMsg::CrankAPR {  } => crank_apr(deps, env, info),
        ExecuteMsg::RateAssurance {  } => rate_assurance(deps, env, info),
    }
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
    total_deposit_tokens: Uint128,
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
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, TokenFactoryError> {
    //Load State
    let apr_tracker = APR_TRACKER.load(deps.storage)?;
    let config = CONFIG.load(deps.storage)?;
 
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
    //UNCOMMENT
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
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, TokenFactoryError> {
    let config = CONFIG.load(deps.storage)?;
    let apr_tracker = APR_TRACKER.load(deps.storage)?;
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
 
/// Update contract configuration
/// This function is only callable by an owner with non_token_contract_auth set to true
fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    mars_redbank_addr: Option<String>,
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
    }
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

//This checks the Red Bank to make sure its solvent & if not it discounts the total deposit tokens so that...
//..call users take the risk of a Red Bank insolvency instead of it being a race to withdraw
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
    //Set total deposit tokens
    let total_deposit_tokens = vault_user_info.amount;

    //Return the total deposit tokens
    Ok(total_deposit_tokens)
    
}


#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, env: Env, _msg: MigrateMsg) -> Result<Response, TokenFactoryError> {
    // // Load APR tracker
    // let mut apr_tracker = APR_TRACKER.load(deps.storage)?;
    // apr_tracker.aprs = vec![];
    // //Reset APR tracker
    // APR_TRACKER.save(deps.storage, &apr_tracker)?;

    Ok(Response::default())
}