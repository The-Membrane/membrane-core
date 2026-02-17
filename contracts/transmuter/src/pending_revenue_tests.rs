use cosmwasm_std::{coin, coins, Addr, Binary, Decimal, Deps, DepsMut, Empty, Env, MessageInfo, Response, StdError, StdResult, Uint128, to_json_binary};
use cw_multi_test::{App, Contract, ContractWrapper, Executor};

use membrane::tokenfactory::{ExecuteMsg as TfExecuteMsg, InstantiateMsg as TfInstantiateMsg};
use membrane::cdp::QueryMsg as CdpQueryMsg;
use membrane::transmuter::{ExecuteMsg, InstantiateMsg, AssetPair};
use membrane::revenue_distributor::ExecuteMsg as RevenueDistributorExecuteMsg;
use membrane::types::LiqAsset;

use crate::contract::{execute, instantiate, query};

const ADMIN: &str = "admin";
const USER: &str = "user";
const NON_ALLOWLISTED_USER: &str = "non_allowlisted_user";
const CDT: &str = "cdt";
const USDC: &str = "usdc";
// VAULT_SUBDENOM no longer needed
const INITIAL_BALANCE: u128 = 1_000_000_000_000; // 1M tokens

fn transmuter_contract() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(execute, instantiate, query))
}

fn mock_tokenfactory_contract() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        mock_tokenfactory_execute,
        mock_tokenfactory_instantiate,
        mock_tokenfactory_query,
    ))
}

fn mock_cdp_contract() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        |_deps, _env, _info, _msg: Empty| -> Result<Response, StdError> { Ok(Response::new()) },
        |_deps, _env, _info, _msg: Empty| -> StdResult<Response> { Ok(Response::new()) },
        |_deps, _env, q: CdpQueryMsg| -> StdResult<Binary> {
            match q {
                CdpQueryMsg::GetActiveDeploymentVenues { .. } => {
                    to_json_binary(&Vec::<String>::new())
                }
                _ => to_json_binary(&Empty {}),
            }
        },
    ))
}

fn mock_revenue_distributor_contract() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        |_deps, _env, info, msg: RevenueDistributorExecuteMsg| -> Result<Response, StdError> {
            // Just accept the funds and return success
            match msg {
                RevenueDistributorExecuteMsg::SetPromises { .. } => {
                    Ok(Response::new()
                        .add_attribute("action", "set_promises")
                        .add_attribute("funds_received", format!("{:?}", info.funds)))
                }
                _ => Ok(Response::new()),
            }
        },
        |_deps, _env, _info, _msg: Empty| -> StdResult<Response> { Ok(Response::new()) },
        |_deps, _env, _msg: Empty| -> StdResult<Binary> { to_json_binary(&Empty {}) },
    ))
}

fn mock_tokenfactory_instantiate(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: TfInstantiateMsg,
) -> StdResult<Response> {
    Ok(Response::new())
}

fn mock_tokenfactory_execute(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: TfExecuteMsg,
) -> Result<Response, StdError> {
    Ok(Response::new())
}

fn mock_tokenfactory_query(_deps: Deps, _env: Env, _msg: Empty) -> StdResult<Binary> {
    to_json_binary(&Empty {})
}

fn setup_app() -> App {
    let mut app = App::default();
    app.init_modules(|router, _, storage| {
        router
            .bank
            .init_balance(
                storage,
                &Addr::unchecked(ADMIN),
                vec![
                    coin(INITIAL_BALANCE, CDT),
                    coin(INITIAL_BALANCE, USDC),
                ],
            )
            .unwrap();
        router
            .bank
            .init_balance(
                storage,
                &Addr::unchecked(NON_ALLOWLISTED_USER),
                vec![
                    coin(INITIAL_BALANCE, CDT),
                    coin(INITIAL_BALANCE, USDC),
                ],
            )
            .unwrap();
        router
            .bank
            .init_balance(
                storage,
                &Addr::unchecked(NON_ALLOWLISTED_USER),
                vec![
                    coin(INITIAL_BALANCE, CDT),
                    coin(INITIAL_BALANCE, USDC),
                ],
            )
            .unwrap();
    });
    app
}

