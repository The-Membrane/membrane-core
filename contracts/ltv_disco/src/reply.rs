use cosmwasm_std::{DepsMut, Env, Reply, Response, StdResult, Uint128};

use crate::state::{BAD_DEBT_PROPAGATION, PENDING_BAD_DEBT};

pub fn handle_transmuter_withdraw_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.result.into_result() {
        //On success, we remove the pending bad debt
        //We only reply on success for bad debt RETRIES
        Ok(_result) => {
            //Get the asset from the BadDebtPropagation
            let propagation = BAD_DEBT_PROPAGATION.load(deps.storage)?;

            //Update the pending bad debt map
            PENDING_BAD_DEBT.remove(deps.storage, propagation.asset.clone());

            Ok(Response::new()
                .add_attribute("method", "handle_transmuter_withdraw_reply_success")
                .add_attribute("asset", propagation.asset.clone())
            )
        }
        //On error, we add to pending bad debt for the asset
        Err(_) => {
            //Load the BadDebtPropagation
            let bad_debt_propagation = BAD_DEBT_PROPAGATION.load(deps.storage)?;

            //Add to pending bad debt
            PENDING_BAD_DEBT.update(deps.storage, bad_debt_propagation.asset.clone(), |amount| -> StdResult<Uint128> {
                Ok(amount.unwrap_or(Uint128::zero()) + bad_debt_propagation.amount)
            })?;

            Ok(Response::new()
                .add_attribute("method", "handle_transmuter_withdraw_reply_error")
                .add_attribute("asset", bad_debt_propagation.asset)
                .add_attribute("amount", bad_debt_propagation.amount.to_string())
            ) 
        }
    }
}