//Token factory fork
//https://github.com/osmosis-labs/bindings/blob/main/contracts/tokenfactory

use std::convert::TryInto;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Addr,
    Reply, Response, StdError, StdResult, Uint128, SubMsg, CosmosMsg, BankMsg, coins, Decimal, Order,
};
use cw2::set_contract_version;
use membrane::helpers::get_asset_liquidity;
use membrane::math::decimal_multiplication;
use osmosis_std::types::osmosis::gamm::v1beta1::GammQuerier;

use crate::error::TokenFactoryError;
use crate::state::{TokenInfo, CONFIG, TOKENS, PENDING, PendingTokenInfo};
use membrane::osmosis_proxy::{
    Config, ExecuteMsg, GetDenomResponse, InstantiateMsg, QueryMsg, TokenInfoResponse,
};
use membrane::cdp::{QueryMsg as CDPQueryMsg};
use membrane::types::{Pool, PoolStateResponse, Basket, Owner};
use osmosis_std::types::osmosis::tokenfactory::v1beta1::{self as TokenFactory, QueryDenomsFromCreatorResponse, MsgCreateDenomResponse};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:osmosis-proxy";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Constants
const MAX_LIMIT: u32 = 64;

const CREATE_DENOM_REPLY_ID: u64 = 1u64;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    _msg: InstantiateMsg,
) -> Result<Response, TokenFactoryError> {
    let config = Config {
        owners: vec![
            Owner {
                owner: info.sender.clone(),
                total_minted: Uint128::zero(),
                liquidity_multiplier: Some(Decimal::zero()),
                stability_pool_ratio: Some(Decimal::zero()),
                non_token_contract_auth: true, 
            }],
        debt_auction: None,
        positions_contract: None,
        liquidity_contract: None,
    };
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
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
) -> Result<Response, TokenFactoryError> {
    match msg {
        ExecuteMsg::CreateDenom {
            subdenom,
            max_supply,
        } => create_denom(
            deps,
            env,
            info,
            subdenom,
            max_supply,
        ),
        ExecuteMsg::ChangeAdmin {
            denom,
            new_admin_address,
        } => change_admin(deps, env, info, denom, new_admin_address),
        ExecuteMsg::MintTokens {
            denom,
            amount,
            mint_to_address,
        } => mint_tokens(deps, env, info, denom, amount, mint_to_address),
        ExecuteMsg::BurnTokens {
            denom,
            amount,
            burn_from_address,
        } => burn_tokens(deps, env, info, denom, amount, burn_from_address),
        ExecuteMsg::EditTokenMaxSupply { denom, max_supply } => {
            edit_token_max(deps, info, denom, max_supply)
        }
        ExecuteMsg::UpdateConfig {
            owners,
            add_owner,
            debt_auction,
            positions_contract,
            liquidity_contract,
        } => update_config(deps, info, owners, debt_auction, positions_contract, liquidity_contract, add_owner),
        ExecuteMsg::EditOwner { owner, liquidity_multiplier, stability_pool_ratio, non_token_contract_auth } => {
            edit_owner(deps, info, owner, liquidity_multiplier, stability_pool_ratio, non_token_contract_auth)
        }
    }
}

/// Update contract configuration
/// This function is only callable by an owner with non_token_contract_auth set to true
fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owners: Option<Vec<Owner>>,
    debt_auction: Option<String>,
    positions_contract: Option<String>,
    liquidity_contract: Option<String>,
    add_owner: bool,
) -> Result<Response, TokenFactoryError> {
    let mut config = CONFIG.load(deps.storage)?;

    let (authorized, owner_index) = validate_authority(config.clone(), info.clone());
    if !authorized || !config.owners[owner_index].non_token_contract_auth {
        return Err(TokenFactoryError::Unauthorized {});
    }

    //Edit Owner
    if let Some(owners) = owners {
        if add_owner {
            //Add all new owners
            for owner in owners {
                //Validate Owner address
                deps.api.addr_validate(&owner.owner.to_string())?;

                //Add owner to config
                config.owners.push( owner );
            }
        } else {
            //Filter out owners that are in the owners list
            for owner in owners {
                config.owners = config
                    .clone()
                    .owners
                    .into_iter()
                    .filter(|stored_owner| stored_owner.owner.to_string() != owner.owner)
                    .collect::<Vec<Owner>>();
            }
        }
    }

    //Edit Contracts
    if let Some(debt_auction) = debt_auction {
        config.debt_auction = Some(deps.api.addr_validate(&debt_auction)?);
    }
    if let Some(positions_contract) = positions_contract {
        config.positions_contract = Some(deps.api.addr_validate(&positions_contract)?);
    }
    if let Some(liquidity_contract) = liquidity_contract {
        config.liquidity_contract = Some(deps.api.addr_validate(&liquidity_contract)?);
    }

    //Save Config
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attributes(vec![
        attr("method", "update_config"),
        attr("updated_config", format!("{:?}", config)),
        ]))
}

