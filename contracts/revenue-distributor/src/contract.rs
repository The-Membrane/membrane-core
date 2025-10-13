use cosmwasm_std::{
    entry_point, to_json_binary, BankMsg, CosmosMsg, DepsMut, Env, MessageInfo, Response,
    StdResult, SubMsg, WasmMsg, Reply, Uint128, Decimal, Coin,
};
use membrane::revenue_distributor::{Config, RevenueDestination, RevenuePromise, ExecuteMsg, InstantiateMsg, QueryMsg, RDVaultInfoMessage};
use membrane::staking::ExecuteMsg as StakingExecuteMsg;
use membrane::math::decimal_multiplication;
use membrane::transmuter::ExecuteMsg as TransmuterExecuteMsg;
use membrane::types::Asset;

use crate::error::ContractError;
use crate::reply::{
    DISTRIBUTION_REPLY_ID,
    REVENUE_DESTINATION_REPLY_ID,
    ENTER_VAULT_REPLY_ID,
    handle_distribution_reply,
    handle_revenue_destination_reply,
    handle_enter_vault_reply,
};
use crate::state::{
    CONFIG,
    DISTRIBUTION_PROP,
    FAILED_DISTRIBUTIONS,
    PROMISES,
    LTV_DISCO_DISTRIBUTION,
    VAULT_PROPOGATION,
};

use cw2::set_contract_version;


// version info for migration info
const CONTRACT_NAME: &str = "crates.io:revenue-distributor";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");


/// Contract instantiation
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let owner = deps.api.addr_validate(&msg.owner)?;

    let config = Config {
        owner,
        canonical_asset: msg.canonical_asset,
        revenue_destinations: msg.revenue_destinations,
        ltv_disco: deps.api.addr_validate(&msg.ltv_disco)?,
        transmuter_vault: membrane::types::VaultInfo {
            vault_addr: deps.api.addr_validate(&msg.transmuter_vault.vault_addr)?,
            deposit_token: msg.transmuter_vault.deposit_token,
            vault_token: msg.transmuter_vault.vault_token,
        },
    };

    CONFIG.save(deps.storage, &config)?;
    PROMISES.save(deps.storage, &vec![])?;
    DISTRIBUTION_PROP.save(deps.storage, &vec![])?;
    VAULT_PROPOGATION.save(deps.storage, &vec![])?;
    
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::new().add_attribute("method", "instantiate"))
}

/// Contract execution
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::SetPromises { promises, ltv_disco_distribution } => set_promises(deps, env, info, promises, ltv_disco_distribution),
        ExecuteMsg::DistributePromises { limit } => distribute_promises(deps, env, info, limit),
        ExecuteMsg::UpdateConfig {
            revenue_destinations,
            ltv_disco,
            transmuter_vault,
        } => update_config(deps, env, info, revenue_destinations, ltv_disco, transmuter_vault),
        ExecuteMsg::ClearFailedDistributions {} => clear_failed_distributions(deps, env, info),
        ExecuteMsg::ClearPendingDistributions {} => clear_pending_distributions(deps, env, info),
        ExecuteMsg::RetryFailedDistribute { limit } => retry_failed_distribute(deps, env, info, limit),
    }
}

