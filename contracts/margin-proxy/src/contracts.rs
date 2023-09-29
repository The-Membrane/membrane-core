use std::str::FromStr;

use cosmwasm_std::{
    attr, entry_point, to_binary, Binary, Deps, DepsMut, Env, MessageInfo, 
    Response, StdResult, Uint128, WasmQuery, SubMsg, Storage, Addr, CosmosMsg, WasmMsg, Reply, StdError, QueryRequest, Decimal, QuerierWrapper, Attribute, Order, Coin, ReplyOn,
};
use cw2::set_contract_version;

use cw_storage_plus::Bound;
use membrane::helpers::{router_native_to_native, get_contract_balances, asset_to_coin};
use membrane::margin_proxy::{Config, ExecuteMsg, InstantiateMsg, QueryMsg};
use membrane::math::decimal_multiplication;
use membrane::cdp::{ExecuteMsg as CDP_ExecuteMsg, QueryMsg as CDP_QueryMsg, PositionResponse};
use membrane::types::{AssetInfo, Basket, Asset};

use crate::error::ContractError;
use crate::state::{CONFIG, COMPOSITION_CHECK, USERS, NEW_POSITION_INFO, NUM_OF_LOOPS, LOOP_PARAMETERS, OWNERSHIP_TRANSFER, ROUTER_DEPOSIT_MSG};

// Contract name and version used for migration.
const CONTRACT_NAME: &str = "margin_proxy";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

// Pagination defaults
const PAGINATION_DEFAULT_LIMIT: u64 = 10;
const PAGINATION_MAX_LIMIT: u64 = 30;

//Reply IDs
const EXISTING_DEPOSIT_REPLY_ID: u64 = 1u64;
const NEW_DEPOSIT_REPLY_ID : u64 = 2u64;
const LOOP_REPLY_ID: u64 = 3u64;
const CLOSE_POSITION_REPLY_ID: u64 = 4u64;
const ROUTER_REPLY_ID: u64 = 5u64;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let config: Config;
    if let Some(owner) = msg.owner {
        config = Config {
            owner: deps.api.addr_validate(&owner)?,
            apollo_router_contract: deps.api.addr_validate(&msg.apollo_router_contract)?,
            positions_contract: deps.api.addr_validate(&msg.positions_contract)?,
            max_slippage: msg.max_slippage,
        };
    } else {
        config = Config {
            owner: info.sender,
            apollo_router_contract: deps.api.addr_validate(&msg.apollo_router_contract)?,
            positions_contract: deps.api.addr_validate(&msg.positions_contract)?,
            max_slippage: msg.max_slippage,
        };
    }

    //Save Config
    CONFIG.save(deps.storage, &config)?;

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
        ExecuteMsg::Deposit { position_id } => {
            deposit_to_cdp( deps, env, info, position_id )
        },
        ExecuteMsg::Loop { 
            position_id, 
            num_loops,
            target_LTV 
        } => {
            loop_leverage(deps.storage, deps.querier, env, info.sender, position_id, num_loops, target_LTV)
        },
        ExecuteMsg::ClosePosition { position_id, max_spread } => {
            close_posiion(deps, info, position_id, max_spread)
        },
        ExecuteMsg::UpdateConfig {
            owner,
            apollo_router_contract,
            positions_contract,
            max_slippage,
        } => update_config(deps, info, owner, apollo_router_contract, positions_contract, max_slippage),
    }
}

/// Calls ClosePosition on the Positions contract
fn close_posiion(
    deps: DepsMut,
    info: MessageInfo,
    position_id: Uint128,
    max_spread: Decimal,
) -> Result<Response, ContractError>{
    //Load Config
    let config: Config = CONFIG.load(deps.storage)?;

    //Validate Position ownership
    validate_user_ownership(deps.storage, info.clone().sender, position_id.clone())?;

    //Create SubMsg, reply_on_success
    let msg = CosmosMsg::Wasm(WasmMsg::Execute { 
        contract_addr: config.clone().positions_contract.to_string(), 
        msg: to_binary(&CDP_ExecuteMsg::ClosePosition { 
            position_id,
            max_spread: max_spread,
            send_to: Some(info.clone().sender.to_string())
        })?, 
        funds: vec![],
    });

    let sub_msg = SubMsg::reply_on_success(msg, CLOSE_POSITION_REPLY_ID);

    Ok(Response::new()
        .add_attributes(vec![
            attr("user", info.clone().sender.to_string()),
            attr("position_id", position_id.to_string()),
        ])
        .add_submessage(sub_msg)
    )
}