/// Edit Owner params
/// This function is only callable by an owner with non_token_contract_auth set to true
fn edit_owner(
    deps: DepsMut,
    info: MessageInfo,
    owner: String,
    liquidity_multiplier: Option<Decimal>,
    stability_pool_ratio: Option<Decimal>,
    non_token_contract_auth: Option<bool>,
) -> Result<Response, TokenFactoryError>{
    let mut config = CONFIG.load(deps.storage)?;

    //Assert Authority
    let (authorized, owner_index) = validate_authority(config.clone(), info.clone());
    if !authorized || !config.owners[owner_index].non_token_contract_auth {
        return Err(TokenFactoryError::Unauthorized {});
    }
    let valid_owner_addr = deps.api.addr_validate(&owner)?;

    //Find Owner to edit
    if let Some((owner_index, mut owner)) = config.clone().owners
        .into_iter()
        .enumerate()
        .find(|(_i, owner)| owner.owner == valid_owner_addr){
        //Update Optionals
        if liquidity_multiplier.clone().is_some() {
            owner.liquidity_multiplier = liquidity_multiplier;
        }
        if stability_pool_ratio.clone().is_some() {
            owner.stability_pool_ratio = stability_pool_ratio;
        }
        if let Some(toggle) = non_token_contract_auth.clone() {
            owner.non_token_contract_auth = toggle;
        }

        //Update Owner
        config.owners[owner_index] = owner;
    } else { return Err(TokenFactoryError::CustomError { val: String::from("Non-existent owner address") }) }

    //Save edited Owner
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("edited_owner", format!("{:?}", config.owners[owner_index])))
}

/// Assert info.sender is an owner
fn validate_authority(config: Config, info: MessageInfo) -> (bool, usize) {
    //Owners && Debt Auction have contract authority
    match config
        .owners
        .into_iter()
        .enumerate()
        .find(|(_i, owner)| owner.owner == info.sender)
    {
        Some((index, _owner)) => (true, index),
        None => (false, 0),        
    }
}

/// Create a new denom using TokenFactory.
/// Saves the denom in the reply.
pub fn create_denom(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    subdenom: String,
    max_supply: Option<Uint128>,
) -> Result<Response, TokenFactoryError> {
    let config = CONFIG.load(deps.storage)?;

    //Assert Authority
    let (authorized, _owner_index) = validate_authority(config.clone(), info.clone());
    if !authorized {
        return Err(TokenFactoryError::Unauthorized {});
    }

    if subdenom.eq("") {
        return Err(TokenFactoryError::InvalidSubdenom { subdenom });
    }    

    //Create Msg
    let msg = TokenFactory::MsgCreateDenom { sender: env.contract.address.to_string(), subdenom: subdenom.clone() };
    let create_denom_msg = SubMsg::reply_on_success(msg, CREATE_DENOM_REPLY_ID );
    
    //Save PendingTokenInfo
    PENDING.save(deps.storage, &PendingTokenInfo { subdenom: subdenom.clone(), max_supply })?;

    let res = Response::new()
        .add_attribute("method", "create_denom")
        .add_attribute("sub_denom", subdenom)
        .add_attribute("max_supply", max_supply.unwrap_or_else(Uint128::zero))
        .add_submessage(create_denom_msg);

    Ok(res)
}

