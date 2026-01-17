use cosmwasm_std::{
    entry_point, to_json_binary, Addr, Binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env,
    MessageInfo, QueryRequest, Response, StdError, StdResult, Uint128, WasmMsg, WasmQuery,
};
use cw2::set_contract_version;

use membrane::mars_mirror::{
    Config, ExecuteMsg, InstantiateMsg, MoveProgress, QueryMsg, MarsLTVInfoResponse,
};
use membrane::mars_params::{QueryMsg as MarsParams_QueryMsg, AssetParams};
use std::str::FromStr;
use membrane::ltv_disco::{ExecuteMsg as LTV_Disco_ExecuteMsg, QueryMsg as LTV_Disco_QueryMsg, ManagedDepositKeysResponse};
use membrane::types::Locked;

use crate::error::ContractError;
use crate::state::{CONFIG, MOVE_PROGRESS};

const CONTRACT_NAME: &str = "membrane-mars-mirror";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let owner = msg
        .owner
        .map(|o| deps.api.addr_validate(&o))
        .transpose()?
        .unwrap_or_else(|| info.sender.clone());

    // Validate addresses
    let mars_params_contract = deps.api.addr_validate(&msg.mars_params_contract)?;
    let disco_contract = deps.api.addr_validate(&msg.disco_contract)?;

    let config = Config {
        owner: owner.clone(),
        mars_params_contract: mars_params_contract.to_string(),
        disco_contract,
    };

    CONFIG.save(deps.storage, &config)?;
    MOVE_PROGRESS.save(
        deps.storage,
        &MoveProgress {
            last_processed_key: None,
            total_keys: 0,
            processed_count: 0,
        },
    )?;

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::new()
        .add_attribute("action", "instantiate")
        .add_attribute("owner", owner.as_str()))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::UpdateConfig {
            owner,
            mars_params_contract,
            disco_contract,
        } => execute_update_config(
            deps,
            info,
            owner,
            mars_params_contract,
            disco_contract,
        ),
        // ExecuteMsg::Deposit {
        //     user,
        //     asset,
        //     lock,
        // } => execute_deposit(deps, env, info, user, asset, lock),
        ExecuteMsg::ProcessMoves {
            limit,
            start_after,
        } => execute_process_moves(deps, env, info, limit, start_after),
        // ExecuteMsg::MoveSingleDeposit {
        //     deposit_key,
        //     new_ltv,
        //     new_max_borrow_ltv,
        // } => execute_move_single_deposit(deps, env, info, deposit_key, new_ltv, new_max_borrow_ltv),
    }
}

fn execute_update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    mars_params_contract: Option<String>,
    disco_contract: Option<String>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    ensure_owner(&config, &info.sender)?;

    if let Some(owner_str) = owner {
        config.owner = deps.api.addr_validate(&owner_str)?;
    }

    if let Some(mars_params) = mars_params_contract {
        deps.api.addr_validate(&mars_params)?;
        config.mars_params_contract = mars_params;
    }

    if let Some(disco) = disco_contract {
        config.disco_contract = deps.api.addr_validate(&disco)?;
    }


    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("action", "update_config"))
}

// fn execute_deposit(
//     deps: DepsMut,
//     env: Env,
//     info: MessageInfo,
//     user: Option<String>,
//     asset: String,
//     lock: Option<Locked>,
// ) -> Result<Response, ContractError> {
//     let config = CONFIG.load(deps.storage)?;

//     // Get MBRN amount from funds
//     let mbrn_amount = info
//         .funds
//         .iter()
//         .find(|f| {
//             // Assume incentive_denom is MBRN - this should match Transmuter's incentive_denom
//             // In production, this might need to be configurable
//             true // For now, accept any denom sent
//         })
//         .ok_or_else(|| ContractError::Validation("No funds provided".into()))?
//         .amount;

//     if mbrn_amount.is_zero() {
//         return Err(ContractError::Validation("Amount must be greater than zero".into()));
//     }

//     // Query Mars for asset LTVs
//     let mars_ltv_info = query_mars_ltv_info(deps.as_ref(), &config, &asset)?;

//     // Determine user address
//     let user_addr = user
//         .map(|u| deps.api.addr_validate(&u))
//         .transpose()?
//         .unwrap_or_else(|| info.sender.clone());

