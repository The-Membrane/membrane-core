#![allow(non_snake_case)]
use cosmwasm_std::{to_json_binary, Binary, Decimal, Deps, StdResult, Uint128};

use membrane::collateral::{PositionValueResponse, QueryMsg as CollateralQueryMsg};
use membrane::debt::{DebtPositionsResponse, QueryMsg as DebtQueryMsg};
use membrane::liquidation_engine::{
    LastLiquidationTimestampForAssetResponse, LiquidationEventResponse,
    LiquidationOutcomeResponse, LiquidationStatsResponse, LiquidationTimerResponse,
    PositionAssetSnapshotResponse, SimulateLiquidationResponse,
};
use membrane::types::UserInfo;

use crate::liquidations::get_repay_quantities;
use crate::state::{CONFIG, LIQUIDATION_STATS, LIQUIDATION_TIMERS};

/// Simulate a liquidation to estimate costs.
///
/// Queries Debt and Collateral contracts for position state,
/// runs insolvency check, and returns estimated repay amounts and fees.
///
/// CDP parity: maps to cdp/query.rs::query_simulate_liquidation() +
/// cdp/liquidations.rs::simulate_liquidation_sales()
pub fn query_simulate_liquidation(
    deps: Deps,
    position_id: Uint128,
    position_owner: String,
) -> StdResult<Binary> {
    let config = CONFIG.load(deps.storage)?;

    // Query Collateral contract for position value and LTV thresholds
    let position_value: PositionValueResponse = deps.querier.query_wasm_smart(
        config.collateral_contract.to_string(),
        &CollateralQueryMsg::GetPositionValue {
            position_owner: position_owner.clone(),
            position_id,
        },
    )?;

    // Query Debt contract for position debt
    let debt_positions: DebtPositionsResponse = deps.querier.query_wasm_smart(
        config.debt_contract.to_string(),
        &DebtQueryMsg::GetDebtPositions {
            start_after: None,
            limit: None,
            user_info: Some(UserInfo {
                position_id,
                position_owner: position_owner.clone(),
            }),
            user: None,
        },
    )?;

    let total_debt: Uint128 = debt_positions
        .positions
        .iter()
        .map(|p| p.credit_amount)
        .sum();

    // Compute LTV
    let collateral_value = position_value.total_value;
    // Use $1 credit price (decimals=0) as approximation for query context.
    // decimals=0 so get_value(total_debt) = total_debt * price / 10^0 = total_debt * 1 = total_debt
    let credit_price = membrane::oracle::PriceResponse {
        prices: vec![],
        price: Decimal::one(),
        decimals: 0,
    };
    let debt_value = credit_price.get_value(total_debt).unwrap_or(Decimal::zero());
    let current_ltv = membrane::risk::compute_ltv(collateral_value, debt_value);
    let is_insolvent = !membrane::risk::check_solvency(current_ltv, position_value.avg_max_ltv);

    let (repay_amount, repay_value, caller_fee_rate, protocol_fee_rate) = if is_insolvent {
        match get_repay_quantities(
            total_debt,
            &credit_price,
            collateral_value,
            position_value.avg_max_ltv,
            position_value.avg_max_borrow_ltv,
            config.liq_fee,
        ) {
            Ok(result) => result,
            Err(_) => (Uint128::zero(), Decimal::zero(), Decimal::zero(), Decimal::zero()),
        }
    } else {
        (Uint128::zero(), Decimal::zero(), Decimal::zero(), Decimal::zero())
    };

    // Convert fee rates to fee values for the response
    let caller_fee = repay_value * caller_fee_rate;
    let protocol_fee = repay_value * protocol_fee_rate;

    // Check timer state for this position
    let pos_id_str = position_id.to_string();
    let existing_timer =
        LIQUIDATION_TIMERS.may_load(deps.storage, (position_owner.as_str(), pos_id_str.as_str()))?;

    let (has_active_timer, timer_expiry) = if let Some(timer) = &existing_timer {
        let expiry = timer.start_time + config.liquidation_delay;
        (true, Some(expiry))
    } else {
        (false, None)
    };

    // Determine if calling liquidate would start a timer (under threshold, no existing timer)
    let would_start_timer = if is_insolvent {
        // Historically each asset's (BTC, ATOM, OSMO) breach rate & recovery rate
        // optimize at a 4% max 1hr window without extending the window so large
        // that the protocol takes insane risk. Neither breaches the 4% window much
        // in the hour either so we can ensure users have the full hour delay 99%
        // of the time. See docs/liquidation_delay_window.md for full analysis.
        let immediate_threshold = position_value
            .avg_max_ltv
            .checked_mul(Decimal::one() + position_value.avg_max_threshold_to_delay)
            .unwrap_or(Decimal::MAX);
        current_ltv <= immediate_threshold && existing_timer.is_none()
    } else {
        false
    };

    let response = SimulateLiquidationResponse {
        is_insolvent,
        current_ltv,
        repay_amount,
        caller_fee,
        protocol_fee,
        has_active_timer,
        timer_expiry,
        would_start_timer,
    };

    to_json_binary(&response)
}