/// Set revenue promises for distribution
/// Validates that total promised amount <= sent amount
/// Any excess amount will be distributed to revenue_destinations
pub fn set_promises(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    promises: Vec<RevenuePromise>,
    ltv_disco_distributions: Option<Vec<Asset>>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut saved_promises = PROMISES.load(deps.storage)?;

    // Validate that we received exactly one asset type and it matches canonical asset
    if info.funds.len() != 1 {
        return Err(ContractError::Std(cosmwasm_std::StdError::GenericErr {
            msg: "Must send exactly one asset type".to_string(),
        }));
    }

    let sent_asset = &info.funds[0];
    let sent_amount = sent_asset.amount;

    // Validate that sent asset matches canonical asset
    if sent_asset.denom != config.canonical_asset.info.to_string() {
        return Err(ContractError::Std(cosmwasm_std::StdError::GenericErr {
            msg: format!(
                "Sent asset {} doesn't match canonical asset {}",
                sent_asset.denom,
                config.canonical_asset.info.to_string()
            ),
        }));
    }

    // Calculate total promised amount
    let mut total_promised = Uint128::zero();
    for promise in &promises {
        total_promised += promise.amount;
    }

    // Validate that promised amount <= sent amount
    if total_promised > sent_amount {
        return Err(ContractError::InvalidAmount {
            sent: sent_amount.u128(),
            promised: total_promised.u128(),
        });
    }
    // If ratios are provided, compute LTV_Distribution entries from the sent amount.
    // This is just to be able to track ratios for revenue to each asset.
    if let Some(distributions) = ltv_disco_distributions {
        // Record per-asset totals directly
        for asset in distributions.iter() {
            let key = asset.info.to_string();
            let current = LTV_DISCO_DISTRIBUTION.load(deps.storage, key.clone()).unwrap_or_else(|_| Uint128::zero());
            LTV_DISCO_DISTRIBUTION.save(deps.storage, key, &(current + asset.amount))?;
        }
    }

    //Aggregate new promises to existing promises
    for promise in promises {
        if let Some(existing_promise) = saved_promises.iter_mut().find(|p| p.address == promise.address) {
            existing_promise.amount += promise.amount;
        } else {
            saved_promises.push(promise);
        }
    }
    // Save promises
    PROMISES.save(deps.storage, &saved_promises)?;

    Ok(Response::new()
        .add_attribute("method", "set_promises")
        .add_attribute("promises_count", saved_promises.len().to_string())
        .add_attribute("total_promised", total_promised.to_string())
        .add_attribute("sent_amount", sent_amount.to_string()))
}

/// Distribute all current promises and clear them
/// Uses reply_on_error for failed distributions to continue with others
pub fn distribute_promises(
    mut deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    limit: Option<u32>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    // Load current promises
    let promises = PROMISES.load(deps.storage)?;
    if promises.is_empty() {
        return Err(ContractError::NoPromisesSet {});
    }

    let limit = limit.unwrap_or(promises.len() as u32);
    let mut messages: Vec<SubMsg> = vec![];
    let mut total_distributed = Uint128::zero();
    
    // We'll save only non-LTV promises in DISTRIBUTION_PROP for reply handling
    let mut non_ltv_promises: Vec<RevenuePromise> = vec![];
    // Track LTV promises queued (remove immediately after enqueuing submsgs)
    let mut ltv_promises_to_remove: Vec<RevenuePromise> = vec![];

    //Get balance of canonical asset
    let canonical_asset_amount = deps.querier.query_balance(
        env.contract.address.clone(),
        config.canonical_asset.info.to_string(),
    )?.amount;

    // Distribute promises
    for promise in &promises[..limit as usize] {
        let asset_coin = cosmwasm_std::Coin {
            denom: config.canonical_asset.info.to_string(),
            amount: promise.amount,
        };

        // Check if this is a revenue destination (staking contract)
        let is_revenue_destination = config
            .revenue_destinations
            .iter()
            .any(|dest| dest.destination.to_string() == promise.address);

        // LTV Disco should not be used as a promise, only as a destination
        let is_ltv_disco = promise.address == config.ltv_disco.to_string();
        if is_ltv_disco { ltv_promises_to_remove.push(promise.clone()); } else { non_ltv_promises.push(promise.clone()); }
        if is_ltv_disco { continue; }

        let msgs: Vec<SubMsg> = if is_revenue_destination {
            // Use DepositFee for revenue destinations
            vec![SubMsg::reply_always(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: promise.address.clone(),
                msg: to_json_binary(&StakingExecuteMsg::DepositFee {})?,
                funds: vec![asset_coin],
            }), DISTRIBUTION_REPLY_ID)]
        } else {
            // Use Bank send for affiliate fees
            vec![SubMsg::reply_always(CosmosMsg::Bank(BankMsg::Send {
                to_address: promise.address.clone(),
                amount: vec![asset_coin],
            }), DISTRIBUTION_REPLY_ID)]
        };

        messages.extend(msgs);
        total_distributed += promise.amount;
    }

    // Save only non-LTV pending promises for distribution replies
    DISTRIBUTION_PROP.save(deps.storage, &non_ltv_promises)?;

    // Remove LTV Disco promises that were enqueued fully
    if !ltv_promises_to_remove.is_empty() {
        let mut all_promises = PROMISES.load(deps.storage)?;
        for rem in ltv_promises_to_remove {
            if let Some(idx) = all_promises.iter().position(|p| p.address == rem.address && p.amount == rem.amount) {
                all_promises.remove(idx);
            }
        }
        PROMISES.save(deps.storage, &all_promises)?;
    }

    // Calculate remaining amount for revenue destinations
    let remaining_amount = canonical_asset_amount - total_distributed;

    if !remaining_amount.is_zero() && !config.revenue_destinations.is_empty() {
        // Distribute remaining amount to revenue destinations
        for destination in &config.revenue_destinations {
            if destination.distribution_ratio.is_zero() {
                continue;
            }

            let destination_amount = decimal_multiplication(
                Decimal::from_ratio(remaining_amount, Uint128::one()), 
                destination.distribution_ratio)?;
            
            if !destination_amount.is_zero() {
                let asset_coin = cosmwasm_std::Coin {
                    denom: config.canonical_asset.info.to_string(),
                    amount: destination_amount.to_uint_floor(),
                };

                // LTV Disco special path for destination handling
                if destination.destination == config.ltv_disco {
                    // Queue EnterVault for all assets
                    let fake_promise = RevenuePromise { address: destination.destination.to_string(), amount: asset_coin.amount };
                    let ltv_msgs = ltv_disco_enter_vault_msgs(
                        &mut deps, 
                        &config, 
                        &fake_promise
                    )?;
                    messages.extend(ltv_msgs);
                } else {
                    let msg = CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: destination.destination.to_string(),
                        msg: to_json_binary(&StakingExecuteMsg::DepositFee {})?,
                        funds: vec![asset_coin],
                    });
                    messages.push(SubMsg::reply_always(msg, REVENUE_DESTINATION_REPLY_ID));
                }
            }
        }
    }

    // Note: Promises will be cleared in the reply handler based on success/failure

    Ok(Response::new()
        .add_submessages(messages)
        .add_attribute("method", "distribute_promises")
        .add_attribute("promises_distributed", promises.len().to_string())
        .add_attribute("total_distributed", total_distributed.to_string())
        .add_attribute("remaining_distributed", remaining_amount.to_string()))
}