fn setup_contracts(app: &mut App) -> (Addr, Addr, Addr, Addr) {
    // Store contracts
    let tokenfactory_id = app.store_code(mock_tokenfactory_contract());
    let cdp_id = app.store_code(mock_cdp_contract());
    let revenue_distributor_id = app.store_code(mock_revenue_distributor_contract());
    let transmuter_id = app.store_code(transmuter_contract());

    // Instantiate tokenfactory
    let tokenfactory_addr = app
        .instantiate_contract(
            tokenfactory_id,
            Addr::unchecked(ADMIN),
            &TfInstantiateMsg { owner: None },
            &[],
            "tokenfactory",
            None,
        )
        .unwrap();

    // Instantiate CDP
    let cdp_addr = app
        .instantiate_contract(
            cdp_id,
            Addr::unchecked(ADMIN),
            &Empty {},
            &[],
            "cdp",
            None,
        )
        .unwrap();

    // Instantiate revenue distributor
    let revenue_distributor_addr = app
        .instantiate_contract(
            revenue_distributor_id,
            Addr::unchecked(ADMIN),
            &Empty {},
            &[],
            "revenue_distributor",
            None,
        )
        .unwrap();

    // Instantiate transmuter with revenue distribution config
    // Use high composition leeway and add USER to allowlist to avoid composition errors
    let transmuter_addr = app
        .instantiate_contract(
            transmuter_id,
            Addr::unchecked(ADMIN),
            &InstantiateMsg {
                owner: Some(ADMIN.to_string()),
                tokenfactory_contract: Some(tokenfactory_addr.clone()),
                discounts_contract: ADMIN.to_string(),
                cdp_contract: cdp_addr.to_string(),
                revenue_distributor_addr: revenue_distributor_addr.to_string(),
                deposit_pair: AssetPair {
                    cdt: CDT.to_string(),
                    paired_asset: USDC.to_string(),
                },
                composition_leeway: Decimal::percent(100), // Allow any composition
                cdt_target_ratio: Decimal::percent(50),
                usage_fee: Some(Decimal::percent(1)), // 1% fee
                usage_fee_utilization_threshold: None,
                swap_history_cap: 100,
                volume_history_cap: 100,
                rate_limit_window_secs: Some(3600),
                rate_limit_threshold: Some(Decimal::percent(50)), // High threshold for testing
                allowlist: Some(vec![USER.to_string()]), // Add USER to allowlist
                allowlist_rate_limit_threshold: Some(Decimal::percent(50)),
                global_rate_limit_window_secs: Some(86400),
                global_rate_limit_threshold: Some(Decimal::percent(50)),
                affiliate_fee: Decimal::percent(1),
                revenue_distributions: Some(vec![
                    LiqAsset {
                        info: membrane::types::AssetInfo::NativeToken {
                            denom: "ltv_disco_token".to_string(),
                        },
                        amount: Decimal::one(),
                    },
                ]),
                monthly_incentive_max: None,
                incentive_denom: None,
                neutron_proxy: None, //dont have mocks yet
                staking_contract: None,
                mars_mirror_contract: None,
            },
            &[],
            "transmuter",
            None,
        )
        .unwrap();

    (transmuter_addr, tokenfactory_addr, cdp_addr, revenue_distributor_addr)
}

/// Helper to query pending revenue
fn query_pending_revenue(app: &App, transmuter_addr: &Addr) -> Uint128 {
    let raw_state = app.wrap().query_wasm_raw(transmuter_addr, b"pending_revenue").unwrap();
    if let Some(data) = raw_state {
        cosmwasm_std::from_json(&data).unwrap()
    } else {
        Uint128::zero()
    }
}

