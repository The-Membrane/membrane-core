#![allow(non_snake_case)]
use cosmwasm_std::{to_json_binary, Binary, Decimal, Deps, StdResult, Uint128};

use membrane::collateral::{CollateralPositionsResponse, QueryMsg as CollateralQueryMsg};
use membrane::debt::{
    DebtPositionResponse, DebtPositionsResponse, FullPosition,
};
use membrane::types::UserInfo;

use crate::state::{
    ACTIVE_DEPLOYMENT_VENUES, AFFILIATES, BASKET, CONFIG, HISTORICAL_INTEREST_RATES,
    LIQUIDATION_STATS, POSITIONS, RATES, USER_INTENTS,
};

/// Query debt positions with pagination.
///
/// CDP parity: maps to cdp/query.rs::query_basket_positions() (debt portion)
pub fn query_debt_positions(
    deps: Deps,
    _start_after: Option<String>,
    _limit: Option<u32>,
    user_info: Option<UserInfo>,
    user: Option<String>,
) -> StdResult<Binary> {
    // Single position lookup
    if let Some(ui) = user_info {
        let owner = deps.api.addr_validate(&ui.position_owner)?;
        let positions = POSITIONS
            .may_load(deps.storage, owner.clone())?
            .unwrap_or_default();

        let target = positions.into_iter().find(|p| p.position_id == ui.position_id);

        let response_positions = match target {
            Some(pos) => {
                let total_debt: Uint128 = pos.rate_segments.iter().map(|s| s.amount).sum::<Uint128>()
                    + pos.peg_rate_segments.iter().map(|s| s.amount).sum::<Uint128>();

                vec![DebtPositionResponse {
                    position_id: pos.position_id,
                    credit_amount: total_debt,
                    rate_segments: pos.rate_segments,
                    peg_rate_segments: pos.peg_rate_segments,
                    deployed_to: pos.deployed_to,
                    pending_interest: pos.pending_interest,
                    total_interest_accrued: pos.total_interest_accrued,
                }]
            }
            None => vec![],
        };

        return to_json_binary(&DebtPositionsResponse {
            user: ui.position_owner,
            positions: response_positions,
        });
    }

    // User positions lookup
    if let Some(user_str) = user {
        let owner = deps.api.addr_validate(&user_str)?;
        let positions = POSITIONS
            .may_load(deps.storage, owner)?
            .unwrap_or_default();

        let response_positions: Vec<DebtPositionResponse> = positions
            .into_iter()
            .map(|pos| {
                let total_debt: Uint128 = pos.rate_segments.iter().map(|s| s.amount).sum::<Uint128>()
                    + pos.peg_rate_segments.iter().map(|s| s.amount).sum::<Uint128>();

                DebtPositionResponse {
                    position_id: pos.position_id,
                    credit_amount: total_debt,
                    rate_segments: pos.rate_segments,
                    peg_rate_segments: pos.peg_rate_segments,
                    deployed_to: pos.deployed_to,
                    pending_interest: pos.pending_interest,
                    total_interest_accrued: pos.total_interest_accrued,
                }
            })
            .collect();

        return to_json_binary(&DebtPositionsResponse {
            user: user_str,
            positions: response_positions,
        });
    }

    // TODO: Paginated all-positions query
    to_json_binary(&Vec::<DebtPositionsResponse>::new())
}

/// Query full positions (combines debt + collateral data).
/// Queries Collateral contract for collateral data then merges with debt data.
///
/// CDP parity: maps to cdp/query.rs::query_basket_positions() (full position)
pub fn query_full_positions(
    deps: Deps,
    start_after: Option<String>,
    limit: Option<u32>,
    user_info: Option<UserInfo>,
    user: Option<String>,
) -> StdResult<Binary> {
    let config = CONFIG.load(deps.storage)?;
    let collateral_contract = match config.collateral_contract {
        Some(addr) => addr,
        None => return to_json_binary(&Vec::<FullPosition>::new()),
    };

    // Single position lookup
    if let Some(ref ui) = user_info {
        let owner = deps.api.addr_validate(&ui.position_owner)?;
        let positions = POSITIONS
            .may_load(deps.storage, owner)?
            .unwrap_or_default();

        let target = positions.into_iter().find(|p| p.position_id == ui.position_id);

        let full_positions = match target {
            Some(pos) => {
                // Query collateral contract for this position's collateral
                let collateral_resp: CollateralPositionsResponse = deps.querier.query_wasm_smart(
                    collateral_contract.to_string(),
                    &CollateralQueryMsg::GetCollateralPositions {
                        start_after: None,
                        limit: None,
                        user_info: user_info.clone(),
                        user: None,
                    },
                )?;

                let (collateral_assets, ratios) = collateral_resp.positions.first()
                    .map(|cp| (cp.collateral_assets.clone(), cp.cAsset_ratios.clone()))
                    .unwrap_or_default();

                let total_debt: Uint128 = pos.rate_segments.iter().map(|s| s.amount).sum::<Uint128>()
                    + pos.peg_rate_segments.iter().map(|s| s.amount).sum::<Uint128>();

                vec![FullPosition {
                    position_id: pos.position_id,
                    collateral_assets,
                    cAsset_ratios: ratios,
                    credit_amount: total_debt,
                    rate_segments: pos.rate_segments,
                    peg_rate_segments: pos.peg_rate_segments,
                    avg_borrow_LTV: Decimal::zero(),
                    avg_max_LTV: Decimal::zero(),
                    deployed_to: pos.deployed_to,
                    pending_interest: pos.pending_interest,
                    total_interest_accrued: pos.total_interest_accrued,
                }]
            }
            None => vec![],
        };

        return to_json_binary(&full_positions);
    }

    // Single user lookup
    if let Some(ref user_str) = user {
        let owner = deps.api.addr_validate(user_str)?;
        let positions = POSITIONS
            .may_load(deps.storage, owner)?
            .unwrap_or_default();

        // Query collateral contract for all of this user's collateral
        let collateral_resp: CollateralPositionsResponse = deps.querier.query_wasm_smart(
            collateral_contract.to_string(),
            &CollateralQueryMsg::GetCollateralPositions {
                start_after: None,
                limit: None,
                user_info: None,
                user: user.clone(),
            },
        )?;

        let full_positions: Vec<FullPosition> = positions.into_iter().map(|pos| {
            // Find matching collateral position by ID
            let (collateral_assets, ratios) = collateral_resp.positions.iter()
                .find(|cp| cp.position_id == pos.position_id)
                .map(|cp| (cp.collateral_assets.clone(), cp.cAsset_ratios.clone()))
                .unwrap_or_default();

            let total_debt: Uint128 = pos.rate_segments.iter().map(|s| s.amount).sum::<Uint128>()
                + pos.peg_rate_segments.iter().map(|s| s.amount).sum::<Uint128>();

            FullPosition {
                position_id: pos.position_id,
                collateral_assets,
                cAsset_ratios: ratios,
                credit_amount: total_debt,
                rate_segments: pos.rate_segments,
                peg_rate_segments: pos.peg_rate_segments,
                avg_borrow_LTV: Decimal::zero(),
                avg_max_LTV: Decimal::zero(),
                deployed_to: pos.deployed_to,
                pending_interest: pos.pending_interest,
                total_interest_accrued: pos.total_interest_accrued,
            }
        }).collect();

        return to_json_binary(&full_positions);
    }

    // Paginated all-positions query
    // For pagination, we only return debt data without cross-contract queries
    // (cross-contract queries for every user would be too expensive in a paginated context)
    let _limit = limit.unwrap_or(10) as usize;
    let _start = start_after;
    // TODO: Paginated all-positions query with debt-only data
    to_json_binary(&Vec::<FullPosition>::new())
}

