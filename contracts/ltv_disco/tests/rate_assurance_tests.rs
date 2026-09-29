mod test_helpers;

use cosmwasm_std::testing::{mock_env, mock_info};
use cosmwasm_std::{Uint128, WasmMsg, CosmosMsg};
use membrane::ltv_disco::ExecuteMsg;
use ltv_disco::contract::execute;
use ltv_disco::state::{RATE_ASSURANCE, ASSET_QUEUES};
use test_helpers::*;

// =================== Authorization ===================

#[test]
fn test_rate_assurance_only_self_callable() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // Non-contract sender should get Unauthorized
    let info = mock_info(&addr("attacker"), &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::RateAssurance {
            asset: COLLATERAL.to_string(),
            slot: 90,
        },
    )
    .unwrap_err();
    assert_eq!(err.to_string(), "Unauthorized");
}

// =================== Rate Assurance After Deposit ===================

#[test]
fn test_rate_assurance_passes_after_deposit() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // Simulate self-call (contract address from mock_env is "cosmos2contract")
    let info = mock_info(&env.contract.address.to_string(), &[]);
    let res = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::RateAssurance {
            asset: COLLATERAL.to_string(),
            slot: 90,
        },
    );
    assert!(res.is_ok(), "Rate assurance should pass after a standard deposit");
}

// =================== Storage Population ===================

#[test]
fn test_rate_assurance_saved_after_deposit() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    // Before any deposit, RATE_ASSURANCE should not exist for this slot
    let before = RATE_ASSURANCE
        .may_load(&deps.storage, (COLLATERAL.to_string(), "90".to_string()))
        .unwrap();
    assert!(before.is_none(), "Rate assurance should not exist before any deposit");

    let env = mock_env();
    // First deposit: update_rate_assurance is called before slot totals are updated,
    // so the slot has zero vault tokens and the save is skipped.
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // Second deposit: now the slot already has tokens, so update_rate_assurance
    // captures and saves the pre-change rate.
    submit_deposit(&mut deps, &env, &addr("user2"), COLLATERAL, 90, 10_000);

    // After the second deposit, RATE_ASSURANCE should be populated
    let after = RATE_ASSURANCE
        .may_load(&deps.storage, (COLLATERAL.to_string(), "90".to_string()))
        .unwrap();
    assert!(after.is_some(), "Rate assurance should be saved after second deposit");
    assert!(
        !after.unwrap().is_zero(),
        "Rate assurance value should be non-zero"
    );
}

// =================== Callback Emission ===================

#[test]
fn test_rate_assurance_complete_unstake_emits_callback() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let mut env = mock_env();

    // Deposit two users so the slot is not emptied by the unstake
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    submit_deposit(&mut deps, &env, &addr("user2"), COLLATERAL, 90, 10_000);

    // Request unstake for user1 (partial of the slot)
    request_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0, None);

    // Advance past cooldown
    advance_time(&mut env, 172801);

    let res = complete_unstake(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 0);

    // Check that response messages contain a WasmMsg for RateAssurance
    let has_rate_assurance_msg = res.messages.iter().any(|sub_msg| {
        if let CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr, msg, ..
        }) = &sub_msg.msg
        {
            if contract_addr == env.contract.address.as_str() {
                if let Ok(exec_msg) = cosmwasm_std::from_json::<ExecuteMsg>(msg) {
                    matches!(exec_msg, ExecuteMsg::RateAssurance { .. })
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        }
    });

    assert!(
        has_rate_assurance_msg,
        "complete_unstake should emit a RateAssurance self-callback"
    );
}

#[test]
fn test_rate_assurance_submit_deposit_emits_callback() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    let res = submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // Check that response messages contain a WasmMsg for RateAssurance
    let has_rate_assurance_msg = res.messages.iter().any(|sub_msg| {
        if let CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr, msg, ..
        }) = &sub_msg.msg
        {
            if contract_addr == env.contract.address.as_str() {
                if let Ok(exec_msg) = cosmwasm_std::from_json::<ExecuteMsg>(msg) {
                    matches!(exec_msg, ExecuteMsg::RateAssurance { .. })
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        }
    });

    assert!(
        has_rate_assurance_msg,
        "submit_deposit should emit a RateAssurance self-callback"
    );
}