/// Loop position composition at a desired LTV.
/// Stop loops when num_of_loops is reached or when increase of credit is less than 2.
fn loop_leverage(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    sender: Addr,
    position_id: Uint128,
    num_loops: Option<u64>,
    target_LTV: Decimal,
) -> Result<Response, ContractError>{

    //Load Config
    let config = CONFIG.load(storage)?;

    //Validate Position ownership
    validate_user_ownership(storage, sender.clone(), position_id.clone())?;

    //Query Collateral Composition so we know what ratios to loop w/
    let position_response = querier.query::<PositionResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().positions_contract.to_string(), 
        msg: to_binary(&CDP_QueryMsg::GetPosition {
            position_id, 
            position_owner: env.contract.address.to_string(), 
        })?
    }))?;

    //Save Position Info
    COMPOSITION_CHECK.save(storage, &position_response)?;

    //Save NUM_OF_LOOPS
    NUM_OF_LOOPS.save(storage, &num_loops)?;

    //Save LOOP_PARAMETERS
    LOOP_PARAMETERS.save(storage, &(sender, target_LTV))?;

    //Increase Debt Msg for position
    let msg = CosmosMsg::Wasm(WasmMsg::Execute { 
        contract_addr: config.clone().positions_contract.to_string(), 
        msg: to_binary(&CDP_ExecuteMsg::IncreaseDebt { 
            position_id: position_id.clone(), 
            amount: None, 
            LTV: Some(target_LTV), 
            mint_to_addr: None 
        })?, 
        funds: vec![],
    });
    //Reply_always
    let sub_msg = SubMsg::reply_always(msg, LOOP_REPLY_ID);

    //Response Builder
    let mut attrs = vec![
        attr("method", "looped_leverage"),
        attr("position_id", position_id)
    ];

    if let Some(num) = num_loops {
        attrs.push(attr("num_of_loops_left", num.to_string()));
    } else {
        attrs.push(attr("target_LTV", target_LTV.to_string()));
    }
    
    Ok(Response::new()
        .add_submessage(sub_msg)
        .add_attributes(attrs)
    )
    
}

/// Validate user's ownership of a position within the contract
fn validate_user_ownership(
    storage: &mut dyn Storage,
    user: Addr,
    position_id: Uint128,
) -> Result<(), ContractError>{
    
    //If id isn't found in the list for the User, error
    if let None = USERS.load(storage, user )?.into_iter().find(|id| id == &position_id){
        return Err(ContractError::InvalidID { id: position_id })
    }
    //If the user owns the position outside of the contract..
    //we still error bc the contract needs to own the position for the user

    Ok(())
}

/// Deposit assets to a Position
fn deposit_to_cdp(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,    
    position_id: Option<Uint128>,
) -> Result<Response, ContractError>{
    //Load Config
    let config = CONFIG.load( deps.storage )?;

    //If there is a position_id, make sure the current user is the owner of that position
    if let Some(position_id) = position_id {
        validate_user_ownership(deps.storage, info.clone().sender, position_id.clone())?;        
    };

    //Errors if any assets are Osmosis LPs
    if info.clone().funds.into_iter().any(|coin| coin.denom.contains("gamm")){
        return Err(ContractError::NoLPs {  })
    }
   
    //Create Reponse objects
    let sub_msg: SubMsg;
    let mut attrs: Vec<Attribute> = vec![
        attr("user", info.clone().sender),
        attr("deposited_funds", format!("{:?}", info.clone().funds)),
    ];

    //Create Deposit Msg
    if position_id.is_none(){

        //Create DepositMsg
        let deposit_msg = CDP_ExecuteMsg::Deposit {
            position_owner: None, //Margin Contract
            position_id: None, //New position
        };
    
        let msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.clone().positions_contract.to_string(),
            msg: to_binary(&deposit_msg)?,
            funds: info.clone().funds,
        });

        //Create a submsg
        //Reply on success and query the position ID for this deposit
        sub_msg = SubMsg::reply_on_success(msg, NEW_DEPOSIT_REPLY_ID);

        //Save User so the contract knows who to save the new position id under
        NEW_POSITION_INFO.save(deps.storage, &info.clone().sender)?;

    } else {
        //Adding Position_id to User's list        
        let unwrapped_id = position_id.unwrap();

        attrs.push( attr("position_id", unwrapped_id.to_string()) );


        let deposit_msg = CDP_ExecuteMsg::Deposit {
            position_owner: None, //Margin Contract
            position_id: position_id.clone(), 
        };
    
        let msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.clone().positions_contract.to_string(),
            msg: to_binary(&deposit_msg)?,
            funds: info.clone().funds,
        });

        //Submsg reply on success
        sub_msg = SubMsg::reply_on_success(msg, EXISTING_DEPOSIT_REPLY_ID);

        //Confirm that deposits are of the same composition as the position by querying cAsset + ratios now and asserting equality in the reply
        let position_response = deps.querier.query::<PositionResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: config.clone().positions_contract.to_string(), 
            msg: to_binary(&CDP_QueryMsg::GetPosition {
                position_id: unwrapped_id, 
                position_owner: env.contract.address.to_string(), 
            })?
        }))?;

        //Save Position Info
        COMPOSITION_CHECK.save(deps.storage, &position_response)?;
    }
    
    
    Ok( Response::new()
        .add_submessage(sub_msg)
        .add_attributes(attrs)
    )
}


