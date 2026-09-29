#![allow(non_snake_case)]
use std::str::FromStr;

use cosmwasm_std::{
    attr, to_json_binary, BankMsg, Coin, CosmosMsg, Decimal, DepsMut, Env, Reply, Response,
    SubMsg, SubMsgResult, Uint128, WasmMsg,
};

use membrane::collateral::{
    CollateralAssessmentResponse, ExecuteMsg as CollateralExecuteMsg,
};
use membrane::debt::{AccrueDebtResponse, ExecuteMsg as DebtExecuteMsg};
use membrane::liq_queue::{
    CollateralForDebtResponse, ExecuteMsg as LQ_ExecuteMsg, QueryMsg as LQ_QueryMsg,
};
use membrane::liquidation_engine::{CallbackMsg, ExecuteMsg};
use membrane::math::Uint256;
use membrane::types::{Asset, AssetInfo};

use crate::error::ContractError;
use crate::liquidations::get_repay_quantities;
use crate::state::{
    BAD_DEBT_REPLY_ID, CLAIM_UPDATE_REPLY_ID, COLLATERAL_ASSESS_REPLY_ID, CONFIG,
    DEBT_ACCRUE_REPLY_ID, DEBT_UPDATE_REPLY_ID, FEE_TRANSFER_REPLY_ID,
    LIQUIDATION_PROPAGATION, LIQUIDATION_STATS, LIQUIDATION_TIMERS,
    LIQ_QUEUE_REPLY_ID, LIQ_QUEUE_TRANSFER_REPLY_ID, LiquidationEvent,
    LiquidationOutcome, MAX_LIQUIDATION_STATS, PositionAssetSnapshot,
    SELL_WALL_REPLY_ID,
};

/// Build position asset snapshots from collateral assessment data.
fn build_asset_snapshots(
    collateral_assets: &[membrane::types::cAsset],
    prices: &[membrane::oracle::PriceResponse],
) -> Vec<PositionAssetSnapshot> {
    collateral_assets
        .iter()
        .enumerate()
        .map(|(i, ca)| {
            let value = if i < prices.len() {
                prices[i].get_value(ca.asset.amount).unwrap_or(Decimal::zero())
            } else {
                Decimal::zero()
            };
            PositionAssetSnapshot {
                denom: ca.asset.info.to_string(),
                amount: ca.asset.amount,
                value,
                max_ltv: ca.max_LTV,
            }
        })
        .collect()
}

/// Push a LiquidationEvent into LIQUIDATION_STATS with FIFO eviction.
fn save_liquidation_event(
    storage: &mut dyn cosmwasm_std::Storage,
    event: LiquidationEvent,
) -> Result<(), ContractError> {
    let mut stats = LIQUIDATION_STATS
        .may_load(storage)?
        .unwrap_or_default();
    stats.push(event);
    if stats.len() > MAX_LIQUIDATION_STATS {
        let excess = stats.len() - MAX_LIQUIDATION_STATS;
        stats.drain(..excess);
    }
    LIQUIDATION_STATS.save(storage, &stats)?;
    Ok(())
}

/// Main reply dispatcher for Liquidation Engine.
///
/// Reply chain: 1 → 2 → 9 → 3 → 4 → 5 → 6 → 7 → 8
///
/// CDP parity: maps to the liquidation-related portions of cdp/reply.rs
pub fn handle_reply(
    deps: DepsMut,
    env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    match msg.id {
        DEBT_ACCRUE_REPLY_ID => handle_debt_accrue_reply(deps, env, msg),
        COLLATERAL_ASSESS_REPLY_ID => handle_collateral_assess_reply(deps, env, msg),
        FEE_TRANSFER_REPLY_ID => handle_fee_transfer_reply(deps, env, msg),
        LIQ_QUEUE_TRANSFER_REPLY_ID => handle_liq_queue_transfer_reply(deps, env, msg),
        LIQ_QUEUE_REPLY_ID => handle_liq_queue_reply(deps, env, msg),
        SELL_WALL_REPLY_ID => handle_sell_wall_reply(deps, env, msg),
        DEBT_UPDATE_REPLY_ID => handle_debt_update_reply(deps, env, msg),
        CLAIM_UPDATE_REPLY_ID => handle_claim_update_reply(deps, env, msg),
        BAD_DEBT_REPLY_ID => {
            // No-op handler for BadDebtCheck callback errors.
            // BadDebtCheck is sent as reply_on_error so failures don't revert the liquidation.
            Ok(Response::new().add_attribute("method", "bad_debt_reply_noop"))
        }
        id => Err(ContractError::CustomError {
            val: format!("Unknown reply ID: {}", id),
        }),
    }
}

/// Helper: find an attribute value from a SubMsgResult's events.
/// Searches all events for the first attribute matching `key`.
fn find_attribute_value(
    result: &cosmwasm_std::SubMsgResponse,
    key: &str,
) -> Result<String, ContractError> {
    for event in &result.events {
        for attr in &event.attributes {
            if attr.key == key {
                return Ok(attr.value.clone());
            }
        }
    }
    Err(ContractError::CustomError {
        val: format!("Attribute '{}' not found in reply events", key),
    })
}

/// Step 1 reply: Debt.AccrueAndReturnDebt succeeded.
/// Parse debt data from "debt_response" attribute, store in propagation,
/// then send SubMsg to Collateral.RefreshAndAssess.
///
/// CDP parity: maps to the first step of cdp/liquidations.rs flow
fn handle_debt_accrue_reply(
    deps: DepsMut,
    _env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    match msg.result {
        SubMsgResult::Ok(response) => {
            let config = CONFIG.load(deps.storage)?;
            let mut prop = LIQUIDATION_PROPAGATION.load(deps.storage)?;

            // Parse AccrueDebtResponse from the "debt_response" attribute.
            // The Debt contract serializes this as JSON binary in the attribute value.
            let debt_json = find_attribute_value(&response, "debt_response")?;
            let debt_response: AccrueDebtResponse =
                cosmwasm_std::from_json(debt_json.as_bytes()).map_err(|e| {
                    ContractError::CustomError {
                        val: format!("Failed to parse AccrueDebtResponse: {}", e),
                    }
                })?;

            // Store parsed debt data in propagation
            prop.total_debt = debt_response.total_debt;
            prop.credit_price = debt_response.credit_price;
            prop.venue_recall_amount = debt_response.venue_recall_amount;
            LIQUIDATION_PROPAGATION.save(deps.storage, &prop)?;

            // Build SubMsg to Collateral.RefreshAndAssess
            let assess_msg = WasmMsg::Execute {
                contract_addr: config.collateral_contract.to_string(),
                msg: to_json_binary(&CollateralExecuteMsg::RefreshAndAssess {
                    position_owner: prop.position_owner.to_string(),
                    position_id: prop.position_id,
                })?,
                funds: vec![],
            };

            Ok(Response::new()
                .add_submessage(SubMsg::reply_on_success(
                    assess_msg,
                    COLLATERAL_ASSESS_REPLY_ID,
                ))
                .add_attribute("method", "debt_accrue_reply_success")
                .add_attribute("total_debt", debt_response.total_debt.to_string()))
        }
        SubMsgResult::Err(err) => Err(ContractError::CustomError {
            val: format!("Debt accrual failed during liquidation: {}", err),
        }),
    }
}

