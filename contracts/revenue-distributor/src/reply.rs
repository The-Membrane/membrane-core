use cosmwasm_std::{
    DepsMut, Env, Response, SubMsgResult, Coin, CosmosMsg, WasmMsg, to_json_binary, SubMsg, Uint128,
};
use membrane::points_system::ExecuteMsg as PointsSystemExecuteMsg;
use membrane::revenue_distributor::ExecuteMsg;

use crate::error::ContractError;
use crate::state::{
    CONFIG,
    DISTRIBUTION_PROP,
    FAILED_DISTRIBUTIONS,
    PROMISES,
    PRE_TAKE_REVENUE_BALANCE,
    PRE_TAKE_REVENUE_PER_ASSET,
};

// Reply IDs
pub const DISTRIBUTION_REPLY_ID: u64 = 1u64;
pub const REVENUE_DESTINATION_REPLY_ID: u64 = 2u64;
pub const TAKE_REVENUE_REPLY_ID: u64 = 3u64;

/// Handle distribution reply (for both revenue destinations and affiliate fees)
pub fn handle_distribution_reply(
    deps: DepsMut,
    _env: Env,
    msg: cosmwasm_std::Reply,
) -> Result<Response, ContractError> {
    // Load the pending promises from DISTRIBUTION_PROP
    let mut pending_promises = DISTRIBUTION_PROP.load(deps.storage)?;
    
    if pending_promises.is_empty() {
        return Ok(Response::new()
            .add_attribute("method", "reply_distribution")
            .add_attribute("status", "no_pending_promises"));
    }
    
    // Pop the first promise (FIFO order)
    let promise = pending_promises.remove(0);
    
    // Update the DISTRIBUTION_PROP state
    DISTRIBUTION_PROP.save(deps.storage, &pending_promises)?;
    
    match msg.result {
        SubMsgResult::Ok(_) => {
            // Success: Remove the promise from the main PROMISES state
            let mut all_promises = PROMISES.load(deps.storage)?;
            if let Some(index) = all_promises.iter().position(|p| p.address == promise.address && p.amount == promise.amount) {
                all_promises.remove(index);
            }
            PROMISES.save(deps.storage, &all_promises)?;
            
            // Check if this is an affiliate (not a revenue destination) and award points
            let config = CONFIG.load(deps.storage)?;
            let is_revenue_destination = config
                .revenue_destinations
                .iter()
                .any(|dest| dest.destination.to_string() == promise.address);
            
            let mut response = Response::new()
                .add_attribute("method", "reply_distribution")
                .add_attribute("status", "success")
                .add_attribute("address", promise.address.clone())
                .add_attribute("amount", promise.amount.to_string());
            
            // If not a revenue destination, it's an affiliate - award points
            if !is_revenue_destination {
                if let Some(points_system) = &config.points_system_contract {
                    let points_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: points_system.to_string(),
                        msg: to_json_binary(&PointsSystemExecuteMsg::GivePointsForAffiliateFee {
                            affiliate: promise.address.clone(),
                            fee_amount: promise.amount,
                        })?,
                        funds: vec![],
                    });
                    // Use reply_on_error so points failure doesn't fail the distribution
                    response = response.add_submessage(SubMsg::reply_on_error(points_msg, 0));
                }
            }
            
            Ok(response)
        }
        SubMsgResult::Err(err) => {
            // Failure: Add to failed distributions and remove from main promises
            let mut all_promises = PROMISES.load(deps.storage)?;
            if let Some(index) = all_promises.iter().position(|p| p.address == promise.address && p.amount == promise.amount) {
                all_promises.remove(index);
            }
            PROMISES.save(deps.storage, &all_promises)?;
            
            // Add to failed distributions
            let current_failed = FAILED_DISTRIBUTIONS.load(deps.storage, promise.address.clone()).unwrap_or(0);
            FAILED_DISTRIBUTIONS.save(deps.storage, promise.address.clone(), &(current_failed + promise.amount.u128()))?;
            
            Ok(Response::new()
                .add_attribute("method", "reply_distribution")
                .add_attribute("status", "failed")
                .add_attribute("address", promise.address)
                .add_attribute("amount", promise.amount.to_string())
                .add_attribute("error", err))
        }
    }
}