/// Query credit redemption rate.
///
/// CDP parity: maps to cdp/query.rs::query_basket_credit_interest()
pub fn query_credit_rate(deps: Deps) -> StdResult<Binary> {
    let basket = BASKET.load(deps.storage)?;
    to_json_binary(&basket.credit_price)
}

/// Query collateral interest rates.
///
/// CDP parity: maps to cdp/query.rs::query_collateral_rates()
/// Returns Vec<Decimal> of per-asset interest rates in basket collateral_types order.
pub fn query_collateral_interest(deps: Deps) -> StdResult<Binary> {
    let rates = RATES.load(deps.storage)?;
    let collateral_rates: Vec<Decimal> = rates
        .lastest_collateral_rates
        .into_iter()
        .map(|rate| rate.rate)
        .collect();
    to_json_binary(&collateral_rates)
}

/// Query rates store.
pub fn query_rates(deps: Deps) -> StdResult<Binary> {
    let rates = RATES.may_load(deps.storage)?;
    to_json_binary(&rates)
}

/// Simulate LTV mint for a position.
///
/// CDP parity: maps to cdp/query.rs insolvency-related simulation
pub fn query_simulate_mint(
    _deps: Deps,
    _position_info: UserInfo,
    _ltv: Decimal,
) -> StdResult<Binary> {
    // TODO: Calculate max borrowable amount at given LTV
    to_json_binary(&Uint128::zero())
}

/// Query user deployment intents.
///
/// CDP parity: maps to cdp/query.rs::query_user_intent_state()
pub fn query_user_intents(
    deps: Deps,
    _start_after: Option<String>,
    _limit: Option<u32>,
    users: Vec<String>,
) -> StdResult<Binary> {
    let mut results = vec![];
    for user_str in users {
        if let Ok(user) = deps.api.addr_validate(&user_str) {
            let intents = USER_INTENTS.may_load(deps.storage, user)?;
            results.push((user_str, intents));
        }
    }
    to_json_binary(&results)
}

/// Query affiliates for a position.
pub fn query_affiliates(deps: Deps, position_id: Uint128) -> StdResult<Binary> {
    let affiliate = AFFILIATES.may_load(deps.storage, position_id.u128())?;
    to_json_binary(&affiliate)
}

/// Query active deployment venues.
///
/// CDP parity: maps to cdp/query.rs::query_active_deployment_venues()
pub fn query_active_deployment_venues(
    deps: Deps,
    venue: Option<String>,
    _start_after: Option<String>,
    _limit: Option<u32>,
) -> StdResult<Binary> {
    let venues = ACTIVE_DEPLOYMENT_VENUES
        .may_load(deps.storage)?
        .unwrap_or_default();

    // If a specific venue is requested, filter for it
    if let Some(target) = venue {
        let found = venues.iter().any(|v| v == &target);
        return to_json_binary(&found);
    }

    to_json_binary(&venues)
}

/// Query liquidation stats.
///
/// CDP parity: maps to cdp/query.rs::query_liquidation_stats()
pub fn query_liquidation_stats(
    deps: Deps,
    _start_after: Option<u64>,
    _limit: Option<u32>,
) -> StdResult<Binary> {
    let stats = LIQUIDATION_STATS
        .may_load(deps.storage)?
        .unwrap_or_default();
    to_json_binary(&stats)
}

/// Query historical interest rates.
///
/// CDP parity: maps to cdp/query.rs::query_historical_interest_rates()
pub fn query_historical_interest_rates(deps: Deps, asset: String) -> StdResult<Binary> {
    let history = HISTORICAL_INTEREST_RATES
        .may_load(deps.storage, asset)?
        .unwrap_or_default();
    to_json_binary(&history)
}