/// Step 2 reply: Collateral.RefreshAndAssess succeeded.
/// Parse collateral assessment, run insolvency check, calculate fees,
/// compute per-asset fee amounts (CDP per_asset_fulfillments lines 745-858),
/// then chain to fee extraction via AuthorizedTransfer → FEE_TRANSFER_REPLY_ID.
///
/// CDP parity: maps to collateral assessment + insolvency check + fee computation
/// in cdp/liquidations.rs::liquidate() + per_asset_fulfillments()
fn handle_collateral_assess_reply(
    mut deps: DepsMut,
    env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    match msg.result {
        SubMsgResult::Ok(response) => {
            let config = CONFIG.load(deps.storage)?;
            let mut prop = LIQUIDATION_PROPAGATION.load(deps.storage)?;

            // Parse CollateralAssessmentResponse from the "assessment" attribute
            let assessment_json = find_attribute_value(&response, "assessment")?;
            let assessment: CollateralAssessmentResponse =
                cosmwasm_std::from_json(assessment_json.as_bytes()).map_err(|e| {
                    ContractError::CustomError {
                        val: format!("Failed to parse CollateralAssessmentResponse: {}", e),
                    }
                })?;

            // Circuit breaker: error if any collateral asset is frozen
            if assessment.any_frozen {
                return Err(ContractError::CustomError {
                    val: format!(
                        "Circuit breaker: frozen assets detected: {:?}",
                        assessment.frozen_assets
                    ),
                });
            }

            // Compute total collateral value from assessment prices and position assets
            let mut collateral_value = Decimal::zero();
            for (i, ca) in assessment.collateral_assets.iter().enumerate() {
                if i < assessment.prices.len() {
                    let asset_value = assessment.prices[i].get_value(ca.asset.amount)?;
                    collateral_value += asset_value;
                }
            }

            // Compute current LTV before solvency check (needed for SavedByDelay detection)
            let current_ltv = {
                let debt_value = prop.credit_price.get_value(prop.total_debt)?;
                membrane::risk::compute_ltv(collateral_value, debt_value)
            };

            // Check for SavedByDelay: position had a timer but became solvent.
            // Must happen before get_repay_quantities (which errors on solvent positions).
            let timer_key = (
                prop.position_owner.as_str(),
                prop.position_id.to_string(),
            );
            let timer_key_ref = (timer_key.0, timer_key.1.as_str());
            let existing_timer = LIQUIDATION_TIMERS.may_load(deps.storage, timer_key_ref)?;

            let is_solvent = membrane::risk::check_solvency(current_ltv, assessment.avg_max_ltv);
            if is_solvent {
                if let Some(timer) = &existing_timer {
                    // Position recovered during the delay — record SavedByDelay event
                    let snapshots = build_asset_snapshots(
                        &assessment.collateral_assets,
                        &assessment.prices,
                    );
                    save_liquidation_event(
                        deps.storage,
                        LiquidationEvent {
                            position_id: prop.position_id,
                            position_owner: prop.position_owner.clone(),
                            caller: prop.caller.clone(),
                            total_debt: prop.total_debt,
                            total_repaid: Decimal::zero(),
                            bad_debt: Uint128::zero(),
                            caller_fee_value: Decimal::zero(),
                            protocol_fee_value: Decimal::zero(),
                            timestamp: env.block.time.seconds(),
                            outcome: LiquidationOutcome::SavedByDelay,
                            current_ltv,
                            timer_started_at: Some(timer.start_time),
                            position_assets: snapshots,
                        },
                    )?;
                    // Clean up the stale timer
                    LIQUIDATION_TIMERS.remove(deps.storage, timer_key_ref);
                    LIQUIDATION_PROPAGATION.remove(deps.storage);
                }
                return Err(ContractError::PositionSolvent {});
            }

            // Calculate repay quantities (uses get_value for correct decimal-adjusted debt value)
            let (repay_amount, repay_value, mut caller_fee_rate, mut protocol_fee_rate) =
                get_repay_quantities(
                    prop.total_debt,
                    &prop.credit_price,
                    collateral_value,
                    assessment.avg_max_ltv,
                    assessment.avg_max_borrow_ltv,
                    config.liq_fee,
                )?;

            // Store current LTV on propagation for the final event
            prop.current_ltv = current_ltv;

            // ================================================================
            // Liquidation delay logic
            //
            // Positions under the immediate-liquidation threshold get a timer:
            //   - First call: start timer, pay caller fees only, return early
            //   - Second call (after timer expires): full liquidation
            // Positions over the threshold: liquidate immediately (no delay)
            // ================================================================
            // Historically each asset's (BTC, ATOM, OSMO) breach rate & recovery rate
            // optimize at a 4% max 1hr window without extending the window so large
            // that the protocol takes insane risk. Neither breaches the 4% window much
            // in the hour either so we can ensure users have the full hour delay 99%
            // of the time. See docs/liquidation_delay_window.md for full analysis.
            let immediate_threshold = assessment.avg_max_ltv
                .checked_mul(Decimal::one() + assessment.avg_max_threshold_to_delay)
                .unwrap_or(Decimal::one());

            if current_ltv > immediate_threshold {
                // Over threshold: clear any existing timer and proceed with immediate liquidation
                if let Some(timer) = &existing_timer {
                    // Had a timer but broke the protective window
                    prop.liquidation_outcome = Some("BrokeWindow".to_string());
                    prop.timer_started_at = Some(timer.start_time);
                    LIQUIDATION_TIMERS.remove(deps.storage, timer_key_ref);
                } else {
                    // No prior timer — immediate liquidation
                    prop.liquidation_outcome = Some("Immediate".to_string());
                }
                // Fall through to normal liquidation flow below
            } else if let Some(timer) = &existing_timer {
                let expiry = timer.start_time + config.liquidation_delay;
                if env.block.time.seconds() >= expiry {
                    // Timer expired: clear timer and proceed with full liquidation
                    prop.liquidation_outcome = Some("DelayExpired".to_string());
                    prop.timer_started_at = Some(timer.start_time);
                    LIQUIDATION_TIMERS.remove(deps.storage, timer_key_ref);
                    // Fall through to normal liquidation flow below
                } else {
                    // Timer still active: reject
                    return Err(ContractError::LiquidationTimerActive { expiry });
                }
            } else {
                // No timer, under threshold: start timer and set up for caller-fee-only extraction
                let timer_start = env.block.time.seconds();
                LIQUIDATION_TIMERS.save(
                    deps.storage,
                    timer_key_ref,
                    &membrane::liquidation_engine::LiquidationTimer {
                        start_time: timer_start,
                        caller: prop.caller.clone(),
                    },
                )?;
                // Record TimerStarted event
                let snapshots = build_asset_snapshots(
                    &assessment.collateral_assets,
                    &assessment.prices,
                );
                save_liquidation_event(
                    deps.storage,
                    LiquidationEvent {
                        position_id: prop.position_id,
                        position_owner: prop.position_owner.clone(),
                        caller: prop.caller.clone(),
                        total_debt: prop.total_debt,
                        total_repaid: Decimal::zero(),
                        bad_debt: Uint128::zero(),
                        caller_fee_value: Decimal::zero(),
                        protocol_fee_value: Decimal::zero(),
                        timestamp: timer_start,
                        outcome: LiquidationOutcome::TimerStarted,
                        current_ltv,
                        timer_started_at: Some(timer_start),
                        position_assets: snapshots,
                    },
                )?;
                // Zero out protocol fee — only caller fees on timer-start
                protocol_fee_rate = Decimal::zero();
                prop.timer_start_only = true;
            }

            // ================================================================
            // Aggregate fee cap (CDP lines 237-255)
            //
            // If total fee value (caller + protocol) would exceed collateral value,
            // cap caller_fee to min(1%, collateral_value/repay_value) and zero protocol.
            // This prevents fees from consuming more than the position is worth.
            // ================================================================
            if !repay_value.is_zero() {
                let caller_fee_value_est = repay_value
                    .checked_mul(caller_fee_rate)
                    .unwrap_or(Decimal::MAX);
                let protocol_fee_value_est = repay_value
                    .checked_mul(protocol_fee_rate)
                    .unwrap_or(Decimal::MAX);
                let total_fee_value_est = caller_fee_value_est
                    .checked_add(protocol_fee_value_est)
                    .unwrap_or(Decimal::MAX);

                if total_fee_value_est > collateral_value {
                    // CDP: cap caller to min(1%, leftover_position_value / repay_value), zero protocol
                    let max_rate = collateral_value
                        .checked_div(repay_value)
                        .unwrap_or(Decimal::zero());
                    caller_fee_rate = std::cmp::min(Decimal::percent(1), max_rate);
                    protocol_fee_rate = Decimal::zero();
                }
            }

            // ================================================================
            // Per-asset fee computation (CDP per_asset_fulfillments lines 745-858)
            //
            // CDP approach: for each collateral asset:
            //   1. collateral_repay_amount = get_amount(repay_value * ratio) — the fee base
            //   2. caller_fee = collateral_repay_amount * caller_fee_rate
            //   3. protocol_fee = collateral_repay_amount * protocol_fee_rate
            //   4. Cap each at position amount, deduct from claims
            // ================================================================
            let mut caller_fee_assets: Vec<Coin> = vec![];
            let mut protocol_fee_assets: Vec<Coin> = vec![];
            let mut total_fee_assets: Vec<Asset> = vec![];
            let mut fee_deducted_collateral = assessment.collateral_assets.clone();
            let mut caller_fee_value_paid = Decimal::zero();
            let mut protocol_fee_value_total = Decimal::zero();

            for (i, ca) in assessment.collateral_assets.iter().enumerate() {
                if i >= assessment.ratios.len() || i >= assessment.prices.len() {
                    continue;
                }

                // CDP line 753: per-asset collateral repay amount (fee base)
                // Overflow protection: deeply insolvent → overflow → MAX → cap zeroes it
                let collateral_repay_amount = repay_value
                    .checked_mul(assessment.ratios[i])
                    .ok()
                    .and_then(|v| assessment.prices[i].get_amount(v).ok())
                    .unwrap_or(Uint128::MAX);

                // CDP line 763: caller_fee = collateral_repay_amount * caller_fee_rate
                let mut caller_fee_amount =
                    Decimal::from_ratio(collateral_repay_amount, 1u128)
                        .checked_mul(caller_fee_rate)
                        .ok()
                        .map(|d| d.to_uint_floor())
                        .unwrap_or(Uint128::MAX);

                // CDP line 770: cap at position amount
                if caller_fee_amount >= ca.asset.amount {
                    caller_fee_amount = Uint128::zero();
                }

                // CDP line 775-779: track caller fee value paid
                if let Ok(fee_val) = assessment.prices[i].get_value(caller_fee_amount) {
                    caller_fee_value_paid += fee_val;
                }

                // CDP line 782: deduct caller fee from position
                fee_deducted_collateral[i].asset.amount = fee_deducted_collateral[i]
                    .asset
                    .amount
                    .saturating_sub(caller_fee_amount);

                // CDP line 800: protocol_fee = collateral_repay_amount * protocol_fee_rate
                let mut protocol_fee_amount =
                    Decimal::from_ratio(collateral_repay_amount, 1u128)
                        .checked_mul(protocol_fee_rate)
                        .ok()
                        .map(|d| d.to_uint_floor())
                        .unwrap_or(Uint128::MAX);

                // CDP line 807: cap at remaining (after caller fee deduction)
                if protocol_fee_amount >= fee_deducted_collateral[i].asset.amount {
                    protocol_fee_amount = Uint128::zero();
                }

                // CDP line 815: deduct protocol fee from position
                fee_deducted_collateral[i].asset.amount = fee_deducted_collateral[i]
                    .asset
                    .amount
                    .saturating_sub(protocol_fee_amount);

                // CDP line 775: track protocol fee value
                if let Ok(fee_val) = assessment.prices[i].get_value(protocol_fee_amount) {
                    protocol_fee_value_total += fee_val;
                }

                // Track per-asset fee coins (CDP lines 835-858)
                let denom = ca.asset.info.to_string();
                if !caller_fee_amount.is_zero() {
                    caller_fee_assets.push(Coin {
                        denom: denom.clone(),
                        amount: caller_fee_amount,
                    });
                }
                if !protocol_fee_amount.is_zero() {
                    protocol_fee_assets.push(Coin {
                        denom: denom.clone(),
                        amount: protocol_fee_amount,
                    });
                }

                // Combined fee for single AuthorizedTransfer
                let total_fee = caller_fee_amount + protocol_fee_amount;
                if !total_fee.is_zero() {
                    total_fee_assets.push(Asset {
                        info: ca.asset.info.clone(),
                        amount: total_fee,
                    });
                }
            }

            // Store assessment data in propagation.
            // Use fee-deducted collateral so subsequent steps see post-fee amounts.
            prop.collateral_assets = fee_deducted_collateral;
            prop.cAsset_ratios = assessment.ratios.clone();
            prop.cAsset_prices = assessment.prices.clone();
            // per_asset_repayment is built in build_next_after_fees alongside the LQ
            // CollateralForDebt query, ensuring entries match actual LQ SubMsgs (Fix #3).
            prop.per_asset_repayment = vec![];
            prop.caller_fee_value_paid = caller_fee_value_paid;
            prop.protocol_fee_value = protocol_fee_value_total;
            prop.caller_fee_assets = caller_fee_assets;
            prop.protocol_fee_assets = protocol_fee_assets;
            LIQUIDATION_PROPAGATION.save(deps.storage, &prop)?;

            // Chain to fee extraction if there are fees to extract.
            // AuthorizedTransfer moves fee collateral: position → liq engine contract,
            // then handle_fee_transfer_reply distributes caller fees via BankMsg::Send.
            if !total_fee_assets.is_empty() {
                let fee_msg = WasmMsg::Execute {
                    contract_addr: config.collateral_contract.to_string(),
                    msg: to_json_binary(&CollateralExecuteMsg::AuthorizedTransfer {
                        position_id: prop.position_id,
                        position_owner: prop.position_owner.to_string(),
                        assets: total_fee_assets,
                        recipient: env.contract.address.to_string(),
                    })?,
                    funds: vec![],
                };

                return Ok(Response::new()
                    .add_submessage(SubMsg::reply_on_success(
                        fee_msg,
                        FEE_TRANSFER_REPLY_ID,
                    ))
                    .add_attribute("method", "collateral_assess_reply_success")
                    .add_attribute("current_ltv", current_ltv.to_string())
                    .add_attribute("repay_amount", repay_amount.to_string())
                    .add_attribute("caller_fee_rate", caller_fee_rate.to_string())
                    .add_attribute("protocol_fee_rate", protocol_fee_rate.to_string()));
            }

            // No fees to extract
            if prop.timer_start_only {
                // Timer-start with no fees: return early (timer already saved above)
                return Ok(Response::new()
                    .add_attribute("method", "liquidation_timer_started")
                    .add_attribute("position_id", prop.position_id.to_string())
                    .add_attribute("position_owner", prop.position_owner.to_string())
                    .add_attribute("current_ltv", current_ltv.to_string())
                    .add_attribute("caller_fee_value", "0"));
            }

            // Full liquidation: chain directly to LQ transfer or sell wall
            let next_msg = build_next_after_fees(&mut deps, &config, &mut prop)?;
            Ok(Response::new()
                .add_submessage(next_msg)
                .add_attribute("method", "collateral_assess_reply_success")
                .add_attribute("current_ltv", current_ltv.to_string())
                .add_attribute("repay_amount", repay_amount.to_string())
                .add_attribute("no_fees", "true"))
        }
        SubMsgResult::Err(err) => Err(ContractError::CustomError {
            val: format!("Collateral assessment failed during liquidation: {}", err),
        }),
    }
}