/// Handle revenue destination reply (DepositFee call)
pub fn handle_revenue_destination_reply(
    _deps: DepsMut,
    _env: Env,
    msg: cosmwasm_std::Reply,
) -> Result<Response, ContractError> {
    match msg.result {
        SubMsgResult::Ok(_) => {
            Ok(Response::new()
                .add_attribute("method", "reply_revenue_destination")
                .add_attribute("status", "success"))
        }
        SubMsgResult::Err(err) => {
            // For revenue destinations, we don't track individual failures
            // but we log them for debugging
            Ok(Response::new()
                .add_attribute("method", "reply_revenue_destination")
                .add_attribute("status", "failed")
                .add_attribute("error", err))
        }
    }
}

/// Handle reply from CDP TakeRevenue
/// Uses saved per_asset_rev and calls SetPromises
pub fn handle_take_revenue_reply(
    deps: DepsMut,
    env: Env,
    msg: cosmwasm_std::Reply,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    
    match msg.result {
        SubMsgResult::Ok(_response) => {
            // Load the balance we saved before calling TakeRevenue
            let pre_balance = PRE_TAKE_REVENUE_BALANCE.load(deps.storage)
                .unwrap_or(Uint128::zero());
            
            // Get current CDT balance
            let current_balance = deps.querier.query_balance(
                env.contract.address.clone(),
                config.canonical_asset.info.to_string(),
            )?.amount;
            
            // Calculate actual revenue received (difference)
            let actual_revenue = current_balance.checked_sub(pre_balance)
                .unwrap_or(Uint128::zero());
            
            // Load the per_asset_rev we saved before calling TakeRevenue
            let per_asset_rev = PRE_TAKE_REVENUE_PER_ASSET.load(deps.storage)
                .unwrap_or_default();
            
            // Clear the saved data
            PRE_TAKE_REVENUE_BALANCE.remove(deps.storage);
            PRE_TAKE_REVENUE_PER_ASSET.remove(deps.storage);
            
            // Only proceed if we actually received revenue
            if !actual_revenue.is_zero() && !per_asset_rev.is_empty() {
                // Call SetPromises with empty promises and per-asset distribution
                // Then call DistributePromises to actually distribute
                let set_promises_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: env.contract.address.to_string(),
                    msg: to_json_binary(&ExecuteMsg::SetPromises {
                        promises: vec![],
                        ltv_disco_distribution: Some(per_asset_rev),
                    })?,
                    funds: vec![Coin {
                        denom: config.canonical_asset.info.to_string(),
                        amount: actual_revenue, // Use only the difference, not full balance
                    }],
                });
                
                let distribute_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: env.contract.address.to_string(),
                    msg: to_json_binary(&ExecuteMsg::DistributePromises { limit: None })?,
                    funds: vec![],
                });
                
                return Ok(Response::new()
                    .add_message(set_promises_msg)
                    .add_message(distribute_msg)
                    .add_attribute("method", "handle_take_revenue_reply")
                    .add_attribute("pre_balance", pre_balance.to_string())
                    .add_attribute("current_balance", current_balance.to_string())
                    .add_attribute("actual_revenue", actual_revenue.to_string()));
            }
            
            Ok(Response::new()
                .add_attribute("method", "handle_take_revenue_reply")
                .add_attribute("status", "no_revenue")
                .add_attribute("pre_balance", pre_balance.to_string())
                .add_attribute("current_balance", current_balance.to_string())
                .add_attribute("actual_revenue", actual_revenue.to_string()))
        }
        SubMsgResult::Err(err) => {
            // Clear the saved data on error
            PRE_TAKE_REVENUE_BALANCE.remove(deps.storage);
            PRE_TAKE_REVENUE_PER_ASSET.remove(deps.storage);
            
            Ok(Response::new()
                .add_attribute("method", "handle_take_revenue_reply")
                .add_attribute("status", "failed")
                .add_attribute("error", err))
        }
    }
}

