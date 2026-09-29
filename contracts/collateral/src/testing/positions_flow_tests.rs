#![allow(non_snake_case)]

use cosmwasm_std::testing::{mock_dependencies, mock_dependencies_with_balances, message_info, MOCK_CONTRACT_ADDR};
use cosmwasm_std::{coin, Addr, Coin, Decimal, Env, Timestamp, Uint128};

use membrane::types::{Asset, AssetInfo};

use crate::contract::execute;
use crate::positions::{authorized_sell_collateral, withdraw_callback};
use crate::state::{
    CollateralRateAssurance, COLLATERAL_RATE_ASSURANCE, POSITIONS,
    SELL_COLLATERAL_PROPAGATION, WITHDRAW_PROPAGATION,
};
use crate::testing::test_helpers::helpers::*;

fn mock_env_at(time_secs: u64) -> Env {
    let mut env = cosmwasm_std::testing::mock_env();
    env.block.time = Timestamp::from_seconds(time_secs);
    env
}

// ========================================================================
// Group A: authorized_sell_collateral (7 tests)
// ========================================================================

#[test]
fn sell_unauthorized_user_errors() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner,
        Uint128::one(),
        vec![make_casset_with_amount("uatom", 1000, 50, 60)],
    );

    // Random user tries to call
    let info = message_info(&Addr::unchecked("random_user"), &[]);
    let res = authorized_sell_collateral(
        deps.as_mut(),
        env,
        info,
        Uint128::one(),
        owner_str.to_string(),
        Decimal::percent(5),
        "recipient".to_string(),
        None,
    );
    assert!(res.is_err());
    assert!(format!("{:?}", res.unwrap_err()).contains("Unauthorized"));
}

#[test]
fn sell_debt_contract_can_call() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner,
        Uint128::one(),
        vec![make_casset_with_amount("uatom", 1000, 50, 60)],
    );

    let info = message_info(&Addr::unchecked("debt_contract"), &[]);
    let res = authorized_sell_collateral(
        deps.as_mut(),
        env,
        info,
        Uint128::one(),
        owner_str.to_string(),
        Decimal::percent(5),
        "recipient".to_string(),
        None,
    );
    assert!(res.is_ok());
    // Should have SubMsgs for the swap
    assert!(!res.unwrap().messages.is_empty());
}

#[test]
fn sell_liquidation_contract_can_call() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner,
        Uint128::one(),
        vec![make_casset_with_amount("uatom", 1000, 50, 60)],
    );

    let info = message_info(&Addr::unchecked("liquidation_contract"), &[]);
    let res = authorized_sell_collateral(
        deps.as_mut(),
        env,
        info,
        Uint128::one(),
        owner_str.to_string(),
        Decimal::percent(5),
        "recipient".to_string(),
        None,
    );
    assert!(res.is_ok());
}

#[test]
fn sell_no_chain_proxy_errors() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    // Init config WITHOUT chain_proxy
    let mut config = init_config(deps.as_mut().storage);
    config.chain_proxy = None;
    crate::state::CONFIG.save(deps.as_mut().storage, &config).unwrap();

    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner,
        Uint128::one(),
        vec![make_casset_with_amount("uatom", 1000, 50, 60)],
    );

    let info = message_info(&Addr::unchecked("debt_contract"), &[]);
    let res = authorized_sell_collateral(
        deps.as_mut(),
        env,
        info,
        Uint128::one(),
        owner_str.to_string(),
        Decimal::percent(5),
        "recipient".to_string(),
        None,
    );
    assert!(res.is_err());
    assert!(format!("{:?}", res.unwrap_err()).contains("chain proxy"));
}

#[test]
fn sell_builds_swap_submsgs_for_each_asset() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner,
        Uint128::one(),
        vec![
            make_casset_with_amount("uatom", 1000, 50, 60),
            make_casset_with_amount("uosmo", 2000, 40, 50),
        ],
    );

    let info = message_info(&Addr::unchecked("debt_contract"), &[]);
    let res = authorized_sell_collateral(
        deps.as_mut(),
        env,
        info,
        Uint128::one(),
        owner_str.to_string(),
        Decimal::percent(5),
        "recipient".to_string(),
        None,
    ).unwrap();

    // Should have 2 SubMsgs (one per asset)
    assert_eq!(res.messages.len(), 2);
}