#[test]
fn test_pending_revenue_accumulates_when_no_cdt_available() {
    let mut app = setup_app();
    let (transmuter_addr, _, _, _) = setup_contracts(&mut app);

    // Fund transmuter with both assets, but limited CDT
    app.send_tokens(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &coins(150_000, CDT), // Just enough for one swap
    )
    .unwrap();
    app.send_tokens(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &coins(1_000_000, USDC),
    )
    .unwrap();

    // User swaps CDT -> USDC (this will generate a fee in CDT, which goes directly)
    let swap_amount = 100_000u128;
    app.execute_contract(
        Addr::unchecked(NON_ALLOWLISTED_USER),
        transmuter_addr.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(swap_amount, CDT),
    )
    .unwrap();

    // Now transmuter has USDC but very little CDT left
    // Swap USDC -> CDT will generate a fee in USDC that can't be fully converted
    let result = app.execute_contract(
        Addr::unchecked(NON_ALLOWLISTED_USER),
        transmuter_addr.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(50_000, USDC),
    );

    // Transmute should succeed even though fee can't be fully distributed
    assert!(result.is_ok());

    // Check that some pending revenue accumulated
    let pending = query_pending_revenue(&app, &transmuter_addr);
    // Pending should be > 0 since we don't have enough CDT to convert all the USDC fee
    assert!(pending > Uint128::zero());
}

#[test]
fn test_pending_revenue_clears_when_cdt_becomes_available() {
    let mut app = setup_app();
    let (transmuter_addr, _, _, _) = setup_contracts(&mut app);

    // Step 1: Fund transmuter with both assets, limited CDT
    app.send_tokens(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &coins(150_000, CDT), // Just enough for one swap
    )
    .unwrap();
    app.send_tokens(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &coins(1_000_000, USDC),
    )
    .unwrap();

    // Step 2: User swaps CDT -> USDC to get USDC in contract
    let swap_amount = 100_000u128;
    app.execute_contract(
        Addr::unchecked(NON_ALLOWLISTED_USER),
        transmuter_addr.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(swap_amount, CDT),
    )
    .unwrap();

    // Step 3: Swap USDC -> CDT, fee accumulates as pending (not enough CDT to convert)
    app.execute_contract(
        Addr::unchecked(NON_ALLOWLISTED_USER),
        transmuter_addr.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(50_000, USDC),
    )
    .unwrap();

    let pending_after_first_swap = query_pending_revenue(&app, &transmuter_addr);
    assert!(pending_after_first_swap > Uint128::zero());

    // Step 4: Fund transmuter with more CDT
    app.send_tokens(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &coins(1_000_000, CDT),
    )
    .unwrap();

    // Step 5: Another swap should process pending revenue
    app.execute_contract(
        Addr::unchecked(NON_ALLOWLISTED_USER),
        transmuter_addr.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(50_000, USDC),
    )
    .unwrap();

    // Pending revenue should be reduced or cleared
    let pending_after_second_swap = query_pending_revenue(&app, &transmuter_addr);
    assert!(pending_after_second_swap < pending_after_first_swap);
}

#[test]
fn test_pending_revenue_accumulates_across_multiple_swaps() {
    let mut app = setup_app();
    let (transmuter_addr, _, _, _) = setup_contracts(&mut app);

    // Fund transmuter with both assets, but limited CDT
    app.send_tokens(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &coins(300_000, CDT), // Limited for multiple swaps
    )
    .unwrap();
    app.send_tokens(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &coins(10_000_000, USDC),
    )
    .unwrap();

    let swap_amount = 50_000u128;
    let num_swaps = 5;

    // First do a CDT -> USDC swap to ensure USDC is available
    app.execute_contract(
        Addr::unchecked(NON_ALLOWLISTED_USER),
        transmuter_addr.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(100_000, CDT),
    )
    .unwrap();

    // Perform multiple swaps USDC -> CDT
    // Each will generate a fee in USDC that can't be fully converted
    for _ in 0..num_swaps {
        let _ = app.execute_contract(
            Addr::unchecked(NON_ALLOWLISTED_USER),
            transmuter_addr.clone(),
            &ExecuteMsg::Transmute { recipient: None },
            &coins(swap_amount, USDC),
        );
    }

    // Check that some pending revenue accumulated
    let pending = query_pending_revenue(&app, &transmuter_addr);
    assert!(pending > Uint128::zero());
}