/// Change the admin of a denom created from this contract
pub fn change_admin(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    denom: String,
    new_admin_address: String,
) -> Result<Response, TokenFactoryError> {

    let config = CONFIG.load(deps.storage)?;
    //Assert Authority
    let (authorized, _owner_index) = validate_authority(config.clone(), info.clone());
    if !authorized {
        return Err(TokenFactoryError::Unauthorized {});
    }

    deps.api.addr_validate(&new_admin_address)?;

    validate_denom(denom.clone())?;

    let change_admin_msg = TokenFactory::MsgChangeAdmin {
        denom: denom.clone(),
        sender: env.contract.address.to_string(),
        new_admin: new_admin_address.clone(),
    };

    let res = Response::new()
        .add_attribute("method", "change_admin")
        .add_attribute("denom", denom)
        .add_attribute("new_admin_address", new_admin_address)
        .add_message(change_admin_msg);

    Ok(res)
}

/// Edit token max supply
fn edit_token_max(
    deps: DepsMut,
    info: MessageInfo,
    denom: String,
    max_supply: Uint128,
) -> Result<Response, TokenFactoryError> {

    let config = CONFIG.load(deps.storage)?;
    //Assert Authority
    let (authorized, _owner_index) = validate_authority(config.clone(), info.clone());
    if !authorized {
        return Err(TokenFactoryError::Unauthorized {});
    }

    //Update Token Max
    TOKENS.update(
        deps.storage,
        denom.clone(),
        |token_info| -> Result<TokenInfo, TokenFactoryError> {
            match token_info {
                Some(mut token_info) => {
                    token_info.max_supply = Some(max_supply);

                    Ok(token_info)
                }
                None => {
                    Err(TokenFactoryError::CustomError {
                        val: String::from("Denom was not created in this contract"),
                    })
                }
            }
        },
    )?;

    //If max supply is changed to under current_supply, it halts new mints.

    Ok(Response::new().add_attributes(vec![
        attr("method", "edit_token_max"),
        attr("denom", denom),
        attr("new_max", max_supply),
    ]))
}

/// Mint tokens to an address
pub fn mint_tokens(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    denom: String,
    amount: Uint128,
    mint_to_address: String,
) -> Result<Response, TokenFactoryError> {
    let mut config = CONFIG.load(deps.storage)?;

    //Assert Authority
    let (authorized, owner_index) = validate_authority(config.clone(), info.clone());
    if !authorized {
        return Err(TokenFactoryError::Unauthorized {});
    }    

    //Validate mint_to_address
    deps.api.addr_validate(&mint_to_address)?;

    if amount.eq(&Uint128::new(0_u128)) {
        return Result::Err(TokenFactoryError::ZeroAmount {});
    }
    //Validate denom
    validate_denom(denom.clone())?;

    //Debt Auction can mint over max supply
    let mut mint_allowed = false;
    if let Some(debt_auction) = config.clone().debt_auction {
        if info.sender == debt_auction {
            mint_allowed = true;
        }
    };

    if let Some(positions_contract) = config.clone().positions_contract { 
        //Set owner
        let mut owner = config.clone().owners[owner_index].clone();
        //Get CDP denom
        let basket: Basket = deps.querier.query_wasm_smart(positions_contract, &CDPQueryMsg::GetBasket {  })?;

        //If minting the CDP asset
        if denom == basket.clone().credit_asset.info.to_string() {            
            //If there is a mint limit on the owner
            if let Some(liquidity_multiplier) = owner.liquidity_multiplier {

                //Get liquidity 
                let cdp_liquidity = get_asset_liquidity(
                    deps.querier, 
                    config.clone().liquidity_contract.unwrap().to_string(), 
                    basket.clone().credit_asset.info)?;
                
                //Calculate Owner's cap 
                let cap = decimal_multiplication(liquidity_multiplier,  Decimal::from_ratio(cdp_liquidity, Uint128::one()))?
                * Uint128::one();

                //Assert mints are below the owner's LM * liquidity 
                //Or if the owner is the positions contract; (Position contract has interest rate incentives to reduce debt)
                if owner.total_minted + amount <= cap || owner.owner == config.clone().positions_contract.unwrap_or_else(|| Addr::unchecked("")) {
                    //Update total_minted
                    owner.total_minted += amount;
                } else { return Err(TokenFactoryError::MintCapped {  }) }
            } else {
                owner.total_minted += amount;
            }
        }
        //Save Owner
        config.owners[owner_index] = owner;
        CONFIG.save(deps.storage, &config)?;
    }
   

    //Update Token Supply
    TOKENS.update(
        deps.storage,
        denom.clone(),
        |token_info| -> Result<TokenInfo, TokenFactoryError> {
            match token_info {
                Some(mut token_info) => {
                    if token_info.clone().max_supply.is_some() {
                        if token_info.current_supply <= token_info.max_supply.unwrap()
                            || mint_allowed
                        {
                            token_info.current_supply += amount;
                            mint_allowed = true;
                        }
                    } else {
                        token_info.current_supply += amount;
                        mint_allowed = true;
                    }

                    Ok(token_info)
                }
                None => {
                    Err(TokenFactoryError::CustomError {
                        val: String::from("Denom was not created in this contract"),
                    })
                }
            }
        },
    )?;

    //Create mint msg
    let mint_tokens_msg: CosmosMsg = TokenFactory::MsgMint{
        sender: env.contract.address.to_string(), 
        amount: Some(osmosis_std::types::cosmos::base::v1beta1::Coin{
            denom: denom.clone(),
            amount: amount.to_string(),
        }), 
    }.into();    

    //Send minted assets to mint_to_address
    let send_msg: CosmosMsg = CosmosMsg::Bank(BankMsg::Send { 
        to_address: mint_to_address.clone(),
        amount: coins(amount.u128(), denom.clone()),
    });

    let mut res = Response::new()
        .add_attribute("method", "mint_tokens")
        .add_attribute("mint_status", mint_allowed.to_string())
        .add_attribute("denom", denom.clone())
        .add_attribute("amount", Uint128::zero());

    //If a mint was made/allowed
    if mint_allowed {
        res = Response::new()
            .add_attribute("method", "mint_tokens")
            .add_attribute("mint_status", mint_allowed.to_string())
            .add_attribute("denom", denom)
            .add_attribute("amount", amount)
            .add_attribute("mint_to_address", mint_to_address)
            .add_messages(vec![mint_tokens_msg, send_msg]);
    }

    Ok(res)
}

