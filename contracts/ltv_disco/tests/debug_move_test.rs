// Debug test for move operation
mod effective_locked_tokens_tests;

use cosmwasm_std::testing::{mock_info};
use cosmwasm_std::{coins, Decimal, Uint128, from_json};
use ltv_disco::contract::{execute, query};
use ltv_disco::state::LTV_QUEUES;
use membrane::ltv_disco::{ExecuteMsg, BackingDepositInput, QueryMsg, LTVQueueResponse};

#[test]
fn debug_move_effective_total_update() {
    use effective_locked_tokens_tests::instantiate_contract_with_epoch;

    let (mut deps, env, _epoch_state) = instantiate_contract_with_epoch(0, 86400, 1000);

    // Create queue
    let info = mock_info("cdp_contract", &[]);
    let msg = ExecuteMsg::CreateQueue { asset: "uusd".to_string() };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Submit deposit
    let info = mock_info("user1", &coins(10000, "uusd"));
    let msg = ExecuteMsg::SubmitDeposit {
        deposit_input: BackingDepositInput {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
            epoch_start_time: None,
        },
        deposit_owner: None,
        locked: None,
        deposit_id: None,
        manager: None,
        affiliate_address: None,
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Get deposit_id
    let queue: LTVQueueResponse = from_json(
        query(deps.as_ref(), env.clone(), QueryMsg::GetLTVQueue { asset: "uusd".to_string() }).unwrap()
    ).unwrap();
    let deposit_id = queue.queue.current_deposit_id - Uint128::one();

    // Get initial effective total from storage
    let queue = LTV_QUEUES.load(&deps.storage, "uusd".to_string()).unwrap();
    let slot = queue.slots.iter().find(|s| s.ltv == Decimal::percent(60)).unwrap();
    let group = slot.deposit_groups.iter().find(|g| g.max_borrow_ltv == Decimal::percent(40)).unwrap();
    let initial_effective = group.total_unused_locked_vault_tokens;
    println!("Initial effective (source, 40%): {}", initial_effective);
    assert!(initial_effective > Uint128::zero(), "Should have initial effective total");

    // Move deposit to different group
    let info = mock_info("user1", &[]);
    let msg = ExecuteMsg::MoveDeposit {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        deposit_id,
        destination: BackingDepositInput {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(50), // Different max_borrow_ltv
            epoch_start_time: None,
        },
        amount: None,
        user: None,
        epoch_start_time: 0,
    };
    let result = execute(deps.as_mut(), env.clone(), info, msg);
    if let Err(e) = &result {
        println!("Move failed: {:?}", e);
    }
    result.unwrap();

    // Check source group effective total (should be 0)
    let queue = LTV_QUEUES.load(&deps.storage, "uusd".to_string()).unwrap();
    let slot = queue.slots.iter().find(|s| s.ltv == Decimal::percent(60)).unwrap();
    let source_group = slot.deposit_groups.iter().find(|g| g.max_borrow_ltv == Decimal::percent(40)).unwrap();
    let source_effective = source_group.total_unused_locked_vault_tokens;
    println!("Source effective after move (40%): {}", source_effective);
    println!("Source total_locked after move (40%): {}", source_group.total_locked_vault_tokens);
    println!("Source total_vault_tokens after move (40%): {}", source_group.total_vault_tokens);

    // Check destination group effective total (should be > 0)
    let dest_group = slot.deposit_groups.iter().find(|g| g.max_borrow_ltv == Decimal::percent(50)).unwrap();
    let dest_effective = dest_group.total_unused_locked_vault_tokens;
    println!("Dest effective after move (50%): {}", dest_effective);
    println!("Dest total_locked after move (50%): {}", dest_group.total_locked_vault_tokens);
    println!("Dest total_vault_tokens after move (50%): {}", dest_group.total_vault_tokens);

    assert_eq!(source_effective, Uint128::zero(), "Source group should have zero effective total");
    assert!(dest_effective > Uint128::zero(), "Destination group should have effective total");
}