//     // Create deposit message to Disco
//     // Parse deposit key format to determine ltv and max_borrow_ltv
//     // Use Mars LTVs or provided target LTVs from intent (if we had them)
//     // For now, use Mars LTVs directly
//     let deposit_msg = CosmosMsg::Wasm(WasmMsg::Execute {
//         contract_addr: config.disco_contract.to_string(),
//         msg: to_json_binary(&LTV_Disco_ExecuteMsg::SubmitDeposit {
//             deposit_input: membrane::ltv_disco::BackingDepositInput {
//                 asset: asset.clone(),
//                 ltv: mars_ltv_info.max_ltv,
//                 max_borrow_ltv: mars_ltv_info.max_borrow_ltv,
//             },
//             deposit_owner: Some(user_addr.to_string()),
//             locked: lock,
//             deposit_id: None,
//             manager: Some(env.contract.address.to_string()), // Set this contract as manager
//         })?,
//         funds: info.funds,
//     });

//     Ok(Response::new()
//         .add_message(deposit_msg)
//         .add_attribute("action", "deposit")
//         .add_attribute("user", user_addr.to_string())
//         .add_attribute("asset", asset)
//         .add_attribute("amount", mbrn_amount.to_string()))
// }

fn execute_process_moves(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    limit: Option<u32>,
    start_after: Option<String>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    
    // Only contract owner can call this
    // ensure_owner(&config, &info.sender)?;

    // Query Disco for managed deposit keys
    let response: ManagedDepositKeysResponse = deps.querier.query::<ManagedDepositKeysResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.disco_contract.to_string(),
        msg: to_json_binary(&LTV_Disco_QueryMsg::GetManagedDepositKeys {
            manager: env.contract.address.to_string(),
            limit,
            start_after,
        })?,
    }))?;

    if response.keys.is_empty() {
        // No more keys to process, clear progress
        MOVE_PROGRESS.remove(deps.storage);
        return Ok(Response::new()
            .add_attribute("action", "process_moves")
            .add_attribute("processed", "0")
            .add_attribute("complete", "true"));
    }

    let process_limit = limit.unwrap_or(10).min(20) as usize;
    let keys_to_process: Vec<String> = response.keys.into_iter().take(process_limit).collect();

    let mut msgs = Vec::new();
    let mut processed_count = 0u64;
    let mut last_processed = None;

    // Process each key
    for deposit_key in keys_to_process {
        // Parse deposit key: "asset:ltv:max_borrow_ltv:user:deposit_id"
        let parts: Vec<&str> = deposit_key.split(':').collect();
        if parts.len() != 5 {
            continue;
        }
        let asset = parts[0];

        // Query Mars market_v2 for LTVs
        let mars_ltv_info = query_mars_ltv_info(deps.as_ref(), &config, asset)?;

        //Normalize mars ltvs by rounding down 
        let mars_max_ltv = mars_ltv_info.max_ltv.floor();
        let mars_max_borrow_ltv = mars_ltv_info.max_borrow_ltv.floor();

        // Create move message to Disco
        // Note: We need to parse the current deposit to get the full parameters
        // For now, we'll use MoveDeposit with the deposit_id from the key
        // But MoveDeposit requires asset, ltv, max_borrow_ltv, deposit_id, destination
        // We need to query the deposit first to get current values
        // Actually, looking at MoveDeposit signature, we need: asset, ltv, max_borrow_ltv, deposit_id, destination
        // We can extract asset, ltv, max_borrow_ltv from the key, and deposit_id too
        
        let current_ltv = Decimal::from_str(parts[1])
            .map_err(|_| ContractError::Validation("Invalid LTV in deposit key".into()))?;
        let current_max_borrow_ltv = Decimal::from_str(parts[2])
            .map_err(|_| ContractError::Validation("Invalid max_borrow_ltv in deposit key".into()))?;
        let user = parts[3].to_string();
        let deposit_id = Uint128::from_str(parts[4])
            .map_err(|_| ContractError::Validation("Invalid deposit_id in deposit key".into()))?;

        // Only move if LTVs have changed
        if current_ltv != mars_max_ltv || current_max_borrow_ltv != mars_max_borrow_ltv {
            let move_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.disco_contract.to_string(),
                msg: to_json_binary(&LTV_Disco_ExecuteMsg::MoveDeposit {
                    asset: asset.to_string(),
                    ltv: current_ltv,
                    max_borrow_ltv: current_max_borrow_ltv,
                    deposit_id,
                    destination: membrane::ltv_disco::BackingDepositInput {
                        asset: asset.to_string(),
                        ltv: mars_max_ltv,
                        max_borrow_ltv: mars_max_borrow_ltv,
                    },
                    amount: None, // Move full deposit
                    user: Some(user),
                })?,
                funds: vec![],
            });

            msgs.push(move_msg);
        }
        
        last_processed = Some(deposit_key.clone());
        processed_count += 1;
    }

    // Save progress
    let progress = MOVE_PROGRESS.may_load(deps.storage)?.unwrap_or(MoveProgress {
        last_processed_key: None,
        total_keys: response.total,
        processed_count: 0,
    });

    MOVE_PROGRESS.save(
        deps.storage,
        &MoveProgress {
            last_processed_key: last_processed.clone(),
            total_keys: response.total,
            processed_count: progress.processed_count + processed_count,
        },
    )?;

    let complete = processed_count < process_limit as u64
        || progress.processed_count + processed_count >= response.total;

    Ok(Response::new()
        .add_messages(msgs)
        .add_attribute("action", "process_moves")
        .add_attribute("processed", processed_count.to_string())
        .add_attribute("total", response.total.to_string())
        .add_attribute("complete", if complete { "true" } else { "false" }))
}