/// Burns tokens 
pub fn burn_tokens(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    denom: String,
    amount: Uint128,
    burn_from_address: String,
) -> Result<Response, TokenFactoryError> {    
    let mut config = CONFIG.load(deps.storage)?;

    //Assert Authority
    let (authorized, owner_index) = validate_authority(config.clone(), info.clone());
    if !authorized {
        return Err(TokenFactoryError::Unauthorized {});
    }

    if amount.eq(&Uint128::new(0_u128)) {
        return Result::Err(TokenFactoryError::ZeroAmount {});
    }

    validate_denom(denom.clone())?;

    //Update Owner total_mints
    config.owners[owner_index].total_minted = match config.owners[owner_index].total_minted.checked_sub(amount){
        Ok(diff) => diff,
        Err(err) => return Err(TokenFactoryError::CustomError { val: err.to_string() })
    };
    CONFIG.save(deps.storage, &config)?;


    //Update Token Supply
    TOKENS.update(
        deps.storage,
        denom.clone(),
        |token_info| -> Result<TokenInfo, TokenFactoryError> {
            match token_info {
                Some(mut token_info) => {
                    token_info.current_supply -= amount;
                    Ok(token_info)
                }
                None => {
                    Err(TokenFactoryError::CustomError {
                        val: String::from("Denom was not created in this contract"),
                    })
                }
            }
        },
    )?;

    let burn_token_msg: CosmosMsg = TokenFactory::MsgBurn {
        sender: env.contract.address.to_string(),
        amount: Some(osmosis_std::types::cosmos::base::v1beta1::Coin{
            denom,
            amount: amount.to_string(),
        }),
    }.into();

    let res = Response::new()
        .add_attribute("method", "burn_tokens")
        .add_attribute("amount", amount)
        .add_attribute("burn_from_address", burn_from_address)
        .add_message(burn_token_msg);

    Ok(res)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::GetOwner { owner } => to_binary(&get_contract_owner(deps, owner)?),
        QueryMsg::GetDenom {
            creator_address,
            subdenom,
        } => to_binary(&get_denom(deps, creator_address, subdenom)?),
        QueryMsg::GetContractDenoms { limit } => to_binary(&get_contract_denoms(deps, limit)?),
        QueryMsg::PoolState { id } => to_binary(&get_pool_state(deps, id)?),
        QueryMsg::GetTokenInfo { denom } => to_binary(&get_token_info(deps, denom)?),
    }
}