/// Update contract configuration (admin only)
pub fn update_config(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    revenue_destinations: Option<Vec<RevenueDestination>>,
    ltv_disco: Option<String>,
    transmuter_vault: Option<RDVaultInfoMessage>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;

    // Only owner can update config
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    // Update fields if provided
    if let Some(destinations) = revenue_destinations { config.revenue_destinations = destinations; }
    if let Some(ltv) = ltv_disco { config.ltv_disco = deps.api.addr_validate(&ltv)?; }
    if let Some(v) = transmuter_vault { 
        config.transmuter_vault = membrane::types::VaultInfo {
            vault_addr: deps.api.addr_validate(&v.vault_addr)?,
            deposit_token: v.deposit_token,
            vault_token: v.vault_token,
        };
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("method", "update_config"))
}

/// Clear failed distributions (admin only)
pub fn clear_failed_distributions(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only owner can clear failed distributions
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    // Clear all failed distributions
    let failed_distributions: Vec<String> = FAILED_DISTRIBUTIONS
        .keys(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .map(|item| item.unwrap())
        .collect();

    for key in failed_distributions {
        FAILED_DISTRIBUTIONS.remove(deps.storage, key);
    }

    Ok(Response::new()
        .add_attribute("method", "clear_failed_distributions"))
}

/// Clear pending distributions (admin only)
pub fn clear_pending_distributions(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only owner can clear pending distributions
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    // Clear pending distributions
    DISTRIBUTION_PROP.save(deps.storage, &vec![])?;

    Ok(Response::new()
        .add_attribute("method", "clear_pending_distributions"))
}

/// Retry failed distributions (admin only)
pub fn retry_failed_distribute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    limit: Option<u32>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only owner can retry failed distributions
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    // Get all failed distributions
    let failed_distributions: Vec<(String, u128)> = FAILED_DISTRIBUTIONS
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .map(|item| item.unwrap())
        .collect();

    if failed_distributions.is_empty() {
        return Ok(Response::new()
            .add_attribute("method", "retry_failed_distribute")
            .add_attribute("status", "no_failed_distributions"));
    }

    let limit = limit.unwrap_or(failed_distributions.len() as u32);
    let mut messages: Vec<SubMsg> = vec![];
    let mut retry_promises = vec![];

    // Convert failed distributions back to promises for retry
    for (address, amount) in failed_distributions.iter().take(limit as usize) {
        let address_clone = address.clone();
        let promise = RevenuePromise {
            address: address_clone.clone(),
            amount: Uint128::from(*amount),
        };
        retry_promises.push(promise.clone());

        // Create the distribution message
        let asset_coin = cosmwasm_std::Coin {
            denom: config.canonical_asset.info.to_string(),
            amount: Uint128::from(*amount),
        };

        // Check if this is a revenue destination (staking contract)
        let is_revenue_destination = config
            .revenue_destinations
            .iter()
            .any(|dest| dest.destination.to_string() == address_clone);

        let msg = if is_revenue_destination {
            // Use DepositFee for revenue destinations
            CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: address_clone.clone(),
                msg: to_json_binary(&StakingExecuteMsg::DepositFee {})?,
                funds: vec![asset_coin.clone()],
            })
        } else {
            // Use Bank send for affiliate fees
            CosmosMsg::Bank(BankMsg::Send {
                to_address: address_clone.clone(),
                amount: vec![asset_coin.clone()],
            })
        };

        // Use reply_always to handle both success and failure
        messages.push(SubMsg::reply_always(msg, DISTRIBUTION_REPLY_ID));
    }

    // Store retry promises in DISTRIBUTION_PROP
    DISTRIBUTION_PROP.save(deps.storage, &retry_promises)?;

    // Remove the retried failed distributions
    for (address, _) in failed_distributions.into_iter().take(limit as usize) {
        FAILED_DISTRIBUTIONS.remove(deps.storage, address);
    }

    Ok(Response::new()
        .add_submessages(messages)
        .add_attribute("method", "retry_failed_distribute")
        .add_attribute("retry_count", limit.to_string())
        .add_attribute("status", "retrying"))
}