/// Step 9 reply: Collateral.AuthorizedTransfer (fee extraction) succeeded.
/// Fee collateral is now held by the liq engine contract.
/// Send caller fees via BankMsg::Send, then chain to LQ transfer or sell wall.
///
/// CDP parity: maps to cdp/liquidations.rs per_asset_fulfillments lines 1041-1047
/// (BankMsg::Send caller_coins to fee_recipient)
fn handle_fee_transfer_reply(
    mut deps: DepsMut,
    _env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    match msg.result {
        SubMsgResult::Ok(_response) => {
            let config = CONFIG.load(deps.storage)?;
            let mut prop = LIQUIDATION_PROPAGATION.load(deps.storage)?;

            let mut msgs: Vec<CosmosMsg> = vec![];

            // CDP line 1041-1047: Send caller fees via BankMsg::Send.
            // Now valid because AuthorizedTransfer moved tokens to liq engine.
            if !prop.caller_fee_assets.is_empty() {
                msgs.push(CosmosMsg::Bank(BankMsg::Send {
                    to_address: prop.caller.to_string(),
                    amount: prop.caller_fee_assets.clone(),
                }));
            }

            // Protocol fees: STUB for now — tokens stay in liq engine.
            // TODO: Route prop.protocol_fee_assets to revenue_distributor
            // via AddNonCdtRevenue when revenue_distributor is configured.

            // Timer-start: return early after paying caller fees (no liquidation)
            if prop.timer_start_only {
                return Ok(Response::new()
                    .add_messages(msgs)
                    .add_attribute("method", "liquidation_timer_started")
                    .add_attribute("position_id", prop.position_id.to_string())
                    .add_attribute("position_owner", prop.position_owner.to_string())
                    .add_attribute(
                        "caller_fee_coins",
                        format!("{:?}", prop.caller_fee_assets),
                    )
                    .add_attribute(
                        "caller_fee_value",
                        prop.caller_fee_value_paid.to_string(),
                    ));
            }

            // Chain to LQ transfer or sell wall
            let next_msg = build_next_after_fees(&mut deps, &config, &mut prop)?;

            Ok(Response::new()
                .add_messages(msgs)
                .add_submessage(next_msg)
                .add_attribute("method", "fee_transfer_reply_success")
                .add_attribute(
                    "caller_fee_coins",
                    format!("{:?}", prop.caller_fee_assets),
                ))
        }
        SubMsgResult::Err(err) => {
            // Fee transfer failed. Continue liquidation without fee distribution.
            // This is a graceful degradation — liquidation proceeds regardless.
            let config = CONFIG.load(deps.storage)?;
            let mut prop = LIQUIDATION_PROPAGATION.load(deps.storage)?;

            let next_msg = build_next_after_fees(&mut deps, &config, &mut prop)?;

            Ok(Response::new()
                .add_submessage(next_msg)
                .add_attribute("method", "fee_transfer_reply_failed")
                .add_attribute("error", err))
        }
    }
}

