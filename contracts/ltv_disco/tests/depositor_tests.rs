use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{coins, from_json, Decimal, Uint128, to_json_binary, SystemResult, ContractResult, Binary, Addr};
use membrane::ltv_disco::*;
use membrane::types::{cAsset, Asset, AssetInfo, Basket, DepositDenom, PendingRevenue};
use membrane::oracle::PriceResponse;
use ltv_disco::contract::{instantiate, execute, query};
use ltv_disco::state::BACKING_DEPOSITS;
use ltv_disco::execute::make_deposit_key;
use ltv_disco::error::ContractError;

fn setup_mock_basket() -> Basket {
    Basket {
        basket_id: Uint128::new(1),
        current_position_id: Uint128::new(1),
        collateral_types: vec![
            cAsset {
                asset: Asset {
                    info: AssetInfo::NativeToken { denom: "uusd".to_string() },
                    amount: Uint128::zero(),
                },
                max_LTV: Decimal::percent(50),
                max_borrow_LTV: Decimal::percent(30),
                rate_index: Decimal::zero(),
                pool_info: None,
                individual_cost: None,
            },
        ],
        collateral_supply_caps: vec![],
        lastest_collateral_rates: vec![],
        multi_asset_supply_caps: vec![],
        credit_asset: Asset {
            info: AssetInfo::NativeToken { denom: "debt".to_string() },
            amount: Uint128::zero(),
        },
        credit_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6,
        },
        base_interest_rate: Decimal::percent(5),
        pending_revenue: PendingRevenue {
            total_pending: Uint128::zero(),
            per_asset_rev: vec![],
        },
        pending_bad_debt: Uint128::zero(),
        credit_last_accrued: 0,
        rates_last_accrued: 0,
        oracle_set: true,
        negative_rates: false,
        frozen: false,
        distribute_revenue: true,
        cpc_margin_of_error: Decimal::percent(100),
        liq_queue: None,
    }
}

fn instantiate_contract() -> (cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>, cosmwasm_std::Env) {
    let mut deps = mock_dependencies();
    let env = mock_env();
    
    // Setup querier to return mock basket
    let basket = setup_mock_basket();
    deps.querier.update_wasm(move |query| {
        match query {
            cosmwasm_std::WasmQuery::Smart { contract_addr: _, msg } => {
                let parsed: Result<membrane::cdp::QueryMsg, _> = from_json(msg);
                if let Ok(membrane::cdp::QueryMsg::GetBasket {}) = parsed {
                    SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap()))
                } else {
                    SystemResult::Ok(ContractResult::Ok(Binary::default()))
                }
            }
            _ => SystemResult::Ok(ContractResult::Ok(Binary::default())),
        }
    });

    let msg = InstantiateMsg {
        owner: Some("owner".to_string()),
        cdp_contract: "cdp_contract".to_string(),
        deposit_denom: DepositDenom { denom: "uusd".to_string(), vault_info: None },
        cdt_denom: "reward_token".to_string(),
        minimum_deposit: Uint128::new(1000),
        max_ltv: Decimal::percent(80),
        percent_to_disperse: Decimal::percent(10),
        dispersal_window: 2,
        activation_window: 2,
        oracle_contract: "oracle".to_string(),
        chain_proxy_contract: "chain_proxy".to_string(),
        lock_duration_ceiling: Some(365),
        affiliate_fee: Some(Decimal::percent(1)),
        max_management_fee: None,
        ltv_delta_minimum: Some(Decimal::percent(1)),
        emissions_voting_contract: None,
        points_system_contract: None,
        revenue_distributor: None,
        auction_contract: None,
        mbrn_denom: None,
    };

    let info = mock_info("owner", &[]);
    instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();

    (deps, env)
}

#[test]
fn test_self_deposit_has_no_depositor() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    // Self-deposit (no deposit_owner specified)
    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    let info = mock_info("user1", &coins(10000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
        },
    ).unwrap();

    // Check deposit has depositor: None
    let deposit_key = make_deposit_key("uusd", "0.5", "0.3", "user1", &Uint128::one(), 0);
    let deposit = BACKING_DEPOSITS.load(&deps.storage, deposit_key).unwrap();
    assert_eq!(deposit.depositor, None);
    assert_eq!(deposit.withdrawals_enabled, true);
}

#[test]
fn test_deposit_for_other_user_sets_depositor() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    // Deposit for another user
    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    let info = mock_info("depositor1", &coins(10000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: Some("user1".to_string()),
            locked: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
        },
    ).unwrap();

    // Check deposit has depositor set to depositor1
    let deposit_key = make_deposit_key("uusd", "0.5", "0.3", "user1", &Uint128::one(), 0);
    let deposit = BACKING_DEPOSITS.load(&deps.storage, deposit_key).unwrap();
    assert_eq!(deposit.depositor, Some(Addr::unchecked("depositor1")));
    assert_eq!(deposit.withdrawals_enabled, true);
    assert_eq!(deposit.user, Addr::unchecked("user1"));
}

