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
    VAULT_PROPOGATION,
};

// Reply IDs
pub const DISTRIBUTION_REPLY_ID: u64 = 1u64;
pub const REVENUE_DESTINATION_REPLY_ID: u64 = 2u64;
pub const ENTER_VAULT_REPLY_ID: u64 = 3u64;

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

/// Handle enter vault reply (after entering transmuter vault)
pub fn handle_enter_vault_reply(
    deps: DepsMut,
    env: Env,
    _msg: cosmwasm_std::Reply,
) -> Result<Response, ContractError> {
    // After entering vault, send any newly minted VT to LTV_Disco via AddRevenue
    // We pop the first asset denom from VAULT_PROPOGATION to track which asset this deposit was for.
    let mut assets_queue = VAULT_PROPOGATION.load(deps.storage)?;
    let maybe_asset = if assets_queue.is_empty() { None } else { Some(assets_queue.remove(0)) };
    VAULT_PROPOGATION.save(deps.storage, &assets_queue)?;

    let config = CONFIG.load(deps.storage)?;

    // Determine amount of VT to send: entire VT balance of this contract
    let vt_denom = config.transmuter_vault.vault_token.clone();
    let vt_balance = deps.querier.query_balance(env.contract.address.clone(), vt_denom.clone())?.amount;

    if vt_balance.is_zero() {
        return Ok(Response::new().add_attribute("method", "reply_enter_vault").add_attribute("status", "no_vt"));
    }

    // Send VT to LTV Disco
    if let Some(asset_denom) = maybe_asset.clone() {
        let send_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.ltv_disco.to_string(),
            msg: to_json_binary(&LTVExecuteMsg::AddRevenue { asset: asset_denom.clone() })?,
            funds: vec![Coin { denom: vt_denom, amount: vt_balance }],
        });
        return Ok(Response::new().add_message(send_msg).add_attribute("method", "reply_enter_vault"));
    }

    Ok(Response::new().add_attribute("method", "reply_enter_vault").add_attribute("status", "no_asset"))
}