/// Step 3 reply: Collateral.AuthorizedTransfer to liq queue succeeded.
/// Now call the liq queue contract to process (liquidate) the transferred collateral.
fn handle_liq_queue_transfer_reply(
    mut deps: DepsMut,
    _env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    match msg.result {
        SubMsgResult::Ok(_response) => {
            let config = CONFIG.load(deps.storage)?;
            let mut prop = LIQUIDATION_PROPAGATION.load(deps.storage)?;

            let liq_queue = config.liq_queue.clone().ok_or(ContractError::CustomError {
                val: "Liq queue not configured".to_string(),
            })?;

            // Build per-asset Liquidate SubMsgs to the liq queue.
            // Each collateral asset gets its own LQ::Liquidate call.
            // Uses fee-deducted collateral amounts from propagation.
            let mut sub_msgs: Vec<SubMsg> = vec![];

            for (i, ca) in prop.collateral_assets.iter().enumerate() {
                if i >= prop.cAsset_ratios.len() || i >= prop.cAsset_prices.len() {
                    continue;
                }

                // Calculate how much collateral to send for this asset
                let repay_value =
                    prop.credit_price.get_value(prop.total_debt)? * prop.cAsset_ratios[i];
                let collateral_repay_amount = prop.cAsset_prices[i].get_amount(repay_value)?;
                // Cap at fee-deducted position amount
                let capped = std::cmp::min(collateral_repay_amount, ca.asset.amount);

                if capped.is_zero() {
                    continue;
                }

                let liq_msg = LQ_ExecuteMsg::Liquidate {
                    credit_price: prop.credit_price.clone(),
                    collateral_price: prop.cAsset_prices[i].clone(),
                    collateral_amount: Uint256::from(capped.u128()),
                    bid_for: ca.asset.info.clone(),
                };

                let msg = WasmMsg::Execute {
                    contract_addr: liq_queue.to_string(),
                    msg: to_json_binary(&liq_msg)?,
                    funds: vec![],
                };

                // reply_on_success: if LQ fails for one asset, that's handled in the error branch
                sub_msgs.push(SubMsg::reply_on_success(msg, LIQ_QUEUE_REPLY_ID));
            }

            if sub_msgs.is_empty() {
                // No assets to liquidate via LQ - skip to sell wall
                let sell_msg = build_sell_wall_submsg(&mut deps, &config, &mut prop)?;
                return Ok(Response::new()
                    .add_submessage(sell_msg)
                    .add_attribute("method", "liq_queue_transfer_reply_skip_to_sell_wall"));
            }

            Ok(Response::new()
                .add_submessages(sub_msgs)
                .add_attribute("method", "liq_queue_transfer_reply_success"))
        }
        SubMsgResult::Err(err) => {
            // Liq queue transfer failed - skip to sell wall
            let config = CONFIG.load(deps.storage)?;
            let mut prop = LIQUIDATION_PROPAGATION.load(deps.storage)?;

            let sell_msg = build_sell_wall_submsg(&mut deps, &config, &mut prop)?;
            Ok(Response::new()
                .add_submessage(sell_msg)
                .add_attribute("method", "liq_queue_transfer_reply_skip")
                .add_attribute("reason", err))
        }
    }
}

