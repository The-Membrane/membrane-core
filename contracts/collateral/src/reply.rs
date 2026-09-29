#![allow(non_snake_case)]
use cosmwasm_std::{attr, BankMsg, Coin, CosmosMsg, DepsMut, Env, Reply, Response, Uint128};

use membrane::types::{Asset, cAsset};

use crate::error::ContractError;
use crate::state::{
    get_target_position, update_position, BASKET, POSITIONS,
    DEBT_ACCRUE_VALIDATE_REPLY_ID, SELL_COLLATERAL_PROPAGATION, SELL_COLLATERAL_REPLY_ID,
    WITHDRAW_PROPAGATION, WITHDRAW_REPLY_ID,
};

/// Main reply dispatcher.
///
/// CDP parity: maps to the Collateral-relevant portions of cdp/reply.rs
pub fn handle_reply(
    deps: DepsMut,
    env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    match msg.id {
        WITHDRAW_REPLY_ID => handle_withdraw_reply(deps, env, msg),
        DEBT_ACCRUE_VALIDATE_REPLY_ID => handle_debt_accrue_validate_reply(deps, env, msg),
        SELL_COLLATERAL_REPLY_ID => handle_sell_collateral_reply(deps, env, msg),
        id => Err(ContractError::CustomError {
            val: format!("Unknown reply ID: {}", id),
        }),
    }
}

/// Handle reply from Debt.AccrueAndValidate SubMsg during withdrawal flow.
///
/// On success: proceed with withdrawal (process BankMsgs, update position, update supply caps).
/// On error: tx reverts automatically (SubMsg error propagation).
///
/// CDP parity: maps to cdp/reply.rs::handle_withdraw_reply()
fn handle_withdraw_reply(
    mut deps: DepsMut,
    env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    match msg.result {
        cosmwasm_std::SubMsgResult::Ok(_response) => {
            // Debt contract has confirmed solvency.
            // Process the actual withdrawal.

            // Load propagation state saved during withdraw()
            let propagation = WITHDRAW_PROPAGATION.load(deps.storage)?;

            let owner = deps.api.addr_validate(&propagation.position_info.position_owner)?;
            let position_id = propagation.position_info.position_id;

            // Load position
            let (_index, mut position) =
                get_target_position(deps.storage, owner.clone(), position_id)?;

            // Build BankMsg::Send for each withdrawn asset and remove from position
            let mut bank_msgs: Vec<CosmosMsg> = vec![];
            for (i, prev_asset) in propagation.positions_prev_collateral.iter().enumerate() {
                let withdraw_amount = propagation
                    .withdraw_amounts
                    .get(i)
                    .copied()
                    .unwrap_or(Uint128::zero());

                if withdraw_amount.is_zero() {
                    continue;
                }

                // Send withdrawn asset to the position owner
                bank_msgs.push(CosmosMsg::Bank(BankMsg::Send {
                    to_address: owner.to_string(),
                    amount: vec![Coin {
                        denom: prev_asset.info.to_string(),
                        amount: withdraw_amount,
                    }],
                }));

                // Remove withdrawn amount from position's collateral_assets
                if let Some(collateral_index) = position
                    .collateral_assets
                    .iter()
                    .position(|ca| ca.asset.info.equal(&prev_asset.info))
                {
                    let current_amount = position.collateral_assets[collateral_index].asset.amount;
                    if withdraw_amount >= current_amount {
                        // Fully withdrawn -- remove the cAsset entry
                        position.collateral_assets.remove(collateral_index);
                    } else {
                        // Partial withdraw -- reduce amount
                        position.collateral_assets[collateral_index].asset.amount =
                            current_amount - withdraw_amount;
                    }
                }
            }

            // Build cAsset list for supply cap update (withdrawn amounts)
            let basket = BASKET.load(deps.storage)?;
            let withdrawn_cassets: Vec<cAsset> = propagation
                .positions_prev_collateral
                .iter()
                .enumerate()
                .filter_map(|(i, a)| {
                    let amount = propagation
                        .withdraw_amounts
                        .get(i)
                        .copied()
                        .unwrap_or(Uint128::zero());
                    if amount.is_zero() {
                        return None;
                    }
                    basket
                        .collateral_types
                        .iter()
                        .find(|ct| ct.asset.info.equal(&a.info))
                        .map(|ct| cAsset {
                            asset: Asset {
                                info: a.info.clone(),
                                amount,
                            },
                            ..ct.clone()
                        })
                })
                .collect();

            let position_collateral = position.collateral_assets.clone();

            // If position has no more collateral, remove it entirely
            if position.collateral_assets.is_empty() {
                POSITIONS.update(
                    deps.storage,
                    owner.clone(),
                    |old_positions| -> cosmwasm_std::StdResult<Vec<membrane::collateral::CollateralPosition>> {
                        match old_positions {
                            Some(positions) => {
                                let filtered: Vec<_> = positions
                                    .into_iter()
                                    .filter(|p| p.position_id != position_id)
                                    .collect();
                                Ok(filtered)
                            }
                            None => Ok(vec![]),
                        }
                    },
                )?;
            } else {
                // Save updated position
                update_position(deps.storage, owner.clone(), position)?;
            }

            // Update supply caps via risk_engine (subtract withdrawn amounts from tally)
            let _ = crate::risk_engine::update_basket_tally(
                &mut deps,
                env,
                withdrawn_cassets,
                position_collateral,
                false, // add = false (removing)
                false, // from_liquidation = false
            );

            // Clear propagation state
            WITHDRAW_PROPAGATION.remove(deps.storage);

            Ok(Response::new()
                .add_messages(bank_msgs)
                .add_attributes(vec![
                    attr("method", "withdraw_reply_success"),
                    attr("position_id", position_id.to_string()),
                    attr("position_owner", owner.to_string()),
                ]))
        }
        cosmwasm_std::SubMsgResult::Err(err) => {
            // Debt contract rejected the withdrawal (position would be insolvent).
            // SubMsg error causes entire tx to revert.
            Err(ContractError::CustomError {
                val: format!("Withdrawal rejected by Debt contract: {}", err),
            })
        }
    }
}

