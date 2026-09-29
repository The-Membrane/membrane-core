mod test_helpers;

use cosmwasm_std::testing::{mock_env, mock_info, MockApi, MockQuerier};
use cosmwasm_std::{coins, from_json, MemoryStorage, OwnedDeps, Uint128};
use membrane::ltv_disco::{Config, ExecuteMsg, QueryMsg};
use ltv_disco::contract::{execute, instantiate, query};
use ltv_disco::state::{CONFIG, PENDING_BAD_DEBT};
use test_helpers::*;

pub const MBRN: &str = "mbrn_denom";

/// Setup contract with mbrn_denom configured (needed for SendMBRNForSale tests)
fn setup_contract_with_mbrn(deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>) -> Config {
    let mut msg = standard_instantiate_msg();
    msg.mbrn_denom = Some(MBRN.to_string());
    let info = mock_info(&addr("creator"), &[]);
    let env = mock_env();
    instantiate(deps.as_mut(), env, info, msg).unwrap();
    CONFIG.load(&deps.storage).unwrap()
}

// =================== AddBadDebt ===================

#[test]
fn test_add_bad_debt_unauthorized() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // Non-CDP caller should fail
    let info = mock_info(&addr("random"), &[]);
    let err = execute(
        deps.as_mut(), env, info,
        ExecuteMsg::AddBadDebt {
            asset: COLLATERAL.to_string(),
            amount: Uint128::new(1000),
        },
    ).unwrap_err();
    assert_eq!(err.to_string(), "Unauthorized");
}

#[test]
fn test_add_bad_debt_no_deposits() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    // No deposits — should fail with zero MBRN
    let info = mock_info(&addr("cdp_contract"), &[]);
    let err = execute(
        deps.as_mut(), env, info,
        ExecuteMsg::AddBadDebt {
            asset: COLLATERAL.to_string(),
            amount: Uint128::new(1000),
        },
    ).unwrap_err();
    assert!(err.to_string().contains("No MBRN deposits"));
}

#[test]
fn test_add_bad_debt_no_queue() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);

    let env = mock_env();
    let info = mock_info(&addr("cdp_contract"), &[]);
    let err = execute(
        deps.as_mut(), env, info,
        ExecuteMsg::AddBadDebt {
            asset: "nonexistent".to_string(),
            amount: Uint128::new(1000),
        },
    ).unwrap_err();
    // Should error trying to load queue
    assert!(err.to_string().len() > 0);
}

#[test]
fn test_add_bad_debt_sets_pending_and_starts_sale() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    submit_deposit(&mut deps, &env, &addr("user2"), COLLATERAL, 80, 5_000);

    let info = mock_info(&addr("cdp_contract"), &[]);
    let res = execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::AddBadDebt {
            asset: COLLATERAL.to_string(),
            amount: Uint128::new(2000),
        },
    ).unwrap();

    // Check pending bad debt is set (freezes withdrawals)
    let pending = PENDING_BAD_DEBT.load(&deps.storage, COLLATERAL.to_string()).unwrap();
    assert_eq!(pending, Uint128::new(2000));

    // Check a StartMBRNSale message was sent
    assert_eq!(res.messages.len(), 1);

    // Verify attributes
    let attrs: Vec<_> = res.attributes.iter().map(|a| (a.key.as_str(), a.value.as_str())).collect();
    assert!(attrs.contains(&("method", "add_bad_debt")));
    assert!(attrs.contains(&("bad_debt_cdt", "2000")));
    assert!(attrs.contains(&("mbrn_available", "15000"))); // 10_000 + 5_000
}