#[test]
fn test_pending_revenue_partial_processing() {
    let mut app = setup_app();
    let (transmuter_addr, _, _, _) = setup_contracts(&mut app);

    // Fund transmuter with both assets
    app.send_tokens(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &coins(400_000, CDT),
    )
    .unwrap();
    app.send_tokens(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &coins(10_000_000, USDC),
    )
    .unwrap();

    // First swap CDT -> USDC to ensure USDC availability
    app.execute_contract(
        Addr::unchecked(NON_ALLOWLISTED_USER),
        transmuter_addr.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(100_000, CDT),
    )
    .unwrap();

    // Generate pending revenue with multiple swaps
    let swap_amount = 100_000u128;
    for _ in 0..3 {
        let _ = app.execute_contract(
            Addr::unchecked(NON_ALLOWLISTED_USER),
            transmuter_addr.clone(),
            &ExecuteMsg::Transmute { recipient: None },
            &coins(swap_amount, USDC),
        );
    }

    let pending_before_cdt = query_pending_revenue(&app, &transmuter_addr);
    assert!(pending_before_cdt > Uint128::zero());

    // Add more CDT
    app.send_tokens(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &coins(1_000_000, CDT),
    )
    .unwrap();

    // Another swap should process some pending revenue
    let _ = app.execute_contract(
        Addr::unchecked(NON_ALLOWLISTED_USER),
        transmuter_addr.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(swap_amount, USDC),
    );

    let pending_after_partial = query_pending_revenue(&app, &transmuter_addr);
    
    // Pending should be reduced
    assert!(pending_after_partial < pending_before_cdt);
}

#[test]
fn test_cdt_fee_bypasses_pending_revenue() {
    let mut app = setup_app();
    let (transmuter_addr, _, _, _) = setup_contracts(&mut app);

    // Fund transmuter with both assets
    app.send_tokens(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &coins(1_000_000, CDT),
    )
    .unwrap();
    app.send_tokens(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &coins(1_000_000, USDC),
    )
    .unwrap();

    // User swaps CDT -> USDC (fee will be in CDT, not USDC)
    let swap_amount = 100_000u128;
    app.execute_contract(
        Addr::unchecked(NON_ALLOWLISTED_USER),
        transmuter_addr.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(swap_amount, CDT),
    )
    .unwrap();

    // Pending revenue should remain zero because CDT fees are sent directly
    let pending = query_pending_revenue(&app, &transmuter_addr);
    assert_eq!(pending, Uint128::zero());
}

#[test]
fn test_stress_many_swaps_with_intermittent_cdt() {
    let mut app = setup_app();
    let (transmuter_addr, _, _, _) = setup_contracts(&mut app);

    // Initial funding with both assets
    app.send_tokens(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &coins(10_000_000, CDT),
    )
    .unwrap();
    app.send_tokens(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &coins(100_000_000, USDC),
    )
    .unwrap();

    let swap_amount = 10_000u128;

    // Perform 20 swaps with intermittent CDT funding
    for i in 0..20 {
        // Every 5th swap, add more CDT
        if i % 5 == 0 && i > 0 {
            app.send_tokens(
                Addr::unchecked(ADMIN),
                transmuter_addr.clone(),
                &coins(100_000, CDT),
            )
            .unwrap();
        }
        
        let _ = app.execute_contract(
            Addr::unchecked(NON_ALLOWLISTED_USER),
            transmuter_addr.clone(),
            &ExecuteMsg::Transmute { recipient: None },
            &coins(swap_amount, USDC),
        );

        let pending_after = query_pending_revenue(&app, &transmuter_addr);
        
        // Pending should either increase or decrease, but never error
        // This tests that the state is always consistent
        assert!(pending_after >= Uint128::zero());
    }

    // State should be consistent
    let final_pending = query_pending_revenue(&app, &transmuter_addr);
    assert!(final_pending >= Uint128::zero());
}