#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.id {
        EXISTING_DEPOSIT_REPLY_ID => handle_existing_deposit_reply(deps, env, msg),
        NEW_DEPOSIT_REPLY_ID => handle_new_deposit_reply(deps, msg),
        LOOP_REPLY_ID => handle_loop_reply(deps, env, msg),
        CLOSE_POSITION_REPLY_ID => handle_close_position_reply(deps, env, msg),
        ROUTER_REPLY_ID => handle_router_deposit_reply(deps, env, msg),
        id => Err(StdError::generic_err(format!("invalid reply id: {}", id))),
    }
}

/// On success, sell for collateral composition, redeposit & call loop fn again.
/// Increment Loop number.
fn handle_loop_reply(
    deps: DepsMut,
    env: Env,
    msg: Reply,
) -> StdResult<Response>{
    
    match msg.result.into_result() {
        Ok(result) => {

            let debt_mint_event = result
                .events
                .into_iter()
                .find(|e| e.attributes.iter().any(|attr| attr.key == "total_loan"))
                .ok_or_else(|| StdError::generic_err(format!("unable to find debt_mint event")))?;
            
            //Get increased_debt amount
            let increased_debt_amount = &debt_mint_event
                .attributes
                .iter()
                .find(|attr| attr.key == "increased_by")
                .unwrap()
                .value;

            //Get total loan
            let total_loan = {
                let string_loan = &debt_mint_event
                    .attributes
                    .iter()
                    .find(|attr| attr.key == "total_loan")
                    .unwrap()
                    .value;

                Uint128::from_str(string_loan).unwrap()
            };

            //Get how much credit was minted
            let credit_amount = Decimal::from_str(increased_debt_amount)?;

            //If credit amount is < 2, end loop
            if credit_amount < Decimal::percent(2_000_000_00){
                return Ok(Response::new()
                        .add_attribute("loop_finished", "true")
                        .add_attribute("total_loan", total_loan.to_string()))
            }

            //Load Config
            let config: Config = CONFIG.load(deps.storage)?;

            //Load Previous Composition
            let previous_composition = COMPOSITION_CHECK.load(deps.storage)?;
            
            //Create (AssetInfo, ratio) tuple list
            let composition_to_loop = previous_composition.clone().cAsset_ratios
                .into_iter()
                .enumerate()
                .map(|(i, ratio)| (previous_composition.clone().collateral_assets[i].clone().asset.info, ratio) )
                .collect::<Vec<(AssetInfo, Decimal)>>();

            //Query Basket credit asset
            let credit_asset = deps.querier.query::<Basket>(&QueryRequest::Wasm(WasmQuery::Smart { 
                contract_addr: config.clone().positions_contract.to_string(), 
                msg: to_binary(&CDP_QueryMsg::GetBasket { })?
            }))?
            .credit_asset;

            //Initialize messages
            let mut messages = vec![];

            //Sell new debt for collateral composition & redeposit 
            for (collateral, ratio) in composition_to_loop {
                
                let credit_to_sell = decimal_multiplication(credit_amount, ratio)?;

                let msg = router_native_to_native(                    
                    config.clone().apollo_router_contract.to_string(),                    
                    credit_asset.clone().info,
                    collateral,          
                    Some(config.clone().positions_contract.to_string()),
                    (credit_to_sell * Uint128::new(1u128)).u128(),
                )?;
                //Add a reply msg to execute the hook msg
                messages.push(SubMsg::new(msg));
            }
            //Save Router Reply Hook Msg
            let hook_msg = to_binary(&CDP_ExecuteMsg::Deposit { 
                position_id: Some(previous_composition.clone().position_id),
                position_owner: Some(env.contract.address.to_string()), //Owner is this contract
            })?;
            ROUTER_DEPOSIT_MSG.save(deps.storage, &hook_msg)?;
            //Update the last router_msg to be a ROUTER_REPLY
            let msg_index = messages.clone().len()-1;
            messages[msg_index].id = ROUTER_REPLY_ID;
            messages[msg_index].reply_on = ReplyOn::Success;
            
            //Load parameters from last loop
            let loop_parameters = LOOP_PARAMETERS.load(deps.storage)?;

            //Update number of loops left
            let num_of_loops_left = if let Some(num) = NUM_OF_LOOPS.load(deps.storage)?{
                //If num_of_loops_left is 0, finish the loop
                if num == 0 {                  
                    return Ok(Response::new()
                        .add_attribute("loop_finished", "true")
                        .add_attribute("total_loan", total_loan.to_string()))
                } else {
                    Some(num - 1u64)
                }
            } else {
                None
            };

            //Recall loop function
            let res = match loop_leverage(
                deps.storage, 
                deps.querier, 
                env, 
                loop_parameters.0, 
                previous_composition.clone().position_id, 
                num_of_loops_left, 
                loop_parameters.1
            ){
                Ok(res) => res,
                Err(err) => return Err(StdError::GenericErr { msg: err.to_string() })
            };            
            
            //Push new debt mint messages to the end
            //Credit sales + collateral deposits -> new debt
            messages.extend(res.messages);

            
            //Response Builder
            let mut attrs = vec![
                attr("debt_increased_by", credit_amount.to_string())
            ];

            if let Some(num) = num_of_loops_left {
                attrs.push(attr("num_of_loops_left", num.to_string()));
            } else {
                attrs.push(attr("target_LTV", loop_parameters.1.to_string()));
            }
            
            Ok(Response::new()
                .add_submessages(messages)
                .add_attributes(attrs)
            )

        },
        Err(string) => {            
            //Error likely means the target_LTV was hit
            Ok(Response::new().add_attribute("increase_debt_error", string))
        }
    }
}