#[test]
fn sell_saves_propagation() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner,
        Uint128::one(),
        vec![make_casset_with_amount("uatom", 1000, 50, 60)],
    );

    let info = message_info(&Addr::unchecked("debt_contract"), &[]);
    let _ = authorized_sell_collateral(
        deps.as_mut(),
        env,
        info,
        Uint128::one(),
        owner_str.to_string(),
        Decimal::percent(5),
        "recipient".to_string(),
        None,
    ).unwrap();

    // Propagation should be saved
    let prop = SELL_COLLATERAL_PROPAGATION.load(deps.as_ref().storage).unwrap();
    assert_eq!(prop.collateral_sold.len(), 1);
    assert_eq!(prop.collateral_sold[0].denom, "uatom");
    assert_eq!(prop.collateral_sold[0].amount, Uint128::new(1000));
}

#[test]
fn sell_zero_amount_assets_skipped() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner,
        Uint128::one(),
        vec![
            make_casset_with_amount("uatom", 1000, 50, 60),
            make_casset_with_amount("uosmo", 0, 40, 50), // zero amount
        ],
    );

    let info = message_info(&Addr::unchecked("debt_contract"), &[]);
    let res = authorized_sell_collateral(
        deps.as_mut(),
        env,
        info,
        Uint128::one(),
        owner_str.to_string(),
        Decimal::percent(5),
        "recipient".to_string(),
        None,
    ).unwrap();

    // Only 1 SubMsg (uosmo skipped due to zero amount)
    assert_eq!(res.messages.len(), 1);

    // Propagation should only have uatom
    let prop = SELL_COLLATERAL_PROPAGATION.load(deps.as_ref().storage).unwrap();
    assert_eq!(prop.collateral_sold.len(), 1);
}

// ========================================================================
// Group A2: authorized_sell_collateral with sell_amounts (3 tests)
// ========================================================================

#[test]
fn sell_specific_amounts_sells_only_requested() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner,
        Uint128::one(),
        vec![
            make_casset_with_amount("uatom", 1000, 50, 60),
            make_casset_with_amount("uosmo", 2000, 40, 50),
        ],
    );

    let info = message_info(&Addr::unchecked("liquidation_contract"), &[]);
    // Only sell 300 uatom and 500 uosmo
    let sell_amounts = Some(vec![
        Asset {
            info: AssetInfo::NativeToken { denom: "uatom".to_string() },
            amount: Uint128::new(300),
        },
        Asset {
            info: AssetInfo::NativeToken { denom: "uosmo".to_string() },
            amount: Uint128::new(500),
        },
    ]);
    let res = authorized_sell_collateral(
        deps.as_mut(),
        env,
        info,
        Uint128::one(),
        owner_str.to_string(),
        Decimal::percent(5),
        "recipient".to_string(),
        sell_amounts,
    ).unwrap();

    // Should have 2 SubMsgs (one per specified asset)
    assert_eq!(res.messages.len(), 2);

    // Propagation should record the specific amounts
    let prop = SELL_COLLATERAL_PROPAGATION.load(deps.as_ref().storage).unwrap();
    assert_eq!(prop.collateral_sold.len(), 2);
    assert_eq!(prop.collateral_sold[0].denom, "uatom");
    assert_eq!(prop.collateral_sold[0].amount, Uint128::new(300));
    assert_eq!(prop.collateral_sold[1].denom, "uosmo");
    assert_eq!(prop.collateral_sold[1].amount, Uint128::new(500));
}

#[test]
fn sell_specific_amounts_capped_at_position_balance() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner,
        Uint128::one(),
        vec![make_casset_with_amount("uatom", 500, 50, 60)],
    );

    let info = message_info(&Addr::unchecked("liquidation_contract"), &[]);
    // Request 1000 uatom but position only has 500
    let sell_amounts = Some(vec![Asset {
        info: AssetInfo::NativeToken { denom: "uatom".to_string() },
        amount: Uint128::new(1000),
    }]);
    let res = authorized_sell_collateral(
        deps.as_mut(),
        env,
        info,
        Uint128::one(),
        owner_str.to_string(),
        Decimal::percent(5),
        "recipient".to_string(),
        sell_amounts,
    ).unwrap();

    assert_eq!(res.messages.len(), 1);

    // Propagation should be capped at 500
    let prop = SELL_COLLATERAL_PROPAGATION.load(deps.as_ref().storage).unwrap();
    assert_eq!(prop.collateral_sold[0].amount, Uint128::new(500));
}

