#![allow(non_snake_case)]
use std::str::FromStr;

use cosmwasm_std::{
    attr, to_json_binary, Coin, CosmosMsg, Decimal, DepsMut, Env, Reply, Response,
    SubMsg, Uint128, WasmMsg,
};

use membrane::chain_proxy::ExecuteMsg as ChainProxyExecuteMsg;
use membrane::collateral::{CollateralAssessmentResponse, ExecuteMsg as CollateralExecuteMsg};
use membrane::types::AssetInfo;

use crate::debt_manager::{
    apply_delta_to_caps, apply_delta_to_peg_debt, apply_delta_to_regular_debt,
    combined_breakdown_for_caps, create_segments_from_split, SegmentSplit,
};
use crate::error::ContractError;
use crate::rates::get_total_position_debt;
use crate::state::{
    get_target_position, update_position,
    BASKET, CDT_SUPPLY, CLOSE_POSITION_PROPAGATION, CLOSE_POSITION_SELL_REPLY_ID,
    CLOSE_POSITION_WITHDRAW_REPLY_ID, COLLATERAL_REFRESH_REPLY_ID, CONFIG,
    DEPLOYABLE_VENUE_PROPAGATION, DEPLOYABLE_VENUE_REPLY_ID,
    INCREASE_DEBT_PROPAGATION, RATES, REVENUE_REPLY_ID,
    VENUE_RECALL_PROPAGATION, VENUE_RECALL_REPLY_ID,
};

/// Main reply dispatcher for Debt contract.
///
/// CDP parity: maps to the Debt-relevant portions of cdp/reply.rs
pub fn handle_reply(
    deps: DepsMut,
    env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    match msg.id {
        COLLATERAL_REFRESH_REPLY_ID => handle_collateral_refresh_reply(deps, env, msg),
        CLOSE_POSITION_SELL_REPLY_ID => handle_close_position_sell_reply(deps, env, msg),
        CLOSE_POSITION_WITHDRAW_REPLY_ID => handle_close_position_withdraw_reply(deps, env, msg),
        DEPLOYABLE_VENUE_REPLY_ID => handle_deployable_venue_reply(deps, env, msg),
        VENUE_RECALL_REPLY_ID => handle_venue_recall_reply(deps, env, msg),
        REVENUE_REPLY_ID => handle_revenue_reply(deps, env, msg),
        id => Err(ContractError::CustomError {
            val: format!("Unknown reply ID: {}", id),
        }),
    }
}

