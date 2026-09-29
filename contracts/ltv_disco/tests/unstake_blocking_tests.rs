mod test_helpers;

use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info, MockApi, MockQuerier};
use cosmwasm_std::{to_json_binary, MemoryStorage, OwnedDeps, Uint128, SystemResult, ContractResult};
use membrane::ltv_disco::ExecuteMsg;
use membrane::emissions_voting::HasAnyVotesResponse;
use ltv_disco::contract::execute;
use ltv_disco::state::BACKING_DEPOSITS;
use test_helpers::*;

// =================== Helper: Custom Querier for Emissions ===================

/// Create deps with a wasm querier that returns `HasAnyVotesResponse` for all wasm queries.
fn setup_deps_with_emissions_querier(has_votes: bool) -> OwnedDeps<MemoryStorage, MockApi, MockQuerier> {
    let mut deps = mock_dependencies();
    let response = HasAnyVotesResponse { has_votes };
    let binary = to_json_binary(&response).unwrap();
    deps.querier.update_wasm(move |_query| {
        SystemResult::Ok(ContractResult::Ok(binary.clone()))
    });
    deps
}

// =================== Withdrawals Disabled ===================

#[test]
fn test_unstake_blocked_when_withdrawals_disabled() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // Directly modify the deposit in storage to disable withdrawals
    let deposit_key = format!("{}:{}:{}:{}", COLLATERAL, 90, addr("user1"), 0);
    let mut deposit = BACKING_DEPOSITS.load(&deps.storage, deposit_key.clone()).unwrap();
    assert!(deposit.withdrawals_enabled, "New deposit should start with withdrawals enabled");
    deposit.withdrawals_enabled = false;
    BACKING_DEPOSITS.save(&mut deps.storage, deposit_key, &deposit).unwrap();

    // Attempt to request unstake - should fail
    let info = mock_info(&addr("user1"), &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::RequestUnstake {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
            amount: None,
        },
    )
    .unwrap_err();
    assert_eq!(err.to_string(), "Withdrawals disabled");
}

// =================== Emissions Vote Blocking ===================

#[test]
fn test_unstake_blocked_by_emissions_votes() {
    // Use custom querier that returns has_votes: true
    let mut deps = setup_deps_with_emissions_querier(true);
    setup_contract_with_emissions(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // Attempt to request unstake - should fail because user has active votes
    let info = mock_info(&addr("user1"), &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::RequestUnstake {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
            amount: None,
        },
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("Cannot unstake while you have active emissions votes"),
        "Expected emissions vote error, got: {}",
        err,
    );
}

#[test]
fn test_unstake_allowed_no_emissions_votes() {
    // Use custom querier that returns has_votes: false
    let mut deps = setup_deps_with_emissions_querier(false);
    setup_contract_with_emissions(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // Request unstake should succeed when user has no active votes
    request_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0, None);

    let requests = query_unstakes(&deps, &addr("user1"), COLLATERAL);
    assert_eq!(requests.requests.len(), 1, "Unstake request should be created");
    assert_eq!(requests.requests[0].vault_tokens, vt(10_000));
}

#[test]
fn test_unstake_allowed_no_emissions_contract() {
    // Standard setup with no emissions voting contract configured
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // Request unstake should succeed without any emissions contract check
    request_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0, None);

    let requests = query_unstakes(&deps, &addr("user1"), COLLATERAL);
    assert_eq!(requests.requests.len(), 1, "Unstake request should be created");
    assert_eq!(requests.requests[0].vault_tokens, vt(10_000));
}

// =================== Default Withdrawals Enabled ===================

#[test]
fn test_deposit_default_withdrawals_enabled() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // Verify the deposit has withdrawals_enabled = true by default
    let deposit = query_deposit(&deps, &addr("user1"), COLLATERAL, 90, 0);
    assert!(
        deposit.deposit.withdrawals_enabled,
        "New deposits must always have withdrawals_enabled = true",
    );
}

#[test]
fn test_deposit_on_behalf_preserves_withdrawals_enabled() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    // Deposit on behalf of user1 from a different sender
    submit_deposit_for_user(
        &mut deps,
        &env,
        &addr("depositor"),
        &addr("user1"),
        COLLATERAL,
        90,
        10_000,
        None,
    );

    // Verify the deposit created on behalf of user1 has withdrawals_enabled = true
    let deposit = query_deposit(&deps, &addr("user1"), COLLATERAL, 90, 0);
    assert!(
        deposit.deposit.withdrawals_enabled,
        "Deposits on behalf of another user must have withdrawals_enabled = true",
    );
}