/// Handle reply from router contract
pub fn handle_router_deposit_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(_result) => {
            //Load Previous Composition
            let previous_composition = COMPOSITION_CHECK.load(deps.storage)?;
            
            //Query contract balance of all position collateral assets
            let asset_balances = get_contract_balances(
                deps.querier, 
                env.clone(), 
                previous_composition.clone().collateral_assets
                    .into_iter()
                    .map(|asset| asset.asset.info)
                    .collect::<Vec<AssetInfo>>()
            )?;

            //Map balances to Asset object
            let asset_balances: Vec<Asset> = asset_balances
                .into_iter()
                .enumerate()
                .filter(|(_, amount)| !amount.is_zero())//Skip if balance is 0            
                .map(|(i, amount)| Asset {
                    info: previous_composition.collateral_assets[i].clone().asset.info,
                    amount,
                })
                .collect::<Vec<Asset>>();

            //Map asset_balances to coins
            let asset_balances: Vec<Coin> = asset_balances
                .into_iter()
                .map(|asset| asset_to_coin(asset))
                .collect::<StdResult<Vec<Coin>>>()?;
            

            //Load DEPOSIT msg binary from storage
            let hook_msg: Binary = ROUTER_DEPOSIT_MSG.load(deps.storage)?;

            //Create repay_msg with queried funds
            //This works because the contract doesn't hold excess credit_asset, all repayments are burned & revenue isn't minted
            let deposit_msg = CosmosMsg::Wasm(WasmMsg::Execute { 
                contract_addr: CONFIG.load(deps.storage)?.positions_contract.to_string(), 
                msg: hook_msg, 
                funds: asset_balances.clone(),
            });

            Ok(Response::new().add_message(deposit_msg).add_attribute("amount_deposited", format!("{:?}", asset_balances)))
        },
        
        Err(err) => {
            //Its reply on success only
            Ok(Response::new().add_attribute("error", err))
        }
    }    
}