#[test]
fn test_add_bad_debt_stacks_pending() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // First bad debt
    let info = mock_info(&addr("cdp_contract"), &[]);
    execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::AddBadDebt {
            asset: COLLATERAL.to_string(),
            amount: Uint128::new(1000),
        },
    ).unwrap();

    // Second bad debt for same asset
    let info = mock_info(&addr("cdp_contract"), &[]);
    execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::AddBadDebt {
            asset: COLLATERAL.to_string(),
            amount: Uint128::new(500),
        },
    ).unwrap();

    let pending = PENDING_BAD_DEBT.load(&deps.storage, COLLATERAL.to_string()).unwrap();
    assert_eq!(pending, Uint128::new(1500)); // 1000 + 500
}

#[test]
fn test_add_bad_debt_no_slash_on_deposit_tokens() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    let info = mock_info(&addr("cdp_contract"), &[]);
    execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::AddBadDebt {
            asset: COLLATERAL.to_string(),
            amount: Uint128::new(5000),
        },
    ).unwrap();

    // Deposit tokens should NOT be slashed — slashing happens in SendMBRNForSale
    let queue_resp = query_queue(&deps, COLLATERAL);
    let slots = &queue_resp.queues[0].1.slots;
    let slot90 = slots.iter().find(|s| s.max_ltv == cosmwasm_std::Decimal::percent(90)).unwrap();
    assert_eq!(slot90.total_deposit_tokens, Uint128::new(10_000), "Deposits should not be slashed by add_bad_debt");
}

#[test]
fn test_add_bad_debt_different_assets_independent() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);
    create_queue(&mut deps, COLLATERAL);
    create_queue(&mut deps, "other_asset");

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    submit_deposit(&mut deps, &env, &addr("user1"), "other_asset", 90, 5_000);

    let info = mock_info(&addr("cdp_contract"), &[]);
    execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::AddBadDebt {
            asset: COLLATERAL.to_string(),
            amount: Uint128::new(1000),
        },
    ).unwrap();

    // Only COLLATERAL should be frozen
    let pending_collateral = PENDING_BAD_DEBT.load(&deps.storage, COLLATERAL.to_string()).unwrap();
    assert_eq!(pending_collateral, Uint128::new(1000));
    let pending_other = PENDING_BAD_DEBT.may_load(&deps.storage, "other_asset".to_string()).unwrap();
    assert!(pending_other.is_none() || pending_other.unwrap().is_zero());
}

// =================== Withdrawal Freeze ===================

#[test]
fn test_request_unstake_blocked_during_bad_debt() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // Add bad debt → freeze
    let info = mock_info(&addr("cdp_contract"), &[]);
    execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::AddBadDebt {
            asset: COLLATERAL.to_string(),
            amount: Uint128::new(1000),
        },
    ).unwrap();

    // Request unstake should fail
    let info = mock_info(&addr("user1"), &[]);
    let err = execute(
        deps.as_mut(), env, info,
        ExecuteMsg::RequestUnstake {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
            amount: None,
        },
    ).unwrap_err();
    assert!(err.to_string().contains("Withdrawals frozen"));
}

#[test]
fn test_complete_unstake_blocked_during_bad_debt() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // Request unstake BEFORE bad debt
    request_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0, None);

    // Add bad debt → freeze
    let info = mock_info(&addr("cdp_contract"), &[]);
    execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::AddBadDebt {
            asset: COLLATERAL.to_string(),
            amount: Uint128::new(1000),
        },
    ).unwrap();

    // Advance past cooldown
    advance_time(&mut env, 200_000);

    // Complete unstake should fail even though cooldown passed
    let info = mock_info(&addr("user1"), &[]);
    let err = execute(
        deps.as_mut(), env, info,
        ExecuteMsg::CompleteUnstake {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
        },
    ).unwrap_err();
    assert!(err.to_string().contains("Withdrawals frozen"));
}