#[test]
fn sell_specific_amounts_zero_skipped() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner,
        Uint128::one(),
        vec![
            make_casset_with_amount("uatom", 1000, 50, 60),
            make_casset_with_amount("uosmo", 2000, 40, 50),
        ],
    );

    let info = message_info(&Addr::unchecked("liquidation_contract"), &[]);
    // Request 0 uatom and 500 uosmo
    let sell_amounts = Some(vec![
        Asset {
            info: AssetInfo::NativeToken { denom: "uatom".to_string() },
            amount: Uint128::zero(),
        },
        Asset {
            info: AssetInfo::NativeToken { denom: "uosmo".to_string() },
            amount: Uint128::new(500),
        },
    ]);
    let res = authorized_sell_collateral(
        deps.as_mut(),
        env,
        info,
        Uint128::one(),
        owner_str.to_string(),
        Decimal::percent(5),
        "recipient".to_string(),
        sell_amounts,
    ).unwrap();

    // Only 1 SubMsg (uatom zero amount skipped)
    assert_eq!(res.messages.len(), 1);

    let prop = SELL_COLLATERAL_PROPAGATION.load(deps.as_ref().storage).unwrap();
    assert_eq!(prop.collateral_sold.len(), 1);
    assert_eq!(prop.collateral_sold[0].denom, "uosmo");
}

// ========================================================================
// Group B: withdraw_callback (5 tests)
// ========================================================================

#[test]
fn callback_updates_position() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner.clone(),
        Uint128::one(),
        vec![make_casset_with_amount("uatom", 1000, 50, 60)],
    );

    save_withdraw_propagation(
        deps.as_mut().storage,
        owner_str.as_str(),
        Uint128::one(),
        vec![Asset {
            info: AssetInfo::NativeToken { denom: "uatom".to_string() },
            amount: Uint128::new(1000),
        }],
        vec![Uint128::new(400)],
        vec![Uint128::new(5000)],
    );

    let res = withdraw_callback(deps.as_mut(), env, Uint128::one(), owner_str.to_string());
    assert!(res.is_ok());

    let positions = POSITIONS.load(deps.as_ref().storage, owner).unwrap();
    assert_eq!(positions[0].collateral_assets[0].asset.amount, Uint128::new(600));
}

#[test]
fn callback_clears_propagation() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner,
        Uint128::one(),
        vec![make_casset_with_amount("uatom", 1000, 50, 60)],
    );

    save_withdraw_propagation(
        deps.as_mut().storage,
        owner_str.as_str(),
        Uint128::one(),
        vec![Asset {
            info: AssetInfo::NativeToken { denom: "uatom".to_string() },
            amount: Uint128::new(1000),
        }],
        vec![Uint128::new(100)],
        vec![Uint128::new(5000)],
    );

    let _ = withdraw_callback(deps.as_mut(), env, Uint128::one(), owner_str.to_string()).unwrap();

    assert!(WITHDRAW_PROPAGATION.may_load(deps.as_ref().storage).unwrap().is_none());
}

#[test]
fn callback_full_removal() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner.clone(),
        Uint128::one(),
        vec![make_casset_with_amount("uatom", 500, 50, 60)],
    );

    save_withdraw_propagation(
        deps.as_mut().storage,
        owner_str.as_str(),
        Uint128::one(),
        vec![Asset {
            info: AssetInfo::NativeToken { denom: "uatom".to_string() },
            amount: Uint128::new(500),
        }],
        vec![Uint128::new(500)],
        vec![Uint128::new(5000)],
    );

    let _ = withdraw_callback(deps.as_mut(), env, Uint128::one(), owner_str.to_string()).unwrap();

    // Position's collateral should be empty
    let positions = POSITIONS.load(deps.as_ref().storage, owner).unwrap();
    assert!(positions[0].collateral_assets.is_empty());
}

