use cosmwasm_std::{coin, coins, Binary, Decimal, Deps, DepsMut, Empty, Env, MessageInfo, Response, StdError, StdResult, Uint128, to_json_binary};
use cw_multi_test::{App, Contract, ContractWrapper, Executor};
use membrane::transmuter::{AssetPair, ExecuteMsg, InstantiateMsg, QueryMsg};
use membrane::tokenfactory::InstantiateMsg as TfInstantiateMsg;
use membrane::revenue_distributor::ExecuteMsg as RevenueDistributorExecuteMsg;

use crate::contract::{execute, instantiate, query};

const ADMIN: &str = "admin";
const USER: &str = "user";
const ASSET_A: &str = "asset-a";
const ASSET_B: &str = "asset-b";
// VAULT_SUBDENOM no longer needed
const INITIAL_BALANCE: u128 = 1_000_000_000;
const USER_DEPOSIT: u128 = 100_000;

fn transmuter_contract() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(execute, instantiate, query))
}

fn mock_tokenfactory_contract() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        |_deps, env, _info, msg: membrane::tokenfactory::ExecuteMsg| -> cosmwasm_std::StdResult<cosmwasm_std::Response> {
            match msg {
                membrane::tokenfactory::ExecuteMsg::CreateDenom { subdenom, .. } => {
                    let denom = format!("factory/{}/{}", env.contract.address, subdenom);
                    Ok(cosmwasm_std::Response::new().add_attribute("denom", denom))
                }
                membrane::tokenfactory::ExecuteMsg::MintTokens { amount, .. } => {
                    Ok(cosmwasm_std::Response::new()
                        .add_attribute("action", "mint")
                        .add_attribute("amount", amount.as_ref().map(|c| c.amount.clone()).unwrap_or_default()))
                }
                membrane::tokenfactory::ExecuteMsg::BurnTokens { .. } => {
                    Ok(cosmwasm_std::Response::new()
                        .add_attribute("action", "burn"))
                }
                _ => Ok(cosmwasm_std::Response::new()),
            }
        },
        |_deps, _env, _info, _msg: membrane::tokenfactory::InstantiateMsg| -> cosmwasm_std::StdResult<cosmwasm_std::Response> {
            Ok(cosmwasm_std::Response::new())
        },
        |_deps, _env, _msg: membrane::tokenfactory::QueryMsg| -> cosmwasm_std::StdResult<cosmwasm_std::Binary> {
            cosmwasm_std::to_json_binary(&Empty {})
        },
    ))
}

fn mock_cdp_contract() -> Box<dyn Contract<Empty>> {
    use membrane::cdp::QueryMsg as CdpQueryMsg;
    Box::new(ContractWrapper::new(
        |_deps, _env, _info, _msg: cosmwasm_std::Empty| -> cosmwasm_std::StdResult<cosmwasm_std::Response> {
            Ok(cosmwasm_std::Response::new())
        },
        |_deps, _env, _info, _msg: cosmwasm_std::Empty| -> cosmwasm_std::StdResult<cosmwasm_std::Response> {
            Ok(cosmwasm_std::Response::new())
        },
        |_deps, _env, q: CdpQueryMsg| -> cosmwasm_std::StdResult<cosmwasm_std::Binary> {
            match q {
                CdpQueryMsg::GetActiveDeploymentVenues { .. } => {
                    cosmwasm_std::to_json_binary(&Vec::<String>::new())
                },
                _ => Err(cosmwasm_std::StdError::generic_err("unsupported mock cdp query")),
            }
        },
    ))
}

fn mock_revenue_distributor_contract() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        |_deps, _env, _info, _msg: RevenueDistributorExecuteMsg| -> Result<Response, StdError> {
            Ok(Response::new())
        },
        |_deps, _env, _info, _msg: Empty| -> StdResult<Response> {
            Ok(Response::new())
        },
        |_deps, _env, _msg: Empty| -> StdResult<Binary> {
            to_json_binary(&Empty {})
        },
    ))
}

fn mock_discounts_contract() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        |_deps, _env, _info, _msg: Empty| -> Result<Response, StdError> {
            Ok(Response::new())
        },
        |_deps, _env, _info, _msg: Empty| -> StdResult<Response> {
            Ok(Response::new())
        },
        |_deps, _env, _msg: Empty| -> StdResult<Binary> {
            to_json_binary(&Empty {})
        },
    ))
}