#[test]
fn test_request_unstake_allowed_for_unfrozen_asset() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);
    create_queue(&mut deps, COLLATERAL);
    create_queue(&mut deps, "other_asset");

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    submit_deposit(&mut deps, &env, &addr("user1"), "other_asset", 90, 5_000);

    // Freeze only COLLATERAL
    let info = mock_info(&addr("cdp_contract"), &[]);
    execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::AddBadDebt {
            asset: COLLATERAL.to_string(),
            amount: Uint128::new(1000),
        },
    ).unwrap();

    // other_asset should still allow unstake
    let info = mock_info(&addr("user1"), &[]);
    execute(
        deps.as_mut(), env, info,
        ExecuteMsg::RequestUnstake {
            asset: "other_asset".to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
            amount: None,
        },
    ).unwrap(); // should succeed
}

#[test]
fn test_request_unstake_allowed_after_freeze_cleared() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // Freeze
    let info = mock_info(&addr("cdp_contract"), &[]);
    execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::AddBadDebt {
            asset: COLLATERAL.to_string(),
            amount: Uint128::new(1000),
        },
    ).unwrap();

    // Clear freeze
    let info = mock_info(&addr("auction_contract"), &[]);
    execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::ClearBadDebtFreeze {
            asset: COLLATERAL.to_string(),
            cdt_amount: Uint128::new(1000),
        },
    ).unwrap();

    // Now unstake should work
    let info = mock_info(&addr("user1"), &[]);
    execute(
        deps.as_mut(), env, info,
        ExecuteMsg::RequestUnstake {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
            amount: None,
        },
    ).unwrap(); // should succeed
}

// =================== ClearBadDebtFreeze ===================

#[test]
fn test_clear_bad_debt_freeze_unauthorized() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);

    let env = mock_env();
    let info = mock_info(&addr("random"), &[]);
    let err = execute(
        deps.as_mut(), env, info,
        ExecuteMsg::ClearBadDebtFreeze {
            asset: COLLATERAL.to_string(),
            cdt_amount: Uint128::new(1000),
        },
    ).unwrap_err();
    assert_eq!(err.to_string(), "Unauthorized");
}

#[test]
fn test_clear_bad_debt_freeze_full() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // Set pending
    let info = mock_info(&addr("cdp_contract"), &[]);
    execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::AddBadDebt {
            asset: COLLATERAL.to_string(),
            amount: Uint128::new(1000),
        },
    ).unwrap();

    // Clear fully
    let info = mock_info(&addr("auction_contract"), &[]);
    execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::ClearBadDebtFreeze {
            asset: COLLATERAL.to_string(),
            cdt_amount: Uint128::new(1000),
        },
    ).unwrap();

    // Should be removed from state entirely
    let pending = PENDING_BAD_DEBT.may_load(&deps.storage, COLLATERAL.to_string()).unwrap();
    assert!(pending.is_none());
}

#[test]
fn test_clear_bad_debt_freeze_partial() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // Set pending = 1500 (two bad debts)
    let info = mock_info(&addr("cdp_contract"), &[]);
    execute(deps.as_mut(), env.clone(), info, ExecuteMsg::AddBadDebt {
        asset: COLLATERAL.to_string(), amount: Uint128::new(1000),
    }).unwrap();
    let info = mock_info(&addr("cdp_contract"), &[]);
    execute(deps.as_mut(), env.clone(), info, ExecuteMsg::AddBadDebt {
        asset: COLLATERAL.to_string(), amount: Uint128::new(500),
    }).unwrap();

    // Clear 1000 of 1500
    let info = mock_info(&addr("auction_contract"), &[]);
    execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::ClearBadDebtFreeze {
            asset: COLLATERAL.to_string(),
            cdt_amount: Uint128::new(1000),
        },
    ).unwrap();

    // 500 should remain → still frozen
    let pending = PENDING_BAD_DEBT.load(&deps.storage, COLLATERAL.to_string()).unwrap();
    assert_eq!(pending, Uint128::new(500));

    // Unstake should still be blocked
    let info = mock_info(&addr("user1"), &[]);
    let err = execute(
        deps.as_mut(), env, info,
        ExecuteMsg::RequestUnstake {
            asset: COLLATERAL.to_string(),
            slot: 90,
            deposit_id: Uint128::zero(),
            amount: None,
        },
    ).unwrap_err();
    assert!(err.to_string().contains("Withdrawals frozen"));
}