/// Handle reply from Collateral.RefreshAndAssess during IncreaseDebt flow.
///
/// On success: parse collateral assessment, validate solvency, create segments, mint CDT.
///
/// CDP parity: maps to the collateral assessment parsing part of cdp/positions.rs::increase_debt()
fn handle_collateral_refresh_reply(
    deps: DepsMut,
    env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    match msg.result {
        cosmwasm_std::SubMsgResult::Ok(response) => {
            // Step 1: Load propagation state
            let prop = INCREASE_DEBT_PROPAGATION.load(deps.storage)?;
            let config = CONFIG.load(deps.storage)?;
            let mut basket = BASKET.load(deps.storage)?;
            let mut rates = RATES.load(deps.storage)?;

            let owner = deps.api.addr_validate(&prop.position_owner)?;
            let (_index, mut position) =
                get_target_position(deps.storage, owner.clone(), prop.position_id)?;

            // Step 2: Parse CollateralAssessmentResponse from reply events/attributes
            let assessment_json = response
                .events
                .iter()
                .flat_map(|e| e.attributes.iter())
                .find(|a| a.key == "assessment_response")
                .ok_or_else(|| ContractError::CustomError {
                    val: "Missing assessment_response in reply".to_string(),
                })?
                .value
                .clone();

            let assessment: CollateralAssessmentResponse =
                cosmwasm_std::from_json(assessment_json.as_bytes()).map_err(|e| {
                    ContractError::CustomError {
                        val: format!("Failed to parse CollateralAssessmentResponse: {}", e),
                    }
                })?;

            // Step 3: Check if any assets are frozen
            if assessment.any_frozen {
                return Err(ContractError::Frozen {});
            }

            // Step 4: Calculate total collateral value from assessment
            let mut total_collateral_value = Decimal::zero();
            for (i, c_asset) in assessment.collateral_assets.iter().enumerate() {
                if let Some(price_resp) = assessment.prices.get(i) {
                    let asset_value = Decimal::from_ratio(c_asset.asset.amount, 1u128)
                        * price_resp.price;
                    total_collateral_value += asset_value;
                }
            }

            // Step 5: Determine mint amount
            let existing_debt = get_total_position_debt(&position);
            let credit_price = basket.credit_price.price;

            let mint_amount = if let Some(amount) = prop.amount {
                amount
            } else if let Some(ltv) = prop.ltv {
                // Calculate from LTV: debt_value = collateral_value * ltv
                // mint_amount = (target_debt_value - existing_debt_value) / credit_price
                let target_debt_value = total_collateral_value * ltv;
                let existing_debt_value = Decimal::from_ratio(existing_debt, 1u128) * credit_price;
                let additional_debt_value = target_debt_value
                    .checked_sub(existing_debt_value)
                    .unwrap_or(Decimal::zero());
                if additional_debt_value.is_zero() || credit_price.is_zero() {
                    return Err(ContractError::CustomError {
                        val: "LTV target already exceeded or credit price is zero".to_string(),
                    });
                }
                (additional_debt_value / credit_price).to_uint_floor()
            } else {
                return Err(ContractError::CustomError {
                    val: "Must specify amount or LTV".to_string(),
                });
            };

            if mint_amount.is_zero() {
                return Err(ContractError::CustomError {
                    val: "Mint amount is zero".to_string(),
                });
            }

            // Step 6: Run solvency / borrow LTV checks
            let total_debt_after = existing_debt + mint_amount;
            let debt_value_after = Decimal::from_ratio(total_debt_after, 1u128) * credit_price;

            if !total_collateral_value.is_zero() {
                let current_ltv =
                    membrane::risk::compute_ltv(total_collateral_value, debt_value_after);
                if !membrane::risk::can_borrow(current_ltv, assessment.avg_max_borrow_ltv) {
                    return Err(ContractError::MaxBorrowLTVExceeded {});
                }
                if !membrane::risk::check_solvency(current_ltv, assessment.avg_max_ltv) {
                    return Err(ContractError::Insolvent {});
                }
            }

            // Step 7: Check debt minimum
            if total_debt_after < config.debt_minimum {
                return Err(ContractError::BelowDebtMinimum {
                    minimum: config.debt_minimum,
                });
            }

            // Step 8: Build debt split for segment creation
            let split = if let Some(ref ds) = prop.debt_split {
                if prop.peg_debt {
                    SegmentSplit {
                        variable: ds.peg_variable,
                        one_month: ds.peg_one_month,
                        three_month: ds.peg_three_month,
                        six_month: ds.peg_six_month,
                        one_month_rollover: ds.peg_one_month_rollover,
                        three_month_rollover: ds.peg_three_month_rollover,
                        six_month_rollover: ds.peg_six_month_rollover,
                    }
                } else {
                    SegmentSplit {
                        variable: ds.variable,
                        one_month: ds.one_month,
                        three_month: ds.three_month,
                        six_month: ds.six_month,
                        one_month_rollover: ds.one_month_rollover,
                        three_month_rollover: ds.three_month_rollover,
                        six_month_rollover: ds.six_month_rollover,
                    }
                }
            } else {
                // Default: all variable
                SegmentSplit {
                    variable: Some(Decimal::one()),
                    ..SegmentSplit::default()
                }
            };

            // Step 9: Create rate segments
            let combined_view = combined_breakdown_for_caps(&basket.credit_asset);
            let base_rate = rates.base_interest_rate;

            let is_first_debt = existing_debt.is_zero();

            let deltas = if prop.peg_debt {
                create_segments_from_split(
                    mint_amount,
                    &split,
                    &rates.fixed_rate_caps,
                    base_rate,
                    &mut position.peg_rate_segments,
                    &combined_view,
                    total_debt_after,
                    &env,
                )?
            } else {
                create_segments_from_split(
                    mint_amount,
                    &split,
                    &rates.fixed_rate_caps,
                    base_rate,
                    &mut position.rate_segments,
                    &combined_view,
                    total_debt_after,
                    &env,
                )?
            };

            // Step 10: Apply deltas to basket and rates
            if prop.peg_debt {
                apply_delta_to_peg_debt(&mut basket.credit_asset, &deltas);
            } else {
                apply_delta_to_regular_debt(&mut basket.credit_asset, &deltas);
            }
            apply_delta_to_caps(&mut rates.fixed_rate_caps, &deltas);

            // Step 11: Build messages
            let mut msgs: Vec<SubMsg> = vec![];

            // Mint CDT via chain proxy
            let credit_denom = match &basket.credit_asset.info {
                AssetInfo::NativeToken { denom } => denom.clone(),
                AssetInfo::Token { address } => address.to_string(),
            };

            let mint_to = if let Some(ref addr) = prop.mint_to_addr {
                addr.clone()
            } else {
                prop.position_owner.clone()
            };

            if let Some(ref chain_proxy) = config.chain_proxy {
                let mint_msg = WasmMsg::Execute {
                    contract_addr: chain_proxy.to_string(),
                    msg: to_json_binary(&ChainProxyExecuteMsg::MintTokens {
                        denom: credit_denom.clone(),
                        amount: mint_amount,
                        mint_to_address: mint_to.clone(),
                    })?,
                    funds: vec![],
                };
                msgs.push(SubMsg::new(mint_msg));
            }

            // Track CDT supply increase
            let mut cdt_supply = CDT_SUPPLY.load(deps.storage)?;
            cdt_supply += mint_amount;
            CDT_SUPPLY.save(deps.storage, &cdt_supply)?;

            // Step 12: If first debt, activate supply caps
            if is_first_debt {
                if let Some(ref collateral_contract) = config.collateral_contract {
                    let activate_msg = WasmMsg::Execute {
                        contract_addr: collateral_contract.to_string(),
                        msg: to_json_binary(&CollateralExecuteMsg::ActivateSupplyCaps {
                            position_id: prop.position_id,
                            position_owner: prop.position_owner.clone(),
                        })?,
                        funds: vec![],
                    };
                    msgs.push(SubMsg::new(activate_msg));
                }
            }

            // Save state
            RATES.save(deps.storage, &rates)?;
            BASKET.save(deps.storage, &basket)?;
            update_position(deps.storage, owner, position)?;

            // Clean up propagation state
            INCREASE_DEBT_PROPAGATION.remove(deps.storage);

            Ok(Response::new()
                .add_submessages(msgs)
                .add_attributes(vec![
                    attr("method", "collateral_refresh_reply_success"),
                    attr("position_id", prop.position_id),
                    attr("mint_amount", mint_amount),
                    attr("mint_to", mint_to),
                    attr("peg_debt", prop.peg_debt.to_string()),
                    attr("total_debt_after", total_debt_after),
                ]))
        }
        cosmwasm_std::SubMsgResult::Err(err) => {
            // Clean up propagation state on failure
            INCREASE_DEBT_PROPAGATION.remove(deps.storage);
            Err(ContractError::CustomError {
                val: format!("Collateral refresh failed: {}", err),
            })
        }
    }
}