/// Handle reply from Debt.AccrueAndValidate during withdrawal flow.
/// This is the accrual reply that confirms debt is valid before proceeding.
///
/// On success: parse total_debt from reply attributes and continue withdrawal flow
/// by loading propagation state and building BankMsgs.
///
/// CDP parity: maps to the accrue validation portion of cdp/reply.rs
fn handle_debt_accrue_validate_reply(
    mut deps: DepsMut,
    env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    match msg.result {
        cosmwasm_std::SubMsgResult::Ok(response) => {
            // Accrual and validation passed.
            // Parse total_debt from reply attributes if present.
            let _total_debt: Uint128 = response
                .events
                .iter()
                .flat_map(|e| &e.attributes)
                .find(|a| a.key == "total_debt")
                .and_then(|a| a.value.parse::<u128>().ok())
                .map(Uint128::from)
                .unwrap_or(Uint128::zero());

            // Load propagation state saved during withdraw()
            let propagation = WITHDRAW_PROPAGATION.load(deps.storage)?;

            let owner = deps.api.addr_validate(&propagation.position_info.position_owner)?;
            let position_id = propagation.position_info.position_id;

            // Load position
            let (_index, mut position) =
                get_target_position(deps.storage, owner.clone(), position_id)?;

            // Build BankMsg::Send for each withdrawn asset and update position
            let mut bank_msgs: Vec<CosmosMsg> = vec![];
            for (i, prev_asset) in propagation.positions_prev_collateral.iter().enumerate() {
                let withdraw_amount = propagation
                    .withdraw_amounts
                    .get(i)
                    .copied()
                    .unwrap_or(Uint128::zero());

                if withdraw_amount.is_zero() {
                    continue;
                }

                // Send withdrawn asset to the position owner
                bank_msgs.push(CosmosMsg::Bank(BankMsg::Send {
                    to_address: owner.to_string(),
                    amount: vec![Coin {
                        denom: prev_asset.info.to_string(),
                        amount: withdraw_amount,
                    }],
                }));

                // Remove withdrawn amount from position's collateral_assets
                if let Some(collateral_index) = position
                    .collateral_assets
                    .iter()
                    .position(|ca| ca.asset.info.equal(&prev_asset.info))
                {
                    let current_amount = position.collateral_assets[collateral_index].asset.amount;
                    if withdraw_amount >= current_amount {
                        position.collateral_assets.remove(collateral_index);
                    } else {
                        position.collateral_assets[collateral_index].asset.amount =
                            current_amount - withdraw_amount;
                    }
                }
            }

            // Build cAsset list for supply cap update
            let basket = BASKET.load(deps.storage)?;
            let withdrawn_cassets: Vec<cAsset> = propagation
                .positions_prev_collateral
                .iter()
                .enumerate()
                .filter_map(|(i, a)| {
                    let amount = propagation
                        .withdraw_amounts
                        .get(i)
                        .copied()
                        .unwrap_or(Uint128::zero());
                    if amount.is_zero() {
                        return None;
                    }
                    basket
                        .collateral_types
                        .iter()
                        .find(|ct| ct.asset.info.equal(&a.info))
                        .map(|ct| cAsset {
                            asset: Asset {
                                info: a.info.clone(),
                                amount,
                            },
                            ..ct.clone()
                        })
                })
                .collect();

            let position_collateral = position.collateral_assets.clone();

            // If position has no more collateral, remove it entirely
            if position.collateral_assets.is_empty() {
                POSITIONS.update(
                    deps.storage,
                    owner.clone(),
                    |old_positions| -> cosmwasm_std::StdResult<Vec<membrane::collateral::CollateralPosition>> {
                        match old_positions {
                            Some(positions) => {
                                let filtered: Vec<_> = positions
                                    .into_iter()
                                    .filter(|p| p.position_id != position_id)
                                    .collect();
                                Ok(filtered)
                            }
                            None => Ok(vec![]),
                        }
                    },
                )?;
            } else {
                update_position(deps.storage, owner.clone(), position)?;
            }

            // Update supply caps via risk_engine (subtract withdrawn amounts)
            let _ = crate::risk_engine::update_basket_tally(
                &mut deps,
                env,
                withdrawn_cassets,
                position_collateral,
                false, // add = false (removing)
                false, // from_liquidation = false
            );

            // Clear propagation state
            WITHDRAW_PROPAGATION.remove(deps.storage);

            Ok(Response::new()
                .add_messages(bank_msgs)
                .add_attributes(vec![
                    attr("method", "debt_accrue_validate_reply_success"),
                    attr("position_id", position_id.to_string()),
                    attr("position_owner", owner.to_string()),
                ]))
        }
        cosmwasm_std::SubMsgResult::Err(err) => Err(ContractError::CustomError {
            val: format!("Debt accrual/validation failed: {}", err),
        }),
    }
}

