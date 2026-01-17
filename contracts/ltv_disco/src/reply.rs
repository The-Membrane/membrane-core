use cosmwasm_std::{DepsMut, Env, Reply, Response, StdResult, Uint128, Coin, to_json_binary, CosmosMsg, WasmMsg, Decimal, attr, SubMsg};
use std::str::FromStr;

use crate::state::{SWAP_PROPAGATION, COMPOUND_PROPAGATION, CONFIG};
use crate::error::ContractError;
use membrane::ltv_disco::ExecuteMsg;
use membrane::cdp::ExecuteMsg as CDP_ExecuteMsg;

//DON'T DELETE
// pub fn handle_transmuter_withdraw_reply(deps: DepsMut, _env: Env, msg: Reply) -> StdResult<Response> {
//     match msg.result.into_result() {
//         //On success, we remove the pending bad debt
//         //We only reply on success for bad debt RETRIES
//         Ok(_result) => {
//             //Get the asset from the BadDebtPropagation
//             let propagation = BAD_DEBT_PROPAGATION.load(deps.storage)?;

//             //Update the pending bad debt map
//             PENDING_BAD_DEBT.remove(deps.storage, propagation.asset.clone());

//             Ok(Response::new()
//                 .add_attribute("method", "handle_transmuter_withdraw_reply_success")
//                 .add_attribute("asset", propagation.asset.clone())
//             )
//         }
//         //On error, we add to pending bad debt for the asset
//         Err(_) => {
//             //Load the BadDebtPropagation
//             let bad_debt_propagation = BAD_DEBT_PROPAGATION.load(deps.storage)?;

//             //Add to pending bad debt
//             PENDING_BAD_DEBT.update(deps.storage, bad_debt_propagation.asset.clone(), |amount| -> StdResult<Uint128> {
//                 Ok(amount.unwrap_or(Uint128::zero()) + bad_debt_propagation.amount)
//             })?;

//             Ok(Response::new()
//                 .add_attribute("method", "handle_transmuter_withdraw_reply_error")
//                 .add_attribute("asset", bad_debt_propagation.asset)
//                 .add_attribute("amount", bad_debt_propagation.amount.to_string())
//             ) 
//         }
//     }
// }

/// Handle reply from liquidation swap
/// Calculates swapped CDT amount and sends it to CDP contract
pub fn handle_liquidation_swap_reply(deps: DepsMut, env: Env, _msg: Reply) -> StdResult<Response> {
    let config = CONFIG.load(deps.storage)?;
    
    // Load CDT balance before swap
    let swap_propagation = SWAP_PROPAGATION.load(deps.storage)?;
    let balance_before = swap_propagation.cdt_balance_before;
    
    // Query current CDT balance
    let current_balance: Coin = deps.querier.query_balance(
        env.contract.address,
        config.cdt_denom.clone(),
    )?;
    
    // Calculate swapped amount (CRITICAL: only send the difference, not the full balance)
    let swapped_amount = current_balance.amount.checked_sub(balance_before)
        .unwrap_or(Uint128::zero());
    
    // Clean up swap propagation state
    SWAP_PROPAGATION.remove(deps.storage);
    
    // Create FulfillBadDebt message with ONLY the swapped amount
    let mut msgs: Vec<CosmosMsg> = vec![];
    if !swapped_amount.is_zero() {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.cdp_contract.to_string(),
            msg: to_json_binary(&CDP_ExecuteMsg::FulfillBadDebt {})?,
            funds: vec![Coin {
                denom: config.cdt_denom,
                amount: swapped_amount,
            }],
        }));
    }
    
    Ok(Response::new()
        .add_messages(msgs)
        .add_attribute("method", "handle_liquidation_swap_reply")
        .add_attribute("balance_before", balance_before.to_string())
        .add_attribute("balance_after", current_balance.amount.to_string())
        .add_attribute("swapped_amount", swapped_amount.to_string())
    )
}