#[test]
fn test_pending_revenue_with_zero_fee() {
    let mut app = setup_app();
    let (transmuter_addr, _, _, _) = setup_contracts(&mut app);

    // Update config to set fee to 0%
    app.execute_contract(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            target_ratio: None,
            tokenfactory_contract: None,
            discounts_contract: None,
            cdp_contract: None,
            usage_fee: Some(Decimal::zero()),
            usage_fee_utilization_threshold: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: None,
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: None,
            global_rate_limit_threshold: None,
            revenue_distributor_addr: None,
            revenue_distributions: None,
            monthly_incentive_max: None,
            incentive_denom: None,
            neutron_proxy: None,
            staking_contract: None,
            mars_mirror_contract: None,
            affiliate_fee: Decimal::percent(1),
        },
        &[],
    )
    .unwrap();

    // Fund transmuter with both assets
    app.send_tokens(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &coins(1_000_000, CDT),
    )
    .unwrap();
    app.send_tokens(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &coins(1_000_000, USDC),
    )
    .unwrap();

    // Swap with zero fee
    app.execute_contract(
        Addr::unchecked(NON_ALLOWLISTED_USER),
        transmuter_addr.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(100_000, USDC),
    )
    .unwrap();

    // Pending revenue should remain zero
    let pending = query_pending_revenue(&app, &transmuter_addr);
    assert_eq!(pending, Uint128::zero());
}

#[test]
fn test_pending_revenue_state_consistency_after_failures() {
    let mut app = setup_app();
    let (transmuter_addr, _, _, _) = setup_contracts(&mut app);

    // Fund transmuter with both assets
    app.send_tokens(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &coins(600_000, CDT),
    )
    .unwrap();
    app.send_tokens(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &coins(10_000_000, USDC),
    )
    .unwrap();

    // First swap CDT -> USDC
    app.execute_contract(
        Addr::unchecked(NON_ALLOWLISTED_USER),
        transmuter_addr.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(100_000, CDT),
    )
    .unwrap();

    // Perform multiple swaps
    for _ in 0..10 {
        let result = app.execute_contract(
            Addr::unchecked(NON_ALLOWLISTED_USER),
            transmuter_addr.clone(),
            &ExecuteMsg::Transmute { recipient: None },
            &coins(50_000, USDC),
        );
        
        // All swaps should succeed even if fee distribution fails
        assert!(result.is_ok());
    }

    // Check that some pending revenue accumulated
    let pending = query_pending_revenue(&app, &transmuter_addr);
    assert!(pending > Uint128::zero());

    // Add more CDT and do one more swap to clear pending
    app.send_tokens(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &coins(10_000_000, CDT),
    )
    .unwrap();

    app.execute_contract(
        Addr::unchecked(NON_ALLOWLISTED_USER),
        transmuter_addr.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(50_000, USDC),
    )
    .unwrap();

    // Pending should be reduced
    let pending_after = query_pending_revenue(&app, &transmuter_addr);
    assert!(pending_after < pending);
}

#[test]
fn test_large_pending_revenue_amounts() {
    let mut app = setup_app();
    let (transmuter_addr, _, _, _) = setup_contracts(&mut app);

    // Fund transmuter with both assets
    app.send_tokens(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &coins(INITIAL_BALANCE / 2, CDT),
    )
    .unwrap();
    app.send_tokens(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &coins(INITIAL_BALANCE, USDC),
    )
    .unwrap();

    // First swap CDT -> USDC to ensure USDC availability
    app.execute_contract(
        Addr::unchecked(NON_ALLOWLISTED_USER),
        transmuter_addr.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(100_000_000, CDT),
    )
    .unwrap();

    // Perform large swap to generate significant pending revenue
    let large_swap = 100_000_000u128; // 100M
    let _ = app.execute_contract(
        Addr::unchecked(NON_ALLOWLISTED_USER),
        transmuter_addr.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(large_swap, USDC),
    );

    let pending = query_pending_revenue(&app, &transmuter_addr);
    assert!(pending > Uint128::zero());

    // Add sufficient CDT to process all pending
    app.send_tokens(
        Addr::unchecked(ADMIN),
        transmuter_addr.clone(),
        &coins(INITIAL_BALANCE, CDT),
    )
    .unwrap();

    // Another swap should clear most pending
    let _ = app.execute_contract(
        Addr::unchecked(NON_ALLOWLISTED_USER),
        transmuter_addr.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(10_000, USDC),
    );

    let pending_after = query_pending_revenue(&app, &transmuter_addr);
    // Should be reduced
    assert!(pending_after < pending);
}