#[test]
fn callback_no_propagation_errors() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner,
        Uint128::one(),
        vec![make_casset_with_amount("uatom", 1000, 50, 60)],
    );

    // No propagation state saved
    let res = withdraw_callback(deps.as_mut(), env, Uint128::one(), owner_str.to_string());
    assert!(res.is_err());
}

#[test]
fn callback_multiple_assets() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let owner_str = deps.api.addr_make("user1");

    init_config(deps.as_mut().storage);
    init_basket(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_stored_price(deps.as_mut().storage, "uatom", Decimal::from_ratio(10u128, 1u128), 900);
    init_stored_price(deps.as_mut().storage, "uosmo", Decimal::from_ratio(5u128, 1u128), 900);

    let owner = Addr::unchecked(owner_str.clone());
    init_position(
        deps.as_mut().storage,
        owner.clone(),
        Uint128::one(),
        vec![
            make_casset_with_amount("uatom", 1000, 50, 60),
            make_casset_with_amount("uosmo", 2000, 40, 50),
        ],
    );

    save_withdraw_propagation(
        deps.as_mut().storage,
        owner_str.as_str(),
        Uint128::one(),
        vec![
            Asset {
                info: AssetInfo::NativeToken { denom: "uatom".to_string() },
                amount: Uint128::new(1000),
            },
            Asset {
                info: AssetInfo::NativeToken { denom: "uosmo".to_string() },
                amount: Uint128::new(2000),
            },
        ],
        vec![Uint128::new(200), Uint128::new(500)],
        vec![Uint128::new(5000), Uint128::new(8000)],
    );

    let _ = withdraw_callback(deps.as_mut(), env, Uint128::one(), owner_str.to_string()).unwrap();

    let positions = POSITIONS.load(deps.as_ref().storage, owner).unwrap();
    assert_eq!(positions[0].collateral_assets[0].asset.amount, Uint128::new(800));
    assert_eq!(positions[0].collateral_assets[1].asset.amount, Uint128::new(1500));
}

// ========================================================================
// Group C: Collateral Rate Assurance (4 tests)
// ========================================================================

#[test]
fn rate_assurance_passes_when_rates_unchanged() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let contract_addr = env.contract.address.to_string();

    // Set the contract's native balance so querier returns 1000 uatom
    deps.querier.bank.update_balance(
        contract_addr.clone(),
        vec![Coin { denom: "uatom".to_string(), amount: Uint128::new(1000) }],
    );

    init_config(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);

    // Set up basket with supply cap for uatom (current_supply=1000)
    init_basket_custom(
        deps.as_mut().storage,
        vec![make_casset("uatom", 50, 60)],
        vec![make_supply_cap_with_supply("uatom", 100, 1000)],
        vec![],
    );

    // Pre-snapshot rate: balance(1000) * 1_000_000 / supply(1000) = 1_000_000
    COLLATERAL_RATE_ASSURANCE
        .save(
            deps.as_mut().storage,
            "uatom".to_string(),
            &CollateralRateAssurance {
                collateral_denom: "uatom".to_string(),
                pre_collateral_per_one: Uint128::new(1_000_000),
            },
        )
        .unwrap();

    // Call execute with CollateralRateAssurance from self (contract address)
    let info = message_info(&Addr::unchecked(contract_addr), &[]);
    let msg = membrane::collateral::ExecuteMsg::CollateralRateAssurance {
        collateral_denoms: Some(vec!["uatom".to_string()]),
    };
    let res = execute(deps.as_mut(), env, info, msg);
    assert!(res.is_ok(), "Expected success but got: {:?}", res.unwrap_err());

    // Snapshot should have been cleaned up
    let assurance = COLLATERAL_RATE_ASSURANCE
        .may_load(deps.as_ref().storage, "uatom".to_string())
        .unwrap();
    assert!(assurance.is_none(), "Snapshot should be removed after successful check");
}