/// Remove User claim over a position in this contract
fn handle_close_position_reply(
    deps: DepsMut,
    _env: Env,
    msg: Reply,
) -> StdResult<Response>{
    match msg.result.into_result() {
        Ok(result) => {

            let close_position_event = result
                .events
                .into_iter()
                .find(|e| e.attributes.iter().any(|attr| attr.key == "basket_id"))
                .ok_or_else(|| StdError::generic_err(format!("unable to find close_position event")))?;
            
            let position_id = {                
                let string_id = &close_position_event
                    .attributes
                    .iter()
                    .find(|attr| attr.key == "position_id")
                    .unwrap()
                    .value;

                Uint128::from_str(string_id).unwrap()
            };

            let user = {
                let string_user = &close_position_event
                    .attributes
                    .iter()
                    .find(|attr| attr.key == "user")
                    .unwrap()
                    .value;

                deps.api.addr_validate(string_user)?
            };

            //Remove Position from User's list
            let user_positions = USERS.load(deps.storage, user.clone())?;

            let updated_positions = user_positions
                .into_iter()
                .filter(|position| position != position_id)
                .collect::<Vec<Uint128>>();

            //Save new User list
            USERS.save(deps.storage, user, &updated_positions)?;

            Ok(Response::new()
                .add_attribute("position_closed", format!("position_id: {}", position_id ))
            )

        },
        Err(string) => {
            //This is only reply_on_success so this shouldn't be reached
            Ok(Response::new().add_attribute("error", string))
        }
    }
}

/// Asserts users only deposit the current Position composition
fn handle_existing_deposit_reply(
    deps: DepsMut,
    env: Env,
    msg: Reply,
) -> StdResult<Response>{
    match msg.result.into_result() {
        Ok(_result) => {
            //Load Config 
            let config = CONFIG.load(deps.storage)?;

            //Load Composition Check
            let previous_composition: PositionResponse = COMPOSITION_CHECK.load(deps.storage)?;
            
            //Confirm cAsset_ratios and cAsset makeup hasn't changed    
            ////Query current Position
            let position_response = deps.querier.query::<PositionResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: config.clone().positions_contract.to_string(), 
                msg: to_binary(&CDP_QueryMsg::GetPosition {
                    position_id: previous_composition.position_id, 
                    position_owner: env.contract.address.to_string(), 
                })?
            }))?;

            //Create (AssetInfo, ratio) tuple list
            let composition = previous_composition.clone().cAsset_ratios
                .into_iter()
                .enumerate()
                .map(|(i, ratio)| (previous_composition.clone().collateral_assets[i].clone().asset.info, ratio) )
                .collect::<Vec<(AssetInfo, Decimal)>>();

            //Create lists for both previous and current position asset_infos
            let previous_assets = previous_composition.clone().collateral_assets
                .into_iter()
                .map(|cAsset| cAsset.asset.info )
                .collect::<Vec<AssetInfo>>();

            let current_assets = position_response.clone().collateral_assets
                .into_iter()
                .map(|cAsset| cAsset.asset.info )
                .collect::<Vec<AssetInfo>>();

            //Assert ratio equality & cAsset equality
            if previous_composition.cAsset_ratios != position_response.cAsset_ratios 
               || 
               previous_assets != current_assets{                
                return Err(StdError::GenericErr { msg: format!("Can only deposit more of the current position composition: {:?}", composition) })
            }

            Ok(Response::new().add_attribute("valid_composition", "true"))

        },
        Err(string) => {
            //This is only reply_on_success so this shouldn't be reached
            Ok(Response::new().add_attribute("error", string))
        }
    }
}

