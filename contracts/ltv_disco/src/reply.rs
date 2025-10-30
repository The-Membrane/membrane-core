use cosmwasm_std::{DepsMut, Env, Reply, Response, StdResult, Uint128, Coin, to_json_binary, CosmosMsg, WasmMsg};

use crate::state::{SWAP_PROPAGATION, CONFIG};
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