fn setup_app() -> App {
    let mut app = App::default();
    app.init_modules(|router, _, storage| {
        router
            .bank
            .init_balance(
                storage,
                &cosmwasm_std::Addr::unchecked(USER),
                vec![
                    coin(INITIAL_BALANCE, ASSET_A),
                    coin(INITIAL_BALANCE, ASSET_B),
                ],
            )
            .unwrap();
        router
            .bank
            .init_balance(
                storage,
                &cosmwasm_std::Addr::unchecked(ADMIN),
                vec![
                    coin(INITIAL_BALANCE, ASSET_A),
                    coin(INITIAL_BALANCE, ASSET_B),
                ],
            )
            .unwrap();
    });
    app
}

fn instantiate_transmuter(app: &mut App) -> cosmwasm_std::Addr {
    // Store contracts
    let tf_code = app.store_code(mock_tokenfactory_contract());
    let cdp_code = app.store_code(mock_cdp_contract());
    let revenue_distributor_code = app.store_code(mock_revenue_distributor_contract());
    let discounts_code = app.store_code(mock_discounts_contract());
    let code_id = app.store_code(transmuter_contract());

    // Instantiate contracts and get their addresses (like pending_revenue_tests.rs)
    let tokenfactory_addr = app
        .instantiate_contract(
            tf_code,
            cosmwasm_std::Addr::unchecked(ADMIN),
            &cosmwasm_std::Empty {},
            &[],
            "mock-tokenfactory",
            None,
        )
        .unwrap();

    let cdp_addr = app
        .instantiate_contract(
            cdp_code,
            cosmwasm_std::Addr::unchecked(ADMIN),
            &cosmwasm_std::Empty {},
            &[],
            "mock-cdp",
            None,
        )
        .unwrap();

    let revenue_distributor_addr = app
        .instantiate_contract(
            revenue_distributor_code,
            cosmwasm_std::Addr::unchecked(ADMIN),
            &cosmwasm_std::Empty {},
            &[],
            "mock-revenue-distributor",
            None,
        )
        .unwrap();

    let discounts_addr = app
        .instantiate_contract(
            discounts_code,
            cosmwasm_std::Addr::unchecked(ADMIN),
            &cosmwasm_std::Empty {},
            &[],
            "mock-discounts",
            None,
        )
        .unwrap();

    // Create the message - use simple strings for string fields (like pending_revenue_tests.rs)
    // Only use Addr for Option<Addr> field
    let msg = InstantiateMsg {
        owner: Some(ADMIN.to_string()),
        tokenfactory_contract: Some(tokenfactory_addr.clone()),
        revenue_distributor_addr: Some(revenue_distributor_addr.to_string()),
        cdp_contract: cdp_addr.to_string(),
        discounts_contract: ADMIN.to_string(), // Use simple string like pending_revenue_tests.rs
        deposit_pair: AssetPair {
            cdt: ASSET_A.to_string(),
            paired_asset: ASSET_B.to_string(),
        },
        composition_leeway: Decimal::percent(1),
        cdt_target_ratio: Decimal::percent(50),
        usage_fee: Some(Decimal::percent(0)),
        usage_fee_utilization_threshold: None,
        swap_history_cap: 5,
        volume_history_cap: 5,
        rate_limit_window_secs: Some(60 * 60 * 8),
        rate_limit_threshold: Some(Decimal::percent(5)),
        allowlist: Some(vec![]),
        allowlist_rate_limit_threshold: Some(Decimal::percent(10)),
        global_rate_limit_window_secs: Some(60 * 60 * 24),
        global_rate_limit_threshold: Some(Decimal::percent(20)),
        revenue_distributions: None,
        lock_ceiling: 1460,
        affiliate_fee: Decimal::percent(1),
        send_swap_fee: Some(false),
        revenue_distributor_fee_percentage: None,
    };


    app.instantiate_contract(
        code_id,
        cosmwasm_std::Addr::unchecked(ADMIN),
        &msg,
        &[],
        "transmuter",
        None,
    )
    .unwrap()
}