#[test]
fn test_clear_bad_debt_saturating_sub() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    let info = mock_info(&addr("cdp_contract"), &[]);
    execute(deps.as_mut(), env.clone(), info, ExecuteMsg::AddBadDebt {
        asset: COLLATERAL.to_string(), amount: Uint128::new(100),
    }).unwrap();

    // Clear more than pending — should saturate to zero, not panic
    let info = mock_info(&addr("auction_contract"), &[]);
    execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::ClearBadDebtFreeze {
            asset: COLLATERAL.to_string(),
            cdt_amount: Uint128::new(9999),
        },
    ).unwrap();

    let pending = PENDING_BAD_DEBT.may_load(&deps.storage, COLLATERAL.to_string()).unwrap();
    assert!(pending.is_none());
}

// =================== SendMBRNForSale ===================

#[test]
fn test_send_mbrn_unauthorized() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    let info = mock_info(&addr("random"), &[]);
    let err = execute(
        deps.as_mut(), env, info,
        ExecuteMsg::SendMBRNForSale {
            amount: Uint128::new(100),
            recipient: addr("buyer"),
            asset: COLLATERAL.to_string(),
        },
    ).unwrap_err();
    assert_eq!(err.to_string(), "Unauthorized");
}

#[test]
fn test_send_mbrn_zero_amount() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);

    let env = mock_env();
    let info = mock_info(&addr("auction_contract"), &[]);
    let err = execute(
        deps.as_mut(), env, info,
        ExecuteMsg::SendMBRNForSale {
            amount: Uint128::zero(),
            recipient: addr("buyer"),
            asset: COLLATERAL.to_string(),
        },
    ).unwrap_err();
    assert!(err.to_string().contains("greater than zero"));
}

#[test]
fn test_send_mbrn_slashes_riskiest_slot_first() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    // Deposit into slots 90 (riskiest) and 50 (safest)
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 5_000);
    submit_deposit(&mut deps, &env, &addr("user2"), COLLATERAL, 50, 8_000);

    // Verify initial state
    let queue_resp = query_queue(&deps, COLLATERAL);
    let slots = &queue_resp.queues[0].1.slots;
    let slot90 = slots.iter().find(|s| s.max_ltv == cosmwasm_std::Decimal::percent(90)).unwrap();
    let slot50 = slots.iter().find(|s| s.max_ltv == cosmwasm_std::Decimal::percent(50)).unwrap();
    assert_eq!(slot90.total_deposit_tokens, Uint128::new(5_000));
    assert_eq!(slot50.total_deposit_tokens, Uint128::new(8_000));

    // Slash 3_000 — should come entirely from slot 90 (riskiest)
    let info = mock_info(&addr("auction_contract"), &[]);
    execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::SendMBRNForSale {
            amount: Uint128::new(3_000),
            recipient: addr("buyer"),
            asset: COLLATERAL.to_string(),
        },
    ).unwrap();

    let queue_resp = query_queue(&deps, COLLATERAL);
    let slots = &queue_resp.queues[0].1.slots;
    let slot90 = slots.iter().find(|s| s.max_ltv == cosmwasm_std::Decimal::percent(90)).unwrap();
    let slot50 = slots.iter().find(|s| s.max_ltv == cosmwasm_std::Decimal::percent(50)).unwrap();
    assert_eq!(slot90.total_deposit_tokens, Uint128::new(2_000)); // 5000 - 3000
    assert_eq!(slot50.total_deposit_tokens, Uint128::new(8_000)); // untouched
}