/// Handle reply from Collateral.AuthorizedSellCollateral during ClosePosition flow.
///
/// CDP parity: maps to cdp/reply.rs::handle_close_position_reply()
fn handle_close_position_sell_reply(
    deps: DepsMut,
    env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    match msg.result {
        cosmwasm_std::SubMsgResult::Ok(response) => {
            // Step 1: Load propagation state
            let prop = CLOSE_POSITION_PROPAGATION.load(deps.storage)?;
            let config = CONFIG.load(deps.storage)?;
            let basket = BASKET.load(deps.storage)?;

            let owner = deps.api.addr_validate(&prop.position_info.position_owner)?;
            let position_id = prop.position_info.position_id;

            // Step 2: Parse sell proceeds from reply events
            // Look for the credit asset balance that the sell produced
            let credit_denom = match &basket.credit_asset.info {
                AssetInfo::NativeToken { denom } => denom.clone(),
                AssetInfo::Token { address } => address.to_string(),
            };

            // Parse proceeds amount from reply events
            let proceeds_amount = response
                .events
                .iter()
                .flat_map(|e| e.attributes.iter())
                .find(|a| a.key == "sell_proceeds_amount")
                .and_then(|a| Uint128::from_str(&a.value).ok())
                .unwrap_or(Uint128::zero());

            // Step 3: Create self-repay message if proceeds > 0
            let mut msgs: Vec<SubMsg> = vec![];

            if proceeds_amount > Uint128::zero() {
                let repay_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: env.contract.address.to_string(),
                    msg: to_json_binary(&membrane::debt::ExecuteMsg::Repay {
                        position_id,
                        position_owner: Some(owner.to_string()),
                        send_excess_to: Some(
                            prop.send_to
                                .clone()
                                .unwrap_or(owner.to_string()),
                        ),
                        debt_split: None,
                    })?,
                    funds: vec![Coin {
                        denom: credit_denom.clone(),
                        amount: proceeds_amount,
                    }],
                });
                msgs.push(SubMsg::new(repay_msg));
            }

            // Step 4: SubMsg to Collateral.AuthorizedWithdraw for remaining collateral
            // The close_percentage determines how much collateral to withdraw
            let collateral_contract = config
                .collateral_contract
                .ok_or(ContractError::CustomError {
                    val: "No collateral contract configured".to_string(),
                })?;

            let send_to_addr = prop
                .send_to
                .clone()
                .unwrap_or(owner.to_string());

            // Send AuthorizedWithdraw for remaining assets as a SubMsg with reply
            let withdraw_msg = WasmMsg::Execute {
                contract_addr: collateral_contract.to_string(),
                msg: to_json_binary(&CollateralExecuteMsg::AuthorizedWithdraw {
                    position_id,
                    position_owner: owner.to_string(),
                    assets: vec![], // Empty = withdraw all remaining
                    send_to: send_to_addr.clone(),
                })?,
                funds: vec![],
            };
            msgs.push(SubMsg::reply_on_success(
                withdraw_msg,
                CLOSE_POSITION_WITHDRAW_REPLY_ID,
            ));

            Ok(Response::new()
                .add_submessages(msgs)
                .add_attributes(vec![
                    attr("method", "close_position_sell_reply_success"),
                    attr("position_id", position_id),
                    attr("proceeds_amount", proceeds_amount),
                    attr("send_to", send_to_addr),
                ]))
        }
        cosmwasm_std::SubMsgResult::Err(err) => {
            // Clean up propagation state on failure
            CLOSE_POSITION_PROPAGATION.remove(deps.storage);
            Err(ContractError::CustomError {
                val: format!("Close position sell failed: {}", err),
            })
        }
    }
}