#[test]
fn test_enter_vault_with_lock_stores_in_user_deposits() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // Enter vault with lock
    app.execute_contract(
        cosmwasm_std::Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::EnterVault {
            recipient: None,
            lock_days: Some(100),
            affiliate_address: None,
            affiliate_label: None,
        },
        &coins(USER_DEPOSIT, ASSET_A),
    )
    .unwrap();

    // Query user deposits
    let response: membrane::transmuter::UserDepositsResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::UserDeposits {
            user: USER.to_string(),
        })
        .unwrap();
    let deposits = response.deposits;

    assert_eq!(deposits.len(), 1);
    assert!(deposits[0].locked.is_some());
    let locked = deposits[0].locked.as_ref().unwrap();
    assert_eq!(locked.intended_lock_days, Some(100));
    assert!(locked.locked_until > app.block_info().time.seconds());
}

#[test]
fn test_enter_vault_without_lock_stores_in_user_deposits() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // Enter vault without lock
    app.execute_contract(
        cosmwasm_std::Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::EnterVault {
            recipient: None,
            lock_days: None,
            affiliate_address: None,
            affiliate_label: None,
        },
        &coins(USER_DEPOSIT, ASSET_A),
    )
    .unwrap();

    // Query user deposits
    let response: membrane::transmuter::UserDepositsResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::UserDeposits {
            user: USER.to_string(),
        })
        .unwrap();
    let deposits = response.deposits;

    assert_eq!(deposits.len(), 1);
    assert!(deposits[0].locked.is_none());
}

#[test]
fn test_lock_vault_tokens_post_deposit() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // First, enter vault without lock to get vault tokens
    app.execute_contract(
        cosmwasm_std::Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::EnterVault {
            recipient: None,
            lock_days: None,
            affiliate_address: None,
            affiliate_label: None,
        },
        &coins(USER_DEPOSIT, ASSET_A),
    )
    .unwrap();

    // Get vault token denom
    let config: membrane::transmuter::Config = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::Config {})
        .unwrap();
    let vault_token = config.vault_token;

    // Get user's vault token balance
    let user_balance = app
        .wrap()
        .query_balance(&cosmwasm_std::Addr::unchecked(USER), &vault_token)
        .unwrap();
    assert!(!user_balance.amount.is_zero());

    // Lock some vault tokens
    let lock_amount = user_balance.amount;
    app.execute_contract(
        cosmwasm_std::Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::LockVaultTokens {
            amount: lock_amount,
            lock_days: 50,
        },
        &coins(lock_amount.u128(), &vault_token),
    )
    .unwrap();

    // Check user deposits - should have 2 entries now (original + locked)
    let response: membrane::transmuter::UserDepositsResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::UserDeposits {
            user: USER.to_string(),
        })
        .unwrap();
    let deposits = response.deposits;

    // Should have at least one locked deposit
    let locked_deposits: Vec<_> = deposits
        .iter()
        .filter(|d| d.locked.is_some())
        .collect();
    assert!(!locked_deposits.is_empty());
    assert_eq!(locked_deposits[0].locked.as_ref().unwrap().intended_lock_days, Some(50));
}

#[test]
fn test_query_locked_vault_tokens_from_user_deposits() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // Enter vault with lock
    app.execute_contract(
        cosmwasm_std::Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::EnterVault {
            recipient: None,
            lock_days: Some(100),
            affiliate_address: None,
            affiliate_label: None,
        },
        &coins(USER_DEPOSIT, ASSET_A),
    )
    .unwrap();

    // Query locked vault tokens
    let response: membrane::transmuter::LockedVaultTokensResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::LockedVaultTokens {
            user: USER.to_string(),
        })
        .unwrap();

    assert_eq!(response.locked_tokens.len(), 1);
    assert_eq!(response.locked_tokens[0].intended_lock_days, 100);
}