/// Returns state data regarding a specified contract owner
fn get_contract_owner(deps: Deps, owner: String) -> StdResult<Owner> {
    let config = CONFIG.load(deps.storage)?;
    if let Some(owner) = config.owners.into_iter().find(|stored_owner| stored_owner .owner == owner) {
        Ok(owner)
    } else {
        Err(StdError::generic_err("Owner not found"))
    }
}

/// Returns token info for a specified denom
fn get_token_info(deps: Deps, denom: String) -> StdResult<TokenInfoResponse> {
    let token_info = TOKENS.load(deps.storage, denom.clone())?;
    
    Ok(TokenInfoResponse {
        denom,
        current_supply: token_info.current_supply,
        max_supply: token_info.max_supply.unwrap_or_else(Uint128::zero),
    })
    
}

/// Returns a list of all denoms created by this contract
fn get_contract_denoms(deps: Deps, limit: Option<u32>) -> StdResult<Vec<String>> {
    let limit = limit.unwrap_or_else(|| MAX_LIMIT);

    Ok(
        TOKENS
            .range(deps.storage, None, None, Order::Ascending)
            .take(limit as usize)
            .map(|info|{
                if let Ok(info) = info {
                    info.0
                } else { String::from("error") }
            })
            .collect::<Vec<String>>()
    )
}

/// Returns PoolStateResponse for a specified pool id
fn get_pool_state(
    deps: Deps,
    pool_id: u64,
) -> StdResult<PoolStateResponse> {
    let res = GammQuerier::new(&deps.querier).pool(pool_id)?;
    
    let pool: Pool = res.pool
        .ok_or_else(|| StdError::NotFound {
            kind: "pool".to_string(),
        })?
        // convert `Any` to `Pool`
        .try_into()?;

    Ok(pool.into_pool_state_response())
    
}

/// Returns denom for a specified creator address and subdenom
fn get_denom(deps: Deps, creator_addr: String, subdenom: String) -> StdResult<GetDenomResponse> {
    let response: QueryDenomsFromCreatorResponse = TokenFactory::TokenfactoryQuerier::new(&deps.querier).denoms_from_creator(creator_addr)?;

    let denom = if let Some(denom) = response.denoms.into_iter().find(|denoms| denoms.contains(&subdenom)){
        denom
    } else {
        return Err(StdError::GenericErr { msg: String::from("Can't find subdenom in list of contract denoms") })
    };

    Ok(GetDenomResponse {
        denom,
    })
}

/// Validate token factory denom
pub fn validate_denom(denom: String) -> Result<(), TokenFactoryError> {
    let denom_to_split = denom.clone();
    let tokenfactory_denom_parts: Vec<&str> = denom_to_split.split('/').collect();

    if tokenfactory_denom_parts.len() != 3 {
        return Result::Err(TokenFactoryError::InvalidDenom {
            denom,
            message: std::format!(
                "denom must have 3 parts separated by /, had {}",
                tokenfactory_denom_parts.len()
            ),
        });
    }

    let prefix = tokenfactory_denom_parts[0];

    if !prefix.eq_ignore_ascii_case("factory") {
        return Result::Err(TokenFactoryError::InvalidDenom {
            denom,
            message: std::format!("prefix must be 'factory', was {}", prefix),
        });
    }

    Result::Ok(())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.id {
        CREATE_DENOM_REPLY_ID => handle_create_denom_reply(deps, env, msg),
        id => Err(StdError::generic_err(format!("invalid reply id: {}", id))),
    }
}

/// Find & save created full denom
fn handle_create_denom_reply(
    deps: DepsMut,
    _env: Env,
    msg: Reply,
) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(result) => {
            //Load Pending TokenInfo
            let PendingTokenInfo { subdenom:_, max_supply} = PENDING.load(deps.storage)?;

            if let Some(b) = result.data {
                let res: MsgCreateDenomResponse = match b.try_into().map_err(TokenFactoryError::Std){
                    Ok(res) => res,
                    Err(err) => return Err(StdError::GenericErr { msg: String::from(err.to_string()) })
                };
                //Save Denom Info
                TOKENS.save(
                    deps.storage,
                    res.new_token_denom.clone(),
                    &TokenInfo {
                        current_supply: Uint128::zero(),
                        max_supply,
                    },
                )?;
            } else {
                return Err(StdError::GenericErr { msg: String::from("No data in reply") })
            }
        } //We only reply on success
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }
    Ok(Response::new())
}