/// Handle reply from Collateral.AuthorizedWithdraw during ClosePosition flow.
fn handle_close_position_withdraw_reply(
    deps: DepsMut,
    _env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    match msg.result {
        cosmwasm_std::SubMsgResult::Ok(_response) => {
            // Load and clean up propagation state
            let prop = CLOSE_POSITION_PROPAGATION.load(deps.storage)?;
            CLOSE_POSITION_PROPAGATION.remove(deps.storage);

            Ok(Response::new()
                .add_attributes(vec![
                    attr("method", "close_position_withdraw_reply_success"),
                    attr("position_id", prop.position_info.position_id),
                    attr("position_owner", prop.position_info.position_owner),
                ]))
        }
        cosmwasm_std::SubMsgResult::Err(err) => {
            // Clean up propagation state on failure
            CLOSE_POSITION_PROPAGATION.remove(deps.storage);
            Err(ContractError::CustomError {
                val: format!("Close position withdraw failed: {}", err),
            })
        }
    }
}

/// Handle reply from deployable venue execution.
///
/// CDP parity: maps to cdp/reply.rs::handle_deployable_venue_reply()
fn handle_deployable_venue_reply(
    deps: DepsMut,
    _env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    match msg.result {
        cosmwasm_std::SubMsgResult::Ok(_response) => {
            // Load and clean up propagation state on success
            let prop = DEPLOYABLE_VENUE_PROPAGATION.load(deps.storage)?;
            DEPLOYABLE_VENUE_PROPAGATION.remove(deps.storage);

            Ok(Response::new()
                .add_attributes(vec![
                    attr("method", "deployable_venue_reply_success"),
                    attr("venue", prop.venue_address),
                    attr("position_id", prop.position_info.position_id),
                ]))
        }
        cosmwasm_std::SubMsgResult::Err(err) => {
            // On error: mark the venue's failed_liquidation flag on the position
            if let Ok(prop) = DEPLOYABLE_VENUE_PROPAGATION.load(deps.storage) {
                let owner = deps
                    .api
                    .addr_validate(&prop.position_info.position_owner);
                if let Ok(owner_addr) = owner {
                    if let Ok((_idx, mut position)) = get_target_position(
                        deps.storage,
                        owner_addr.clone(),
                        prop.position_info.position_id,
                    ) {
                        // Set failed_liquidation on the matching venue
                        let venue_addr = deps.api.addr_validate(&prop.venue_address);
                        if let Ok(validated_addr) = venue_addr {
                            if let Some(venue) = position
                                .deployed_to
                                .iter_mut()
                                .find(|v| v.address == validated_addr)
                            {
                                venue.failed_liquidation = true;
                            }
                            // Save updated position
                            let _ = update_position(deps.storage, owner_addr, position);
                        }
                    }
                }
                // Clean up propagation state
                DEPLOYABLE_VENUE_PROPAGATION.remove(deps.storage);
            }

            // Non-critical: log and continue
            Ok(Response::new()
                .add_attribute("method", "deployable_venue_reply_error")
                .add_attribute("failed_liquidation", "true")
                .add_attribute("error", err))
        }
    }
}

