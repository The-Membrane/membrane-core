use cosmwasm_std::{DepsMut, Env, Reply, Response, StdResult, Uint128, Coin, to_json_binary, CosmosMsg, WasmMsg, attr, SubMsg};
use std::str::FromStr;

use crate::state::{COMPOUND_PROPAGATION, CONFIG};
use crate::error::ContractError;
use membrane::ltv_disco::ExecuteMsg;

/// Handle reply from compound swap
///
/// After CDT is swapped to deposit tokens via neutron_proxy, this handler:
/// 1. Calculates how many new deposit tokens were received
/// 2. Distributes them proportionally to each deposit based on their CDT contribution
/// 3. Creates SubMsgs to call submit_deposit for each deposit (reusing existing deposit logic)
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

    // Calculate newly received deposit tokens
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
        // Parse deposit key: "asset:slot:user:deposit_id"
        let parts: Vec<&str> = deposit_key_str.split(':').collect();
        if parts.len() != 4 {
            continue; // Skip invalid keys
        }

        let asset_str = parts[0].to_string();
        let slot = match u8::from_str(parts[1]) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let user_str = parts[2].to_string();
        let deposit_id = match Uint128::from_str(parts[3]) {
            Ok(id) => id,
            Err(_) => continue,
        };

        // Calculate proportional share of new deposit tokens
        let deposit_share = new_deposit_tokens.multiply_ratio(cdt_contribution, total_contributed);

        if deposit_share.is_zero() {
            continue;
        }

        // Create SubMsg to call submit_deposit with the proportional share
        let submit_deposit_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::SubmitDeposit {
                revenue_destination: None,
                affiliate_address: None,
                deposit_input: membrane::ltv_disco::BackingDepositInput {
                    asset: asset_str,
                    slot,
                },
                deposit_owner: Some(user_str),
                deposit_id: Some(deposit_id),
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