/// Step 4 reply: Liq queue processing completed.
/// Parse repayment from liq queue events, then sell remaining collateral via sell wall.
///
/// CDP parity: maps to cdp/reply.rs::handle_liq_queue_reply()
fn handle_liq_queue_reply(
    mut deps: DepsMut,
    _env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    match msg.result {
        SubMsgResult::Ok(response) => {
            let config = CONFIG.load(deps.storage)?;
            let mut prop = LIQUIDATION_PROPAGATION.load(deps.storage)?;

            // Parse liq queue response events.
            // The liq queue emits: repay_amount, collateral_amount, collateral_token, collateral_info
            let repay_str = find_attribute_value(&response, "repay_amount")
                .unwrap_or_else(|_| "0".to_string());
            let repay_amount = Uint128::from_str(&repay_str).unwrap_or(Uint128::zero());

            let collateral_amount_str = find_attribute_value(&response, "collateral_amount")
                .unwrap_or_else(|_| "0".to_string());
            let collateral_send_amount =
                Uint128::from_str(&collateral_amount_str).unwrap_or(Uint128::zero());

            let _collateral_token = find_attribute_value(&response, "collateral_token")
                .unwrap_or_default();

            // Update propagation with liq queue repayment
            prop.liq_queue_repayment += Decimal::from_ratio(repay_amount, Uint128::one());
            prop.total_repaid += Decimal::from_ratio(repay_amount, Uint128::one());

            // Remove the first per_asset_repayment entry (FIFO matching CDP pattern)
            if !prop.per_asset_repayment.is_empty() {
                prop.per_asset_repayment.remove(0);
            }

            LIQUIDATION_PROPAGATION.save(deps.storage, &prop)?;

            // Safety net: log consumed collateral for observability.
            // With CollateralForDebt query in build_next_after_fees, the transferred
            // amount should match collateral_send_amount exactly. Any discrepancy
            // means the LQ consumed less than transferred (excess sits in LQ).
            // The CollateralForDebt query prevents this in normal operation since
            // query and execute happen within the same transaction.

            // If there are still per_asset_repayments pending, more LQ replies are coming.
            // The last LQ reply (when per_asset_repayment is empty) triggers the sell wall.
            if !prop.per_asset_repayment.is_empty() {
                return Ok(Response::new()
                    .add_attribute("method", "liq_queue_reply_success")
                    .add_attribute("repay_amount", repay_amount.to_string())
                    .add_attribute("collateral_consumed", collateral_send_amount.to_string())
                    .add_attribute("remaining_assets", prop.per_asset_repayment.len().to_string()));
            }

            // All LQ replies processed. Check if debt is fully repaid.
            let total_repaid_uint =
                prop.total_repaid.to_uint_floor();
            if total_repaid_uint >= prop.total_debt {
                // Fully repaid by liq queue - skip sell wall, go to debt update
                let debt_update_msg = build_debt_update_submsg(&config, &prop)?;
                return Ok(Response::new()
                    .add_submessage(debt_update_msg)
                    .add_attribute("method", "liq_queue_reply_fully_repaid")
                    .add_attribute("total_repaid", prop.total_repaid.to_string()));
            }

            // Remaining debt: sell collateral via sell wall
            let sell_msg = build_sell_wall_submsg(&mut deps, &config, &mut prop)?;
            Ok(Response::new()
                .add_submessage(sell_msg)
                .add_attribute("method", "liq_queue_reply_success")
                .add_attribute("repay_amount", repay_amount.to_string())
                .add_attribute(
                    "remaining_debt",
                    prop.total_debt
                        .checked_sub(total_repaid_uint)
                        .unwrap_or(Uint128::zero())
                        .to_string(),
                ))
        }
        SubMsgResult::Err(err) => {
            // Liq queue failed - continue to sell wall for the full remaining amount
            let config = CONFIG.load(deps.storage)?;
            let mut prop = LIQUIDATION_PROPAGATION.load(deps.storage)?;

            let sell_msg = build_sell_wall_submsg(&mut deps, &config, &mut prop)?;
            Ok(Response::new()
                .add_submessage(sell_msg)
                .add_attribute("method", "liq_queue_reply_fallback")
                .add_attribute("error", err))
        }
    }
}

