#![allow(non_snake_case)]
use cosmwasm_std::{
    to_json_binary, Addr, CosmosMsg, Decimal, DepsMut, Env, MessageInfo, Response, SubMsg,
    Uint128, WasmMsg,
};

use membrane::collateral::{PositionValueResponse, QueryMsg as CollateralQueryMsg};
use membrane::debt::{DebtPositionsResponse, ExecuteMsg as DebtExecuteMsg, QueryMsg as DebtQueryMsg};
use membrane::liquidation_engine::LiquidationPropagation;
use membrane::oracle::PriceResponse;
use membrane::types::UserInfo;

use crate::error::ContractError;
use crate::state::{CONFIG, DEBT_ACCRUE_REPLY_ID, LIQUIDATION_PROPAGATION};

/// Main liquidation entry point.
/// Orchestrates: accrue debt → assess collateral → liq queue → sell wall → debt update → claim update → bad debt check.
///
/// CDP parity: maps to cdp/liquidations.rs::liquidate()
pub fn liquidate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    position_id: Uint128,
    position_owner: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let valid_position_owner = deps.api.addr_validate(&position_owner)?;

    // Initialize propagation state before the SubMsg chain starts.
    // Reply handlers will load, update, and save this state as the chain progresses.
    let propagation = LiquidationPropagation {
        per_asset_repayment: vec![],
        liq_queue_repayment: Decimal::zero(),
        protocol_fee_value: Decimal::zero(),
        caller_fee_assets: vec![],
        protocol_fee_assets: vec![],
        sell_wall_amounts: vec![],
        cAsset_ratios: vec![],
        cAsset_prices: vec![],
        collateral_assets: vec![],
        total_debt: Uint128::zero(),
        credit_price: membrane::oracle::PriceResponse {
            prices: vec![],
            price: Decimal::zero(),
            decimals: 6,
        },
        caller_fee_value_paid: Decimal::zero(),
        total_repaid: Decimal::zero(),
        venue_recall_amount: Uint128::zero(),
        position_owner: valid_position_owner.clone(),
        position_id,
        caller: info.sender.clone(),
        timer_start_only: false,
        liquidation_outcome: None,
        timer_started_at: None,
        current_ltv: Decimal::zero(),
    };
    LIQUIDATION_PROPAGATION.save(deps.storage, &propagation)?;

    // Step 1: Send SubMsg to Debt.AccrueAndReturnDebt
    // This accrues interest and returns total debt, credit price, segments.
    let debt_msg = WasmMsg::Execute {
        contract_addr: config.debt_contract.to_string(),
        msg: to_json_binary(&DebtExecuteMsg::AccrueAndReturnDebt {
            position_owner: position_owner.clone(),
            position_id,
        })?,
        funds: vec![],
    };

    Ok(Response::new()
        .add_submessage(SubMsg::reply_on_success(debt_msg, DEBT_ACCRUE_REPLY_ID))
        .add_attribute("method", "liquidate")
        .add_attribute("position_id", position_id.to_string())
        .add_attribute("position_owner", position_owner)
        .add_attribute("caller", info.sender.to_string()))
}