#[test]
fn test_top_up_preserves_depositor() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    // First deposit by depositor1 for user1
    let info = mock_info("depositor1", &coins(10000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: Some("user1".to_string()),
            locked: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
        },
    ).unwrap();

    // Top-up by different depositor (should preserve original depositor)
    let info = mock_info("depositor2", &coins(5000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: Some("user1".to_string()),
            locked: None,
            deposit_id: Some(Uint128::one()),
            manager: None,
            affiliate_address: None,
        },
    ).unwrap();

    // Check deposit still has original depositor
    let deposit_key = make_deposit_key("uusd", "0.5", "0.3", "user1", &Uint128::one(), 0);
    let deposit = BACKING_DEPOSITS.load(&deps.storage, deposit_key).unwrap();
    assert_eq!(deposit.depositor, Some(Addr::unchecked("depositor1")));
}

#[test]
fn test_toggle_withdrawals_by_depositor() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    // Deposit for another user
    let info = mock_info("depositor1", &coins(10000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: Some("user1".to_string()),
            locked: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
        },
    ).unwrap();

    // Toggle withdrawals to disabled by depositor
    let info = mock_info("depositor1", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ToggleWithdrawals {
            user: "user1".to_string(),
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::one(),
            enabled: false,
            epoch_start_time: 0,
        },
    ).unwrap();

    // Check withdrawals are disabled
    let deposit_key = make_deposit_key("uusd", "0.5", "0.3", "user1", &Uint128::one(), 0);
    let deposit = BACKING_DEPOSITS.load(&deps.storage, deposit_key.clone()).unwrap();
    assert_eq!(deposit.withdrawals_enabled, false);

    // Toggle back to enabled
    let info = mock_info("depositor1", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ToggleWithdrawals {
            user: "user1".to_string(),
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::one(),
            enabled: true,
            epoch_start_time: 0,
        },
    ).unwrap();

    // Check withdrawals are enabled again
    let deposit = BACKING_DEPOSITS.load(&deps.storage, deposit_key).unwrap();
    assert_eq!(deposit.withdrawals_enabled, true);
}

#[test]
fn test_toggle_withdrawals_unauthorized() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    // Deposit for another user
    let info = mock_info("depositor1", &coins(10000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: Some("user1".to_string()),
            locked: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
        },
    ).unwrap();

    // Try to toggle withdrawals by non-depositor (should fail)
    let info = mock_info("unauthorized_user", &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ToggleWithdrawals {
            user: "user1".to_string(),
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::one(),
            enabled: false,
            epoch_start_time: 0,
        },
    );

    assert!(res.is_err());
    match res.unwrap_err() {
        ContractError::Unauthorized {} => {},
        _ => panic!("Expected Unauthorized error"),
    }
}

#[test]
fn test_toggle_withdrawals_self_deposit_fails() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    // Self-deposit
    let info = mock_info("user1", &coins(10000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
        },
    ).unwrap();

    // Try to toggle withdrawals on self-deposit (should fail)
    let info = mock_info("user1", &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ToggleWithdrawals {
            user: "user1".to_string(),
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::one(),
            enabled: false,
            epoch_start_time: 0,
        },
    );

    assert!(res.is_err());
    match res.unwrap_err() {
        ContractError::CustomError { val } => {
            assert!(val.contains("Cannot toggle withdrawals for self-deposits"));
        },
        _ => panic!("Expected CustomError about self-deposits"),
    }
}

#[test]
fn test_withdraw_when_disabled_fails() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    // Deposit for another user
    let info = mock_info("depositor1", &coins(10000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: Some("user1".to_string()),
            locked: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
        },
    ).unwrap();

    // Disable withdrawals
    let info = mock_info("depositor1", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ToggleWithdrawals {
            user: "user1".to_string(),
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::one(),
            enabled: false,
            epoch_start_time: 0,
        },
    ).unwrap();

    // Try to withdraw (should fail)
    let info = mock_info("user1", &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::WithdrawDeposit {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::one(),
            amount: Some(Uint128::new(5000)),
            epoch_start_time: 0,
        },
    );

    assert!(res.is_err());
    match res.unwrap_err() {
        ContractError::WithdrawalsDisabled {} => {},
        _ => panic!("Expected WithdrawalsDisabled error"),
    }
}

#[test]
fn test_withdraw_when_enabled_succeeds() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    // Deposit for another user
    let info = mock_info("depositor1", &coins(10000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: Some("user1".to_string()),
            locked: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
        },
    ).unwrap();

    // Withdrawals should be enabled by default, so withdrawal should succeed
    let info = mock_info("user1", &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::WithdrawDeposit {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::one(),
            amount: Some(Uint128::new(5000)),
            epoch_start_time: 0,
        },
    );

    assert!(res.is_ok());
}

#[test]
fn test_self_deposit_withdraw_always_allowed() {
    let (mut deps, env) = instantiate_contract();
    
    // Create queue first
    let info = mock_info("owner", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CreateQueue {
            asset: "uusd".to_string(),
        },
    ).unwrap();

    let deposit_input = BackingDepositInput {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        epoch_start_time: Some(0),
    };

    // Self-deposit
    let info = mock_info("user1", &coins(10000, "uusd"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: deposit_input.clone(),
            deposit_owner: None,
            locked: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
        },
    ).unwrap();

    // Self-deposits should always allow withdrawal (depositor is None)
    let info = mock_info("user1", &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::WithdrawDeposit {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            deposit_id: Uint128::one(),
            amount: Some(Uint128::new(5000)),
            epoch_start_time: 0,
        },
    );

    assert!(res.is_ok());
}