/// Step 5 reply: Sell wall (Collateral.AuthorizedSellCollateral) completed.
/// Parse sell proceeds, then update debt via Debt.LiquidationDebtUpdate.
///
/// CDP parity: maps to cdp/reply.rs::handle_sell_collateral_reply()
fn handle_sell_wall_reply(
    deps: DepsMut,
    _env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    match msg.result {
        SubMsgResult::Ok(_response) => {
            let config = CONFIG.load(deps.storage)?;
            let prop = LIQUIDATION_PROPAGATION.load(deps.storage)?;

            // After sell wall completes, the collateral contract has handled the swap.
            // The sell proceeds (credit tokens) are used to repay debt.
            // We take the total repaid so far (from liq queue) and treat the sell wall
            // as covering the remainder up to total_debt.
            // The actual repayment mechanics are handled by Debt.LiquidationDebtUpdate.

            // Build SubMsg to Debt.LiquidationDebtUpdate
            let debt_update_msg = build_debt_update_submsg(&config, &prop)?;

            Ok(Response::new()
                .add_submessage(debt_update_msg)
                .add_attribute("method", "sell_wall_reply_success")
                .add_attribute("liq_queue_repaid", prop.liq_queue_repayment.to_string())
                .add_attribute("total_debt", prop.total_debt.to_string()))
        }
        SubMsgResult::Err(err) => {
            // Sell wall failed. Still attempt debt update with what was repaid via liq queue.
            let config = CONFIG.load(deps.storage)?;
            let prop = LIQUIDATION_PROPAGATION.load(deps.storage)?;

            let debt_update_msg = build_debt_update_submsg(&config, &prop)?;

            Ok(Response::new()
                .add_submessage(debt_update_msg)
                .add_attribute("method", "sell_wall_reply_partial")
                .add_attribute("sell_wall_error", err)
                .add_attribute("liq_queue_repaid", prop.liq_queue_repayment.to_string()))
        }
    }
}

/// Step 6 reply: Debt.LiquidationDebtUpdate completed.
/// Build collateral reduction list for sell wall amounts only, then send to
/// Collateral.LiquidationClaimUpdate.
///
/// Fees and LQ transfers are already deducted from position claims via
/// AuthorizedTransfer (Steps 9 and 3). Only the sell wall amount needs
/// explicit claim reduction via LiquidationClaimUpdate.
///
/// CDP parity: maps to the debt update -> claim update transition
fn handle_debt_update_reply(
    deps: DepsMut,
    _env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    match msg.result {
        SubMsgResult::Ok(_response) => {
            let config = CONFIG.load(deps.storage)?;
            let prop = LIQUIDATION_PROPAGATION.load(deps.storage)?;

            // All claim reductions are already handled:
            // - Fees: AuthorizedTransfer (Step 9) reduces position claims
            // - LQ: AuthorizedTransfer (Step 3) reduces position claims
            // - Sell wall: collateral contract's handle_sell_collateral_reply reduces claims
            //
            // Passing empty reductions to avoid double-counting the sell wall
            // (the collateral contract already reduced claims in its internal reply handler).
            let collateral_reductions: Vec<Asset> = vec![];

            // SubMsg to Collateral.LiquidationClaimUpdate
            let claim_msg = WasmMsg::Execute {
                contract_addr: config.collateral_contract.to_string(),
                msg: to_json_binary(&CollateralExecuteMsg::LiquidationClaimUpdate {
                    position_id: prop.position_id,
                    position_owner: prop.position_owner.to_string(),
                    collateral_reductions,
                })?,
                funds: vec![],
            };

            Ok(Response::new()
                .add_submessage(SubMsg::reply_on_success(claim_msg, CLAIM_UPDATE_REPLY_ID))
                .add_attribute("method", "debt_update_reply_success")
                .add_attribute("sell_wall_reductions", "handled_by_collateral_contract"))
        }
        SubMsgResult::Err(err) => Err(ContractError::CustomError {
            val: format!("Debt update failed during liquidation: {}", err),
        }),
    }
}