/// Calculate repay quantities for a liquidation.
///
/// Returns: (repay_amount, repay_value, caller_fee_rate, protocol_fee_rate)
/// - repay_amount: credit units to repay
/// - repay_value: dollar value of repayment (used as per-asset fee base)
/// - caller_fee_rate: current_LTV - avg_max_LTV (dynamic, scales with insolvency depth)
/// - protocol_fee_rate: config.liq_fee (static)
///
/// CDP parity: maps to cdp/liquidations.rs::get_repay_quantities() + fee rate setup
/// - Formula: ((current_LTV - borrow_LTV) / current_LTV) * loan_value
/// - Over-100% LTV: repay_value = collateral_value (use all collateral)
/// - Caller fee rate: current_LTV - avg_max_LTV (CDP line 161)
/// - Protocol fee rate: config.liq_fee (CDP line 237)
pub fn get_repay_quantities(
    total_debt: Uint128,
    credit_price: &PriceResponse,
    collateral_value: Decimal,
    avg_max_ltv: Decimal,
    avg_borrow_ltv: Decimal,
    liq_fee: Decimal,
) -> Result<(Uint128, Decimal, Decimal, Decimal), ContractError> {
    // Use get_value for correct decimal-adjusted debt value (CDP parity: basket.credit_price.get_value)
    let debt_value = credit_price.get_value(total_debt)?;

    // Check insolvency against max_LTV (liquidation threshold)
    let current_ltv = membrane::risk::compute_ltv(collateral_value, debt_value);
    if membrane::risk::check_solvency(current_ltv, avg_max_ltv) {
        return Err(ContractError::PositionSolvent {});
    }

    // CDP formula: repay_value = ((current_LTV - borrow_LTV) / current_LTV) * loan_value
    // This computes the portion of debt needed to bring LTV back to borrow_LTV.
    // Over-100% LTV: use all collateral value (CDP line 472-473).
    let repay_value = if current_ltv >= Decimal::one() {
        collateral_value
    } else {
        let excess = current_ltv
            .checked_sub(avg_borrow_ltv)
            .unwrap_or(Decimal::zero());
        let fraction = excess
            .checked_div(current_ltv)
            .unwrap_or(Decimal::zero());
        debt_value * fraction
    };

    // Repay amount in credit units (capped at total_debt)
    // Use get_amount for correct decimal-adjusted conversion back to credit units
    let repay_amount = credit_price
        .get_amount(repay_value)
        .map(|raw| std::cmp::min(raw, total_debt))
        .unwrap_or(total_debt);

    // Fee RATES (not values) — applied per-asset in Step 2
    // CDP line 161: caller_fee = current_LTV - avg_max_LTV (dynamic, LTV-excess)
    let caller_fee_rate = current_ltv
        .checked_sub(avg_max_ltv)
        .unwrap_or(Decimal::zero());
    // CDP line 237: protocol_fee = config.liq_fee (static)
    let protocol_fee_rate = liq_fee;

    Ok((repay_amount, repay_value, caller_fee_rate, protocol_fee_rate))
}

/// Bad debt check after liquidation completes.
///
/// Queries remaining collateral value and debt for the position.
/// If collateral value < $1 and outstanding debt > 0, this is bad debt.
///
/// CDP parity: maps to cdp/contract.rs::check_and_fulfill_bad_debt()
pub fn bad_debt_check(
    deps: DepsMut,
    _env: Env,
    position_id: Uint128,
    position_owner: Addr,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Step 1: Query Collateral contract for remaining position value
    let position_value: PositionValueResponse = deps.querier.query_wasm_smart(
        config.collateral_contract.to_string(),
        &CollateralQueryMsg::GetPositionValue {
            position_owner: position_owner.to_string(),
            position_id,
        },
    )?;

    // Step 2: Query Debt contract for remaining debt
    let debt_positions: DebtPositionsResponse = deps.querier.query_wasm_smart(
        config.debt_contract.to_string(),
        &DebtQueryMsg::GetDebtPositions {
            start_after: None,
            limit: None,
            user_info: Some(UserInfo {
                position_id,
                position_owner: position_owner.to_string(),
            }),
            user: None,
        },
    )?;

    let remaining_debt: Uint128 = debt_positions
        .positions
        .iter()
        .map(|p| p.credit_amount)
        .sum();

    // Step 3: If collateral value < $1 and debt > 0, this is bad debt
    let one_dollar = Decimal::one();
    if position_value.total_value < one_dollar && !remaining_debt.is_zero() {
        let mut msgs: Vec<CosmosMsg> = vec![];

        // Route to debt auction if configured
        if let Some(ref debt_auction) = config.debt_auction {
            let auction_msg = WasmMsg::Execute {
                contract_addr: debt_auction.to_string(),
                msg: to_json_binary(
                    &membrane::auction::ExecuteMsg::StartAuction {
                        repayment_position_info: Some(UserInfo {
                            position_id,
                            position_owner: position_owner.to_string(),
                        }),
                        send_to: None,
                        auction_asset: membrane::types::Asset {
                            amount: remaining_debt,
                            info: membrane::types::AssetInfo::NativeToken {
                                denom: "credit".to_string(),
                            },
                        },
                        per_asset_distribution: None,
                    },
                )?,
                funds: vec![],
            };
            msgs.push(CosmosMsg::Wasm(auction_msg));
        }

        return Ok(Response::new()
            .add_messages(msgs)
            .add_attribute("method", "bad_debt_check")
            .add_attribute("position_id", position_id.to_string())
            .add_attribute("bad_debt_detected", "true")
            .add_attribute("remaining_debt", remaining_debt.to_string())
            .add_attribute(
                "collateral_value",
                position_value.total_value.to_string(),
            ));
    }

    Ok(Response::new()
        .add_attribute("method", "bad_debt_check")
        .add_attribute("position_id", position_id.to_string())
        .add_attribute("bad_debt_detected", "false"))
}