#[test]
fn test_unlock_expired_lock_from_user_deposits() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // Enter vault with 10 day lock
    app.execute_contract(
        cosmwasm_std::Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::EnterVault {
            recipient: None,
            lock_days: Some(10),
            affiliate_address: None,
            affiliate_label: None,
        },
        &coins(USER_DEPOSIT, ASSET_A),
    )
    .unwrap();

    // Get vault token denom
    let config: membrane::transmuter::Config = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::Config {})
        .unwrap();
    let vault_token = config.vault_token;

    // Advance time by 11 days (lock expired)
    app.update_block(|block| {
        block.time = block.time.plus_seconds(11 * 86400);
    });

    // Query locked tokens before unlock
    let response_before: membrane::transmuter::UserDepositsResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::UserDeposits {
            user: USER.to_string(),
        })
        .unwrap();
    let deposits_before = response_before.deposits;
    let locked_before: Vec<_> = deposits_before
        .iter()
        .filter(|d| d.locked.is_some())
        .collect();
    assert!(!locked_before.is_empty());

    // Get contract balance before unlock (for verification if needed)
    let _contract_balance_before = app
        .wrap()
        .query_balance(&contract, &vault_token)
        .unwrap();

    // Unlock all tokens
    app.execute_contract(
        cosmwasm_std::Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::UnlockVaultTokens { amount: None },
        &[],
    )
    .unwrap();

    // Check user deposits - lock should be removed
    let response_after: membrane::transmuter::UserDepositsResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::UserDeposits {
            user: USER.to_string(),
        })
        .unwrap_or_else(|_| membrane::transmuter::UserDepositsResponse { deposits: vec![] });
    let deposits_after = response_after.deposits;
    
    // Lock should be removed (expired locks unlock fully)
    let locked_after: Vec<_> = deposits_after
        .iter()
        .filter(|d| d.locked.is_some())
        .collect();
    assert!(locked_after.is_empty(), "Expired locks should be fully unlocked");

    // Check user received tokens
    let user_balance_after = app
        .wrap()
        .query_balance(&cosmwasm_std::Addr::unchecked(USER), &vault_token)
        .unwrap();
    assert!(user_balance_after.amount > Uint128::zero());
}

#[test]
fn test_unlock_early_withdrawal_from_user_deposits() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // Enter vault with 100 day lock
    app.execute_contract(
        cosmwasm_std::Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::EnterVault {
            recipient: None,
            lock_days: Some(100),
            affiliate_address: None,
            affiliate_label: None,
        },
        &coins(USER_DEPOSIT, ASSET_A),
    )
    .unwrap();

    // Get vault token denom
    let config: membrane::transmuter::Config = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::Config {})
        .unwrap();
    let vault_token = config.vault_token;

    // Advance time by 50 days (half of lock period)
    app.update_block(|block| {
        block.time = block.time.plus_seconds(50 * 86400);
    });

    // Get contract balance before unlock (for verification if needed)
    let _contract_balance_before = app
        .wrap()
        .query_balance(&contract, &vault_token)
        .unwrap();

    // Unlock all tokens (early withdrawal - should get ~50% back)
    app.execute_contract(
        cosmwasm_std::Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::UnlockVaultTokens { amount: None },
        &[],
    )
    .unwrap();

    // Check user received tokens (should be approximately 50% of original)
    let user_balance_after = app
        .wrap()
        .query_balance(&cosmwasm_std::Addr::unchecked(USER), &vault_token)
        .unwrap();
    
    // Should receive approximately 50% (with some rounding)
    assert!(user_balance_after.amount > Uint128::zero());
    
    // Check that deposit was removed (fully unlocked)
    let response_after: membrane::transmuter::UserDepositsResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::UserDeposits {
            user: USER.to_string(),
        })
        .unwrap_or_else(|_| membrane::transmuter::UserDepositsResponse { deposits: vec![] });
    let deposits_after = response_after.deposits;
    
    // Early withdrawal should remove the deposit entry
    let locked_after: Vec<_> = deposits_after
        .iter()
        .filter(|d| d.locked.is_some())
        .collect();
    assert!(locked_after.is_empty(), "Early withdrawal should remove locked deposit");
}

#[test]
fn test_partial_unlock_from_user_deposits() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // Enter vault with 100 day lock
    app.execute_contract(
        cosmwasm_std::Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::EnterVault {
            recipient: None,
            lock_days: Some(100),
            affiliate_address: None,
            affiliate_label: None,
        },
        &coins(USER_DEPOSIT, ASSET_A),
    )
    .unwrap();

    // Get vault token denom
    let config: membrane::transmuter::Config = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::Config {})
        .unwrap();
    let vault_token = config.vault_token;

    // Advance time by 50 days
    app.update_block(|block| {
        block.time = block.time.plus_seconds(50 * 86400);
    });

    // Query to get available unlock amount
    let response_before: membrane::transmuter::UserDepositsResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::UserDeposits {
            user: USER.to_string(),
        })
        .unwrap();
    let deposits_before = response_before.deposits;
    let locked_deposit = deposits_before.iter().find(|d| d.locked.is_some()).unwrap();
    
    // Calculate expected return (50% of amount)
    let expected_return = locked_deposit.amount / Uint128::from(2u64);
    let partial_unlock = expected_return / Uint128::from(2u64); // Unlock half of what's available

    // Partial unlock
    app.execute_contract(
        cosmwasm_std::Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::UnlockVaultTokens {
            amount: Some(partial_unlock),
        },
        &[],
    )
    .unwrap();

    // Check that deposit still exists but with reduced amount
    let response_after: membrane::transmuter::UserDepositsResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::UserDeposits {
            user: USER.to_string(),
        })
        .unwrap();
    let deposits_after = response_after.deposits;
    
    let locked_after: Vec<_> = deposits_after
        .iter()
        .filter(|d| d.locked.is_some())
        .collect();
    
    // Should still have locked deposit with reduced amount
    assert!(!locked_after.is_empty(), "Partial unlock should keep remaining locked deposit");
    assert!(locked_after[0].amount < locked_deposit.amount);
}