/// Handle reply from sell collateral SubMsg (via Chain Proxy).
///
/// On success: load propagation, update position (reduce sold amounts),
/// update supply caps, and clear propagation state.
///
/// CDP parity: maps to cdp/reply.rs::handle_sell_collateral_reply()
fn handle_sell_collateral_reply(
    mut deps: DepsMut,
    env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    match msg.result {
        cosmwasm_std::SubMsgResult::Ok(_response) => {
            // Sell completed successfully.

            // Load propagation state saved during authorized_sell_collateral()
            let propagation = SELL_COLLATERAL_PROPAGATION.load(deps.storage)?;

            let owner = deps.api.addr_validate(&propagation.position_info.position_owner)?;
            let position_id = propagation.position_info.position_id;

            // Load position
            let (_index, mut position) =
                get_target_position(deps.storage, owner.clone(), position_id)?;

            // Remove sold collateral amounts from position
            for sold_coin in propagation.collateral_sold.iter() {
                if let Some(collateral_index) = position
                    .collateral_assets
                    .iter()
                    .position(|ca| ca.asset.info.to_string() == sold_coin.denom)
                {
                    let current_amount = position.collateral_assets[collateral_index].asset.amount;
                    if sold_coin.amount >= current_amount {
                        // Fully sold -- remove the cAsset entry
                        position.collateral_assets.remove(collateral_index);
                    } else {
                        // Partial sell -- reduce amount
                        position.collateral_assets[collateral_index].asset.amount =
                            current_amount - sold_coin.amount;
                    }
                }
            }

            // Build cAsset list for supply cap update (sold amounts)
            let basket = BASKET.load(deps.storage)?;
            let sold_cassets: Vec<cAsset> = propagation
                .collateral_sold
                .iter()
                .filter_map(|coin| {
                    if coin.amount.is_zero() {
                        return None;
                    }
                    basket
                        .collateral_types
                        .iter()
                        .find(|ct| ct.asset.info.to_string() == coin.denom)
                        .map(|ct| cAsset {
                            asset: Asset {
                                info: ct.asset.info.clone(),
                                amount: coin.amount,
                            },
                            ..ct.clone()
                        })
                })
                .collect();

            let position_collateral = position.collateral_assets.clone();

            // If position has no more collateral, remove it entirely
            if position.collateral_assets.is_empty() {
                POSITIONS.update(
                    deps.storage,
                    owner.clone(),
                    |old_positions| -> cosmwasm_std::StdResult<Vec<membrane::collateral::CollateralPosition>> {
                        match old_positions {
                            Some(positions) => {
                                let filtered: Vec<_> = positions
                                    .into_iter()
                                    .filter(|p| p.position_id != position_id)
                                    .collect();
                                Ok(filtered)
                            }
                            None => Ok(vec![]),
                        }
                    },
                )?;
            } else {
                // Save updated position
                update_position(deps.storage, owner.clone(), position)?;
            }

            // Update supply caps via risk_engine (subtract sold amounts from tally)
            let _ = crate::risk_engine::update_basket_tally(
                &mut deps,
                env,
                sold_cassets,
                position_collateral,
                false, // add = false (removing)
                false, // from_liquidation = false
            );

            // Clear propagation state
            SELL_COLLATERAL_PROPAGATION.remove(deps.storage);

            Ok(Response::new()
                .add_attributes(vec![
                    attr("method", "sell_collateral_reply_success"),
                    attr("position_id", position_id.to_string()),
                    attr("position_owner", owner.to_string()),
                    attr("collateral_sold", format!("{:?}", propagation.collateral_sold)),
                ]))
        }
        cosmwasm_std::SubMsgResult::Err(err) => Err(ContractError::CustomError {
            val: format!("Sell collateral failed: {}", err),
        }),
    }
}