// fn execute_move_single_deposit(
//     deps: DepsMut,
//     env: Env,
//     info: MessageInfo,
//     deposit_key: String,
//     new_ltv: Decimal,
//     new_max_borrow_ltv: Decimal,
// ) -> Result<Response, ContractError> {
//     let config = CONFIG.load(deps.storage)?;
//     ensure_owner(&config, &info.sender)?;

//     // Parse deposit key
//     let parts: Vec<&str> = deposit_key.split(':').collect();
//     if parts.len() != 5 {
//         return Err(ContractError::Validation("Invalid deposit key format".into()));
//     }

//     let asset = parts[0];
//     let current_ltv = Decimal::from_str(parts[1])
//         .map_err(|_| ContractError::Validation("Invalid LTV in deposit key".into()))?;
//     let current_max_borrow_ltv = Decimal::from_str(parts[2])
//         .map_err(|_| ContractError::Validation("Invalid max_borrow_ltv in deposit key".into()))?;
//     let deposit_id = Uint128::from_str(parts[4])
//         .map_err(|_| ContractError::Validation("Invalid deposit_id in deposit key".into()))?;

//     let move_msg = CosmosMsg::Wasm(WasmMsg::Execute {
//         contract_addr: config.disco_contract.to_string(),
//         msg: to_json_binary(&LTV_Disco_ExecuteMsg::MoveDeposit {
//             asset: asset.to_string(),
//             ltv: current_ltv,
//             max_borrow_ltv: current_max_borrow_ltv,
//             deposit_id,
//             destination: membrane::ltv_disco::BackingDepositInput {
//                 asset: asset.to_string(),
//                 ltv: new_ltv,
//                 max_borrow_ltv: new_max_borrow_ltv,
//             },
//             amount: None,
//         })?,
//         funds: vec![],
//     });

//     Ok(Response::new()
//         .add_message(move_msg)
//         .add_attribute("action", "move_single_deposit")
//         .add_attribute("deposit_key", deposit_key))
// }

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::MoveProgress {} => to_json_binary(&MOVE_PROGRESS.may_load(deps.storage)?.unwrap_or(MoveProgress {
            last_processed_key: None,
            total_keys: 0,
            processed_count: 0,
        })),
        QueryMsg::MarsLTVInfo { asset } => {
            let config = CONFIG.load(deps.storage)?;
            let result = query_mars_ltv_info(deps, &config, &asset)
                .map_err(|e| StdError::generic_err(e.to_string()))?;
            to_json_binary(&result)
        }
    }
}

fn query_mars_ltv_info(
    deps: Deps,
    config: &Config,
    asset: &str,
) -> Result<MarsLTVInfoResponse, ContractError> {
    // Query Mars Params contract for AssetParams
    let query_msg = QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.mars_params_contract.clone(),
        msg: to_json_binary(&MarsParams_QueryMsg::AssetParams {
            denom: asset.to_string(),
        })?,
    });

    // Query Mars Params for asset parameters
    // AssetParams has: max_loan_to_value (max_borrow_ltv) and liquidation_threshold (max_ltv)
    let asset_params: Option<AssetParams> = deps.querier.query(&query_msg)
        .map_err(|e| ContractError::Std(StdError::generic_err(format!("Failed to query Mars Params: {}", e))))?;
    
    // Extract LTVs from AssetParams
    let params = asset_params.ok_or_else(|| ContractError::Validation(format!("Asset {} not found in Mars Params", asset)))?;

    Ok(MarsLTVInfoResponse {
        max_ltv: params.liquidation_threshold,
        max_borrow_ltv: params.max_loan_to_value,
    })
}

fn ensure_owner(config: &Config, sender: &Addr) -> Result<(), ContractError> {
    if &config.owner != sender {
        return Err(ContractError::Unauthorized {});
    }
    Ok(())
}

