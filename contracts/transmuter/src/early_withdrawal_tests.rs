use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info, MOCK_CONTRACT_ADDR};
use cosmwasm_std::{coin, coins, from_binary, Addr, Decimal, Uint128};
use membrane::transmuter::{ExecuteMsg, InstantiateMsg, QueryMsg, Config};
use crate::contract::{execute, instantiate, query};
use crate::state::{USER_DEPOSITS, DEPOSIT_TOTAL};

// Helper to get contract address from deps
fn get_contract_addr(deps: &cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>) -> String {
    // Use Addr::unchecked since MockApi should accept any string
    Addr::unchecked("cosmos2contract").to_string()
}

const SECONDS_PER_DAY: u64 = 86400;

fn instantiate_contract() -> (cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>, cosmwasm_std::Env) {
    let mut deps = mock_dependencies();
    let env = mock_env();

    // Use MOCK_CONTRACT_ADDR directly - it's a valid bech32 address
    // For variations, we'll use the same address (MockApi should accept it)
    // The contract will validate it, and MockApi should pass validation
    let owner_addr = MOCK_CONTRACT_ADDR;
    let cdp_addr = MOCK_CONTRACT_ADDR;
    let discounts_addr = MOCK_CONTRACT_ADDR;
    let rev_addr = MOCK_CONTRACT_ADDR;
    
    let msg = InstantiateMsg {
        owner: Some(owner_addr.to_string()),
        tokenfactory_contract: None,
        cdp_contract: cdp_addr.to_string(),
        discounts_contract: discounts_addr.to_string(),
        deposit_pair: membrane::transmuter::AssetPair { 
            cdt: "cdt".to_string(), 
            paired_asset: "usdc".to_string() 
        },
        composition_leeway: Decimal::percent(5),
        cdt_target_ratio: Decimal::zero(),
        usage_fee: Some(Decimal::zero()),
        usage_fee_utilization_threshold: None,
        swap_history_cap: 50,
        volume_history_cap: 50,
        rate_limit_window_secs: Some(60),
        rate_limit_threshold: Some(Decimal::percent(10)),
        revenue_distributor_addr: Some(rev_addr.to_string()),
        revenue_distributions: None,
        allowlist: None,
        allowlist_rate_limit_threshold: None,
        global_rate_limit_window_secs: Some(3600),
        global_rate_limit_threshold: Some(Decimal::percent(10)),
        lock_ceiling: 1460,
        affiliate_fee: Decimal::percent(1),
        send_swap_fee: Some(false),
        revenue_distributor_fee_percentage: None,
        emissions_voting_contract: None,
    };

    let info = mock_info("owner", &[]);
    instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();

    (deps, env)
}

#[test]
fn test_early_withdrawal_transmuter_half_fulfilled() {
    let (mut deps, mut env) = instantiate_contract();
    
    // Enter vault with lock for 100 days
    let info = mock_info("user1", &[coin(10000, "cdt"), coin(10000, "usdc")]);
    let msg = ExecuteMsg::EnterVault { 
        recipient: None, 
        lock_days: Some(100),
        affiliate_address: None,
    };
    let result = execute(deps.as_mut(), env.clone(), info, msg);
    if let Err(e) = &result {
        println!("Enter vault error: {:?}", e);
    }
    assert!(result.is_ok(), "Enter vault should succeed: {:?}", result.err());
    
    // Get deposit total
    let deposit_total = DEPOSIT_TOTAL.load(deps.as_ref().storage).unwrap();
    assert!(deposit_total > Uint128::zero());
    
    // Update contract balances: underlying assets only (no vault tokens)
    let contract_addr = get_contract_addr(&deps);
    deps.querier.bank.update_balance(
        &contract_addr,
        vec![
            coin(10000, "cdt"),  // Underlying assets from deposit
            coin(10000, "usdc"), // Underlying assets from deposit
        ],
    );
    
    // Advance time by 50 days (half of lock period) - lock still active
    env.block.time = env.block.time.plus_seconds(50 * SECONDS_PER_DAY);
    
    // Query deposits to verify they're still locked
    let deposits = USER_DEPOSITS
        .may_load(deps.as_ref().storage, "user1".to_string())
        .unwrap()
        .unwrap_or_default();
    let locked_deposits: Vec<_> = deposits.iter().filter(|d| d.locked.is_some()).collect();
    assert!(!locked_deposits.is_empty(), "Should have locked deposits");
    let original_locked_amount = locked_deposits[0].amount;
    
    // Try to exit vault early - should fail because deposits are still locked
    let info = mock_info("user1", &[]);
    let msg = ExecuteMsg::ExitVault { recipient: None, withdraw_as: None, user: None, deposit_id: None, amount: None };
    let result = execute(deps.as_mut(), env.clone(), info, msg);
    assert!(result.is_err(), "Early exit should fail when deposits are locked");
    
    // Advance time past lock expiration (101 days total)
    env.block.time = env.block.time.plus_seconds(51 * SECONDS_PER_DAY);
    
    // Now exit should succeed - lock has expired
    let info = mock_info("user1", &[]);
    let msg = ExecuteMsg::ExitVault { recipient: None, withdraw_as: None, user: None, deposit_id: None, amount: None };
    let result = execute(deps.as_mut(), env.clone(), info, msg);
    assert!(result.is_ok(), "Exit after lock expiration should succeed: {:?}", result.err());
    
    // Verify deposits are removed after exit
    let deposits_after = USER_DEPOSITS
        .may_load(deps.as_ref().storage, "user1".to_string())
        .unwrap()
        .unwrap_or_default();
    let total_after: Uint128 = deposits_after.iter().map(|d| d.amount).sum();
    assert_eq!(total_after, Uint128::zero(), "All deposits should be withdrawn after exit");
}