#[test]
fn test_send_mbrn_cascades_to_safer_slots() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 3_000);
    submit_deposit(&mut deps, &env, &addr("user2"), COLLATERAL, 80, 4_000);
    submit_deposit(&mut deps, &env, &addr("user3"), COLLATERAL, 50, 10_000);

    // Slash 6_000 — 3_000 from slot 90 (exhausted), 3_000 from slot 80
    let info = mock_info(&addr("auction_contract"), &[]);
    execute(
        deps.as_mut(), env.clone(), info,
        ExecuteMsg::SendMBRNForSale {
            amount: Uint128::new(6_000),
            recipient: addr("buyer"),
            asset: COLLATERAL.to_string(),
        },
    ).unwrap();

    let queue_resp = query_queue(&deps, COLLATERAL);
    let slots = &queue_resp.queues[0].1.slots;
    let slot90 = slots.iter().find(|s| s.max_ltv == cosmwasm_std::Decimal::percent(90)).unwrap();
    let slot80 = slots.iter().find(|s| s.max_ltv == cosmwasm_std::Decimal::percent(80)).unwrap();
    let slot50 = slots.iter().find(|s| s.max_ltv == cosmwasm_std::Decimal::percent(50)).unwrap();
    assert_eq!(slot90.total_deposit_tokens, Uint128::zero()); // fully slashed
    assert_eq!(slot80.total_deposit_tokens, Uint128::new(1_000)); // 4000 - 3000
    assert_eq!(slot50.total_deposit_tokens, Uint128::new(10_000)); // untouched
}

#[test]
fn test_send_mbrn_insufficient_across_all_slots() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 5_000);

    // Try to slash more than available
    let info = mock_info(&addr("auction_contract"), &[]);
    let err = execute(
        deps.as_mut(), env, info,
        ExecuteMsg::SendMBRNForSale {
            amount: Uint128::new(10_000),
            recipient: addr("buyer"),
            asset: COLLATERAL.to_string(),
        },
    ).unwrap_err();
    assert!(err.to_string().contains("Insufficient MBRN"));
}

#[test]
fn test_send_mbrn_multiple_calls_reduce_progressively() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // First pull: 3000
    let info = mock_info(&addr("auction_contract"), &[]);
    execute(deps.as_mut(), env.clone(), info, ExecuteMsg::SendMBRNForSale {
        amount: Uint128::new(3_000), recipient: addr("buyer"), asset: COLLATERAL.to_string(),
    }).unwrap();

    let queue_resp = query_queue(&deps, COLLATERAL);
    let slots = &queue_resp.queues[0].1.slots;
    let slot90 = slots.iter().find(|s| s.max_ltv == cosmwasm_std::Decimal::percent(90)).unwrap();
    assert_eq!(slot90.total_deposit_tokens, Uint128::new(7_000));

    // Second pull: 4000
    let info = mock_info(&addr("auction_contract"), &[]);
    execute(deps.as_mut(), env.clone(), info, ExecuteMsg::SendMBRNForSale {
        amount: Uint128::new(4_000), recipient: addr("buyer"), asset: COLLATERAL.to_string(),
    }).unwrap();

    let queue_resp = query_queue(&deps, COLLATERAL);
    let slots = &queue_resp.queues[0].1.slots;
    let slot90 = slots.iter().find(|s| s.max_ltv == cosmwasm_std::Decimal::percent(90)).unwrap();
    assert_eq!(slot90.total_deposit_tokens, Uint128::new(3_000));

    // Third pull: 5000 — exceeds remaining 3000
    let info = mock_info(&addr("auction_contract"), &[]);
    let err = execute(deps.as_mut(), env, info, ExecuteMsg::SendMBRNForSale {
        amount: Uint128::new(5_000), recipient: addr("buyer"), asset: COLLATERAL.to_string(),
    }).unwrap_err();
    assert!(err.to_string().contains("Insufficient MBRN"));
}

// =================== GetPendingBadDebt Query ===================