#[test]
fn rate_assurance_fails_when_rate_changes() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let contract_addr = env.contract.address.to_string();

    // Contract balance is 2000 uatom, but snapshot was taken when rate was 1_000_000
    // (i.e. balance was 1000 at snapshot time). Now rate = 2000 * 1_000_000 / 1000 = 2_000_000
    // Difference = 1_000_000, which is > 1, so it should fail.
    deps.querier.bank.update_balance(
        contract_addr.clone(),
        vec![Coin { denom: "uatom".to_string(), amount: Uint128::new(2000) }],
    );

    init_config(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);

    // Basket with current_supply=1000
    init_basket_custom(
        deps.as_mut().storage,
        vec![make_casset("uatom", 50, 60)],
        vec![make_supply_cap_with_supply("uatom", 100, 1000)],
        vec![],
    );

    // Pre-snapshot rate of 1_000_000 (from when balance was 1000)
    COLLATERAL_RATE_ASSURANCE
        .save(
            deps.as_mut().storage,
            "uatom".to_string(),
            &CollateralRateAssurance {
                collateral_denom: "uatom".to_string(),
                pre_collateral_per_one: Uint128::new(1_000_000),
            },
        )
        .unwrap();

    // Call from self
    let info = message_info(&Addr::unchecked(contract_addr), &[]);
    let msg = membrane::collateral::ExecuteMsg::CollateralRateAssurance {
        collateral_denoms: Some(vec!["uatom".to_string()]),
    };
    let res = execute(deps.as_mut(), env, info, msg);
    assert!(res.is_err());

    let err = format!("{:?}", res.unwrap_err());
    assert!(
        err.contains("rate assurance failed") || err.contains("Collateral rate assurance failed"),
        "Expected 'rate assurance failed' error, got: {}",
        err,
    );
}

#[test]
fn rate_assurance_unauthorized_not_self() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);

    init_config(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);
    init_basket(deps.as_mut().storage);

    // Call from a non-self sender (not the contract address)
    let info = message_info(&Addr::unchecked("random_user"), &[]);
    let msg = membrane::collateral::ExecuteMsg::CollateralRateAssurance {
        collateral_denoms: Some(vec!["uatom".to_string()]),
    };
    let res = execute(deps.as_mut(), env, info, msg);
    assert!(res.is_err());
    assert!(
        format!("{:?}", res.unwrap_err()).contains("Unauthorized"),
        "Expected Unauthorized error",
    );
}

#[test]
fn create_collateral_rate_assurance_saves_snapshot() {
    let mut deps = mock_dependencies();
    let env = mock_env_at(1000);
    let contract_addr = env.contract.address.to_string();

    // Set the contract's native balance to 5000 uatom
    deps.querier.bank.update_balance(
        contract_addr,
        vec![Coin { denom: "uatom".to_string(), amount: Uint128::new(5000) }],
    );

    init_config(deps.as_mut().storage);
    init_frozen_state(deps.as_mut().storage, false);

    // Basket with current_supply=2500 for uatom
    let basket = init_basket_custom(
        deps.as_mut().storage,
        vec![make_casset("uatom", 50, 60)],
        vec![make_supply_cap_with_supply("uatom", 100, 2500)],
        vec![],
    );

    // Call the helper
    let msgs = {
        let dep_mut = deps.as_mut();
        crate::state::create_collateral_rate_assurance(
            dep_mut.storage,
            dep_mut.querier,
            &env,
            vec!["uatom".to_string()],
            &basket,
        )
        .unwrap()
    };

    // Verify the snapshot was saved
    let assurance = COLLATERAL_RATE_ASSURANCE
        .load(deps.as_ref().storage, "uatom".to_string())
        .unwrap();
    assert_eq!(assurance.collateral_denom, "uatom");
    // Expected rate: 5000 * 1_000_000 / 2500 = 2_000_000
    assert_eq!(assurance.pre_collateral_per_one, Uint128::new(2_000_000));

    // Verify a callback CosmosMsg::Wasm was returned
    assert_eq!(msgs.len(), 1);
    match &msgs[0] {
        cosmwasm_std::CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute {
            contract_addr,
            msg: _,
            funds,
        }) => {
            assert_eq!(contract_addr, env.contract.address.as_str());
            assert!(funds.is_empty());
        }
        other => panic!("Expected CosmosMsg::Wasm(Execute), got: {:?}", other),
    }
}
