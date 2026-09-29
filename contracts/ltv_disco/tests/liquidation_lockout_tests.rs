mod test_helpers;

use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info, MockApi, MockQuerier};
use cosmwasm_std::{to_json_binary, MemoryStorage, OwnedDeps, Uint128, SystemResult, ContractResult};
use membrane::ltv_disco::ExecuteMsg;
use membrane::liquidation_engine::LastLiquidationTimestampForAssetResponse;
use ltv_disco::contract::execute;
use ltv_disco::state::CONFIG;
use test_helpers::*;

// =================== Helper: Custom Querier for Liq Engine ===================

/// Create deps with a wasm querier that returns a `LastLiquidationTimestampForAssetResponse`.
fn setup_deps_with_liq_engine_querier(
    last_liq_timestamp: Option<u64>,
) -> OwnedDeps<MemoryStorage, MockApi, MockQuerier> {
    let mut deps = mock_dependencies();
    let response = LastLiquidationTimestampForAssetResponse {
        timestamp: last_liq_timestamp,
    };
    let binary = to_json_binary(&response).unwrap();
    deps.querier.update_wasm(move |_query| {
        SystemResult::Ok(ContractResult::Ok(binary.clone()))
    });
    deps
}

/// Set the liquidation_engine_contract in config (simulates UpdateConfig).
fn set_liq_engine_in_config(deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>) {
    let mut config = CONFIG.load(&deps.storage).unwrap();
    config.liquidation_engine_contract = Some(
        cosmwasm_std::Addr::unchecked(addr("liq_engine"))
    );
    CONFIG.save(&mut deps.storage, &config).unwrap();
}

// =================== Liquidation Lockout Tests ===================

#[test]
fn test_complete_unstake_blocked_by_recent_liquidation() {
    let mut deps = setup_deps_with_liq_engine_querier(Some(1571797419 + 86400)); // liq at mock_env time + 1 day
    setup_contract(&mut deps);
    set_liq_engine_in_config(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    request_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0, None);

    // Advance 2 days (past the normal cooldown) but the liquidation was at mock_env_time + 1 day
    // so lockout_end = (mock_env_time + 1 day) + 2 days = mock_env_time + 3 days
    // After advancing 2 days, current_time = mock_env_time + 2 days < lockout_end
    advance_time(&mut env, 172801); // Just past 2-day cooldown

    let info = mock_info(&addr("user1"), &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::CompleteUnstake {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
        },
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("liquidation lockout"),
        "Expected liquidation lockout error, got: {}",
        err,
    );
}

#[test]
fn test_complete_unstake_allowed_after_lockout_expires() {
    // Liquidation happened at mock_env base time (1571797419)
    // lockout_end = 1571797419 + 172800 = 1571970219
    let mut deps = setup_deps_with_liq_engine_querier(Some(1571797419));
    setup_contract(&mut deps);
    set_liq_engine_in_config(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    request_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0, None);

    // Advance 2 days + 1 second: past both the cooldown and the lockout
    advance_time(&mut env, 172801);

    // Should succeed — lockout expired
    complete_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0);

    // Verify unstake completed: request should be removed
    let requests = query_unstakes(&deps, &addr("user1"), COLLATERAL);
    assert_eq!(requests.requests.len(), 0, "Unstake request should be removed after completion");
}

#[test]
fn test_complete_unstake_no_lockout_without_liq_engine_config() {
    // Standard deps (no liq engine querier needed)
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    // Do NOT set liquidation_engine_contract — it stays None
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    request_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0, None);

    // Advance just past cooldown
    advance_time(&mut env, 172801);

    // Should succeed — no lockout check when liq engine is not configured
    complete_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0);

    let requests = query_unstakes(&deps, &addr("user1"), COLLATERAL);
    assert_eq!(requests.requests.len(), 0);
}

#[test]
fn test_complete_unstake_no_lockout_when_no_liquidation_events() {
    // Liq engine returns None (no events for this asset)
    let mut deps = setup_deps_with_liq_engine_querier(None);
    setup_contract(&mut deps);
    set_liq_engine_in_config(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    request_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0, None);

    // Advance past cooldown
    advance_time(&mut env, 172801);

    // Should succeed — no liquidation events means no lockout
    complete_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0);

    let requests = query_unstakes(&deps, &addr("user1"), COLLATERAL);
    assert_eq!(requests.requests.len(), 0);
}

#[test]
fn test_lockout_extends_effective_unlock_beyond_cooldown() {
    // Liquidation happens 1 day after the unstake request.
    // Normal unlock = request_time + 2 days
    // Lockout end = (request_time + 1 day) + 2 days = request_time + 3 days
    // So user must wait 3 days total instead of 2.
    let base_time = 1571797419u64; // mock_env base time

    // Liquidation at base_time + 1 day
    let liq_time = base_time + 86400;
    let mut deps = setup_deps_with_liq_engine_querier(Some(liq_time));
    setup_contract(&mut deps);
    set_liq_engine_in_config(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    request_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0, None);

    // At 2.5 days: past normal cooldown but within lockout
    advance_time(&mut env, 86400 * 2 + 43200); // 2.5 days

    let info = mock_info(&addr("user1"), &[]);
    let err = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CompleteUnstake {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
        },
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("liquidation lockout"),
        "Should be blocked at 2.5 days when lockout extends to 3 days, got: {}",
        err,
    );

    // At 3 days + 1 second: past both cooldown and lockout
    let mut env2 = mock_env();
    advance_time(&mut env2, 86400 * 3 + 1);

    complete_unstake(&mut deps, &env2, &addr("user1"), COLLATERAL, 90, 0);

    let requests = query_unstakes(&deps, &addr("user1"), COLLATERAL);
    assert_eq!(requests.requests.len(), 0, "Should complete after lockout expires");
}

#[test]
fn test_lockout_boundary_exact_expiry() {
    // Test exact boundary: lockout_end == current_time should pass (not strictly less than)
    let base_time = 1571797419u64;
    let liq_time = base_time; // Liq at same time as request
    let lockout_end = liq_time + 172800; // = base_time + 2 days

    let mut deps = setup_deps_with_liq_engine_querier(Some(liq_time));
    setup_contract(&mut deps);
    set_liq_engine_in_config(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    request_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0, None);

    // Advance exactly to lockout_end (also equals the normal cooldown end)
    advance_time(&mut env, 172800);

    // At exact boundary: block.time == lockout_end → not < lockout_end → should pass
    complete_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0);

    let requests = query_unstakes(&deps, &addr("user1"), COLLATERAL);
    assert_eq!(requests.requests.len(), 0);
}

#[test]
fn test_lockout_old_liquidation_does_not_block() {
    // Liquidation happened long ago (base_time - 10 days)
    let base_time = 1571797419u64;
    let liq_time = base_time - 864000; // 10 days before

    let mut deps = setup_deps_with_liq_engine_querier(Some(liq_time));
    setup_contract(&mut deps);
    set_liq_engine_in_config(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    request_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0, None);

    // Advance past normal cooldown
    advance_time(&mut env, 172801);

    // Old liquidation's lockout already expired, should succeed
    complete_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0);

    let requests = query_unstakes(&deps, &addr("user1"), COLLATERAL);
    assert_eq!(requests.requests.len(), 0);
}