/// Step 7 reply: Collateral.LiquidationClaimUpdate completed.
/// Final step: bad debt check, record stats, cleanup.
///
/// Caller fees were already sent in Step 9 (handle_fee_transfer_reply).
/// Protocol fees are stubbed (stay in liq engine for now).
fn handle_claim_update_reply(
    deps: DepsMut,
    env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    match msg.result {
        SubMsgResult::Ok(_response) => {
            let config = CONFIG.load(deps.storage)?;
            let prop = LIQUIDATION_PROPAGATION.load(deps.storage)?;

            let mut msgs: Vec<CosmosMsg> = vec![];

            // Bad debt check: if total_repaid < total_debt, record bad debt
            let total_repaid_uint = prop.total_repaid.to_uint_floor();
            if total_repaid_uint < prop.total_debt {
                let bad_debt_amount = prop
                    .total_debt
                    .checked_sub(total_repaid_uint)
                    .unwrap_or(Uint128::zero());

                if !bad_debt_amount.is_zero() {
                    let bad_debt_msg = WasmMsg::Execute {
                        contract_addr: config.debt_contract.to_string(),
                        msg: to_json_binary(&DebtExecuteMsg::RecordBadDebt {
                            position_id: prop.position_id,
                            position_owner: prop.position_owner.to_string(),
                            bad_debt_amount,
                        })?,
                        funds: vec![],
                    };
                    msgs.push(CosmosMsg::Wasm(bad_debt_msg));
                }
            }

            // Caller fees already sent in Step 9 (handle_fee_transfer_reply).
            // No BankMsg::Send needed here.

            // Create BadDebtCheck callback as the final message (error-tolerant).
            // This mirrors the CDP pattern of using a self-callback for bad debt handling.
            let bad_debt_callback = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: env.contract.address.to_string(),
                msg: to_json_binary(&ExecuteMsg::Callback(CallbackMsg::BadDebtCheck {
                    position_id: prop.position_id,
                    position_owner: prop.position_owner.clone(),
                }))?,
                funds: vec![],
            });
            // Reply on error so failures don't revert the whole liquidation
            let bad_debt_sub = SubMsg::reply_on_error(bad_debt_callback, BAD_DEBT_REPLY_ID);

            // Record liquidation event in stats
            let bad_debt_amount = if total_repaid_uint < prop.total_debt {
                prop.total_debt
                    .checked_sub(total_repaid_uint)
                    .unwrap_or(Uint128::zero())
            } else {
                Uint128::zero()
            };
            // Resolve outcome from propagation (set in Step 2)
            let outcome = match prop.liquidation_outcome.as_deref() {
                Some("BrokeWindow") => LiquidationOutcome::BrokeWindow,
                Some("DelayExpired") => LiquidationOutcome::DelayExpired,
                Some("Immediate") => LiquidationOutcome::Immediate,
                _ => LiquidationOutcome::Immediate, // fallback
            };
            // Build asset snapshots from the collateral data carried through the chain
            let snapshots = build_asset_snapshots(
                &prop.collateral_assets,
                &prop.cAsset_prices,
            );
            let liq_event = LiquidationEvent {
                position_id: prop.position_id,
                position_owner: prop.position_owner.clone(),
                caller: prop.caller.clone(),
                total_debt: prop.total_debt,
                total_repaid: prop.total_repaid,
                bad_debt: bad_debt_amount,
                caller_fee_value: prop.caller_fee_value_paid,
                protocol_fee_value: prop.protocol_fee_value,
                timestamp: env.block.time.seconds(),
                outcome,
                current_ltv: prop.current_ltv,
                timer_started_at: prop.timer_started_at,
                position_assets: snapshots,
            };
            save_liquidation_event(deps.storage, liq_event)?;

            // Clean up propagation state
            LIQUIDATION_PROPAGATION.remove(deps.storage);

            Ok(Response::new()
                .add_messages(msgs)
                .add_submessage(bad_debt_sub)
                .add_attributes(vec![
                    attr("method", "claim_update_reply_success"),
                    attr("position_id", prop.position_id.to_string()),
                    attr("position_owner", prop.position_owner.to_string()),
                    attr("total_debt", prop.total_debt.to_string()),
                    attr("total_repaid", prop.total_repaid.to_string()),
                    attr(
                        "liq_queue_repayment",
                        prop.liq_queue_repayment.to_string(),
                    ),
                    attr(
                        "caller_fee_value",
                        prop.caller_fee_value_paid.to_string(),
                    ),
                    attr(
                        "protocol_fee_value",
                        prop.protocol_fee_value.to_string(),
                    ),
                ]))
        }
        SubMsgResult::Err(err) => Err(ContractError::CustomError {
            val: format!("Claim update failed during liquidation: {}", err),
        }),
    }
}

// ============================================================================
// Helper functions for building SubMsgs
// ============================================================================

/// Build the next SubMsg after fee extraction: LQ transfer or sell wall.
///
/// Used by both Step 2 (when no fees to extract) and Step 9 (after fee transfer).
///
/// CDP parity: maps to per_asset_fulfillments lines 861-981 (LQ query + transfer).
/// Queries LQ CollateralForDebt per asset to get exact collateral_needed, then
/// transfers only that amount via AuthorizedTransfer (prevents over-transfer).
/// Also builds per_asset_repayment entries only for assets that get LQ calls (Fix #3).
fn build_next_after_fees(
    deps: &mut DepsMut,
    config: &membrane::liquidation_engine::Config,
    prop: &mut membrane::liquidation_engine::LiquidationPropagation,
) -> Result<SubMsg, ContractError> {
    // If liq queue is configured, query LQ and build transfer
    if let Some(ref liq_queue) = config.liq_queue {
        let mut transfer_assets: Vec<Asset> = vec![];
        let mut per_asset_repayment: Vec<Decimal> = vec![];

        // Build credit_info for LQ query (requires credit_denom in config)
        let credit_info_opt = config
            .credit_denom
            .as_ref()
            .map(|denom| AssetInfo::NativeToken {
                denom: denom.clone(),
            });

        for (i, ca) in prop.collateral_assets.iter().enumerate() {
            if i >= prop.cAsset_ratios.len() || i >= prop.cAsset_prices.len() {
                continue;
            }

            // Per-asset debt allocation (in credit units)
            let repay_amount_per_asset =
                Decimal::from_ratio(prop.total_debt, 1u128) * prop.cAsset_ratios[i];
            let repay_amount_uint = repay_amount_per_asset.to_uint_floor();

            if repay_amount_uint.is_zero() {
                continue;
            }

            // CDP parity: Query LQ CollateralForDebt to get exact collateral_needed.
            // This prevents over-transfer by only sending what the LQ can handle.
            let transfer_amount = if let Some(ref credit_info) = credit_info_opt {
                match deps.querier.query_wasm_smart::<CollateralForDebtResponse>(
                    liq_queue.to_string(),
                    &LQ_QueryMsg::CollateralForDebt {
                        bid_for: ca.asset.info.clone(),
                        collateral_price: prop.cAsset_prices[i].clone(),
                        debt_amount: Uint256::from(repay_amount_uint.u128()),
                        credit_info: credit_info.clone(),
                        credit_price: prop.credit_price.clone(),
                    },
                ) {
                    Ok(res) => {
                        let collateral_needed =
                            Uint128::from_str(&res.collateral_needed).unwrap_or(Uint128::zero());
                        let queue_credit_repaid = repay_amount_uint
                            .checked_sub(
                                Uint128::from_str(&res.leftover_debt)
                                    .unwrap_or(Uint128::zero()),
                            )
                            .unwrap_or(Uint128::zero());

                        // CDP line 955-963: Skip if no collateral needed or no credit repaid
                        if collateral_needed.is_zero() || queue_credit_repaid.is_zero() {
                            continue;
                        }

                        // Cap at fee-deducted position holdings
                        std::cmp::min(collateral_needed, ca.asset.amount)
                    }
                    Err(_) => {
                        // LQ query failed for this asset, skip it
                        continue;
                    }
                }
            } else {
                // No credit_denom configured: fall back to debt-proportional transfer
                let total_repay_value = prop.credit_price.get_value(prop.total_debt)?;
                let asset_repay_value = total_repay_value * prop.cAsset_ratios[i];
                let asset_repay_amount = prop.cAsset_prices[i].get_amount(asset_repay_value)?;
                std::cmp::min(asset_repay_amount, ca.asset.amount)
            };

            if !transfer_amount.is_zero() {
                transfer_assets.push(Asset {
                    info: ca.asset.info.clone(),
                    amount: transfer_amount,
                });
                // Fix #3: Only add per_asset_repayment for assets that get LQ calls
                per_asset_repayment.push(repay_amount_per_asset);
            }
        }

        // Store per_asset_repayment in propagation (matches LQ SubMsg count exactly)
        prop.per_asset_repayment = per_asset_repayment;
        LIQUIDATION_PROPAGATION.save(deps.storage, prop)?;

        if !transfer_assets.is_empty() {
            let transfer_msg = WasmMsg::Execute {
                contract_addr: config.collateral_contract.to_string(),
                msg: to_json_binary(&CollateralExecuteMsg::AuthorizedTransfer {
                    position_id: prop.position_id,
                    position_owner: prop.position_owner.to_string(),
                    assets: transfer_assets,
                    recipient: liq_queue.to_string(),
                })?,
                funds: vec![],
            };

            return Ok(SubMsg::reply_on_success(
                transfer_msg,
                LIQ_QUEUE_TRANSFER_REPLY_ID,
            ));
        }
    }

    // No LQ configured or no assets could be handled: sell wall
    build_sell_wall_submsg(deps, config, prop)
}