// =================== Rate Decrease Detection ===================

#[test]
fn test_rate_assurance_detects_rate_decrease() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // Compute the current actual rate from the queue state
    let queue = ASSET_QUEUES
        .load(&deps.storage, COLLATERAL.to_string())
        .unwrap();
    let slot_data = &queue.slots[0]; // 90% LTV slot is index 0
    let actual_rate = membrane::stability_pool_vault::calculate_base_tokens(
        Uint128::new(1_000_000_000_000),
        slot_data.total_deposit_tokens,
        slot_data.total_vault_tokens,
    )
    .unwrap();

    // Directly write a much higher rate into RATE_ASSURANCE, simulating a situation
    // where the rate has decreased since the snapshot was taken
    let inflated_rate = actual_rate + Uint128::new(1_000);
    RATE_ASSURANCE
        .save(
            &mut deps.storage,
            (COLLATERAL.to_string(), "90".to_string()),
            &inflated_rate,
        )
        .unwrap();

    // Call execute_rate_assurance from the contract's own address
    let info = mock_info(&env.contract.address.to_string(), &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::RateAssurance {
            asset: COLLATERAL.to_string(),
            slot: 90,
        },
    )
    .unwrap_err();

    assert!(
        err.to_string().contains("Rate assurance failed"),
        "Expected 'Rate assurance failed' error, got: {}",
        err
    );
}

// =================== Tolerance ===================

#[test]
fn test_rate_assurance_allows_one_unit_tolerance() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);

    // Read the current actual rate from the queue
    let queue = ASSET_QUEUES
        .load(&deps.storage, COLLATERAL.to_string())
        .unwrap();
    let slot_data = &queue.slots[0]; // 90% LTV slot is index 0
    let actual_rate = membrane::stability_pool_vault::calculate_base_tokens(
        Uint128::new(1_000_000_000_000),
        slot_data.total_deposit_tokens,
        slot_data.total_vault_tokens,
    )
    .unwrap();

    // Set RATE_ASSURANCE to actual_rate + 1 (exactly at tolerance boundary)
    // The check is: current_rate + 1 >= last_rate
    // So if last_rate = actual_rate + 1, then actual_rate + 1 >= actual_rate + 1 => true
    let stored_rate = actual_rate + Uint128::one();
    RATE_ASSURANCE
        .save(
            &mut deps.storage,
            (COLLATERAL.to_string(), "90".to_string()),
            &stored_rate,
        )
        .unwrap();

    // This should pass (within 1 unit tolerance)
    let info = mock_info(&env.contract.address.to_string(), &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RateAssurance {
            asset: COLLATERAL.to_string(),
            slot: 90,
        },
    );
    assert!(
        res.is_ok(),
        "Rate assurance should pass when rate is exactly 1 unit below stored rate, got: {:?}",
        res.err()
    );

    // Now set stored rate to actual_rate + 2 (exceeds tolerance)
    // current_rate + 1 >= actual_rate + 2  =>  actual_rate + 1 >= actual_rate + 2 => false
    let over_tolerance = actual_rate + Uint128::new(2);
    RATE_ASSURANCE
        .save(
            &mut deps.storage,
            (COLLATERAL.to_string(), "90".to_string()),
            &over_tolerance,
        )
        .unwrap();

    let info2 = mock_info(&env.contract.address.to_string(), &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info2,
        ExecuteMsg::RateAssurance {
            asset: COLLATERAL.to_string(),
            slot: 90,
        },
    )
    .unwrap_err();

    assert!(
        err.to_string().contains("Rate assurance failed"),
        "Rate assurance should fail when rate is 2 units below stored rate, got: {}",
        err
    );
}