/// Query the liquidation timer for a specific position.
pub fn query_liquidation_timer(
    deps: Deps,
    position_id: Uint128,
    position_owner: String,
) -> StdResult<Binary> {
    let config = CONFIG.load(deps.storage)?;
    let pos_id_str = position_id.to_string();
    let timer =
        LIQUIDATION_TIMERS.may_load(deps.storage, (position_owner.as_str(), pos_id_str.as_str()))?;

    let response = match timer {
        Some(t) => LiquidationTimerResponse {
            has_timer: true,
            start_time: Some(t.start_time),
            expiry_time: Some(t.start_time + config.liquidation_delay),
            caller: Some(t.caller.to_string()),
        },
        None => LiquidationTimerResponse {
            has_timer: false,
            start_time: None,
            expiry_time: None,
            caller: None,
        },
    };

    to_json_binary(&response)
}

/// Query liquidation history.
pub fn query_liquidation_stats(
    deps: Deps,
    start_after: Option<u32>,
    limit: Option<u32>,
) -> StdResult<Binary> {
    let stats = LIQUIDATION_STATS
        .may_load(deps.storage)?
        .unwrap_or_default();

    let total_count = stats.len() as u32;
    let start = start_after.unwrap_or(0) as usize;
    let limit = limit.unwrap_or(30).min(100) as usize;

    let events: Vec<LiquidationEventResponse> = stats
        .into_iter()
        .skip(start)
        .take(limit)
        .map(|e| {
            use crate::state::LiquidationOutcome;
            let outcome = match e.outcome {
                LiquidationOutcome::TimerStarted => LiquidationOutcomeResponse::TimerStarted,
                LiquidationOutcome::SavedByDelay => LiquidationOutcomeResponse::SavedByDelay,
                LiquidationOutcome::BrokeWindow => LiquidationOutcomeResponse::BrokeWindow,
                LiquidationOutcome::DelayExpired => LiquidationOutcomeResponse::DelayExpired,
                LiquidationOutcome::Immediate => LiquidationOutcomeResponse::Immediate,
            };
            LiquidationEventResponse {
                position_id: e.position_id,
                position_owner: e.position_owner.to_string(),
                caller: e.caller.to_string(),
                total_debt: e.total_debt,
                total_repaid: e.total_repaid,
                bad_debt: e.bad_debt,
                caller_fee_value: e.caller_fee_value,
                protocol_fee_value: e.protocol_fee_value,
                timestamp: e.timestamp,
                outcome,
                current_ltv: e.current_ltv,
                timer_started_at: e.timer_started_at,
                position_assets: e.position_assets.into_iter().map(|a| {
                    PositionAssetSnapshotResponse {
                        denom: a.denom,
                        amount: a.amount,
                        value: a.value,
                        max_ltv: a.max_ltv,
                    }
                }).collect(),
            }
        })
        .collect();

    to_json_binary(&LiquidationStatsResponse {
        events,
        total_count,
    })
}

/// Query the most recent liquidation timestamp for a specific collateral asset.
/// Iterates events in reverse to find the latest event involving this asset,
/// excluding SavedByDelay (position recovered, no actual liquidation risk).
pub fn query_last_liquidation_timestamp_for_asset(
    deps: Deps,
    asset: String,
) -> StdResult<Binary> {
    use crate::state::LiquidationOutcome;

    let stats = LIQUIDATION_STATS
        .may_load(deps.storage)?
        .unwrap_or_default();

    let timestamp = stats
        .iter()
        .rev()
        .filter(|e| !matches!(e.outcome, LiquidationOutcome::SavedByDelay))
        .find(|e| e.position_assets.iter().any(|a| a.denom == asset))
        .map(|e| e.timestamp.max(e.timer_started_at.unwrap_or(0)));

    to_json_binary(&LastLiquidationTimestampForAssetResponse { timestamp })
}