/// Handle reply from venue recall during liquidation.
///
/// reply_on_error means we only enter on failure.
/// Pop the front venue from propagation, set failed_liquidation = true
/// on the position so the venue is skipped on subsequent liquidations.
fn handle_venue_recall_reply(
    deps: DepsMut,
    _env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    match msg.result {
        cosmwasm_std::SubMsgResult::Err(err) => {
            if let Ok(mut prop) = VENUE_RECALL_PROPAGATION.load(deps.storage) {
                if let Some(venue_addr_str) = prop.venues.first().cloned() {
                    // Pop the front venue (this is the one that failed)
                    prop.venues.remove(0);

                    // Mark failed_liquidation on the position
                    let owner = deps.api.addr_validate(&prop.position_owner)?;
                    let (_idx, mut position) = get_target_position(
                        deps.storage,
                        owner.clone(),
                        prop.position_id,
                    )?;

                    if let Ok(venue_addr) = deps.api.addr_validate(&venue_addr_str) {
                        if let Some(venue) = position
                            .deployed_to
                            .iter_mut()
                            .find(|v| v.address == venue_addr)
                        {
                            venue.failed_liquidation = true;
                        }
                    }
                    update_position(deps.storage, owner, position)?;

                    // Save updated propagation (or clean up if empty)
                    if prop.venues.is_empty() {
                        VENUE_RECALL_PROPAGATION.remove(deps.storage);
                    } else {
                        VENUE_RECALL_PROPAGATION.save(deps.storage, &prop)?;
                    }
                }
            }

            Ok(Response::new()
                .add_attribute("method", "venue_recall_reply_error")
                .add_attribute("failed_liquidation", "true")
                .add_attribute("error", err))
        }
        // reply_on_error means success shouldn't reach here, but handle gracefully:
        // pop the front venue from propagation since it succeeded
        _ => {
            if let Ok(mut prop) = VENUE_RECALL_PROPAGATION.load(deps.storage) {
                if !prop.venues.is_empty() {
                    prop.venues.remove(0);
                }
                if prop.venues.is_empty() {
                    VENUE_RECALL_PROPAGATION.remove(deps.storage);
                } else {
                    VENUE_RECALL_PROPAGATION.save(deps.storage, &prop)?;
                }
            }

            Ok(Response::new()
                .add_attribute("method", "venue_recall_reply_ok"))
        }
    }
}

/// Handle reply from revenue distribution.
///
/// CDP parity: maps to cdp/reply.rs::handle_revenue_reply()
fn handle_revenue_reply(
    _deps: DepsMut,
    _env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    match msg.result {
        cosmwasm_std::SubMsgResult::Ok(_response) => {
            Ok(Response::new().add_attribute("method", "revenue_reply_success"))
        }
        cosmwasm_std::SubMsgResult::Err(err) => {
            // Revenue errors are non-critical
            Ok(Response::new()
                .add_attribute("method", "revenue_reply_error")
                .add_attribute("error", err))
        }
    }
}
