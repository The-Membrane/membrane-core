use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{coin, coins, from_binary, Decimal, Uint128};
use membrane::transmuter::{ExecuteMsg, InstantiateMsg, QueryMsg, Config};
use crate::contract::{execute, instantiate, query};
use crate::state::{LOCKED_VAULT_TOKENS, VAULT_TOKEN_SUPPLY};

// Helper to get contract address from deps
fn get_contract_addr(deps: &cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>) -> String {
    deps.as_ref().api.addr_validate("cosmos2contract").unwrap().to_string()
}

const SECONDS_PER_DAY: u64 = 86400;

fn instantiate_contract() -> (cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>, cosmwasm_std::Env) {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let msg = InstantiateMsg {
        owner: Some("owner".to_string()),
        tokenfactory_contract: None,
        cdp_contract: "cdp".to_string(),
        discounts_contract: "discounts".to_string(),
        vault_subdenom: "vt".to_string(),
        deposit_pair: membrane::transmuter::AssetPair { 
            cdt: "cdt".to_string(), 
            paired_asset: "usdc".to_string() 
        },
        composition_leeway: Decimal::percent(5),
        asset_a_to_b_rate: Decimal::one(),
        cdt_target_ratio: Decimal::zero(),
        usage_fee: Some(Decimal::zero()),
        swap_history_cap: 50,
        volume_history_cap: 50,
        rate_limit_window_secs: Some(60),
        rate_limit_threshold: Some(Decimal::percent(10)),
        revenue_distributor_addr: Some("rev".to_string()),
        revenue_distributions: None,
        allowlist: None,
        allowlist_rate_limit_threshold: None,
        global_rate_limit_window_secs: Some(3600),
        global_rate_limit_threshold: Some(Decimal::percent(10)),
        lock_ceiling: 365,
        affiliate_fee: Decimal::percent(1),
        send_swap_fee: Some(false),
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
    
    // Get vault tokens minted
    let vault_supply = VAULT_TOKEN_SUPPLY.load(deps.as_ref().storage).unwrap();
    assert!(vault_supply > Uint128::zero());
    
    // Update contract balances: underlying assets + vault tokens
    let contract_addr = get_contract_addr(&deps);
    deps.querier.update_balance(
        &contract_addr,
        vec![
            coin(10000, "cdt"),  // Underlying assets from deposit
            coin(10000, "usdc"), // Underlying assets from deposit
            coin(vault_supply.u128(), "factory/cosmos2contract/vt"), // Vault tokens minted to contract
        ],
    );
    
    // Advance time by 50 days (half of lock period)
    env.block.time = env.block.time.plus_seconds(50 * SECONDS_PER_DAY);
    
    // Query locked tokens to verify
    let locked_tokens = LOCKED_VAULT_TOKENS
        .may_load(deps.as_ref().storage, "user1".to_string())
        .unwrap()
        .unwrap_or_default();
    assert!(!locked_tokens.is_empty(), "Should have locked tokens");
    let original_locked_amount = locked_tokens[0].amount;
    
    // Unlock vault tokens early - should get 50% back (with 50% loss as fee)
    let info = mock_info("user1", &[]);
    let msg = ExecuteMsg::UnlockVaultTokens {
        amount: None, // Unlock all available
    };
    let result = execute(deps.as_mut(), env.clone(), info, msg);
    if let Err(e) = &result {
        println!("Early unlock error: {:?}", e);
    }
    assert!(result.is_ok(), "Early unlock should succeed: {:?}", result.err());
    
    // Calculate how much user should have received (50% of original locked amount)
    // Update user's balance with unlocked tokens
    let user_unlocked = original_locked_amount / Uint128::new(2); // ~50% returned
    let user1_addr = deps.as_ref().api.addr_validate("user1").unwrap();
    deps.querier.update_balance(
        &user1_addr,
        vec![coin(user_unlocked.u128(), "factory/cosmos2contract/vt")],
    );
    
    // Update contract balance: underlying assets unchanged, only fee vault tokens remain
    let contract_locked = LOCKED_VAULT_TOKENS
        .may_load(deps.as_ref().storage, contract_addr.clone())
        .unwrap()
        .unwrap_or_default();
    let contract_fee_total: Uint128 = contract_locked.iter()
        .filter(|t| t.intended_lock_days == 36500)
        .map(|t| t.amount)
        .sum();
    deps.querier.update_balance(
        &contract_addr,
        vec![
            coin(10000, "cdt"),  // Underlying assets still in contract
            coin(10000, "usdc"), // Underlying assets still in contract
            coin(contract_fee_total.u128(), "factory/cosmos2contract/vt"), // Only fee vault tokens
        ],
    );
    
    // Verify locked tokens are updated
    let locked_tokens_after = LOCKED_VAULT_TOKENS
        .may_load(deps.as_ref().storage, "user1".to_string())
        .unwrap()
        .unwrap_or_default();
    assert!(locked_tokens_after.is_empty(), "All locked tokens should be unlocked");
    
    // Verify contract has a locked vault token entry with the lost amount (fee)
    assert!(contract_fee_total > Uint128::zero(), "Contract should have accumulated fees");
    
    // Now exit the unlocked tokens
    let user_balance = deps.as_ref().querier.query_balance(
        &deps.as_ref().api.addr_validate("user1").unwrap(),
        "factory/cosmos2contract/vt"
    ).unwrap().amount;
    assert!(user_balance > Uint128::zero(), "User should have unlocked tokens");
    
    let info = mock_info("user1", &[coin(user_balance.u128(), "factory/cosmos2contract/vt")]);
    let msg = ExecuteMsg::ExitVault {
        recipient: None,
        withdraw_as: None,
    };
    let result = execute(deps.as_mut(), env.clone(), info, msg);
    assert!(result.is_ok(), "Exit unlocked tokens should succeed: {:?}", result.err());
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
    
    // Get vault tokens minted
    let vault_supply = VAULT_TOKEN_SUPPLY.load(deps.as_ref().storage).unwrap();
    
    // Update contract balances: underlying assets + vault tokens
    let contract_addr = get_contract_addr(&deps);
    deps.querier.update_balance(
        &contract_addr,
        vec![
            coin(10000, "cdt"),  // Underlying assets from deposit
            coin(10000, "usdc"), // Underlying assets from deposit
            coin(vault_supply.u128(), "factory/cosmos2contract/vt"), // Vault tokens minted to contract
        ],
    );
    
    // Advance time past expiration (101 days)
    env.block.time = env.block.time.plus_seconds(101 * SECONDS_PER_DAY);
    
    // Get locked tokens before unlock
    let locked_tokens = LOCKED_VAULT_TOKENS
        .may_load(deps.as_ref().storage, "user1".to_string())
        .unwrap()
        .unwrap_or_default();
    let original_locked_amount = locked_tokens[0].amount;
    
    // Unlock vault tokens after expiration - should get 100% back (no loss)
    let info = mock_info("user1", &[]);
    let msg = ExecuteMsg::UnlockVaultTokens {
        amount: None, // Unlock all available
    };
    let result = execute(deps.as_mut(), env.clone(), info, msg);
    assert!(result.is_ok(), "Unlock after expiration should succeed: {:?}", result.err());
    
    // Update user's balance with unlocked tokens (100% since expired)
    let user1_addr = deps.as_ref().api.addr_validate("user1").unwrap();
    deps.querier.update_balance(
        &user1_addr,
        vec![coin(original_locked_amount.u128(), "factory/cosmos2contract/vt")],
    );
    
    // Update contract balance: underlying assets unchanged, no fee tokens (expired lock)
    deps.querier.update_balance(
        &contract_addr,
        vec![
            coin(10000, "cdt"),  // Underlying assets still in contract
            coin(10000, "usdc"), // Underlying assets still in contract
        ],
    );
    
    // Verify no fees were charged (lock expired)
    let contract_addr = get_contract_addr(&deps);
    let contract_locked = LOCKED_VAULT_TOKENS
        .may_load(deps.as_ref().storage, contract_addr.clone())
        .unwrap()
        .unwrap_or_default();
    let contract_fee_total: Uint128 = contract_locked.iter()
        .filter(|t| t.intended_lock_days == 36500)
        .map(|t| t.amount)
        .sum();
    assert_eq!(contract_fee_total, Uint128::zero(), "No fees should be charged for expired locks");
    
    // Now exit the unlocked tokens
    let user_balance = deps.as_ref().querier.query_balance(
        &deps.as_ref().api.addr_validate("user1").unwrap(),
        "factory/cosmos2contract/vt"
    ).unwrap().amount;
    
    let info = mock_info("user1", &[coin(user_balance.u128(), "factory/cosmos2contract/vt")]);
    let msg = ExecuteMsg::ExitVault {
        recipient: None,
        withdraw_as: None,
    };
    let result = execute(deps.as_mut(), env.clone(), info, msg);
    assert!(result.is_ok(), "Exit unlocked tokens should succeed");
    
    // Verify all tokens were burned
    let remaining_supply = VAULT_TOKEN_SUPPLY.load(deps.as_ref().storage).unwrap();
    assert_eq!(remaining_supply, Uint128::zero(), "All tokens should be burned after full withdrawal");
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
    
    // Get vault tokens minted and update contract balance
    let vault_supply = VAULT_TOKEN_SUPPLY.load(deps.as_ref().storage).unwrap();
    let contract_addr = get_contract_addr(&deps);
    deps.querier.update_balance(
        &contract_addr,
        vec![coin(vault_supply.u128(), "factory/cosmos2contract/vt")],
    );
    
    // Get locked tokens
    let locked_tokens_before = LOCKED_VAULT_TOKENS
        .may_load(deps.as_ref().storage, "user1".to_string())
        .unwrap()
        .unwrap_or_default();
    assert!(!locked_tokens_before.is_empty());
    let total_locked = locked_tokens_before[0].amount;
    
    // Calculate expected return (50% of total)
    let expected_return = total_locked / Uint128::new(2);
    let partial_unlock = expected_return / Uint128::new(2); // Unlock 25% of original
    
    // Unlock partial amount
    let info = mock_info("user1", &[]);
    let msg = ExecuteMsg::UnlockVaultTokens {
        amount: Some(partial_unlock),
    };
    let result = execute(deps.as_mut(), env.clone(), info, msg);
    assert!(result.is_ok(), "Partial unlock should succeed: {:?}", result.err());
    
    // Verify some tokens are still locked
    let locked_tokens_after = LOCKED_VAULT_TOKENS
        .may_load(deps.as_ref().storage, "user1".to_string())
        .unwrap()
        .unwrap_or_default();
    assert!(!locked_tokens_after.is_empty(), "Some tokens should still be locked");
    let remaining_locked: Uint128 = locked_tokens_after.iter().map(|t| t.amount).sum();
    assert!(remaining_locked < total_locked, "Remaining locked should be less than original");
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
    
    // Update contract balances: underlying assets (from deposit) + vault tokens (minted to contract)
    let vault_supply_before_unlock = VAULT_TOKEN_SUPPLY.load(deps.as_ref().storage).unwrap();
    let contract_addr = get_contract_addr(&deps);
    deps.querier.update_balance(
        &contract_addr,
        vec![
            coin(10000, "cdt"),  // Underlying assets from user1 deposit
            coin(10000, "usdc"), // Underlying assets from user1 deposit
            coin(vault_supply_before_unlock.u128(), "factory/cosmos2contract/vt"), // Vault tokens minted to contract
        ],
    );
    
    // Advance time by 50 days
    env.block.time = env.block.time.plus_seconds(50 * SECONDS_PER_DAY);
    
    // Unlock early - should charge fee (50% fulfilled = 50% returned, 50% fee)
    let info = mock_info("user1", &[]);
    let msg = ExecuteMsg::UnlockVaultTokens {
        amount: None, // Unlock all available
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    
    // After unlock: user gets ~50% of vault tokens, contract keeps ~50% as fee
    // Update contract balance to only have the fee portion (locked tokens)
    let contract_locked = LOCKED_VAULT_TOKENS
        .may_load(deps.as_ref().storage, contract_addr.clone())
        .unwrap()
        .unwrap_or_default();
    let fee_entry_1: Uint128 = contract_locked.iter()
        .filter(|t| t.intended_lock_days == 36500)
        .map(|t| t.amount)
        .sum();
    assert!(fee_entry_1 > Uint128::zero(), "First unlock should create fee entry");
    
    // After unlock: user gets ~50% of vault tokens, contract keeps ~50% as fee
    // Underlying assets are still in contract (backing all vault tokens)
    // Update contract balances: underlying assets unchanged + only fee vault tokens
    deps.querier.update_balance(
        &contract_addr,
        vec![
            coin(10000, "cdt"),  // Underlying assets still in contract
            coin(10000, "usdc"), // Underlying assets still in contract
            coin(fee_entry_1.u128(), "factory/cosmos2contract/vt"), // Only fee vault tokens
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
    
    // After user2 enters: contract now has user1's assets + user2's assets
    // Update contract balances with new underlying assets and vault tokens
    let vault_supply_after_user2 = VAULT_TOKEN_SUPPLY.load(deps.as_ref().storage).unwrap();
    deps.querier.update_balance(
        &contract_addr,
        vec![
            coin(20000, "cdt"),  // user1's 10000 + user2's 10000
            coin(20000, "usdc"), // user1's 10000 + user2's 10000
            coin(vault_supply_after_user2.u128(), "factory/cosmos2contract/vt"), // All vault tokens (fee + user2's locked)
        ],
    );
    
    // Advance time by 50 days
    env.block.time = env.block.time.plus_seconds(50 * SECONDS_PER_DAY);
    
    // Unlock early - should add to same fee entry
    let info = mock_info("user2", &[]);
    let msg = ExecuteMsg::UnlockVaultTokens {
        amount: None, // Unlock all available
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    
    // Verify fee entry accumulated (should be larger)
    let contract_locked_after = LOCKED_VAULT_TOKENS
        .may_load(deps.as_ref().storage, contract_addr)
        .unwrap()
        .unwrap_or_default();
    let fee_entry_2: Uint128 = contract_locked_after.iter()
        .filter(|t| t.intended_lock_days == 36500)
        .map(|t| t.amount)
        .sum();
    assert!(fee_entry_2 > fee_entry_1, "Fee entry should accumulate across multiple unlocks");
    
    // Verify there's only one fee entry
    let fee_entries_count = contract_locked_after.iter()
        .filter(|t| t.intended_lock_days == 36500)
        .count();
    assert_eq!(fee_entries_count, 1, "Should have only one fee entry");
}