#[test]
fn test_query_pending_bad_debt_none() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);

    let res = query(
        deps.as_ref(), mock_env(),
        QueryMsg::GetPendingBadDebt { asset: COLLATERAL.to_string() },
    ).unwrap();
    let pending: Uint128 = from_json(res).unwrap();
    assert_eq!(pending, Uint128::zero());
}

#[test]
fn test_query_pending_bad_debt_set() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    let info = mock_info(&addr("cdp_contract"), &[]);
    execute(deps.as_mut(), env, info, ExecuteMsg::AddBadDebt {
        asset: COLLATERAL.to_string(), amount: Uint128::new(777),
    }).unwrap();

    let res = query(
        deps.as_ref(), mock_env(),
        QueryMsg::GetPendingBadDebt { asset: COLLATERAL.to_string() },
    ).unwrap();
    let pending: Uint128 = from_json(res).unwrap();
    assert_eq!(pending, Uint128::new(777));
}

// =================== Integration: Full Lifecycle ===================

#[test]
fn test_full_lifecycle_freeze_slash_unfreeze() {
    let mut deps = setup_deps();
    setup_contract_with_mbrn(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    submit_deposit(&mut deps, &env, &addr("user2"), COLLATERAL, 80, 5_000);

    // 1. Add bad debt → freezes withdrawals
    let info = mock_info(&addr("cdp_contract"), &[]);
    execute(deps.as_mut(), env.clone(), info, ExecuteMsg::AddBadDebt {
        asset: COLLATERAL.to_string(), amount: Uint128::new(3000),
    }).unwrap();

    // 2. Verify frozen
    let info = mock_info(&addr("user1"), &[]);
    assert!(execute(deps.as_mut(), env.clone(), info, ExecuteMsg::RequestUnstake {
        asset: COLLATERAL.to_string(), slot: 90, deposit_id: Uint128::zero(), amount: None,
    }).is_err());

    // 3. Auction pulls MBRN (slashes deposits)
    let info = mock_info(&addr("auction_contract"), &[]);
    execute(deps.as_mut(), env.clone(), info, ExecuteMsg::SendMBRNForSale {
        amount: Uint128::new(4_000), recipient: addr("buyer"), asset: COLLATERAL.to_string(),
    }).unwrap();

    // Verify slot 90 was slashed first
    let queue_resp = query_queue(&deps, COLLATERAL);
    let slots = &queue_resp.queues[0].1.slots;
    let slot90 = slots.iter().find(|s| s.max_ltv == cosmwasm_std::Decimal::percent(90)).unwrap();
    assert_eq!(slot90.total_deposit_tokens, Uint128::new(6_000)); // 10000 - 4000
    let slot80 = slots.iter().find(|s| s.max_ltv == cosmwasm_std::Decimal::percent(80)).unwrap();
    assert_eq!(slot80.total_deposit_tokens, Uint128::new(5_000)); // untouched

    // 4. Still frozen (bad debt not cleared yet)
    let info = mock_info(&addr("user1"), &[]);
    assert!(execute(deps.as_mut(), env.clone(), info, ExecuteMsg::RequestUnstake {
        asset: COLLATERAL.to_string(), slot: 90, deposit_id: Uint128::zero(), amount: None,
    }).is_err());

    // 5. Auction clears freeze
    let info = mock_info(&addr("auction_contract"), &[]);
    execute(deps.as_mut(), env.clone(), info, ExecuteMsg::ClearBadDebtFreeze {
        asset: COLLATERAL.to_string(), cdt_amount: Uint128::new(3000),
    }).unwrap();

    // 6. Withdrawals work again
    let info = mock_info(&addr("user1"), &[]);
    execute(deps.as_mut(), env.clone(), info, ExecuteMsg::RequestUnstake {
        asset: COLLATERAL.to_string(), slot: 90, deposit_id: Uint128::zero(), amount: None,
    }).unwrap(); // should succeed

    // 7. Complete unstake after cooldown
    advance_time(&mut env, 200_000);
    complete_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0);
}