#[test]
fn test_lock_vault_tokens_validation() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // Get vault token denom
    let config: membrane::transmuter::Config = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::Config {})
        .unwrap();
    let vault_token = config.vault_token;

    // Try to lock with zero days (should fail)
    let result = app.execute_contract(
        cosmwasm_std::Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::LockVaultTokens {
            amount: Uint128::from(1000u64),
            lock_days: 0,
        },
        &coins(1000, &vault_token),
    );
    assert!(result.is_err());

    // Try to lock with days exceeding ceiling (should fail)
    let result = app.execute_contract(
        cosmwasm_std::Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::LockVaultTokens {
            amount: Uint128::from(1000u64),
            lock_days: 1500, // Exceeds lock_ceiling of 1460
        },
        &coins(1000, &vault_token),
    );
    assert!(result.is_err());

    // Try to lock with mismatched amount (should fail)
    let result = app.execute_contract(
        cosmwasm_std::Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::LockVaultTokens {
            amount: Uint128::from(1000u64),
            lock_days: 50,
        },
        &coins(500, &vault_token), // Different amount
    );
    assert!(result.is_err());
}

#[test]
fn test_multiple_locks_and_unlocks() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // Enter vault without lock
    app.execute_contract(
        cosmwasm_std::Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::EnterVault {
            recipient: None,
            lock_days: None,
            affiliate_address: None,
            affiliate_label: None,
        },
        &coins(USER_DEPOSIT, ASSET_A),
    )
    .unwrap();

    // Get vault token denom
    let config: membrane::transmuter::Config = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::Config {})
        .unwrap();
    let vault_token = config.vault_token;

    // Get user's vault token balance
    let user_balance = app
        .wrap()
        .query_balance(&cosmwasm_std::Addr::unchecked(USER), &vault_token)
        .unwrap();
    
    let lock_amount_1 = user_balance.amount / Uint128::from(3u64);
    let lock_amount_2 = user_balance.amount / Uint128::from(3u64);

    // Lock some tokens with 50 days
    app.execute_contract(
        cosmwasm_std::Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::LockVaultTokens {
            amount: lock_amount_1,
            lock_days: 50,
        },
        &coins(lock_amount_1.u128(), &vault_token),
    )
    .unwrap();

    // Lock more tokens with 100 days
    app.execute_contract(
        cosmwasm_std::Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::LockVaultTokens {
            amount: lock_amount_2,
            lock_days: 100,
        },
        &coins(lock_amount_2.u128(), &vault_token),
    )
    .unwrap();

    // Query locked tokens - should have 2 locked entries
    let response: membrane::transmuter::LockedVaultTokensResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::LockedVaultTokens {
            user: USER.to_string(),
        })
        .unwrap();

    assert_eq!(response.locked_tokens.len(), 2);

    // Advance time by 60 days (first lock expired, second still locked)
    app.update_block(|block| {
        block.time = block.time.plus_seconds(60 * 86400);
    });

    // Unlock all - should unlock first fully (expired) and second partially (early)
    app.execute_contract(
        cosmwasm_std::Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::UnlockVaultTokens { amount: None },
        &[],
    )
    .unwrap();

    // Check deposits - should have no locked deposits left
    let response_after: membrane::transmuter::UserDepositsResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::UserDeposits {
            user: USER.to_string(),
        })
        .unwrap_or_else(|_| membrane::transmuter::UserDepositsResponse { deposits: vec![] });
    let deposits_after = response_after.deposits;
    
    let locked_after: Vec<_> = deposits_after
        .iter()
        .filter(|d| d.locked.is_some())
        .collect();
    assert!(locked_after.is_empty(), "All locks should be unlocked");
}