fn ltv_disco_enter_vault_msgs(
    deps: &mut DepsMut,
    config: &Config,
    promise: &RevenuePromise,
) -> Result<Vec<SubMsg>, ContractError> {
    // Read current map and compute total
    let all_assets: Vec<(String, Uint128)> = LTV_DISCO_DISTRIBUTION
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .map(|kv| kv.unwrap())
        .collect();
    let total: Uint128 = all_assets.iter().fold(Uint128::zero(), |acc, (_, v)| acc + *v);

    if total.is_zero() || all_assets.is_empty() {
        return Err(ContractError::Std(cosmwasm_std::StdError::GenericErr {
            msg: "No assets to distribute".to_string(),
        }));
    }

    // Set asset order in propagation queue for reply handling
    let asset_denoms: Vec<String> = all_assets.iter().map(|(k, _)| k.clone()).collect();
    VAULT_PROPOGATION.save(deps.storage, &asset_denoms)?;

    // Build EnterVault messages for each asset in order
    let mut msgs: Vec<SubMsg> = vec![];
    for (_asset, asset_total) in all_assets {
        let portion = promise.amount.multiply_ratio(asset_total, total);
        if portion.is_zero() { continue; }
        let enter = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.transmuter_vault.vault_addr.to_string(),
            msg: to_json_binary(&TransmuterExecuteMsg::EnterVault { recipient: None })?,
            funds: vec![Coin { denom: config.canonical_asset.info.to_string(), amount: portion }],
        });
        msgs.push(SubMsg::reply_always(enter, ENTER_VAULT_REPLY_ID));
    }

    Ok(msgs)
}


/// Contract queries
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: cosmwasm_std::Deps, _env: Env, msg: QueryMsg) -> StdResult<cosmwasm_std::Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::Promises {} => to_json_binary(&PROMISES.load(deps.storage)?),
        QueryMsg::FailedDistributions {} => {
            let failed_distributions: Vec<(String, Uint128)> = FAILED_DISTRIBUTIONS
                .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
                .map(|item| {
                    let (key, value) = item.unwrap();
                    (key, Uint128::from(value))
                })
                .collect();
            to_json_binary(&failed_distributions)
        },
        QueryMsg::PendingDistributions {} => to_json_binary(&DISTRIBUTION_PROP.load(deps.storage)?),
    }
}

/// Handle replies from sub-messages
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> Result<Response, ContractError> {
    match msg.id {
        DISTRIBUTION_REPLY_ID => handle_distribution_reply(deps, env, msg),
        REVENUE_DESTINATION_REPLY_ID => handle_revenue_destination_reply(deps, env, msg),
        ENTER_VAULT_REPLY_ID => handle_enter_vault_reply(deps, env, msg),
        _ => Err(ContractError::Std(cosmwasm_std::StdError::GenericErr {
            msg: "Unknown reply ID".to_string(),
        })),
    }
}