/// Fetch & save new position ID under user's address
fn handle_new_deposit_reply(
    deps: DepsMut,
    msg: Reply,
) -> StdResult<Response>{

    match msg.result.into_result() {
        Ok(result) => {
            //Load NEW_POSITION_INFO
            let new_position_user = NEW_POSITION_INFO.load(deps.storage)?;
            
            //Get new Position_ID
            let instantiate_event = result
                .events
                .iter()
                .find(|e| {
                    e.attributes
                        .iter()
                        .any(|attr| attr.key == "position_id")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find deposit event"))
                })?;

            let position_id = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "position_id")
                .unwrap()
                .value;
            let position_id = Uint128::from_str(position_id)?;

            //Save ID to User
            if let Err(err) = append_to_user_list_of_ids(deps.storage, new_position_user.clone(), position_id){
                return Err(StdError::GenericErr { msg: err.to_string() })
            };

            Ok(Response::new()
                .add_attributes(vec![
                    attr("user", new_position_user),
                    attr("new_id",  position_id),
            ]))
        },
        Err(string) => {
            //This is only reply_on_success so this shouldn't be reached
            Ok(Response::new().add_attribute("error", string))
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::GetUserPositions { user } => to_binary(&query_user_positions(deps, env, user)?),
        QueryMsg::GetPositionIDs { limit, start_after } => to_binary(&query_positions(deps, limit, start_after)?),
    }
}

/// Returns a list of Position IDs owned by this contract
fn query_positions(
    deps: Deps,
    option_limit: Option<u64>, //User limit
    start_after: Option<String>, //user    
) -> StdResult<Vec<Uint128>>{
    
    let limit = option_limit
        .unwrap_or(PAGINATION_DEFAULT_LIMIT)
        .min(PAGINATION_MAX_LIMIT) as usize;
    
    let start = if let Some(start) = start_after {
        let start_after_addr = deps.api.addr_validate(&start)?;
        Some(Bound::exclusive(start_after_addr))
    } else {
        None
    };
    let mut positions: Vec<Uint128> = vec![];

    for user in USERS
        .range(deps.storage, start, None, Order::Ascending)
        .take(limit){
            let (_user, user_positions) = user.unwrap();
            
            positions.extend(user_positions);
        }
        

    Ok(positions)
}

/// Returns a list of Positions owned by a user in this contract
fn query_user_positions(
    deps: Deps,
    env: Env,
    user: String,
) -> StdResult<Vec<PositionResponse>>{
    //Load Config
    let config: Config = CONFIG.load(deps.storage)?;

    //Validate User
    let user = deps.api.addr_validate(&user)?;

    //Load User positions
    let user_positions = USERS.load(deps.storage, user)?;

    let mut resp: Vec<PositionResponse> = vec![];

    for position in user_positions {
        
        let position_resp = deps.querier.query::<PositionResponse>(&QueryRequest::Wasm(WasmQuery::Smart { 
            contract_addr: config.clone().positions_contract.to_string(), 
            msg: to_binary(&CDP_QueryMsg::GetPosition { 
                position_id: position, 
                position_owner: env.contract.address.to_string(),
            })?
        }))?;

        resp.push( position_resp );
    }

    Ok( resp )

}

/// Append new ID to user's list of IDs
fn append_to_user_list_of_ids(
    storage: &mut dyn Storage,
    user: Addr,
    position_id: Uint128,
) -> Result<(), ContractError>{

    USERS.update( storage, user, |list_of_ids| -> Result<Vec<Uint128>, ContractError> {
        match list_of_ids {
            Some(mut list) => {
                list.push( position_id );

                Ok(list)
            },
            None => {
                Ok(vec![ position_id ])
            }
        }
    })?;

    Ok(())
}

/// Update contract configuration
fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    apollo_router_contract: Option<String>,
    positions_contract: Option<String>,
    max_slippage: Option<Decimal>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    let mut attrs = vec![attr("method", "update_config")];

    //Assert Authority
    if info.sender != config.owner {
        //Check if ownership transfer is in progress & transfer if so
        if info.sender == OWNERSHIP_TRANSFER.load(deps.storage)? {
            config.owner = info.sender;
        } else {
            return Err(ContractError::Unauthorized {});
        }
    }

    //Save optionals
    if let Some(addr) = owner {
        let valid_addr = deps.api.addr_validate(&addr)?;

        //Set owner transfer state
        OWNERSHIP_TRANSFER.save(deps.storage, &valid_addr)?;
        attrs.push(attr("owner_transfer", valid_addr));  
    }
    if let Some(addr) = apollo_router_contract {
        config.apollo_router_contract = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = positions_contract {
        config.positions_contract = deps.api.addr_validate(&addr)?;
    }
    if let Some(slippage) = max_slippage {
        config.max_slippage = slippage;
    }

    //Save Config
    CONFIG.save(deps.storage, &config)?;
    attrs.push(attr("updated_config", format!("{:?}", config)));

    Ok(Response::new().add_attributes(attrs))
}