/// Build a SubMsg to Collateral.AuthorizedSellCollateral for the remaining
/// unliquidated collateral after liq queue processing.
///
/// CDP parity: maps to sell wall in cdp/liquidations.rs per_asset_fulfillments lines 984-1031
/// Computes per-asset sell amounts from leftover_repayment * ratio (NOT all remaining).
/// Stores sell_wall_amounts in propagation for Step 6 claim reduction.
fn build_sell_wall_submsg(
    deps: &mut DepsMut,
    config: &membrane::liquidation_engine::Config,
    prop: &mut membrane::liquidation_engine::LiquidationPropagation,
) -> Result<SubMsg, ContractError> {
    // CDP line 986-1017: Calculate per-asset sell amounts from leftover repayment
    // leftover_repayment = total_debt - liq queue repaid - venue recall amount
    // Venue recall: CDT being returned from deployment venues (CDP parity: get_deployable_venues_user_repay_amount)
    let lq_repaid_uint = prop.liq_queue_repayment.to_uint_floor();
    let leftover_credit = prop.total_debt
        .saturating_sub(lq_repaid_uint)
        .saturating_sub(prop.venue_recall_amount);

    let mut sell_assets: Vec<Asset> = vec![];

    if !leftover_credit.is_zero() {
        // Convert leftover credit to value for per-asset proportioning
        let leftover_value = prop.credit_price.get_value(leftover_credit)?;

        for (i, ca) in prop.collateral_assets.iter().enumerate() {
            if i >= prop.cAsset_ratios.len() || i >= prop.cAsset_prices.len() {
                continue;
            }

            // CDP line 993: sell_value = leftover_repayment * ratio
            let sell_value = leftover_value
                .checked_mul(prop.cAsset_ratios[i])
                .unwrap_or(Decimal::zero());

            // CDP line 997: convert to collateral amount
            let mut sell_amount = prop.cAsset_prices[i]
                .get_amount(sell_value)
                .unwrap_or(Uint128::zero());

            // Cap at what position has (fee-deducted)
            sell_amount = std::cmp::min(sell_amount, ca.asset.amount);

            if !sell_amount.is_zero() {
                sell_assets.push(Asset {
                    info: ca.asset.info.clone(),
                    amount: sell_amount,
                });
            }
        }
    }

    // Store sell_wall_amounts in propagation for Step 6 exact claim reduction
    prop.sell_wall_amounts = sell_assets.clone();
    LIQUIDATION_PROPAGATION.save(deps.storage, prop)?;

    let sell_msg = WasmMsg::Execute {
        contract_addr: config.collateral_contract.to_string(),
        msg: to_json_binary(&CollateralExecuteMsg::AuthorizedSellCollateral {
            position_id: prop.position_id,
            position_owner: prop.position_owner.to_string(),
            max_spread: Decimal::percent(90), // Prioritize offloading collateral (matches CDP)
            send_proceeds_to: config.debt_contract.to_string(),
            sell_amounts: Some(sell_assets),
        })?,
        funds: vec![],
    };

    Ok(SubMsg::reply_on_success(sell_msg, SELL_WALL_REPLY_ID))
}

/// Build a SubMsg to Debt.LiquidationDebtUpdate.
/// Sends the total amount to repay (capped at total_debt).
fn build_debt_update_submsg(
    config: &membrane::liquidation_engine::Config,
    prop: &membrane::liquidation_engine::LiquidationPropagation,
) -> Result<SubMsg, ContractError> {
    // The repay amount is the total that was actually repaid across all mechanisms
    let total_repaid_uint = prop.total_repaid.to_uint_floor();
    let repay_amount = std::cmp::min(total_repaid_uint, prop.total_debt);

    let debt_update_msg = WasmMsg::Execute {
        contract_addr: config.debt_contract.to_string(),
        msg: to_json_binary(&DebtExecuteMsg::LiquidationDebtUpdate {
            position_id: prop.position_id,
            position_owner: prop.position_owner.to_string(),
            repay_amount,
        })?,
        funds: vec![],
    };

    Ok(SubMsg::reply_on_success(debt_update_msg, DEBT_UPDATE_REPLY_ID))
}