/// Handle reply from compound swap
/// 
/// This function processes the result of a compound swap operation. After CDT is swapped to
/// deposit tokens via neutron_proxy, this handler:
/// 1. Calculates how many new deposit tokens were received
/// 2. Distributes them proportionally to each deposit based on their CDT contribution
/// 3. Creates SubMsgs to call submit_deposit for each deposit (reusing existing deposit logic)
/// 
/// # Proportional Distribution
/// Each deposit receives: new_tokens * (deposit_cdt_contribution / total_cdt_contributed)
/// 
/// # Implementation
/// Instead of duplicating deposit logic, we create SubMsgs that call submit_deposit with:
/// - The proportional share of deposit tokens as funds
/// - The existing deposit_id so it tops up the existing deposit
/// - All state updates (vault tokens, group totals, TVL tracker) are handled by submit_deposit
/// 
/// # Error Handling
/// - Returns early if no new tokens received (swap may have failed or returned zero)
/// - Skips invalid deposit keys gracefully
/// - Validates all decimal/uint conversions
/// 
/// Distributes newly received deposit tokens proportionally to deposits that contributed
pub fn handle_compound_swap_reply(deps: DepsMut, env: Env, _msg: Reply) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    
    // Load compound propagation state
    let propagation = COMPOUND_PROPAGATION.load(deps.storage)
        .map_err(|_| ContractError::CustomError { val: "Compound propagation state not found".to_string() })?;
    
    // Query current deposit token balance
    let current_balance: Coin = deps.querier.query_balance(
        env.contract.address.clone(),
        config.deposit_denom.denom.clone(),
    )?;
    
    // Calculate newly received deposit tokens (current - before)
    // CRITICAL: Only the difference is new tokens from the swap
    // The contract may have had existing deposit tokens, so we must subtract the before balance
    let new_deposit_tokens = current_balance.amount.checked_sub(propagation.deposit_token_balance_before)
        .unwrap_or(Uint128::zero());
    
    // Clean up compound propagation state
    COMPOUND_PROPAGATION.remove(deps.storage);
    
    // If no new tokens, return early
    if new_deposit_tokens.is_zero() {
        return Ok(Response::new()
            .add_attribute("method", "handle_compound_swap_reply")
            .add_attribute("new_deposit_tokens", "0")
            .add_attribute("message", "No new deposit tokens received from swap"));
    }
    
    // Calculate total CDT that was contributed
    let total_contributed: Uint128 = propagation.deposit_contributions.iter()
        .map(|(_, amount)| amount)
        .sum();
    
    if total_contributed.is_zero() {
        return Err(ContractError::CustomError {
            val: "Total contributed amount is zero".to_string(),
        });
    }
    
    let asset = propagation.asset.clone();
    let deposits_count = propagation.deposit_contributions.len();
    let mut submsgs: Vec<SubMsg> = vec![];
    
    
    // Process each deposit contribution and create submit_deposit SubMsgs
    for (deposit_key_str, cdt_contribution) in propagation.deposit_contributions {
        // Parse deposit key to extract components
        let parts: Vec<&str> = deposit_key_str.split(':').collect();
        if parts.len() != 5 && parts.len() != 6 {
            continue; // Skip invalid keys
        }
        
        let asset_str = parts[0].to_string();
        let ltv_str = parts[1];
        let max_borrow_ltv_str = parts[2];
        let user_str = parts[3].to_string();
        let deposit_id_str = parts[4];
        let epoch_start_time = if parts.len() == 6 {
            u64::from_str(parts[5]).ok()
        } else {
            None
        };
        
        let ltv = Decimal::from_str(ltv_str)
            .map_err(|_| ContractError::CustomError { val: "Invalid LTV format".to_string() })?;
        let max_borrow_ltv = Decimal::from_str(max_borrow_ltv_str)
            .map_err(|_| ContractError::CustomError { val: "Invalid max_borrow_ltv format".to_string() })?;
        let deposit_id = Uint128::from_str(deposit_id_str)
            .map_err(|_| ContractError::CustomError { val: "Invalid deposit_id format".to_string() })?;
        
        // Calculate proportional share of new deposit tokens
        // Each deposit gets: new_tokens * (their_cdt / total_cdt)
        // This ensures fair distribution based on contribution size
        let deposit_share = new_deposit_tokens.multiply_ratio(cdt_contribution, total_contributed);
        
        if deposit_share.is_zero() {
            continue;
        }
        
        // Create SubMsg to call submit_deposit with the proportional share
        // submit_deposit will handle all the logic: existing deposit check, vault token calculation,
        // group updates, TVL tracking, etc. Since we pass the deposit_id, it will top-up the existing deposit
        let submit_deposit_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::SubmitDeposit {
                affiliate_address: None,
                deposit_input: membrane::ltv_disco::BackingDepositInput {
                    asset: asset_str.clone(),
                    ltv,
                    max_borrow_ltv,
                    epoch_start_time,
                },
                deposit_owner: Some(user_str), // Specify the owner
                locked: None, // Don't change lock status
                deposit_id: Some(deposit_id), // Use existing deposit_id to top-up
                manager: None,
            })?,
            funds: vec![Coin {
                denom: config.deposit_denom.denom.clone(),
                amount: deposit_share,
            }],
        });
        
        submsgs.push(SubMsg::new(submit_deposit_msg));
    }
    
    Ok(Response::new()
        .add_submessages(submsgs)
        .add_attributes(vec![
            attr("method", "handle_compound_swap_reply"),
            attr("asset", asset),
            attr("new_deposit_tokens", new_deposit_tokens.to_string()),
            attr("deposits_updated", deposits_count.to_string()),
        ]))
}