// =================== Per-Asset Independence ===================

#[test]
fn test_rate_assurance_independent_per_asset() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);

    // Create two different asset queues
    create_queue(&mut deps, COLLATERAL);
    create_queue(&mut deps, "collateral2");

    let env = mock_env();

    // Deposit into both assets
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    submit_deposit(&mut deps, &env, &addr("user1"), "collateral2", 90, 50_000);

    // Second deposits to populate rate assurance storage
    submit_deposit(&mut deps, &env, &addr("user2"), COLLATERAL, 90, 10_000);
    submit_deposit(&mut deps, &env, &addr("user2"), "collateral2", 90, 50_000);

    // Both assets should have independent rate assurance values
    let rate1 = RATE_ASSURANCE
        .may_load(&deps.storage, (COLLATERAL.to_string(), "90".to_string()))
        .unwrap();
    let rate2 = RATE_ASSURANCE
        .may_load(&deps.storage, ("collateral2".to_string(), "90".to_string()))
        .unwrap();
    assert!(rate1.is_some(), "COLLATERAL rate assurance should exist");
    assert!(rate2.is_some(), "collateral2 rate assurance should exist");

    // Sabotage COLLATERAL rate (set impossibly high)
    RATE_ASSURANCE
        .save(
            &mut deps.storage,
            (COLLATERAL.to_string(), "90".to_string()),
            &Uint128::new(999_999_999_999),
        )
        .unwrap();

    // COLLATERAL rate assurance should now fail
    let info = mock_info(&env.contract.address.to_string(), &[]);
    let err = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RateAssurance {
            asset: COLLATERAL.to_string(),
            slot: 90,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("Rate assurance failed"));

    // collateral2 rate assurance should still pass (independent)
    let info2 = mock_info(&env.contract.address.to_string(), &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info2,
        ExecuteMsg::RateAssurance {
            asset: "collateral2".to_string(),
            slot: 90,
        },
    );
    assert!(
        res.is_ok(),
        "collateral2 rate assurance should pass independently, got: {:?}",
        res.err()
    );
}

#[test]
fn test_rate_assurance_independent_per_slot_within_asset() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let env = mock_env();

    // Deposit into slots 90 and 70
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 90, 10_000);
    submit_deposit(&mut deps, &env, &addr("user1"), COLLATERAL, 70, 20_000);

    // Second deposits to populate rate assurance
    submit_deposit(&mut deps, &env, &addr("user2"), COLLATERAL, 90, 10_000);
    submit_deposit(&mut deps, &env, &addr("user2"), COLLATERAL, 70, 20_000);

    // Both slots should have independent rate assurance
    let rate_slot90 = RATE_ASSURANCE
        .may_load(&deps.storage, (COLLATERAL.to_string(), "90".to_string()))
        .unwrap();
    let rate_slot70 = RATE_ASSURANCE
        .may_load(&deps.storage, (COLLATERAL.to_string(), "70".to_string()))
        .unwrap();
    assert!(rate_slot90.is_some(), "Slot 90 rate assurance should exist");
    assert!(rate_slot70.is_some(), "Slot 70 rate assurance should exist");

    // Sabotage slot 90 rate
    RATE_ASSURANCE
        .save(
            &mut deps.storage,
            (COLLATERAL.to_string(), "90".to_string()),
            &Uint128::new(999_999_999_999),
        )
        .unwrap();

    // Slot 90 rate assurance should fail
    let info = mock_info(&env.contract.address.to_string(), &[]);
    let err = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RateAssurance {
            asset: COLLATERAL.to_string(),
            slot: 90,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("Rate assurance failed"));

    // Slot 70 rate assurance should still pass
    let info2 = mock_info(&env.contract.address.to_string(), &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info2,
        ExecuteMsg::RateAssurance {
            asset: COLLATERAL.to_string(),
            slot: 70,
        },
    );
    assert!(
        res.is_ok(),
        "Slot 70 rate assurance should pass independently, got: {:?}",
        res.err()
    );
}
