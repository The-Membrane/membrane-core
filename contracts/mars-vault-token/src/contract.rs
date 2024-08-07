#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, to_json_binary, BankMsg, Binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdError, StdResult, Storage, SubMsg, Uint128, WasmMsg
};use std::cmp::max;
use cw2::set_contract_version;
use membrane::math::{decimal_multiplication, decimal_division};

use crate::error::TokenFactoryError;
use crate::state::{APRInstance, APRTracker, APR_TRACKER, TOKEN_RATE_ASSURANCE, TokenRateAssurance, CONFIG, OWNERSHIP_TRANSFER, VAULT_TOKEN};
use membrane::mars_vault_token::{Config, ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg};
use membrane::mars_redbank::{QueryMsg as Mars_QueryMsg, ExecuteMsg as Mars_ExecuteMsg, UserCollateralResponse, Market};
use membrane::stability_pool_vault::{
    calculate_base_tokens, calculate_vault_tokens, APRResponse
};
use osmosis_std::types::osmosis::tokenfactory::v1beta1::{self as TokenFactory};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:mars-vault-token";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Timeframe constants
const SECONDS_PER_DAY: u64 = 86_400u64;

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
    let new_apr_instance = calc_apr_instance(apr_tracker.clone(), total_deposit_tokens, env.block.time.seconds())?;

    //Save the new APRInstance
    save_apr_instance(deps.storage, new_apr_instance.clone(), env.block.time.seconds(), total_deposit_tokens)?;

    Ok(Response::new().add_attribute("new_apr_instance", format!("{:?}", new_apr_instance)))
}

/// Save a new APRInstance for the APRTracker
fn save_apr_instance(
    storage: &mut dyn Storage,
    apr_instance: APRInstance,
    block_time: u64,
    total_deposit_tokens: Uint128,
) -> StdResult<()> {
    let mut apr_tracker = APR_TRACKER.load(storage)?;
    apr_tracker.aprs.push(apr_instance);
    apr_tracker.last_updated = block_time;
    apr_tracker.last_total_deposit = total_deposit_tokens;
    APR_TRACKER.save(storage, &apr_tracker)?;

    Ok(())
}

/// Calc a new APRInstance for the APRTracker
fn calc_apr_instance(
    apr_tracker: APRTracker,
    total_deposit_tokens: Uint128,
    block_time: u64
) -> StdResult<APRInstance> {
    //Calc APR fields
    let time_since_last_update = max(block_time - apr_tracker.last_updated, 1u64);
    let apr_of_this_update = Decimal::from_ratio(total_deposit_tokens, max(apr_tracker.clone().last_total_deposit, Uint128::one())) - Decimal::one();
    let apr_per_second = decimal_division(apr_of_this_update, Decimal::from_ratio(time_since_last_update, Uint128::one()))?;
    //Save the new APR instance
    let new_apr_instance = APRInstance {
        apr_per_second,
        time_since_last_update,
        apr_of_this_update,
    };
    // println!("new_apr_instance: {:?}, {}", apr_of_this_update, apr_tracker.last_total_deposit);

    Ok(new_apr_instance)
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
    if apr_tracker.last_total_deposit != total_deposit_tokens {     
        let new_apr_instance = calc_apr_instance(apr_tracker.clone(), total_deposit_tokens, env.block.time.seconds())?;
        save_apr_instance(deps.storage, new_apr_instance.clone(), env.block.time.seconds(), total_deposit_tokens + deposit_amount)?;
    } else if apr_tracker.last_total_deposit == Uint128::zero() {
        save_apr_instance(deps.storage, APRInstance {
            apr_per_second: Decimal::zero(),
            time_since_last_update: 0,
            apr_of_this_update: Decimal::zero(),
        }, env.block.time.seconds(), deposit_amount)?;
    }

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
    // println!("vault_tokens_to_distribute: {:?}", vault_tokens_to_distribute);
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
    let new_apr_instance = calc_apr_instance(apr_tracker.clone(), total_deposit_tokens, env.block.time.seconds())?;
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
        QueryMsg::APR {} => to_json_binary(&query_apr(deps, env)?),
    }
}

/// Return APR for the valid durations 7, 30, 90, 365 days
fn query_apr(
    deps: Deps,
    env: Env,
) -> StdResult<APRResponse> {
    let apr_tracker = APR_TRACKER.load(deps.storage)?;
    let mut aprs = APRResponse {
        week_apr: None,
        month_apr: None,
        three_month_apr: None,
        year_apr: None,        
    };
    let mut running_duration = 0;
    let mut running_apr = Decimal::zero();
    //Get total_deposit_tokens
    let total_deposit_tokens = get_total_deposit_tokens(deps, env.clone(), CONFIG.load(deps.storage)?)?;
    //Calc & add new APRInstance
    let new_apr_instance = calc_apr_instance(apr_tracker.clone(), total_deposit_tokens, env.block.time.seconds())?;
    let mut apr_instances = apr_tracker.clone().aprs;
    apr_instances.push(new_apr_instance);
    //We reverse to get the most recent instances first
    apr_instances.reverse();
    
    //Parse instances to allocate APRs to the correct duration
    for apr_instance in apr_instances.into_iter() {
        running_duration += apr_instance.time_since_last_update;
        running_apr += apr_instance.apr_per_second;

        if running_duration >= SECONDS_PER_DAY * 7 && aprs.week_apr.is_none() {
            aprs.week_apr = Some(running_apr);
        } else if running_duration >= SECONDS_PER_DAY * 30 && aprs.month_apr.is_none() {
            aprs.month_apr = Some(running_apr);
        } else if running_duration >= SECONDS_PER_DAY * 90 && aprs.three_month_apr.is_none() {
            aprs.three_month_apr = Some(running_apr);
        } else if running_duration >= SECONDS_PER_DAY * 365 && aprs.year_apr.is_none() {
            aprs.year_apr = Some(running_apr);            
        }        
    }

    Ok(aprs)
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

    //Return the discounted amount
    Ok(users_base_tokens)
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

    //Query the Red Bank balance for its total deposit tokens
    // let total_redbank_deposit_tokens = deps.querier.query_balance(config.mars_redbank_addr.clone(), config.deposit_token.clone())?.amount;

    //BC THE BANK SENDS ASSETS TO BORROWERS WE CAN ONLY ASSERT AN INSOLVENCY IF THEY HAVE LESS THAN WE'VE DEPOSITED

    // If the Red Bank has less deposit tokens than it thinks it does in state, return a discounted amount
    /////This is hack insurance & guarantees that underlying queries return less if the Red Bank has been exploited////////
    // let mut deposit_discount = Decimal::one();
    // if total_redbank_deposit_tokens < total_deposit_tokens {
    //     deposit_discount = Decimal::from_ratio(total_redbank_deposit_tokens, total_deposit_tokens);
    // }
    
    //Apply the discount to the total deposit tokens
    // let discounted_deposit_tokens: Decimal = decimal_multiplication(Decimal::from_ratio(total_deposit_tokens, Uint128::one()), deposit_discount)?;

    //return the discounted amount
    Ok(total_deposit_tokens)
    
}


#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, env: Env, _msg: MigrateMsg) -> Result<Response, TokenFactoryError> {
    Ok(Response::default())
}