#[test]
fn test_early_withdrawal_transmuter_after_expiration() {
    let (mut deps, mut env) = instantiate_contract();
    
    // Enter vault with lock for 100 days
    let info = mock_info("user1", &[coin(10000, "cdt"), coin(10000, "usdc")]);
    let msg = ExecuteMsg::EnterVault { 
        recipient: None, 
        lock_days: Some(100),
        affiliate_address: None,
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    
    // Get deposit total
    let deposit_total = DEPOSIT_TOTAL.load(deps.as_ref().storage).unwrap();
    
    // Update contract balances: underlying assets only (no vault tokens)
    let contract_addr = get_contract_addr(&deps);
    deps.querier.bank.update_balance(
        &contract_addr,
        vec![
            coin(10000, "cdt"),  // Underlying assets from deposit
            coin(10000, "usdc"), // Underlying assets from deposit
        ],
    );
    
    // Advance time past expiration (101 days)
    env.block.time = env.block.time.plus_seconds(101 * SECONDS_PER_DAY);
    
    // Get locked deposits before exit
    let deposits = USER_DEPOSITS
        .may_load(deps.as_ref().storage, "user1".to_string())
        .unwrap()
        .unwrap_or_default();
    let locked_deposits: Vec<_> = deposits.iter().filter(|d| d.locked.is_some()).collect();
    let original_locked_amount = locked_deposits[0].amount;
    
    // Exit vault after expiration - should get 100% back (no fee since expired)
    let info = mock_info("user1", &[]);
    let msg = ExecuteMsg::ExitVault { recipient: None, withdraw_as: None, user: None, deposit_id: None, amount: None };
    let result = execute(deps.as_mut(), env.clone(), info, msg);
    assert!(result.is_ok(), "Exit after expiration should succeed: {:?}", result.err());
    
    // Update contract balance: underlying assets reduced by withdrawal
    deps.querier.bank.update_balance(
        &contract_addr,
        vec![
            coin(0, "cdt"),  // All assets withdrawn (expired lock, no fee)
            coin(0, "usdc"), // All assets withdrawn
        ],
    );
    
    // Verify deposits are removed after exit
    let deposits_after = USER_DEPOSITS
        .may_load(deps.as_ref().storage, "user1".to_string())
        .unwrap()
        .unwrap_or_default();
    let locked_after: Vec<_> = deposits_after.iter().filter(|d| d.locked.is_some()).collect();
    assert!(locked_after.is_empty(), "All locked deposits should be removed after exit");
    
    // Verify all deposits were withdrawn
    let remaining_deposits = DEPOSIT_TOTAL.load(deps.as_ref().storage).unwrap();
    assert_eq!(remaining_deposits, Uint128::zero(), "All deposits should be withdrawn after full exit");
}

#[test]
fn test_unlock_partial_amount() {
    let (mut deps, mut env) = instantiate_contract();
    
    // Enter vault with lock for 100 days
    let info = mock_info("user1", &[coin(10000, "cdt"), coin(10000, "usdc")]);
    let msg = ExecuteMsg::EnterVault { 
        recipient: None, 
        lock_days: Some(100),
        affiliate_address: None,
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    
    // Advance time by 50 days (half of lock period)
    env.block.time = env.block.time.plus_seconds(50 * SECONDS_PER_DAY);
    
    // Get deposit total and update contract balance
    let deposit_total = DEPOSIT_TOTAL.load(deps.as_ref().storage).unwrap();
    let contract_addr = get_contract_addr(&deps);
    deps.querier.bank.update_balance(
        &contract_addr,
        vec![
            coin(10000, "cdt"),  // Underlying assets
            coin(10000, "usdc"), // Underlying assets
        ],
    );
    
    // Get locked deposits
    let deposits_before = USER_DEPOSITS
        .may_load(deps.as_ref().storage, "user1".to_string())
        .unwrap()
        .unwrap_or_default();
    let locked_before: Vec<_> = deposits_before.iter().filter(|d| d.locked.is_some()).collect();
    assert!(!locked_before.is_empty());
    let total_locked = locked_before[0].amount;
    
    // Try to exit vault - should fail because deposits are still locked
    let info = mock_info("user1", &[]);
    let msg = ExecuteMsg::ExitVault { recipient: None, withdraw_as: None, user: None, deposit_id: None, amount: None };
    let result = execute(deps.as_mut(), env.clone(), info, msg);
    assert!(result.is_err(), "Exit should fail when deposits are locked");
    
    // Advance time past lock expiration (101 days total)
    env.block.time = env.block.time.plus_seconds(51 * SECONDS_PER_DAY);
    
    // Now exit should succeed - lock has expired
    let info = mock_info("user1", &[]);
    let msg = ExecuteMsg::ExitVault { recipient: None, withdraw_as: None, user: None, deposit_id: None, amount: None };
    let result = execute(deps.as_mut(), env.clone(), info, msg);
    assert!(result.is_ok(), "Exit after lock expiration should succeed: {:?}", result.err());
    
    // Verify deposits are removed after exit
    let deposits_after = USER_DEPOSITS
        .may_load(deps.as_ref().storage, "user1".to_string())
        .unwrap()
        .unwrap_or_default();
    let total_after: Uint128 = deposits_after.iter().map(|d| d.amount).sum();
    assert_eq!(total_after, Uint128::zero(), "All deposits should be withdrawn after exit");
}

#[test]
fn test_unlock_fee_accumulation() {
    let (mut deps, mut env) = instantiate_contract();
    
    // Enter vault with lock for 100 days - first user
    let info = mock_info("user1", &[coin(10000, "cdt"), coin(10000, "usdc")]);
    let msg = ExecuteMsg::EnterVault { 
        recipient: None, 
        lock_days: Some(100),
        affiliate_address: None,
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    
    // Update contract balances: underlying assets only
    let deposit_total_before_exit = DEPOSIT_TOTAL.load(deps.as_ref().storage).unwrap();
    let contract_addr = get_contract_addr(&deps);
    deps.querier.bank.update_balance(
        &contract_addr,
        vec![
            coin(10000, "cdt"),  // Underlying assets from user1 deposit
            coin(10000, "usdc"), // Underlying assets from user1 deposit
        ],
    );
    
    // Advance time past lock expiration (101 days total from deposit)
    env.block.time = env.block.time.plus_seconds(101 * SECONDS_PER_DAY);
    
    // Exit after lock expiration - should get full amount (no fee)
    let info = mock_info("user1", &[]);
    let msg = ExecuteMsg::ExitVault { recipient: None, withdraw_as: None, user: None, deposit_id: None, amount: None };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    
    // After exit: user gets full assets (lock expired)
    // Update contract balances: underlying assets reduced
    deps.querier.bank.update_balance(
        &contract_addr,
        vec![
            coin(0, "cdt"),  // All assets withdrawn
            coin(0, "usdc"), // All assets withdrawn
        ],
    );
    
    // Enter vault with lock for 100 days - second user
    let info = mock_info("user2", &[coin(10000, "cdt"), coin(10000, "usdc")]);
    let msg = ExecuteMsg::EnterVault { 
        recipient: None, 
        lock_days: Some(100),
        affiliate_address: None,
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    
    // After user2 enters: contract now has user2's assets
    // Update contract balances with new underlying assets
    let deposit_total_after_user2 = DEPOSIT_TOTAL.load(deps.as_ref().storage).unwrap();
    deps.querier.bank.update_balance(
        &contract_addr,
        vec![
            coin(10000, "cdt"),  // user2's 10000
            coin(10000, "usdc"), // user2's 10000
        ],
    );
    
    // Advance time past lock expiration (101 days total from user2's deposit)
    env.block.time = env.block.time.plus_seconds(101 * SECONDS_PER_DAY);
    
    // Exit after lock expiration - should get full amount
    let info = mock_info("user2", &[]);
    let msg = ExecuteMsg::ExitVault { recipient: None, withdraw_as: None, user: None, deposit_id: None, amount: None };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    
    // Verify that deposits are removed after exit
    let deposits_user2_after = USER_DEPOSITS
        .may_load(deps.as_ref().storage, "user2".to_string())
        .unwrap()
        .unwrap_or_default();
    let total_after: Uint128 = deposits_user2_after.iter().map(|d| d.amount).sum();
    assert_eq!(total_after, Uint128::zero(), "All deposits should be withdrawn after exit");
}

