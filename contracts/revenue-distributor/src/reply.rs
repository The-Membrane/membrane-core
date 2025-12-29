use cosmwasm_std::{
    DepsMut, Env, Response, SubMsgResult, Coin, CosmosMsg, WasmMsg, to_json_binary,
};
use membrane::ltv_disco::ExecuteMsg as LTVExecuteMsg;

use crate::error::ContractError;
use crate::state::{
    CONFIG,
    DISTRIBUTION_PROP,
    FAILED_DISTRIBUTIONS,
    PROMISES,
};

// Reply IDs
pub const DISTRIBUTION_REPLY_ID: u64 = 1u64;
pub const REVENUE_DESTINATION_REPLY_ID: u64 = 2u64;

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
            
            Ok(Response::new()
                .add_attribute("method", "reply_distribution")
                .add_attribute("status", "success")
                .add_attribute("address", promise.address)
                .add_attribute("amount", promise.amount.to_string()))
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

